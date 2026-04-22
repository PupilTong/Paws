//! ViewTree: walks the `Document` tree directly and generates minimal
//! updating op-codes by comparing current layout/style data against
//! per-node render state stored on each `PawsElement`.
//!
//! Previous-frame diff state lives on `PawsElement::render_state`
//! (typed as [`IosNodeState`]). There is no separate HashMap or
//! snapshot store — truly one tree.
//!
//! This is NOT a vdom-style diff — there is no node reuse or reordering
//! logic. It is a per-node property-level dirty check.

use engine::dom::Document;
use engine::RenderState;
use style::values::specified::box_::Overflow;
use style::values::specified::font::FONT_MEDIUM_PX;

use crate::ops::{OpBuffer, ViewKind};

/// Sentinel parent ID for the root node. Swift maps this to `rootView`.
const ROOT_PARENT_ID: u64 = u64::MAX;

/// Per-node render state stored directly on each `PawsElement`.
///
/// Replaces the old `FnvHashMap<u64, NodeSnapshot>` — diff state now
/// lives on the DOM node itself. For `()` (tests/headless) this is
/// zero-sized; for iOS it captures the previous frame's properties.
#[derive(Default, Clone, PartialEq)]
pub(crate) struct IosNodeState {
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
    /// `true` once this node has been rendered in a previous frame.
    /// `Default` gives `false`, which triggers full create on first encounter.
    rendered: bool,
}

// SAFETY: IosNodeState is a plain data struct with no thread-affine pointers.
unsafe impl Send for IosNodeState {}

/// Walks the `Document` tree and generates minimal updating op-codes by
/// comparing current layout data against per-node `IosNodeState`.
///
/// The ViewTree is fully `Send` — it holds no UIKit pointers and can
/// safely live on the background engine thread.
pub(crate) struct ViewTree {
    ops: OpBuffer,
}

impl ViewTree {
    /// Creates a new ViewTree with an empty op buffer.
    pub(crate) fn new() -> Self {
        Self {
            ops: OpBuffer::new(),
        }
    }

    /// Returns a reference to the internal op buffer.
    pub(crate) fn ops(&self) -> &OpBuffer {
        &self.ops
    }

    /// Processes the Document tree and emits updating op-codes.
    ///
    /// Walks the tree starting from `root`, comparing each node's current
    /// layout/style against its `render_state` (previous frame). Only emits
    /// ops for properties that actually changed.
    ///
    /// Removed nodes are detected via `doc.removed_render_states`.
    pub(crate) fn process(
        &mut self,
        doc: &mut Document<IosNodeState>,
        root: Option<engine::NodeId>,
    ) {
        self.ops.clear();

        if let Some(root_id) = root {
            self.process_node(doc, root_id, ROOT_PARENT_ID, ViewKind::View, true);
        }

        // Emit Release ops for nodes removed since last commit.
        for (id, prev_state) in doc.removed_render_states() {
            if prev_state.rendered {
                self.ops.push_detach(u64::from(*id), prev_state.kind);
                self.ops.push_release(u64::from(*id), prev_state.kind);
            }
        }
    }

    fn process_node(
        &mut self,
        doc: &mut Document<IosNodeState>,
        node_id: engine::NodeId,
        parent_id: u64,
        parent_kind: ViewKind,
        is_root: bool,
    ) {
        let node = match doc.get_node(node_id) {
            Some(n) => n,
            None => return,
        };
        if !node.has_style() {
            return;
        }

        let nid = u64::from(node_id);
        let kind = determine_kind(node, is_root);
        let layout = node.layout();

        // Extract properties based on kind.
        let (bg, clips, content_size, text_content, font_size, font_weight, text_color) =
            if kind == ViewKind::Text {
                let (fs, fw) = extract_font_properties(node);
                let tc = extract_text_color(node);
                (
                    None,
                    false,
                    None,
                    node.text().map(|s| s.to_string()),
                    fs,
                    fw,
                    tc,
                )
            } else {
                let bg = extract_background_color(node);
                let clips = has_clip_overflow(node);
                let content_size = if matches!(kind, ViewKind::ScrollView) {
                    Some(compute_content_size(doc, node_id))
                } else {
                    None
                };
                (bg, clips, content_size, None, 0.0, 0.0, None)
            };

        let new_state = IosNodeState {
            kind,
            parent_id,
            parent_kind,
            x: layout.location.x,
            y: layout.location.y,
            w: layout.size.width,
            h: layout.size.height,
            bg_color: bg,
            clips,
            content_size,
            text_content,
            font_size,
            font_weight,
            text_color,
            rendered: true,
        };

        let prev = node.render_state();
        if !prev.rendered {
            // New node — emit create + all properties.
            emit_full_node(nid, &new_state, &mut self.ops);
        } else if prev.kind != kind {
            // Kind changed — release old, create new.
            self.ops.push_detach(nid, prev.kind);
            self.ops.push_release(nid, prev.kind);
            emit_full_node(nid, &new_state, &mut self.ops);
        } else {
            // Same kind — only emit changed properties.
            if prev.x != new_state.x
                || prev.y != new_state.y
                || prev.w != new_state.w
                || prev.h != new_state.h
            {
                self.ops.push_set_frame(
                    kind,
                    nid,
                    new_state.x,
                    new_state.y,
                    new_state.w,
                    new_state.h,
                );
            }
            if prev.parent_id != parent_id || prev.parent_kind != parent_kind {
                self.ops.push_attach(nid, kind, parent_id, parent_kind);
            }

            if kind == ViewKind::Text {
                // Text-specific dirty checking.
                if prev.text_content != new_state.text_content {
                    if let Some(ref text) = new_state.text_content {
                        self.ops.push_text_content(nid, text);
                    }
                }
                if prev.font_size != new_state.font_size
                    || prev.font_weight != new_state.font_weight
                {
                    self.ops
                        .push_text_font(nid, new_state.font_size, new_state.font_weight);
                }
                if prev.text_color != new_state.text_color {
                    if let Some((r, g, b, a)) = new_state.text_color {
                        self.ops.push_text_color(nid, r, g, b, a);
                    }
                }
            } else {
                // Element-specific dirty checking.
                if prev.bg_color != bg {
                    if let Some((r, g, b, a)) = bg {
                        self.ops.push_bg_color(nid, r, g, b, a);
                    }
                }
                if prev.clips != clips && matches!(kind, ViewKind::View | ViewKind::ScrollView) {
                    self.ops.push_clips(nid, clips);
                }
                if prev.content_size != content_size {
                    if let Some((cw, ch)) = content_size {
                        self.ops.push_content_size(nid, cw, ch);
                    }
                }
            }
        }

        // Collect children in paint order before mutating render state.
        let children = engine::paint_order_children(doc, node_id);

        doc.get_node_mut(node_id)
            .unwrap()
            .set_render_state(new_state);

        // Recurse children in paint order (already sorted by the engine's
        // stacking context logic).
        for child_id in children {
            self.process_node(doc, child_id, nid, kind, false);
        }
    }
}

/// Emits a full Declare + all property ops for a node.
fn emit_full_node(node_id: u64, state: &IosNodeState, ops: &mut OpBuffer) {
    ops.push_declare(state.kind, node_id, state.parent_id);
    ops.push_set_frame(state.kind, node_id, state.x, state.y, state.w, state.h);

    if state.kind == ViewKind::Text {
        // Text-specific initial ops.
        if let Some(ref text) = state.text_content {
            ops.push_text_content(node_id, text);
        }
        ops.push_text_font(node_id, state.font_size, state.font_weight);
        if let Some((r, g, b, a)) = state.text_color {
            ops.push_text_color(node_id, r, g, b, a);
        }
    } else {
        // Element-specific initial ops.
        if let Some((r, g, b, a)) = state.bg_color {
            ops.push_bg_color(node_id, r, g, b, a);
        }
        if matches!(state.kind, ViewKind::View | ViewKind::ScrollView) {
            ops.push_clips(node_id, state.clips);
        }
        if let Some((cw, ch)) = state.content_size {
            ops.push_content_size(node_id, cw, ch);
        }
    }

    // Attach to parent. ROOT_PARENT_ID is the sentinel for "attach to
    // rootView" — the Swift `attachToParent` resolves it against
    // `rootView` rather than the view map, so we must emit the op in
    // that case too; otherwise the DOM root is created but never added
    // to the host view hierarchy.
    ops.push_attach(node_id, state.kind, state.parent_id, state.parent_kind);
}

// ── Helper functions ────────────────────────────────────────────────────

/// Determines the UIKit object kind for a layout node.
fn determine_kind<S: RenderState>(node: &engine::dom::PawsElement<S>, is_root: bool) -> ViewKind {
    if node.is_text_node() {
        return ViewKind::Text;
    }

    let (overflow_x, overflow_y) = node
        .get_computed_values()
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
fn has_clip_overflow<S: RenderState>(node: &engine::dom::PawsElement<S>) -> bool {
    node.get_computed_values()
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
fn extract_background_color<S: RenderState>(
    node: &engine::dom::PawsElement<S>,
) -> Option<(f32, f32, f32, f32)> {
    use style::values::computed::Color;

    let cv = node.get_computed_values()?;
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
fn extract_font_properties<S: RenderState>(node: &engine::dom::PawsElement<S>) -> (f32, f32) {
    node.get_computed_values()
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
fn extract_text_color<S: RenderState>(
    node: &engine::dom::PawsElement<S>,
) -> Option<(f32, f32, f32, f32)> {
    let cv = node.get_computed_values()?;
    let abs = cv.clone_color();
    Some((
        abs.components.0,
        abs.components.1,
        abs.components.2,
        abs.alpha,
    ))
}

/// Computes the scroll content size from a node's children.
fn compute_content_size<S: RenderState>(doc: &Document<S>, node_id: engine::NodeId) -> (f32, f32) {
    doc.get_node(node_id)
        .unwrap()
        .children
        .iter()
        .filter_map(|&cid| doc.get_node(cid))
        .map(|c| {
            let l = c.layout();
            (l.location.x + l.size.width, l.location.y + l.size.height)
        })
        .fold((0.0, 0.0), |(max_w, max_h), (w, h)| {
            (max_w.max(w), max_h.max(h))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::OpTag;
    use engine::dom::Document;

    /// Creates a RuntimeState with IosNodeState, adds elements, commits (resolves
    /// style + layout). Returns the Document ready for ViewTree processing.
    fn setup_styled_doc(
        setup: impl FnOnce(&mut engine::RuntimeState<TestRenderer>),
    ) -> Document<IosNodeState> {
        let renderer = TestRenderer;
        let mut state =
            engine::RuntimeState::with_renderer("https://test.com".to_string(), renderer);
        setup(&mut state);
        state.commit();
        state.doc
    }

    /// No-op renderer for test document setup. `commit()` triggers style
    /// resolution and layout but the renderer does nothing.
    struct TestRenderer;
    // SAFETY: TestRenderer is a zero-sized unit struct with no data.
    unsafe impl Send for TestRenderer {}
    impl engine::EngineRenderer for TestRenderer {
        type NodeState = IosNodeState;
        fn on_commit(&mut self, _doc: &mut Document<IosNodeState>, _root: Option<engine::NodeId>) {}
    }

    /// Collects all op tags from ViewTree's ops buffer.
    fn collect_tags(tree: &ViewTree) -> Vec<u8> {
        let ops = tree.ops();
        (0..ops.op_count()).filter_map(|i| ops.tag_at(i)).collect()
    }

    #[test]
    fn test_first_frame_emits_full_ops() {
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "width".into(), "100px".into())
                .unwrap();
            state
                .set_inline_style(div, "height".into(), "50px".into())
                .unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
        assert!(tags.contains(&(OpTag::SetViewFrame as u8)));
        assert!(tags.contains(&(OpTag::SetClipsToBounds as u8)));
    }

    #[test]
    fn test_unchanged_frame_emits_nothing() {
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "width".into(), "100px".into())
                .unwrap();
            state
                .set_inline_style(div, "height".into(), "50px".into())
                .unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();

        // First frame — full ops.
        tree.process(&mut doc, root);
        assert!(tree.ops().op_count() > 0);

        // Second frame — same data, no changes.
        tree.process(&mut doc, root);
        assert_eq!(
            tree.ops().op_count(),
            0,
            "unchanged tree should emit zero ops"
        );
    }

    #[test]
    fn test_child_uses_layer_kind() {
        let mut doc = setup_styled_doc(|state| {
            let parent = state.create_element("div".to_string());
            state.append_element(0, parent).unwrap();
            state
                .set_inline_style(parent, "display".into(), "flex".into())
                .unwrap();
            let child = state.create_element("div".to_string());
            state.append_element(parent, child).unwrap();
            state
                .set_inline_style(child, "width".into(), "10px".into())
                .unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
        assert!(tags.contains(&(OpTag::DeclareLayer as u8)));
    }

    #[test]
    fn test_background_color_emitted() {
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "background-color".into(), "red".into())
                .unwrap();
            state
                .set_inline_style(div, "width".into(), "50px".into())
                .unwrap();
            state
                .set_inline_style(div, "height".into(), "50px".into())
                .unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
        assert!(
            tags.contains(&(OpTag::SetBgColor as u8)),
            "background-color should generate a SetBgColor op"
        );
    }

    #[test]
    fn test_extract_background_color_transparent_returns_none() {
        let doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
        });

        let root_id = doc.root_element_id().unwrap();
        let node = doc.get_node(root_id).unwrap();
        let bg = extract_background_color(node);
        assert!(bg.is_none(), "transparent background should return None");
    }

    // ── Text node tests ────────────────────────────────────────────

    #[test]
    fn test_text_node_emits_declare_text() {
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "width".into(), "200px".into())
                .unwrap();
            let txt = state.create_text_node("Hello".to_string());
            state.append_element(div, txt).unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
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
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "width".into(), "200px".into())
                .unwrap();
            let txt = state.create_text_node("Paws".to_string());
            state.append_element(div, txt).unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let ops = tree.ops();
        let text = std::str::from_utf8(&ops.strings_data()[..ops.strings_len()]).unwrap();
        assert!(
            text.contains("Paws"),
            "string table should contain the text"
        );
    }

    /// Builds a DOM tree that mirrors the yew counter example:
    /// `<div><div class="counter"><button>+</button><span>0</span></div></div>`.
    /// Used by the two tests below to exercise the unstyled-tree paint path.
    fn setup_yew_counter_doc(viewport: Option<(f32, f32)>) -> Document<IosNodeState> {
        let renderer = TestRenderer;
        let mut state = match viewport {
            Some((w, h)) => engine::RuntimeState::with_definite_viewport(
                "https://test.com".to_string(),
                renderer,
                w,
                h,
            ),
            None => engine::RuntimeState::with_renderer("https://test.com".to_string(), renderer),
        };
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let counter = state.create_element("div".to_string());
        state
            .set_attribute(counter, "class".into(), "counter".into())
            .unwrap();
        state.append_element(host, counter).unwrap();

        let button = state.create_element("button".to_string());
        state.append_element(counter, button).unwrap();
        let btn_txt = state.create_text_node("+".to_string());
        state.append_element(button, btn_txt).unwrap();

        let span = state.create_element("span".to_string());
        state.append_element(counter, span).unwrap();
        let span_txt = state.create_text_node("0".to_string());
        state.append_element(span, span_txt).unwrap();

        state.commit();
        state.doc
    }

    /// Reads the width of the `SetViewFrame` op for the given node id from
    /// the test ops buffer, or `None` if none was emitted.
    fn view_frame_width(ops: &crate::ops::OpBuffer, target_id: u64) -> Option<f32> {
        for i in 0..ops.op_count() {
            if ops.tag_at(i) != Some(OpTag::SetViewFrame as u8) {
                continue;
            }
            let slot = ops.slot_at(i).unwrap();
            let id = u64::from_le_bytes(slot[1..9].try_into().unwrap());
            if id != target_id {
                continue;
            }
            return Some(f32::from_le_bytes(slot[17..21].try_into().unwrap()));
        }
        None
    }

    /// Regression guard: the yew counter (all unstyled elements) must emit
    /// `DeclareText` + `SetTextContent` ops for both text nodes, and the
    /// string table must carry their contents. The iOS empty-host bug that
    /// motivated this test was an easy miss because every other unit test
    /// in this module sets explicit widths, so layout never collapses.
    #[test]
    fn test_yew_counter_structure_emits_text_ops() {
        let mut doc = setup_yew_counter_doc(None);
        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
        let declare_text_count = tags
            .iter()
            .filter(|&&t| t == OpTag::DeclareText as u8)
            .count();
        assert_eq!(
            declare_text_count, 2,
            "yew counter has two text nodes (\"+\", \"0\"); both must emit DeclareText. \
             Actual ops: {tags:?}"
        );

        let strings = std::str::from_utf8(tree.ops().strings_data()).unwrap();
        assert!(strings.contains('+'), "string table missing '+': {strings:?}");
        assert!(strings.contains('0'), "string table missing '0': {strings:?}");
    }

    /// Documents the viewport dependency that caused the empty-host
    /// regression on the iOS simulator. Without a viewport Taffy lays every
    /// block out at its intrinsic content size, so an unstyled `<div>`
    /// collapses to the width of whatever text it contains (about 9 px for
    /// "+" / "0"). Constructing the `RuntimeState` via
    /// `with_definite_viewport` instead expands block-level elements to the
    /// viewport width.
    #[test]
    fn test_yew_counter_viewport_expands_unstyled_elements() {
        // Case 1: no viewport → root host collapses to content width.
        let mut doc = setup_yew_counter_doc(None);
        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);
        let content_sized_width = view_frame_width(tree.ops(), 1)
            .expect("host root emits SetViewFrame");
        assert!(
            content_sized_width < 50.0,
            "without viewport, unstyled root collapses to content width \
             (expected < 50px, got {content_sized_width}) — this is the bug \
             users see as an empty PawsRendererView on iOS"
        );

        // Case 2: viewport set → root expands to viewport width.
        let mut doc = setup_yew_counter_doc(Some((375.0, 667.0)));
        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);
        let viewport_width =
            view_frame_width(tree.ops(), 1).expect("host root emits SetViewFrame");
        assert!(
            (viewport_width - 375.0).abs() < 0.5,
            "with viewport 375×667, unstyled root should fill width \
             (expected ≈ 375px, got {viewport_width})"
        );
    }

    #[test]
    fn test_text_with_computed_styles() {
        let mut doc = setup_styled_doc(|state| {
            let div = state.create_element("div".to_string());
            state.append_element(0, div).unwrap();
            state
                .set_inline_style(div, "width".into(), "200px".into())
                .unwrap();
            let txt = state.create_text_node("Paws text".to_string());
            state.append_element(div, txt).unwrap();
        });

        let mut tree = ViewTree::new();
        let root = doc.root_element_id();
        tree.process(&mut doc, root);

        let tags = collect_tags(&tree);
        assert!(tags.contains(&(OpTag::DeclareView as u8)));
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
        let ops = tree.ops();
        assert!(ops.strings_len() > 0, "string table should contain text");
        let text = std::str::from_utf8(ops.strings_data()).unwrap();
        assert!(text.contains("Paws text"));
    }
}
