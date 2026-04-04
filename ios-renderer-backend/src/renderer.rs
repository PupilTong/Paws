//! ViewTree: transformation layer that walks the `LayoutBox` tree and
//! generates minimal updating op-codes.
//!
//! The [`ViewTree`] maintains a snapshot of each node's properties from
//! the previous frame. On each [`process`](ViewTree::process) call it
//! compares the current tree against the snapshot and only emits ops for
//! properties that actually changed. New nodes get full Create + property
//! ops, removed nodes get Detach + Release ops.
//!
//! This is NOT a vdom-style diff — there is no node reuse or reordering
//! logic. It is a per-node property-level dirty check.

use engine::LayoutBox;
use fnv::FnvHashMap;
use style::values::specified::box_::Overflow;
use style::values::specified::font::FONT_MEDIUM_PX;

use crate::ops::{OpBuffer, ViewKind};

/// Snapshot of a node's properties from a single frame.
///
/// Used for dirty checking: only emit ops when a property differs from
/// the previous frame's snapshot.
#[derive(Clone, PartialEq)]
struct NodeSnapshot {
    kind: ViewKind,
    parent_id: u64,
    parent_kind: ViewKind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bg_color: Option<(f32, f32, f32, f32)>,
    clips: bool,
    content_size: Option<(f32, f32)>,
    // ── Text fields (only meaningful for ViewKind::Text) ───────
    text_content: Option<String>,
    font_size: f32,
    font_weight: f32,
    text_color: Option<(f32, f32, f32, f32)>,
}

/// Walks `LayoutBox` trees and generates minimal updating op-codes by
/// comparing against the previous frame's state.
///
/// The ViewTree is fully `Send` — it holds no UIKit pointers and can
/// safely live on the background engine thread.
pub(crate) struct ViewTree {
    prev: FnvHashMap<u64, NodeSnapshot>,
}

impl ViewTree {
    pub(crate) fn new() -> Self {
        Self {
            prev: FnvHashMap::default(),
        }
    }

    /// Processes a `LayoutBox` tree and emits updating op-codes into `ops`.
    ///
    /// Compares each node against the previous frame's snapshot:
    /// - **New node** → Declare + all property ops
    /// - **Same kind** → only emit ops for changed properties
    /// - **Kind changed** → Detach + Release old, Declare + all props for new
    /// - **Removed node** → Detach + Release
    pub(crate) fn process(&mut self, layout: &LayoutBox, ops: &mut OpBuffer) {
        ops.clear();
        let mut current = FnvHashMap::default();
        self.process_node(layout, u64::MAX, ViewKind::View, true, ops, &mut current);

        // Emit Release ops for nodes removed since last frame.
        for (id, snap) in &self.prev {
            if !current.contains_key(id) {
                ops.push_detach(*id, snap.kind);
                ops.push_release(*id, snap.kind);
            }
        }

        self.prev = current;
    }

    fn process_node(
        &self,
        node: &LayoutBox,
        parent_id: u64,
        parent_kind: ViewKind,
        is_root: bool,
        ops: &mut OpBuffer,
        current: &mut FnvHashMap<u64, NodeSnapshot>,
    ) {
        let node_id = u64::from(node.node_id);
        let kind = determine_kind(node, is_root);

        // Extract properties based on kind.
        let (bg, clips, content_size, text_content, font_size, font_weight, text_color) =
            if kind == ViewKind::Text {
                let (fs, fw) = extract_font_properties(node);
                let tc = extract_text_color(node);
                (None, false, None, node.text_content.clone(), fs, fw, tc)
            } else {
                let bg = extract_background_color(node);
                let clips = has_clip_overflow(node);
                let content_size = if matches!(kind, ViewKind::ScrollView) {
                    Some(compute_content_size(node))
                } else {
                    None
                };
                (bg, clips, content_size, None, 0.0, 0.0, None)
            };

        let snap = NodeSnapshot {
            kind,
            parent_id,
            parent_kind,
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
            bg_color: bg,
            clips,
            content_size,
            text_content,
            font_size,
            font_weight,
            text_color,
        };

        match self.prev.get(&node_id) {
            None => {
                // New node — emit create + all properties.
                emit_full_node(node_id, &snap, ops);
            }
            Some(prev) if prev.kind != kind => {
                // Kind changed — release old, create new.
                ops.push_detach(node_id, prev.kind);
                ops.push_release(node_id, prev.kind);
                emit_full_node(node_id, &snap, ops);
            }
            Some(prev) => {
                // Same kind — only emit changed properties.
                if prev.x != snap.x || prev.y != snap.y || prev.w != snap.w || prev.h != snap.h {
                    ops.push_set_frame(kind, node_id, snap.x, snap.y, snap.w, snap.h);
                }
                if prev.parent_id != parent_id || prev.parent_kind != parent_kind {
                    ops.push_attach(node_id, kind, parent_id, parent_kind);
                }

                if kind == ViewKind::Text {
                    // Text-specific dirty checking.
                    if prev.text_content != snap.text_content {
                        if let Some(ref text) = snap.text_content {
                            ops.push_text_content(node_id, text);
                        }
                    }
                    if prev.font_size != snap.font_size || prev.font_weight != snap.font_weight {
                        ops.push_text_font(node_id, snap.font_size, snap.font_weight);
                    }
                    if prev.text_color != snap.text_color {
                        if let Some((r, g, b, a)) = snap.text_color {
                            ops.push_text_color(node_id, r, g, b, a);
                        }
                    }
                } else {
                    // Element-specific dirty checking.
                    if prev.bg_color != bg {
                        if let Some((r, g, b, a)) = bg {
                            ops.push_bg_color(node_id, r, g, b, a);
                        }
                    }
                    if prev.clips != clips && matches!(kind, ViewKind::View | ViewKind::ScrollView)
                    {
                        ops.push_clips(node_id, clips);
                    }
                    if prev.content_size != content_size {
                        if let Some((cw, ch)) = content_size {
                            ops.push_content_size(node_id, cw, ch);
                        }
                    }
                }
            }
        }

        current.insert(node_id, snap);

        // Recurse children in z-index order.
        let mut sorted: Vec<&LayoutBox> = node.children.iter().collect();
        sorted.sort_unstable_by_key(|c| c.z_index.unwrap_or(0));
        for child in &sorted {
            self.process_node(child, node_id, kind, false, ops, current);
        }
    }
}

/// Emits a full Declare + all property ops for a node.
fn emit_full_node(node_id: u64, snap: &NodeSnapshot, ops: &mut OpBuffer) {
    ops.push_declare(snap.kind, node_id, snap.parent_id);
    ops.push_set_frame(snap.kind, node_id, snap.x, snap.y, snap.w, snap.h);

    if snap.kind == ViewKind::Text {
        // Text-specific initial ops.
        if let Some(ref text) = snap.text_content {
            ops.push_text_content(node_id, text);
        }
        ops.push_text_font(node_id, snap.font_size, snap.font_weight);
        if let Some((r, g, b, a)) = snap.text_color {
            ops.push_text_color(node_id, r, g, b, a);
        }
    } else {
        // Element-specific initial ops.
        if let Some((r, g, b, a)) = snap.bg_color {
            ops.push_bg_color(node_id, r, g, b, a);
        }
        if matches!(snap.kind, ViewKind::View | ViewKind::ScrollView) {
            ops.push_clips(node_id, snap.clips);
        }
        if let Some((cw, ch)) = snap.content_size {
            ops.push_content_size(node_id, cw, ch);
        }
    }

    // Attach to parent — root uses sentinel u64::MAX which Swift maps to rootView.
    if snap.parent_id != u64::MAX {
        ops.push_attach(node_id, snap.kind, snap.parent_id, snap.parent_kind);
    }
}

// ── Helper functions ────────────────────────────────────────────────────

/// Determines the UIKit object kind for a layout node.
fn determine_kind(node: &LayoutBox, is_root: bool) -> ViewKind {
    if node.is_text {
        return ViewKind::Text;
    }

    let (overflow_x, overflow_y) = node
        .computed_values
        .as_ref()
        .map(|cv| (cv.clone_overflow_x(), cv.clone_overflow_y()))
        .unwrap_or((Overflow::Visible, Overflow::Visible));

    let needs_scroll = matches!(overflow_x, Overflow::Scroll | Overflow::Auto)
        || matches!(overflow_y, Overflow::Scroll | Overflow::Auto);

    if needs_scroll {
        ViewKind::ScrollView
    } else if is_root {
        ViewKind::View
    } else {
        ViewKind::Layer
    }
}

/// Returns `true` if the node's overflow requires clipping.
fn has_clip_overflow(node: &LayoutBox) -> bool {
    node.computed_values
        .as_ref()
        .map(|cv| {
            let ox = cv.clone_overflow_x();
            let oy = cv.clone_overflow_y();
            matches!(ox, Overflow::Hidden | Overflow::Clip)
                || matches!(oy, Overflow::Hidden | Overflow::Clip)
        })
        .unwrap_or(false)
}

/// Extracts the background color from computed values as an RGBA tuple.
///
/// Returns `None` for transparent backgrounds (alpha ≈ 0) or non-absolute colors.
fn extract_background_color(node: &LayoutBox) -> Option<(f32, f32, f32, f32)> {
    use style::values::computed::Color;

    let cv = node.computed_values.as_ref()?;
    match cv.clone_background_color() {
        Color::Absolute(abs) => {
            let r = abs.components.0;
            let g = abs.components.1;
            let b = abs.components.2;
            let a = abs.alpha;
            if a.abs() < f32::EPSILON {
                None
            } else {
                Some((r, g, b, a))
            }
        }
        _ => None,
    }
}

/// Extracts font size and weight from computed values.
fn extract_font_properties(node: &LayoutBox) -> (f32, f32) {
    node.computed_values
        .as_ref()
        .map(|cv| {
            let fs = cv.clone_font_size().computed_size().px();
            let fs = if fs > 0.0 { fs } else { FONT_MEDIUM_PX };
            let fw = cv.clone_font_weight().value();
            (fs, fw)
        })
        .unwrap_or((FONT_MEDIUM_PX, 400.0))
}

/// Extracts text foreground color from computed values.
///
/// `clone_color()` returns an `AbsoluteColor` (the CSS `color` property
/// is always resolved to an absolute value).
fn extract_text_color(node: &LayoutBox) -> Option<(f32, f32, f32, f32)> {
    let cv = node.computed_values.as_ref()?;
    let abs = cv.clone_color();
    Some((
        abs.components.0,
        abs.components.1,
        abs.components.2,
        abs.alpha,
    ))
}

/// Computes the scroll content size from a node's children.
fn compute_content_size(node: &LayoutBox) -> (f32, f32) {
    let w = node
        .children
        .iter()
        .map(|c| c.x + c.width)
        .fold(0.0_f32, f32::max);
    let h = node
        .children
        .iter()
        .map(|c| c.y + c.height)
        .fold(0.0_f32, f32::max);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{OpBuffer, OpTag};

    /// Creates a `LayoutBox` with the given node ID and default fields.
    fn layout(id: u64) -> LayoutBox {
        LayoutBox {
            node_id: engine::NodeId::from(id),
            ..Default::default()
        }
    }

    /// Creates a `LayoutBox` with explicit position and size.
    fn layout_with_frame(id: u64, x: f32, y: f32, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node_id: engine::NodeId::from(id),
            x,
            y,
            width: w,
            height: h,
            ..Default::default()
        }
    }

    /// Creates a text `LayoutBox`.
    fn text_layout(id: u64, text: &str, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node_id: engine::NodeId::from(id),
            width: w,
            height: h,
            is_text: true,
            text_content: Some(text.to_string()),
            ..Default::default()
        }
    }

    /// Collects all op tags from a buffer.
    fn collect_tags(ops: &OpBuffer) -> Vec<u8> {
        (0..ops.op_count()).filter_map(|i| ops.tag_at(i)).collect()
    }

    #[test]
    fn test_first_frame_emits_full_ops() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();
        let node = layout_with_frame(1, 10.0, 20.0, 100.0, 50.0);

        tree.process(&node, &mut ops);

        let tags = collect_tags(&ops);
        // Root: DeclareView + SetViewFrame + SetClipsToBounds (no bg, no content size)
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
        assert!(tags.contains(&(OpTag::SetViewFrame as u8)));
        assert!(tags.contains(&(OpTag::SetClipsToBounds as u8)));
    }

    #[test]
    fn test_unchanged_frame_emits_nothing() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();
        let node = layout_with_frame(1, 10.0, 20.0, 100.0, 50.0);

        // First frame — full ops.
        tree.process(&node, &mut ops);
        assert!(ops.op_count() > 0);

        // Second frame — same data, no changes.
        tree.process(&node, &mut ops);
        assert_eq!(ops.op_count(), 0, "unchanged tree should emit zero ops");
    }

    #[test]
    fn test_changed_frame_emits_set_frame_only() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        tree.process(&layout_with_frame(1, 0.0, 0.0, 100.0, 50.0), &mut ops);

        // Change frame only.
        tree.process(&layout_with_frame(1, 5.0, 10.0, 200.0, 100.0), &mut ops);

        let tags = collect_tags(&ops);
        assert!(tags.contains(&(OpTag::SetViewFrame as u8)));
        // Should NOT re-declare.
        assert!(!tags.contains(&(OpTag::DeclareView as u8)));
    }

    #[test]
    fn test_removed_node_emits_detach_release() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        // Root with child.
        let mut root = layout(1);
        root.children = vec![layout(2)];
        tree.process(&root, &mut ops);

        // Root only — child removed.
        let root_only = layout(1);
        tree.process(&root_only, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::DetachLayer as u8)),
            "removed layer should be detached"
        );
        assert!(
            tags.contains(&(OpTag::ReleaseLayer as u8)),
            "removed layer should be released"
        );
    }

    #[test]
    fn test_child_uses_layer_kind() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![layout(2)];
        tree.process(&root, &mut ops);

        let tags = collect_tags(&ops);
        // Root should be View, child should be Layer.
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
        assert!(tags.contains(&(OpTag::DeclareLayer as u8)));
    }

    #[test]
    fn test_z_index_sorting() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut child_a = layout_with_frame(2, 0.0, 0.0, 10.0, 10.0);
        child_a.z_index = Some(3);
        let mut child_b = layout_with_frame(3, 0.0, 0.0, 10.0, 10.0);
        child_b.z_index = Some(1);

        let mut root = layout(1);
        root.children = vec![child_a, child_b];
        tree.process(&root, &mut ops);

        // Find the DeclareLayer ops and check their node_ids are in z-order.
        let mut layer_ids = Vec::new();
        for i in 0..ops.op_count() {
            if ops.tag_at(i) == Some(OpTag::DeclareLayer as u8) {
                let slot = ops.slot_at(i).unwrap();
                let nid = u64::from_le_bytes(slot[1..9].try_into().unwrap());
                layer_ids.push(nid);
            }
        }
        assert_eq!(
            layer_ids,
            vec![3, 2],
            "z-index 1 (node 3) should come before z-index 3 (node 2)"
        );
    }

    #[test]
    fn test_background_color_emitted() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        // Build a RuntimeState with a colored div to get real computed values.
        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();
        state
            .set_inline_style(id, "background-color".to_string(), "red".to_string())
            .unwrap();
        state
            .set_inline_style(id, "width".to_string(), "50px".to_string())
            .unwrap();
        state
            .set_inline_style(id, "height".to_string(), "50px".to_string())
            .unwrap();

        state.commit();
        let root_id = state.doc.root_element_id().unwrap();
        let layout_box = engine::compute_layout(&mut state.doc, root_id).unwrap();
        tree.process(&layout_box, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::SetBgColor as u8)),
            "background-color should generate a SetBgColor op"
        );
    }

    #[test]
    fn test_extract_background_color_transparent_returns_none() {
        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();

        state.commit();
        let root_id = state.doc.root_element_id().unwrap();
        let layout_box = engine::compute_layout(&mut state.doc, root_id).unwrap();
        let bg = extract_background_color(&layout_box);
        assert!(bg.is_none(), "transparent background should return None");
    }

    // ── Text node tests ────────────────────────────────────────────

    #[test]
    fn test_text_node_emits_declare_text() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::DeclareText as u8)),
            "text node should emit DeclareText"
        );
        assert!(
            tags.contains(&(OpTag::SetTextContent as u8)),
            "text node should emit SetTextContent"
        );
        assert!(
            tags.contains(&(OpTag::SetTextFont as u8)),
            "text node should emit SetTextFont"
        );
    }

    #[test]
    fn test_text_node_string_table() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Paws", 30.0, 16.0)];
        tree.process(&root, &mut ops);

        // Find the SetTextContent op and verify string table content.
        let text = std::str::from_utf8(&ops.strings_data()[..ops.strings_len()]).unwrap();
        assert!(
            text.contains("Paws"),
            "string table should contain the text"
        );
    }

    #[test]
    fn test_text_content_change_emits_update() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root, &mut ops);

        // Change text content.
        let mut root2 = layout(1);
        root2.children = vec![text_layout(2, "World", 40.0, 16.0)];
        tree.process(&root2, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::SetTextContent as u8)),
            "changed text should emit SetTextContent"
        );
        // Should NOT re-declare.
        assert!(
            !tags.contains(&(OpTag::DeclareText as u8)),
            "unchanged kind should not re-declare"
        );
    }

    #[test]
    fn test_removed_text_node_emits_detach_release() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root, &mut ops);

        // Remove text child.
        tree.process(&layout(1), &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::DetachText as u8)),
            "removed text should be detached"
        );
        assert!(
            tags.contains(&(OpTag::ReleaseText as u8)),
            "removed text should be released"
        );
    }

    #[test]
    fn test_text_unchanged_emits_nothing() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root, &mut ops);
        assert!(ops.op_count() > 0);

        // Same text, same frame — no ops.
        tree.process(&root, &mut ops);
        assert_eq!(ops.op_count(), 0, "unchanged text should emit zero ops");
    }

    #[test]
    fn test_text_frame_change_emits_set_frame() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut root = layout(1);
        root.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root, &mut ops);

        // Change text frame dimensions.
        let mut root2 = layout(1);
        root2.children = vec![text_layout(2, "Hello", 80.0, 20.0)];
        tree.process(&root2, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::SetLayerFrame as u8)),
            "changed text frame should emit SetLayerFrame"
        );
        assert!(
            !tags.contains(&(OpTag::DeclareText as u8)),
            "unchanged kind should not re-declare"
        );
    }

    #[test]
    fn test_kind_change_layer_to_text() {
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        // Start as a plain layer child.
        let mut root = layout(1);
        root.children = vec![layout_with_frame(2, 0.0, 0.0, 40.0, 16.0)];
        tree.process(&root, &mut ops);

        // Now the same node becomes a text node.
        let mut root2 = layout(1);
        root2.children = vec![text_layout(2, "Hello", 40.0, 16.0)];
        tree.process(&root2, &mut ops);

        let tags = collect_tags(&ops);
        assert!(
            tags.contains(&(OpTag::DetachLayer as u8)),
            "old layer should be detached"
        );
        assert!(
            tags.contains(&(OpTag::ReleaseLayer as u8)),
            "old layer should be released"
        );
        assert!(
            tags.contains(&(OpTag::DeclareText as u8)),
            "new text node should be declared"
        );
    }

    #[test]
    fn test_text_with_computed_styles() {
        // E2E: use RuntimeState to create a div with text and real computed styles.
        let mut tree = ViewTree::new();
        let mut ops = OpBuffer::new();

        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let div_id = state.create_element("div".to_string());
        state.append_element(0, div_id).unwrap();
        state
            .set_inline_style(div_id, "width".into(), "200px".into())
            .unwrap();

        let txt_id = state.create_text_node("Paws text".to_string());
        state.append_element(div_id, txt_id).unwrap();

        state.commit();
        let root_id = state.doc.root_element_id().unwrap();
        let layout_box = engine::compute_layout(&mut state.doc, root_id).unwrap();
        tree.process(&layout_box, &mut ops);

        let tags = collect_tags(&ops);
        // Root element should get DeclareView
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
        // Text child should get DeclareText + SetTextContent + SetTextFont
        assert!(
            tags.contains(&(OpTag::DeclareText as u8)),
            "text child should generate DeclareText"
        );
        assert!(
            tags.contains(&(OpTag::SetTextContent as u8)),
            "text child should generate SetTextContent"
        );
        assert!(
            tags.contains(&(OpTag::SetTextFont as u8)),
            "text child should generate SetTextFont"
        );
        // Verify string table has text
        assert!(ops.strings_len() > 0, "string table should contain text");
        let text = std::str::from_utf8(ops.strings_data()).unwrap();
        assert!(text.contains("Paws text"));
    }
}
