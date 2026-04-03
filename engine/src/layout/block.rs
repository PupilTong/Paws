//! Layout computation via Taffy's `LayoutPartialTree` trait hierarchy.
//!
//! Taffy's traits are implemented directly on [`Document`] (the "fat tree"
//! pattern). Layout data (cache, unrounded/final layouts) lives on
//! [`PawsElement`] for persistence across passes (future CSS Containment).

use style::servo_arc::Arc;
use style::values::computed::length::CSSPixelLength;
use style::values::computed::length_percentage::CalcLengthPercentage;
use style::values::specified::font::FONT_MEDIUM_PX;

use crate::dom::document::Document;
use crate::dom::NodeType;

use taffy::compute_block_layout;
use taffy::prelude::*;
use taffy::tree::{Layout, LayoutInput, LayoutOutput};
use taffy::{
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_hidden_layout,
    compute_leaf_layout, compute_root_layout, round_layout, CacheTree,
};

// ─── Public output type ──────────────────────────────────────────────

/// A fully-resolved layout node with absolute position, size, and children.
///
/// Produced by [`compute_layout`] and consumed by the iOS renderer backend's
/// conversion layer to build `LayoutNode` trees.
///
/// Style-derived values (overflow, background color, etc.) are accessible
/// through [`computed_values`](Self::computed_values) rather than being
/// extracted into separate fields.
pub struct LayoutBox {
    /// The DOM node ID this layout box corresponds to.
    pub node_id: taffy::NodeId,
    /// X offset relative to the parent's content box.
    pub x: f32,
    /// Y offset relative to the parent's content box.
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Stacking order. `None` means `auto`.
    pub z_index: Option<i32>,
    /// The full computed style for this node (overflow, colors, etc.).
    pub computed_values: Option<Arc<style::properties::ComputedValues>>,
    /// Whether this node is a text leaf node.
    pub is_text: bool,
    /// Text content for text nodes (`None` for element nodes).
    pub text_content: Option<String>,
    pub children: Vec<LayoutBox>,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            node_id: taffy::NodeId::from(0_u64),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            z_index: None,
            computed_values: None,
            is_text: false,
            text_content: None,
            children: Vec::new(),
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────

/// Computes layout for a subtree rooted at `root_id`.
///
/// Layout data is written directly onto DOM nodes (`PawsElement` fields:
/// `layout_cache`, `unrounded_layout`, `final_layout`). Text leaf nodes are
/// measured via the `Document`'s embedded [`TextLayoutContext`].
pub fn compute_layout(doc: &mut Document, root_id: NodeId) -> Option<LayoutBox> {
    // Bail early if the root node has no style.
    doc.get_node(root_id).and_then(|n| n.taffy_style.as_ref())?;

    compute_root_layout(doc, root_id, Size::MAX_CONTENT);
    round_layout(doc, root_id);
    extract_layout_tree(doc, root_id)
}

// ─── ChildIter ───────────────────────────────────────────────────────

/// Zero-allocation iterator over a node's children for Taffy layout traversal.
///
/// Wraps a slice iterator directly — no Vec allocation per traversal call.
pub struct ChildIter<'a>(std::slice::Iter<'a, NodeId>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

// ─── TraversePartialTree ─────────────────────────────────────────────

impl taffy::TraversePartialTree for Document {
    type ChildIter<'a> = ChildIter<'a>;

    #[inline]
    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.node(parent_node_id).children.iter())
    }

    #[inline]
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.node(parent_node_id).children.len()
    }

    #[inline]
    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        self.node(parent_node_id).children[child_index]
    }
}

// ─── TraverseTree (marker) ───────────────────────────────────────────

impl taffy::TraverseTree for Document {}

// ─── LayoutPartialTree ───────────────────────────────────────────────

impl taffy::LayoutPartialTree for Document {
    type CoreContainerStyle<'a> = &'a taffy::Style;

    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("node must have taffy_style for layout")
    }

    fn resolve_calc_value(&self, val: *const (), basis: f32) -> f32 {
        // SAFETY: `val` was created by `CompactLength::calc(ptr)` in
        // `style::convert::length::length_percentage()`, where `ptr` is a
        // `*const CalcLengthPercentage`. The pointee remains live because the
        // `ComputedValues` (which owns it via `Arc`) are borrowed through the
        // `Document` reference held during the layout pass.
        let calc = unsafe { &*(val as *const CalcLengthPercentage) };
        calc.resolve(CSSPixelLength::new(basis)).px()
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_mut(node_id).unrounded_layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |doc, node_id, inputs| {
            let node = doc.node(node_id);
            let style = node
                .taffy_style
                .as_ref()
                .expect("node must have taffy_style");
            let display = style.display;
            let is_text = node.node_type == NodeType::Text;
            let has_children = !node.children.is_empty();

            if display == Display::None {
                return compute_hidden_layout(doc, node_id);
            }

            if is_text {
                return compute_text_leaf(doc, node_id, inputs);
            }

            if !has_children {
                return compute_leaf_layout(
                    inputs,
                    style,
                    |val, basis| doc.resolve_calc_value(val, basis),
                    |_known_dimensions, _available_space| Size::ZERO,
                );
            }

            match display {
                Display::Flex => compute_flexbox_layout(doc, node_id, inputs),
                Display::Grid => compute_grid_layout(doc, node_id, inputs),
                _ => compute_block_layout(doc, node_id, inputs),
            }
        })
    }
}

/// Computes layout for a text leaf node using the Parley text layout context.
fn compute_text_leaf(doc: &mut Document, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
    let node = doc.node(node_id);
    let (font_size, font_weight) = node
        .computed_values
        .as_ref()
        .map(|cv| {
            let fs = cv.clone_font_size().computed_size().px();
            let fs = if fs > 0.0 { fs } else { FONT_MEDIUM_PX };
            let fw = cv.clone_font_weight().value();
            (fs, fw)
        })
        .unwrap_or((FONT_MEDIUM_PX, 400.0));
    let text = node.text_content.as_deref().unwrap_or("");

    let max_width = inputs
        .known_dimensions
        .width
        .or(match inputs.available_space.width {
            AvailableSpace::Definite(w) => Some(w),
            AvailableSpace::MaxContent => None,
            AvailableSpace::MinContent => Some(0.0),
        });

    let (width, height) = doc
        .text_cx
        .measure_text(text, font_size, font_weight, max_width);

    let style = doc
        .node(node_id)
        .taffy_style
        .as_ref()
        .expect("text node must have taffy_style");

    compute_leaf_layout(
        inputs,
        style,
        |val, basis| doc.resolve_calc_value(val, basis),
        |_known_dimensions, _available_space| Size { width, height },
    )
}

// ─── CacheTree ───────────────────────────────────────────────────────

impl CacheTree for Document {
    fn cache_get(
        &self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
    ) -> Option<LayoutOutput> {
        self.node(node_id)
            .layout_cache
            .get(known_dimensions, available_space, run_mode)
    }

    fn cache_store(
        &mut self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
        layout_output: LayoutOutput,
    ) {
        self.node_mut(node_id).layout_cache.store(
            known_dimensions,
            available_space,
            run_mode,
            layout_output,
        );
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.node_mut(node_id).layout_cache.clear();
    }
}

// ─── LayoutFlexboxContainer ──────────────────────────────────────────

impl taffy::LayoutFlexboxContainer for Document {
    type FlexboxContainerStyle<'a> = &'a taffy::Style;

    type FlexboxItemStyle<'a> = &'a taffy::Style;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("flexbox container must have taffy_style")
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("flexbox child must have taffy_style")
    }
}

// ─── LayoutGridContainer ─────────────────────────────────────────────

impl taffy::LayoutGridContainer for Document {
    type GridContainerStyle<'a> = &'a taffy::Style;

    type GridItemStyle<'a> = &'a taffy::Style;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("grid container must have taffy_style")
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("grid child must have taffy_style")
    }
}

// ─── LayoutBlockContainer ────────────────────────────────────────────

impl taffy::LayoutBlockContainer for Document {
    type BlockContainerStyle<'a> = &'a taffy::Style;

    type BlockItemStyle<'a> = &'a taffy::Style;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("block container must have taffy_style")
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("block child must have taffy_style")
    }
}

// ─── RoundTree ───────────────────────────────────────────────────────

impl taffy::RoundTree for Document {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        self.node(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_mut(node_id).final_layout = *layout;
    }
}

// ─── Result extraction ───────────────────────────────────────────────

/// Recursively extracts the positioned layout tree from DOM nodes.
fn extract_layout_tree(doc: &Document, node_id: NodeId) -> Option<LayoutBox> {
    let node = doc.get_node(node_id)?;
    node.taffy_style.as_ref()?;

    let layout = &node.final_layout;

    let children: Vec<LayoutBox> = node
        .children
        .iter()
        .filter_map(|&child_id| extract_layout_tree(doc, child_id))
        .collect();

    // Extract z-index and computed values from the DOM node.
    let (z_index, computed_values) = node
        .computed_values
        .as_ref()
        .map(|cv| {
            use style::values::generics::position::ZIndex;
            let z = match cv.clone_z_index() {
                ZIndex::Integer(n) => Some(n),
                ZIndex::Auto => None,
            };
            (z, Some(Arc::clone(cv)))
        })
        .unwrap_or((None, None));

    let is_text = node.node_type == NodeType::Text;
    let text_content = if is_text {
        node.text_content.clone()
    } else {
        None
    };

    Some(LayoutBox {
        node_id,
        x: layout.location.x,
        y: layout.location.y,
        width: layout.size.width,
        height: layout.size.height,
        z_index,
        computed_values,
        is_text,
        text_content,
        children,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeState;
    use markup5ever::QualName;
    use style::shared_lock::SharedRwLock;
    use url::Url;

    #[test]
    fn test_compute_layout_extract_tree() {
        let guard = SharedRwLock::new();
        let mut doc = Document::new(guard, Url::parse("http://test.com").unwrap());

        let elem1 = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, elem1).unwrap();

        let elem2 = doc.create_element(QualName::new(None, "".into(), "span".into()));
        doc.append_child(elem1, elem2).unwrap();

        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        doc.resolve_style(&style_ctx);

        let layout = compute_layout(&mut doc, elem1);
        assert!(layout.is_some());
        let layout = layout.unwrap();
        assert_eq!(layout.children.len(), 1);
    }

    #[test]
    fn test_layout_no_style_returns_none() {
        let guard = SharedRwLock::new();
        let mut doc = Document::new(guard, Url::parse("http://test.com").unwrap());

        // Don't resolve styles — taffy_style will be None.
        let el = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, el).unwrap();
        assert!(compute_layout(&mut doc, el).is_none());
    }

    #[test]
    fn test_layout_display_none_zero_size() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let parent = state.create_element("div".to_string());
        state.append_element(0, parent).unwrap();
        state
            .set_inline_style(parent, "display".into(), "flex".into())
            .unwrap();

        let hidden = state.create_element("div".to_string());
        state.append_element(parent, hidden).unwrap();
        state
            .set_inline_style(hidden, "display".into(), "none".into())
            .unwrap();

        let visible = state.create_element("div".to_string());
        state.append_element(parent, visible).unwrap();
        state
            .set_inline_style(visible, "width".into(), "50px".into())
            .unwrap();
        state
            .set_inline_style(visible, "height".into(), "50px".into())
            .unwrap();

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(parent as u64)).unwrap();

        // Hidden child should have zero dimensions.
        let hidden_box = &layout.children[0];
        assert_eq!(hidden_box.width, 0.0);
        assert_eq!(hidden_box.height, 0.0);
    }

    #[test]
    fn test_layout_grid_container() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let grid = state.create_element("div".to_string());
        state.append_element(0, grid).unwrap();
        state
            .set_inline_style(grid, "display".into(), "grid".into())
            .unwrap();

        let child = state.create_element("div".to_string());
        state.append_element(grid, child).unwrap();
        state
            .set_inline_style(child, "width".into(), "80px".into())
            .unwrap();
        state
            .set_inline_style(child, "height".into(), "40px".into())
            .unwrap();

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(grid as u64)).unwrap();
        assert_eq!(layout.children.len(), 1);
        assert!(layout.height > 0.0, "grid should have positive height");
    }

    #[test]
    fn test_layout_childless_element() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "width".into(), "100px".into())
            .unwrap();
        state
            .set_inline_style(el, "height".into(), "60px".into())
            .unwrap();

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(el as u64)).unwrap();
        assert_eq!(layout.width, 100.0);
        assert_eq!(layout.height, 60.0);
        assert!(layout.children.is_empty());
    }

    #[test]
    fn test_layout_block_container_with_child() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let block = state.create_element("div".to_string());
        state.append_element(0, block).unwrap();
        state
            .set_inline_style(block, "display".into(), "block".into())
            .unwrap();
        state
            .set_inline_style(block, "width".into(), "200px".into())
            .unwrap();

        let child = state.create_element("div".to_string());
        state.append_element(block, child).unwrap();
        state
            .set_inline_style(child, "height".into(), "30px".into())
            .unwrap();

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(block as u64)).unwrap();
        assert_eq!(layout.width, 200.0);
        assert_eq!(layout.children.len(), 1);
        assert_eq!(layout.children[0].height, 30.0);
    }

    #[test]
    fn test_layout_flex_with_childless_leaf() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let flex = state.create_element("div".to_string());
        state.append_element(0, flex).unwrap();
        state
            .set_inline_style(flex, "display".into(), "flex".into())
            .unwrap();

        // Childless leaf — exercises the !has_children branch in compute_child_layout.
        let leaf = state.create_element("div".to_string());
        state.append_element(flex, leaf).unwrap();
        // No children, no text — pure leaf.

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(flex as u64)).unwrap();
        assert_eq!(layout.children.len(), 1);
    }

    #[test]
    fn test_layout_text_node_measured() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state
            .set_inline_style(div, "width".into(), "200px".into())
            .unwrap();

        let text = state.create_text_node("Hello Paws".to_string());
        state.append_element(div, text).unwrap();

        state.doc.resolve_style(&state.style_context);
        let layout = compute_layout(&mut state.doc, NodeId::from(div as u64)).unwrap();

        // Text child should be present with measured dimensions.
        assert_eq!(layout.children.len(), 1);
        let text_box = &layout.children[0];
        assert!(text_box.is_text, "child should be a text node");
        assert_eq!(text_box.text_content.as_deref(), Some("Hello Paws"));
        assert!(text_box.width > 0.0, "text should have positive width");
        assert!(text_box.height > 0.0, "text should have positive height");
    }

    #[test]
    fn test_commit_with_text_node() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state
            .set_inline_style(div, "display".into(), "block".into())
            .unwrap();
        state
            .set_inline_style(div, "width".into(), "300px".into())
            .unwrap();

        let text = state.create_text_node("Layout test".to_string());
        state.append_element(div, text).unwrap();

        let layout = state.commit();
        assert!(layout.width > 0.0);
        assert!(!layout.children.is_empty(), "div should have text child");
        let text_child = &layout.children[0];
        assert!(text_child.is_text);
        assert_eq!(text_child.text_content.as_deref(), Some("Layout test"));
    }
}
