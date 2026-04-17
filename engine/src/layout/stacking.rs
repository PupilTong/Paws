//! CSS stacking context detection and paint-order sorting.
//!
//! Implements CSS2.1 Appendix E paint ordering within stacking contexts.
//! [`creates_stacking_context`] determines whether a node establishes a new
//! stacking context based on its computed style, and
//! [`paint_order_children`] returns a node's children sorted in correct
//! paint order.

use crate::runtime::RenderState;
use style::properties::ComputedValues;

use crate::dom::document::Document;
use crate::dom::PawsElement;

// ─── Stacking context detection ─────────────────────────────────────

/// Determines whether a node creates a CSS stacking context.
///
/// Checks all CSS2.1 and CSS3 triggers (short-circuits on first match).
/// The `is_root` and `is_flex_or_grid_item` flags are passed by the caller
/// because they depend on tree position rather than the node's own style.
///
/// Reference: <https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_positioned_layout/Stacking_context>
pub(crate) fn creates_stacking_context(
    computed_values: &ComputedValues,
    is_root: bool,
    is_flex_or_grid_item: bool,
) -> bool {
    use style::properties::longhands::position::computed_value::T as Position;
    use style::values::generics::position::ZIndex;

    if is_root {
        return true;
    }

    let position = computed_values.clone_position();
    let z_index = computed_values.clone_z_index();
    let has_z_index = !matches!(z_index, ZIndex::Auto);

    // position: fixed | sticky always create a stacking context.
    if matches!(position, Position::Fixed | Position::Sticky) {
        return true;
    }

    // Positioned element (absolute/relative) with z-index != auto.
    if matches!(position, Position::Absolute | Position::Relative) && has_z_index {
        return true;
    }

    // Flex/grid item with z-index != auto (no position required).
    if is_flex_or_grid_item && has_z_index {
        return true;
    }

    // opacity < 1  (clone_opacity() returns f32 directly)
    if computed_values.clone_opacity() < 1.0 {
        return true;
    }

    // transform is not none
    if !computed_values.clone_transform().0.is_empty() {
        return true;
    }

    // filter is not none
    if !computed_values.clone_filter().0.is_empty() {
        return true;
    }

    // backdrop-filter is not none
    if !computed_values.clone_backdrop_filter().0.is_empty() {
        return true;
    }

    // perspective is not none
    {
        use style::values::generics::box_::GenericPerspective;
        if !matches!(
            computed_values.clone_perspective(),
            GenericPerspective::None
        ) {
            return true;
        }
    }

    // clip-path is not none
    {
        use style::values::computed::basic_shape::ClipPath;
        if !matches!(computed_values.clone_clip_path(), ClipPath::None) {
            return true;
        }
    }

    // mix-blend-mode is not normal
    {
        use style::properties::longhands::mix_blend_mode::computed_value::T as MixBlendMode;
        if !matches!(computed_values.clone_mix_blend_mode(), MixBlendMode::Normal) {
            return true;
        }
    }

    // isolation: isolate
    {
        use style::properties::longhands::isolation::computed_value::T as Isolation;
        if matches!(computed_values.clone_isolation(), Isolation::Isolate) {
            return true;
        }
    }

    // contain: layout | paint (or strict/content which include these)
    {
        use style::values::specified::box_::Contain;
        let contain = computed_values.clone_contain();
        if contain.contains(Contain::LAYOUT) || contain.contains(Contain::PAINT) {
            return true;
        }
    }

    // container-type is not normal
    if !computed_values.clone_container_type().is_normal() {
        return true;
    }

    // will-change creating a stacking context
    {
        use style::values::specified::box_::WillChangeBits;
        if computed_values
            .clone_will_change()
            .bits
            .contains(WillChangeBits::STACKING_CONTEXT_UNCONDITIONAL)
        {
            return true;
        }
    }

    false
}

// ─── Paint-order sorting ────────────────────────────────────────────

/// Classifies a child into a CSS2.1 Appendix E paint layer.
///
/// Lower values paint first (further back):
/// - `0`: stacking context with negative z-index
/// - `1`: non-positioned in-flow (block + inline)
/// - `2`: positioned with z-index auto/0, SC with z-index 0
/// - `3`: stacking context with positive z-index
fn paint_layer<S: RenderState>(node: &PawsElement<S>) -> i8 {
    use style::properties::longhands::position::computed_value::T as Position;

    let z_index = node.z_index().unwrap_or(0);

    if node.creates_stacking_context && z_index < 0 {
        return 0;
    }

    let is_positioned = node
        .computed_values
        .as_ref()
        .is_some_and(|computed| !matches!(computed.clone_position(), Position::Static));

    if !is_positioned && !node.creates_stacking_context {
        return 1;
    }

    if z_index == 0 {
        return 2;
    }

    3
}

/// Returns a node's children sorted by CSS2.1 Appendix E paint order.
///
/// If the node creates a stacking context, children are sorted by
/// `(paint_layer, z_index)` with DOM-order tiebreaking (stable sort).
/// If the node does not create a stacking context, children are returned
/// in DOM order with a simple z-index sort (current behavior preserved).
///
/// Only styled children are returned.
pub fn paint_order_children<S: RenderState>(
    doc: &Document<S>,
    node_id: taffy::NodeId,
) -> Vec<taffy::NodeId> {
    let node = match doc.get_node(node_id) {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut children: Vec<taffy::NodeId> = node
        .children
        .iter()
        .copied()
        .filter(|&cid| doc.get_node(cid).is_some_and(|c| c.has_style()))
        .collect();

    if children.len() <= 1 {
        return children;
    }

    if node.creates_stacking_context {
        // Full CSS2.1 Appendix E paint-order sort (stable for DOM-order tiebreak).
        children.sort_by(|&a, &b| {
            let node_a = doc.get_node(a).unwrap();
            let node_b = doc.get_node(b).unwrap();
            paint_layer(node_a).cmp(&paint_layer(node_b)).then_with(|| {
                node_a
                    .z_index()
                    .unwrap_or(0)
                    .cmp(&node_b.z_index().unwrap_or(0))
            })
        });
    } else {
        // Non-SC node: simple z-index sort (preserves previous behavior).
        children.sort_by_key(|&cid| doc.get_node(cid).and_then(|n| n.z_index()).unwrap_or(0));
    }

    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeState;
    use taffy::prelude::TaffyMaxContent;

    /// Helper: create a styled child under `parent` and return its id.
    fn add_styled_child(state: &mut RuntimeState, parent: u32, styles: &[(&str, &str)]) -> u32 {
        let id = state.create_element("div".to_string());
        state.append_element(parent, id).unwrap();
        for &(prop, val) in styles {
            state
                .set_inline_style(id, prop.to_string(), val.to_string())
                .unwrap();
        }
        id
    }

    fn commit_and_layout(state: &mut RuntimeState, root: u32) {
        state.doc.resolve_style(&state.style_context);
        crate::layout::compute_layout_in_place(
            &mut state.doc,
            taffy::NodeId::from(root as u64),
            taffy::Size::MAX_CONTENT,
        );
    }

    // ── SC detection tests ─────────────────────────────────────────

    #[test]
    fn test_sc_positioned_z_index() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "1")],
        );
        commit_and_layout(&mut state, root);

        let child = state
            .doc
            .get_node(taffy::NodeId::from(root as u64))
            .unwrap()
            .children[0];
        assert!(
            state.doc.get_node(child).unwrap().creates_stacking_context,
            "position: relative + z-index: 1 should create SC"
        );
    }

    #[test]
    fn test_sc_positioned_auto_z_no_sc() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        add_styled_child(&mut state, root, &[("position", "relative")]);
        commit_and_layout(&mut state, root);

        let child = state
            .doc
            .get_node(taffy::NodeId::from(root as u64))
            .unwrap()
            .children[0];
        assert!(
            !state.doc.get_node(child).unwrap().creates_stacking_context,
            "position: relative + z-index: auto should NOT create SC"
        );
    }

    #[test]
    fn test_sc_opacity() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        add_styled_child(&mut state, root, &[("opacity", "0.5")]);
        commit_and_layout(&mut state, root);

        let child = state
            .doc
            .get_node(taffy::NodeId::from(root as u64))
            .unwrap()
            .children[0];
        assert!(
            state.doc.get_node(child).unwrap().creates_stacking_context,
            "opacity: 0.5 should create SC"
        );
    }

    #[test]
    fn test_sc_static_no_sc() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        add_styled_child(&mut state, root, &[("width", "50px")]);
        commit_and_layout(&mut state, root);

        let child = state
            .doc
            .get_node(taffy::NodeId::from(root as u64))
            .unwrap()
            .children[0];
        assert!(
            !state.doc.get_node(child).unwrap().creates_stacking_context,
            "static element should NOT create SC"
        );
    }

    #[test]
    fn test_sc_flex_item_z_index() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let flex = state.create_element("div".to_string());
        state.append_element(0, flex).unwrap();
        state
            .set_inline_style(flex, "display".into(), "flex".into())
            .unwrap();
        add_styled_child(&mut state, flex, &[("z-index", "1")]);
        commit_and_layout(&mut state, flex);

        let child = state
            .doc
            .get_node(taffy::NodeId::from(flex as u64))
            .unwrap()
            .children[0];
        assert!(
            state.doc.get_node(child).unwrap().creates_stacking_context,
            "flex item with z-index should create SC"
        );
    }

    #[test]
    fn test_root_creates_sc() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        commit_and_layout(&mut state, root);

        assert!(
            state
                .doc
                .get_node(taffy::NodeId::from(root as u64))
                .unwrap()
                .creates_stacking_context,
            "root element should always create SC"
        );
    }

    // ── Paint-order tests ──────────────────────────────────────────

    #[test]
    fn test_paint_order_negative_before_positive() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();

        let pos = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "5")],
        );
        let neg = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "-1")],
        );
        commit_and_layout(&mut state, root);

        let ordered = paint_order_children(&state.doc, taffy::NodeId::from(root as u64));
        let ids: Vec<u64> = ordered.iter().map(|&id| u64::from(id)).collect();
        assert_eq!(
            ids,
            vec![neg as u64, pos as u64],
            "negative z-index should paint before positive"
        );
    }

    #[test]
    fn test_paint_order_flow_before_positioned() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();

        let positioned = add_styled_child(
            &mut state,
            root,
            &[
                ("position", "relative"),
                ("z-index", "0"),
                ("width", "10px"),
            ],
        );
        let flow = add_styled_child(&mut state, root, &[("width", "20px")]);
        commit_and_layout(&mut state, root);

        let ordered = paint_order_children(&state.doc, taffy::NodeId::from(root as u64));
        let ids: Vec<u64> = ordered.iter().map(|&id| u64::from(id)).collect();
        assert_eq!(
            ids,
            vec![flow as u64, positioned as u64],
            "in-flow should paint before positioned"
        );
    }

    #[test]
    fn test_paint_order_dom_order_tiebreak() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();

        let a = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "1")],
        );
        let b = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "1")],
        );
        let c = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "1")],
        );
        commit_and_layout(&mut state, root);

        let ordered = paint_order_children(&state.doc, taffy::NodeId::from(root as u64));
        let ids: Vec<u64> = ordered.iter().map(|&id| u64::from(id)).collect();
        assert_eq!(
            ids,
            vec![a as u64, b as u64, c as u64],
            "same z-index should preserve DOM order"
        );
    }

    #[test]
    fn test_sc_isolation_correctness() {
        let mut state = RuntimeState::new("https://test.com".to_string());
        let root = state.create_element("div".to_string());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();

        let sc1 = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "1")],
        );
        add_styled_child(
            &mut state,
            sc1,
            &[("position", "relative"), ("z-index", "999")],
        );
        let sc2 = add_styled_child(
            &mut state,
            root,
            &[("position", "relative"), ("z-index", "2")],
        );
        commit_and_layout(&mut state, root);

        let ordered = paint_order_children(&state.doc, taffy::NodeId::from(root as u64));
        let ids: Vec<u64> = ordered.iter().map(|&id| u64::from(id)).collect();
        assert_eq!(
            ids,
            vec![sc1 as u64, sc2 as u64],
            "SC(z:1) before SC(z:2), grandchild z:999 must not escape"
        );
    }
}
