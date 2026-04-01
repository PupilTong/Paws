//! Layout computation via Taffy's `LayoutPartialTree` trait hierarchy.
//!
//! Taffy's traits are implemented directly on [`Document`] (the "fat tree"
//! pattern). Layout data (cache, unrounded/final layouts) lives on
//! [`PawsElement`] for persistence across passes (future CSS Containment).

use style::servo_arc::Arc;

use crate::dom::document::Document;
use crate::layout::text::TextMeasurer;

use taffy::prelude::*;
use taffy::{compute_root_layout, round_layout};

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

    // SAFETY: `text_measurer` is borrowed for the duration of this function.
    // We store it as a raw pointer on `doc` so the Taffy trait impls can
    // access it. The `transmute` erases the trait-object lifetime to `'static`
    // (required because `Document` has no lifetime parameter); the pointer is
    // cleared via the RAII guard before this function returns, even on panic.
    let ptr: *const dyn TextMeasurer = text_measurer;
    doc.text_measurer = Some(unsafe {
        std::mem::transmute::<*const dyn TextMeasurer, *const dyn TextMeasurer>(ptr)
    });

    // RAII guard ensures `text_measurer` is cleared even if layout panics.
    struct LayoutPassGuard<'a>(&'a mut Document);
    impl Drop for LayoutPassGuard<'_> {
        fn drop(&mut self) {
            self.0.text_measurer = None;
        }
    }
    let guard = LayoutPassGuard(doc);

    compute_root_layout(guard.0, root_id, Size::MAX_CONTENT);
    round_layout(guard.0, root_id);
    extract_layout_tree(guard.0, root_id)
    // guard drops here → text_measurer cleared
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
    use crate::dom::document::Document;
    use crate::layout::text::MockTextMeasurer;
    use crate::runtime::RuntimeState;
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

    #[test]
    fn test_layout_no_style_returns_none() {
        let guard = SharedRwLock::new();
        let mut doc = Document::new(guard, Url::parse("http://test.com").unwrap());
        let measurer = MockTextMeasurer;
        // Don't resolve styles — taffy_style will be None.
        let el = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, el).unwrap();
        assert!(compute_layout(&mut doc, el, &measurer).is_none());
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
        let layout = compute_layout(
            &mut state.doc,
            NodeId::from(parent as u64),
            &MockTextMeasurer,
        )
        .unwrap();

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
        let layout =
            compute_layout(&mut state.doc, NodeId::from(grid as u64), &MockTextMeasurer).unwrap();
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
        let layout =
            compute_layout(&mut state.doc, NodeId::from(el as u64), &MockTextMeasurer).unwrap();
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
        let layout = compute_layout(
            &mut state.doc,
            NodeId::from(block as u64),
            &MockTextMeasurer,
        )
        .unwrap();
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
        let layout =
            compute_layout(&mut state.doc, NodeId::from(flex as u64), &MockTextMeasurer).unwrap();
        assert_eq!(layout.children.len(), 1);
    }
}
