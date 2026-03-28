//! Layout computation via Taffy's `LayoutPartialTree` trait hierarchy.
//!
//! Instead of copying the DOM into an intermediate `TaffyTree`, we implement
//! Taffy's traits directly on a thin adapter ([`PawsLayoutTree`]) that wraps
//! `&mut Document`. Layout data (cache, unrounded/final layouts) lives on
//! [`PawsElement`] for persistence across passes (future CSS Containment).

use style::servo_arc::Arc;
use style::values::computed::length::CSSPixelLength;
use style::values::computed::length_percentage::CalcLengthPercentage;
use style::values::specified::font::FONT_MEDIUM_PX;

use crate::dom::document::Document;
use crate::dom::element::PawsElement;
use crate::dom::NodeType;
use crate::layout::text::TextMeasurer;

use taffy::prelude::*;
use taffy::tree::{Layout, LayoutInput, LayoutOutput};
use taffy::{
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_hidden_layout,
    compute_leaf_layout, compute_root_layout, round_layout, CacheTree,
};

use taffy::compute_block_layout;

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
            children: Vec::new(),
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────

/// Computes layout for a subtree rooted at `root_id`.
///
/// Layout data is written directly onto DOM nodes (`PawsElement` fields:
/// `layout_cache`, `unrounded_layout`, `final_layout`).
pub fn compute_layout(
    doc: &mut Document,
    root_id: NodeId,
    text_measurer: &dyn TextMeasurer,
) -> Option<LayoutBox> {
    // Bail early if the root node has no style.
    doc.get_node(root_id).and_then(|n| n.taffy_style.as_ref())?;

    let mut tree = PawsLayoutTree { doc, text_measurer };
    compute_root_layout(&mut tree, root_id, Size::MAX_CONTENT);
    round_layout(&mut tree, root_id);
    extract_layout_tree(tree.doc, root_id)
}

// ─── Adapter ─────────────────────────────────────────────────────────

/// Thin adapter implementing Taffy's layout traits over the DOM.
///
/// Borrows `Document` mutably for cache/layout writes. Short-lived: constructed
/// per layout pass in [`compute_layout`].
struct PawsLayoutTree<'a> {
    doc: &'a mut Document,
    text_measurer: &'a dyn TextMeasurer,
}

impl PawsLayoutTree<'_> {
    #[inline]
    fn node(&self, id: NodeId) -> &PawsElement {
        self.doc.get_node(id).expect("valid node id during layout")
    }

    #[inline]
    fn node_mut(&mut self, id: NodeId) -> &mut PawsElement {
        self.doc
            .get_node_mut(id)
            .expect("valid node id during layout")
    }
}

// ─── ChildIter ───────────────────────────────────────────────────────

/// Zero-allocation iterator over a node's children.
///
/// Wraps a slice iterator directly — no Vec allocation per traversal call.
/// All children of styled elements are expected to have `taffy_style` set
/// by `resolve_style`, so no filtering is needed.
struct ChildIter<'a>(std::slice::Iter<'a, NodeId>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

// ─── TraversePartialTree ─────────────────────────────────────────────

impl taffy::TraversePartialTree for PawsLayoutTree<'_> {
    type ChildIter<'a>
        = ChildIter<'a>
    where
        Self: 'a;

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

impl taffy::TraverseTree for PawsLayoutTree<'_> {}

// ─── LayoutPartialTree ───────────────────────────────────────────────

impl taffy::LayoutPartialTree for PawsLayoutTree<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

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
        // `Document` reference held by this adapter.
        let calc = unsafe { &*(val as *const CalcLengthPercentage) };
        calc.resolve(CSSPixelLength::new(basis)).px()
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_mut(node_id).unrounded_layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let node = tree.node(node_id);
            let style = node
                .taffy_style
                .as_ref()
                .expect("node must have taffy_style");
            let display = style.display;
            let is_text = node.node_type == NodeType::Text;
            let has_children = tree.child_count(node_id) > 0;

            if display == Display::None {
                return compute_hidden_layout(tree, node_id);
            }

            if is_text {
                return compute_text_leaf(tree, node_id, inputs);
            }

            if !has_children {
                // Non-text leaf with no children.
                let style = tree
                    .node(node_id)
                    .taffy_style
                    .as_ref()
                    .expect("node must have taffy_style");
                return compute_leaf_layout(
                    inputs,
                    style,
                    |val, basis| tree.resolve_calc_value(val, basis),
                    |_known_dimensions, _available_space| Size::ZERO,
                );
            }

            match display {
                Display::Flex => compute_flexbox_layout(tree, node_id, inputs),
                Display::Grid => compute_grid_layout(tree, node_id, inputs),
                _ => compute_block_layout(tree, node_id, inputs),
            }
        })
    }
}

/// Computes layout for a text leaf node using the text measurer.
fn compute_text_leaf(
    tree: &mut PawsLayoutTree<'_>,
    node_id: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let node = tree.node(node_id);
    let font_size = node
        .computed_values
        .as_ref()
        .map(|cv| {
            let fs = cv.clone_font_size().computed_size().px();
            if fs > 0.0 {
                fs
            } else {
                FONT_MEDIUM_PX
            }
        })
        .unwrap_or(FONT_MEDIUM_PX);
    let text = node.text_content.as_deref().unwrap_or("");

    // Pre-measure to get fixed dimensions (matching the previous eager approach).
    let (width, height) = tree.text_measurer.measure_text(text, font_size, None);

    let style = tree
        .node(node_id)
        .taffy_style
        .as_ref()
        .expect("text node must have taffy_style");

    compute_leaf_layout(
        inputs,
        style,
        |val, basis| tree.resolve_calc_value(val, basis),
        |_known_dimensions, _available_space| Size { width, height },
    )
}

// ─── CacheTree ───────────────────────────────────────────────────────

impl CacheTree for PawsLayoutTree<'_> {
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

impl taffy::LayoutFlexboxContainer for PawsLayoutTree<'_> {
    type FlexboxContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

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

impl taffy::LayoutGridContainer for PawsLayoutTree<'_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

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

impl taffy::LayoutBlockContainer for PawsLayoutTree<'_> {
    type BlockContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type BlockItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

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

impl taffy::RoundTree for PawsLayoutTree<'_> {
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

    Some(LayoutBox {
        node_id,
        x: layout.location.x,
        y: layout.location.y,
        width: layout.size.width,
        height: layout.size.height,
        z_index,
        computed_values,
        children,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::text::MockTextMeasurer;
    use markup5ever::QualName;
    use style::shared_lock::SharedRwLock;
    use url::Url;

    #[test]
    fn test_compute_layout_extract_tree() {
        let guard = SharedRwLock::new();
        let mut doc = Document::new(guard, Url::parse("http://test.com").unwrap());
        let measurer = MockTextMeasurer;

        let elem1 = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, elem1).unwrap();

        let elem2 = doc.create_element(QualName::new(None, "".into(), "span".into()));
        doc.append_child(elem1, elem2).unwrap();

        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        doc.resolve_style(&style_ctx);

        let layout = compute_layout(&mut doc, elem1, &measurer);
        assert!(layout.is_some());
        let layout = layout.unwrap();
        assert_eq!(layout.children.len(), 1);
    }
}
