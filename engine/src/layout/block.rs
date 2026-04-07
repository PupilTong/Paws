//! Layout computation via Taffy's `LayoutPartialTree` trait hierarchy.
//!
//! Taffy's traits are implemented directly on [`Document`] (the "fat tree"
//! pattern). Layout data (cache, unrounded/final layouts) lives on
//! [`PawsElement`] for persistence across passes (future CSS Containment).

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

// ─── Public API ──────────────────────────────────────────────────────

/// Computes layout in-place on the Document tree.
///
/// Layout data is written directly onto DOM nodes (`PawsElement` fields:
/// `layout_cache`, `unrounded_layout`, `final_layout`). This is the preferred
/// API — use [`compute_layout`] only if you need a detached `LayoutBox` tree.
///
/// Returns `true` if layout was computed, `false` if the root node has no style.
pub fn compute_layout_in_place<S: Default + Send + 'static>(
    doc: &mut Document<S>,
    root_id: NodeId,
) -> bool {
    if doc
        .get_node(root_id)
        .and_then(|n| n.taffy_style.as_ref())
        .is_none()
    {
        return false;
    }
    compute_root_layout(doc, root_id, Size::MAX_CONTENT);
    round_layout(doc, root_id);
    true
}

// ─── ChildIter ───────────────────────────────────────────────────────

/// Iterator over a node's children for Taffy layout traversal.
///
/// Uses a slice iterator for regular nodes (zero allocation) and an
/// owned vec iterator for shadow hosts (flat tree with slot replacement).
pub enum ChildIter<'a> {
    /// Zero-allocation slice iterator for regular DOM nodes.
    Slice(std::slice::Iter<'a, NodeId>),
    /// Owned iterator for shadow host flat tree children.
    Owned(std::vec::IntoIter<NodeId>),
}

impl Iterator for ChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ChildIter::Slice(iter) => iter.next().copied(),
            ChildIter::Owned(iter) => iter.next(),
        }
    }
}

// ─── TraversePartialTree ─────────────────────────────────────────────

impl<S: Default + Send + 'static> taffy::TraversePartialTree for Document<S> {
    type ChildIter<'a> = ChildIter<'a>;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        let node = self.node(parent_node_id);
        if let Some(sr_id) = node.shadow_root_id {
            // Shadow host: build flat tree children from the shadow root,
            // replacing <slot> elements with their assigned light DOM children.
            let flat = self.flatten_shadow_children(sr_id);
            ChildIter::Owned(flat.into_iter())
        } else {
            ChildIter::Slice(node.children.iter())
        }
    }

    fn child_count(&self, parent_node_id: NodeId) -> usize {
        let node = self.node(parent_node_id);
        if let Some(sr_id) = node.shadow_root_id {
            self.flatten_shadow_children(sr_id).len()
        } else {
            node.children.len()
        }
    }

    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        let node = self.node(parent_node_id);
        if let Some(sr_id) = node.shadow_root_id {
            self.flatten_shadow_children(sr_id)[child_index]
        } else {
            node.children[child_index]
        }
    }
}

impl<S: Default + Send + 'static> Document<S> {
    /// Builds the flat tree children for a shadow root, replacing `<slot>`
    /// elements with their assigned light DOM children (or the slot's own
    /// children as fallback content).
    fn flatten_shadow_children(&self, sr_id: NodeId) -> Vec<NodeId> {
        let sr = self.node(sr_id);
        let mut result = Vec::with_capacity(sr.children.len());
        for &child_id in &sr.children {
            let child = self.node(child_id);
            if child.is_slot_element() {
                if child.assigned_nodes.is_empty() {
                    // Fallback: use the slot's own children
                    result.extend_from_slice(&child.children);
                } else {
                    // Distribute assigned light DOM children
                    result.extend_from_slice(&child.assigned_nodes);
                }
            } else {
                result.push(child_id);
            }
        }
        result
    }
}

// ─── TraverseTree (marker) ───────────────────────────────────────────

impl<S: Default + Send + 'static> taffy::TraverseTree for Document<S> {}

// ─── LayoutPartialTree ───────────────────────────────────────────────

impl<S: Default + Send + 'static> taffy::LayoutPartialTree for Document<S> {
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
            // Shadow hosts may have no direct children but have shadow tree children.
            let has_children = if node.shadow_root_id.is_some() {
                doc.child_count(node_id) > 0
            } else {
                !node.children.is_empty()
            };

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
fn compute_text_leaf<S: Default + Send + 'static>(
    doc: &mut Document<S>,
    node_id: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
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

impl<S: Default + Send + 'static> CacheTree for Document<S> {
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

impl<S: Default + Send + 'static> taffy::LayoutFlexboxContainer for Document<S> {
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

impl<S: Default + Send + 'static> taffy::LayoutGridContainer for Document<S> {
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

impl<S: Default + Send + 'static> taffy::LayoutBlockContainer for Document<S> {
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

impl<S: Default + Send + 'static> taffy::RoundTree for Document<S> {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        self.node(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_mut(node_id).final_layout = *layout;
    }
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
    fn test_compute_layout_in_place() {
        let guard = SharedRwLock::new();
        let mut doc: Document = Document::new(guard, Url::parse("http://test.com").unwrap());

        let elem1 = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, elem1).unwrap();

        let elem2 = doc.create_element(QualName::new(None, "".into(), "span".into()));
        doc.append_child(elem1, elem2).unwrap();

        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        doc.resolve_style(&style_ctx);

        assert!(compute_layout_in_place(&mut doc, elem1));
        let node = doc.get_node(elem1).unwrap();
        // elem1 has one styled child (elem2).
        assert_eq!(
            node.children
                .iter()
                .filter(|&&c| doc.get_node(c).is_some_and(|n| n.has_style()))
                .count(),
            1
        );
    }

    #[test]
    fn test_layout_no_style_returns_false() {
        let guard = SharedRwLock::new();
        let mut doc: Document = Document::new(guard, Url::parse("http://test.com").unwrap());

        // Don't resolve styles — taffy_style will be None.
        let el = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, el).unwrap();
        assert!(!compute_layout_in_place(&mut doc, el));
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
        compute_layout_in_place(&mut state.doc, NodeId::from(parent as u64));

        // Hidden child should have zero dimensions.
        let hidden_node = state.doc.get_node(NodeId::from(hidden as u64)).unwrap();
        assert_eq!(hidden_node.layout().size.width, 0.0);
        assert_eq!(hidden_node.layout().size.height, 0.0);
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
        compute_layout_in_place(&mut state.doc, NodeId::from(grid as u64));

        let grid_node = state.doc.get_node(NodeId::from(grid as u64)).unwrap();
        assert!(
            grid_node.layout().size.height > 0.0,
            "grid should have positive height"
        );
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
        compute_layout_in_place(&mut state.doc, NodeId::from(el as u64));

        let node = state.doc.get_node(NodeId::from(el as u64)).unwrap();
        assert_eq!(node.layout().size.width, 100.0);
        assert_eq!(node.layout().size.height, 60.0);
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
        compute_layout_in_place(&mut state.doc, NodeId::from(block as u64));

        let block_node = state.doc.get_node(NodeId::from(block as u64)).unwrap();
        assert_eq!(block_node.layout().size.width, 200.0);

        let child_node = state.doc.get_node(NodeId::from(child as u64)).unwrap();
        assert_eq!(child_node.layout().size.height, 30.0);
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

        state.doc.resolve_style(&state.style_context);
        assert!(compute_layout_in_place(
            &mut state.doc,
            NodeId::from(flex as u64)
        ));
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
        compute_layout_in_place(&mut state.doc, NodeId::from(div as u64));

        let text_node = state.doc.get_node(NodeId::from(text as u64)).unwrap();
        assert!(text_node.is_text_node(), "child should be a text node");
        assert_eq!(text_node.text_content.as_deref(), Some("Hello Paws"));
        assert!(
            text_node.layout().size.width > 0.0,
            "text should have positive width"
        );
        assert!(
            text_node.layout().size.height > 0.0,
            "text should have positive height"
        );
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

        state.commit();

        let div_node = state.doc.get_node(NodeId::from(div as u64)).unwrap();
        assert!(div_node.layout().size.width > 0.0);

        let text_node = state.doc.get_node(NodeId::from(text as u64)).unwrap();
        assert!(text_node.is_text_node());
        assert_eq!(text_node.text_content.as_deref(), Some("Layout test"));
    }
}
