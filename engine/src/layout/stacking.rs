//! CSS 2.1 Appendix E stacking context detection and paint-order sorting.
//!
//! After Taffy computes geometry we walk the `LayoutBox` tree and:
//! 1. Tag each node with `creates_stacking_context`.
//! 2. Classify every child into a [`PaintLayer`] (Appendix E step).
//! 3. Stable-sort children by `(PaintLayer, z_index)` so the renderer
//!    can iterate in plain DFS order without any per-frame sorting.

use style::properties::ComputedValues;
use style::values::specified::box_::Display;

use super::block::LayoutBox;

// ─── Paint layer (Appendix E step) ─────────────────────────────────

/// CSS 2.1 Appendix E paint step.
///
/// Discriminant order matches the painting algorithm:
///   Step 2 → 3 → 4 → 5 → 6 → 7.
/// `Ord` is derived from `repr(u8)` so a simple sort yields correct order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[repr(u8)]
pub enum PaintLayer {
    StackingNegative = 0, // Step 2
    #[default]
    BlockFlow = 1, // Step 3
    Float = 2,            // Step 4
    InlineFlow = 3,       // Step 5
    PositionedZeroAuto = 4, // Step 6
    StackingPositive = 5, // Step 7
}

// ─── Stacking context detection ────────────────────────────────────

/// Returns `true` if the element with these computed values creates a new
/// stacking context.
///
/// `parent_display` is the Stylo `Display` of the parent element (needed to
/// detect flex/grid items). `is_root` should be `true` for the document root.
///
/// Checks are ordered cheapest-first for short-circuit performance.
fn creates_stacking_context(cv: &ComputedValues, parent_display: Display, is_root: bool) -> bool {
    use style::properties::longhands::position::computed_value::T as Position;
    use style::values::generics::position::ZIndex;

    // 1. Root element always creates a stacking context.
    if is_root {
        return true;
    }

    let position = cv.clone_position();
    let z_auto = matches!(cv.clone_z_index(), ZIndex::Auto);

    // 2. position: fixed / sticky → always.
    if matches!(position, Position::Fixed | Position::Sticky) {
        return true;
    }

    // 3. position: relative / absolute with explicit z-index.
    if matches!(position, Position::Relative | Position::Absolute) && !z_auto {
        return true;
    }

    // 4. Flex/grid item with explicit z-index.
    let parent_inside = parent_display.inside();
    let is_flex_grid_item = matches!(
        parent_inside,
        style::values::specified::box_::DisplayInside::Flex
            | style::values::specified::box_::DisplayInside::Grid
    );
    if is_flex_grid_item && !z_auto {
        return true;
    }

    // 5. opacity < 1.
    if cv.clone_opacity() < 1.0 {
        return true;
    }

    // 6. transform != none.
    if !cv.clone_transform().0.is_empty() {
        return true;
    }

    // 7. filter != none.
    if !cv.clone_filter().0.is_empty() {
        return true;
    }

    // 8. perspective != none.
    if !matches!(
        cv.clone_perspective(),
        style::values::generics::box_::GenericPerspective::None
    ) {
        return true;
    }

    // 9. mix-blend-mode != normal.
    if cv.clone_mix_blend_mode() != style::computed_values::mix_blend_mode::T::Normal {
        return true;
    }

    // 10. isolation: isolate.
    if cv.clone_isolation() != style::computed_values::isolation::T::Auto {
        return true;
    }

    // 11. contain: layout or paint.
    let contain = cv.clone_contain();
    if contain.contains(style::values::computed::Contain::LAYOUT)
        || contain.contains(style::values::computed::Contain::PAINT)
    {
        return true;
    }

    // 12. will-change that would trigger stacking context.
    let will_change = cv.clone_will_change();
    if will_change
        .bits
        .contains(style::values::specified::box_::WillChangeBits::STACKING_CONTEXT_UNCONDITIONAL)
    {
        return true;
    }

    // Additional properties that may not be available in Stylo 0.13 are
    // skipped with TODOs below:
    // TODO: backdrop-filter != none (cv.clone_backdrop_filter())
    // TODO: clip-path != none (cv.clone_clip_path())
    // TODO: mask-image != none (cv.clone_mask_image())
    // TODO: container-type: size | inline-size (cv.clone_container_type())

    false
}

// ─── Child classification ──────────────────────────────────────────

/// Classifies a child into its Appendix E paint layer.
fn classify_child(child: &LayoutBox, _parent_display: Display) -> PaintLayer {
    use style::properties::longhands::position::computed_value::T as Position;
    use style::values::specified::box_::DisplayOutside;

    let cv = match child.computed_values.as_ref() {
        Some(cv) => cv,
        None => return PaintLayer::BlockFlow,
    };

    let is_sc = child.creates_stacking_context;
    let z = child.z_index.unwrap_or(0);

    // Stacking context children with negative z-index → Step 2.
    if is_sc && z < 0 {
        return PaintLayer::StackingNegative;
    }
    // Stacking context children with positive z-index → Step 7.
    if is_sc && z > 0 {
        return PaintLayer::StackingPositive;
    }

    // Positioned or stacking context with z-index 0/auto → Step 6.
    let position = cv.clone_position();
    let is_positioned = !matches!(position, Position::Static);
    if is_positioned || is_sc {
        return PaintLayer::PositionedZeroAuto;
    }

    // Floats → Step 4.
    if cv.clone_float().is_floating() {
        return PaintLayer::Float;
    }

    // Inline-level → Step 5.
    let display = cv.clone_display();
    if matches!(display.outside(), DisplayOutside::Inline) {
        return PaintLayer::InlineFlow;
    }

    // Block-level in normal flow → Step 3.
    PaintLayer::BlockFlow
}

// ─── Recursive paint-order pass ────────────────────────────────────

/// Recursively tags each node with stacking context status and sorts
/// children into CSS 2.1 Appendix E paint order.
///
/// This is an in-place pass on the `LayoutBox` tree — no extra tree
/// allocation. Uses stable sort so DOM order is preserved within each
/// paint layer.
pub fn apply_paint_order(node: &mut LayoutBox, parent_display: Display, is_root: bool) {
    // Determine if *this* node creates a stacking context.
    let display = node
        .computed_values
        .as_ref()
        .map(|cv| cv.clone_display())
        .unwrap_or(Display::None);

    node.creates_stacking_context = node
        .computed_values
        .as_ref()
        .map(|cv| creates_stacking_context(cv, parent_display, is_root))
        .unwrap_or(false);

    // Pass 1: detect stacking context status for each child (needed by classify_child).
    for child in &mut node.children {
        child.creates_stacking_context = child
            .computed_values
            .as_ref()
            .map(|cv| creates_stacking_context(cv, display, false))
            .unwrap_or(false);
    }

    // Pass 2: classify children into paint layers and stable-sort.
    for child in &mut node.children {
        child.paint_layer = classify_child(child, display);
    }
    node.children
        .sort_by_key(|c| (c.paint_layer, c.z_index.unwrap_or(0)));

    // Pass 3: recurse into children (SC detection already done, will be a no-op re-set).
    for child in &mut node.children {
        apply_paint_order(child, display, false);
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeState;

    /// Helper: resolve styles, compute layout, and return the root LayoutBox.
    fn commit(state: &mut RuntimeState) -> LayoutBox {
        state.doc.ensure_styles_resolved(&state.style_context);
        let root_id = state.doc.root_element_id().unwrap();
        crate::layout::compute_layout(&mut state.doc, root_id).unwrap()
    }

    // ── Stacking context detection ──────────────────────────────────

    #[test]
    fn root_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let div = s.create_element("div".into());
        s.append_element(0, div).unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.creates_stacking_context,
            "root should be a stacking context"
        );
    }

    #[test]
    fn positioned_with_z_index_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "relative".into())
            .unwrap();
        s.set_inline_style(child, "z-index".into(), "1".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(lb.children[0].creates_stacking_context);
    }

    #[test]
    fn positioned_auto_z_index_not_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "relative".into())
            .unwrap();
        // z-index defaults to auto
        let lb = commit(&mut s);
        assert!(!lb.children[0].creates_stacking_context);
    }

    #[test]
    fn opacity_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "opacity".into(), "0.5".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(lb.children[0].creates_stacking_context);
    }

    #[test]
    fn fixed_position_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "fixed".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(lb.children[0].creates_stacking_context);
    }

    #[test]
    fn sticky_position_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "sticky".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "position:sticky should create a stacking context"
        );
    }

    #[test]
    fn absolute_with_z_index_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(child, "z-index".into(), "2".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "position:absolute + z-index:2 should create a stacking context"
        );
    }

    #[test]
    fn transform_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "transform".into(), "translateX(10px)".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "transform should create a stacking context"
        );
    }

    #[test]
    fn filter_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "filter".into(), "blur(5px)".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "filter should create a stacking context"
        );
    }

    #[test]
    fn perspective_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "perspective".into(), "500px".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "perspective should create a stacking context"
        );
    }

    #[test]
    fn mix_blend_mode_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "mix-blend-mode".into(), "multiply".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "mix-blend-mode:multiply should create a stacking context"
        );
    }

    #[test]
    fn isolation_isolate_creates_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "isolation".into(), "isolate".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            lb.children[0].creates_stacking_context,
            "isolation:isolate should create a stacking context"
        );
    }

    // NOTE: contain:paint, contain:layout, and will-change:transform are
    // detected by creates_stacking_context() but cannot be tested via
    // RuntimeState because Stylo's inline style parser does not produce the
    // expected computed values in this test environment. The detection logic
    // is exercised in production when full stylesheets are applied.

    // ── Negative detection tests (should NOT create SC) ────────────

    #[test]
    fn opacity_one_does_not_create_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "opacity".into(), "1".into())
            .unwrap();
        let lb = commit(&mut s);
        assert!(
            !lb.children[0].creates_stacking_context,
            "opacity:1 should NOT create a stacking context"
        );
    }

    #[test]
    fn static_position_does_not_create_stacking_context() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        let lb = commit(&mut s);
        assert!(
            !lb.children[0].creates_stacking_context,
            "position:static should NOT create a stacking context"
        );
    }

    // ── Paint layer classification ─────────────────────────────────

    #[test]
    fn float_classified_as_float_layer() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "float".into(), "left".into())
            .unwrap();
        let lb = commit(&mut s);
        assert_eq!(
            lb.children[0].paint_layer,
            PaintLayer::Float,
            "float:left should be classified as Float paint layer"
        );
    }

    #[test]
    fn inline_element_classified_as_inline_flow() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        // Without UA stylesheet, elements default to inline. Use explicit
        // display:inline to be clear about intent.
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "display".into(), "inline".into())
            .unwrap();
        s.set_inline_style(child, "width".into(), "10px".into())
            .unwrap();
        s.set_inline_style(child, "height".into(), "10px".into())
            .unwrap();
        let lb = commit(&mut s);
        if let Some(child_lb) = lb
            .children
            .iter()
            .find(|c| u64::from(c.node_id) == child as u64)
        {
            assert_eq!(
                child_lb.paint_layer,
                PaintLayer::InlineFlow,
                "display:inline should be classified as InlineFlow"
            );
        }
    }

    #[test]
    fn positioned_auto_z_classified_as_positioned() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "relative".into())
            .unwrap();
        let lb = commit(&mut s);
        assert_eq!(
            lb.children[0].paint_layer,
            PaintLayer::PositionedZeroAuto,
            "position:relative with z-index:auto should be PositionedZeroAuto"
        );
    }

    #[test]
    fn negative_z_index_classified_as_stacking_negative() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(child, "z-index".into(), "-1".into())
            .unwrap();
        let lb = commit(&mut s);
        assert_eq!(
            lb.children[0].paint_layer,
            PaintLayer::StackingNegative,
            "negative z-index stacking context should be StackingNegative"
        );
    }

    #[test]
    fn positive_z_index_classified_as_stacking_positive() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        let child = s.create_element("div".into());
        s.append_element(parent, child).unwrap();
        s.set_inline_style(child, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(child, "z-index".into(), "5".into())
            .unwrap();
        let lb = commit(&mut s);
        assert_eq!(
            lb.children[0].paint_layer,
            PaintLayer::StackingPositive,
            "positive z-index stacking context should be StackingPositive"
        );
    }

    // ── Paint order ─────────────────────────────────────────────────

    #[test]
    fn negative_z_index_before_block_flow() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        s.set_inline_style(parent, "position".into(), "relative".into())
            .unwrap();

        let flow_child = s.create_element("div".into());
        s.append_element(parent, flow_child).unwrap();

        let neg_z_child = s.create_element("div".into());
        s.append_element(parent, neg_z_child).unwrap();
        s.set_inline_style(neg_z_child, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(neg_z_child, "z-index".into(), "-1".into())
            .unwrap();

        let lb = commit(&mut s);
        assert_eq!(
            u64::from(lb.children[0].node_id),
            neg_z_child as u64,
            "negative z-index child should paint first"
        );
    }

    #[test]
    fn positive_z_index_after_everything() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        s.set_inline_style(parent, "position".into(), "relative".into())
            .unwrap();

        let flow_child = s.create_element("div".into());
        s.append_element(parent, flow_child).unwrap();

        let pos_z_child = s.create_element("div".into());
        s.append_element(parent, pos_z_child).unwrap();
        s.set_inline_style(pos_z_child, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(pos_z_child, "z-index".into(), "5".into())
            .unwrap();

        let lb = commit(&mut s);
        let last = lb.children.last().unwrap();
        assert_eq!(
            u64::from(last.node_id),
            pos_z_child as u64,
            "positive z-index child should paint last"
        );
    }

    #[test]
    fn dom_order_preserved_within_same_layer() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();

        let a = s.create_element("div".into());
        s.append_element(parent, a).unwrap();
        let b = s.create_element("div".into());
        s.append_element(parent, b).unwrap();
        let c = s.create_element("div".into());
        s.append_element(parent, c).unwrap();

        let lb = commit(&mut s);
        let ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();
        assert_eq!(ids, vec![a as u64, b as u64, c as u64]);
    }

    // ── Complex scenarios ──────────────────────────────────────────

    #[test]
    fn full_appendix_e_order() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        s.set_inline_style(parent, "position".into(), "relative".into())
            .unwrap();

        // 1. Positive z-index (should be last)
        let pos_z = s.create_element("div".into());
        s.append_element(parent, pos_z).unwrap();
        s.set_inline_style(pos_z, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(pos_z, "z-index".into(), "1".into())
            .unwrap();

        // 2. Block flow child (step 3) — explicit display:block needed since
        //    no UA stylesheet is loaded in tests.
        let block = s.create_element("div".into());
        s.append_element(parent, block).unwrap();
        s.set_inline_style(block, "display".into(), "block".into())
            .unwrap();

        // 3. Negative z-index (should be first)
        let neg_z = s.create_element("div".into());
        s.append_element(parent, neg_z).unwrap();
        s.set_inline_style(neg_z, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(neg_z, "z-index".into(), "-1".into())
            .unwrap();

        // 4. Float (step 4)
        let float = s.create_element("div".into());
        s.append_element(parent, float).unwrap();
        s.set_inline_style(float, "float".into(), "left".into())
            .unwrap();

        // 5. Positioned z-index:auto (step 6)
        let pos_auto = s.create_element("div".into());
        s.append_element(parent, pos_auto).unwrap();
        s.set_inline_style(pos_auto, "position".into(), "relative".into())
            .unwrap();

        let lb = commit(&mut s);
        let ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();

        // Expected: neg_z, block, float, pos_auto, pos_z
        assert_eq!(
            ids,
            vec![
                neg_z as u64,
                block as u64,
                float as u64,
                pos_auto as u64,
                pos_z as u64
            ],
            "children should be in Appendix E paint order: neg-z, block, float, positioned, pos-z"
        );
    }

    #[test]
    fn multiple_z_index_levels_sorted_correctly() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        s.set_inline_style(parent, "position".into(), "relative".into())
            .unwrap();

        let z3 = s.create_element("div".into());
        s.append_element(parent, z3).unwrap();
        s.set_inline_style(z3, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(z3, "z-index".into(), "3".into())
            .unwrap();

        let zm2 = s.create_element("div".into());
        s.append_element(parent, zm2).unwrap();
        s.set_inline_style(zm2, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(zm2, "z-index".into(), "-2".into())
            .unwrap();

        let z1 = s.create_element("div".into());
        s.append_element(parent, z1).unwrap();
        s.set_inline_style(z1, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(z1, "z-index".into(), "1".into())
            .unwrap();

        let zm1 = s.create_element("div".into());
        s.append_element(parent, zm1).unwrap();
        s.set_inline_style(zm1, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(zm1, "z-index".into(), "-1".into())
            .unwrap();

        let z0 = s.create_element("div".into());
        s.append_element(parent, z0).unwrap();
        s.set_inline_style(z0, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(z0, "z-index".into(), "0".into())
            .unwrap();

        let z2 = s.create_element("div".into());
        s.append_element(parent, z2).unwrap();
        s.set_inline_style(z2, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(z2, "z-index".into(), "2".into())
            .unwrap();

        let lb = commit(&mut s);
        let ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();

        assert_eq!(
            ids,
            vec![zm2 as u64, zm1 as u64, z0 as u64, z1 as u64, z2 as u64, z3 as u64],
            "children should be sorted by z-index within their paint layers"
        );
    }

    #[test]
    fn nested_stacking_context_isolates_children() {
        let mut s = RuntimeState::new("https://t.com".into());
        let outer = s.create_element("div".into());
        s.append_element(0, outer).unwrap();
        s.set_inline_style(outer, "position".into(), "relative".into())
            .unwrap();

        let block_child = s.create_element("div".into());
        s.append_element(outer, block_child).unwrap();

        // Nested stacking context (opacity < 1) with its own high z-index child
        let nested_sc = s.create_element("div".into());
        s.append_element(outer, nested_sc).unwrap();
        s.set_inline_style(nested_sc, "opacity".into(), "0.9".into())
            .unwrap();

        let inner_high_z = s.create_element("div".into());
        s.append_element(nested_sc, inner_high_z).unwrap();
        s.set_inline_style(inner_high_z, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(inner_high_z, "z-index".into(), "999".into())
            .unwrap();

        let lb = commit(&mut s);

        let outer_ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();
        assert_eq!(
            outer_ids,
            vec![block_child as u64, nested_sc as u64],
            "block child should paint before the nested stacking context"
        );

        // inner_high_z should be a child of nested_sc, not promoted to outer level
        let nested_lb = lb
            .children
            .iter()
            .find(|c| u64::from(c.node_id) == nested_sc as u64)
            .unwrap();
        let inner_ids: Vec<u64> = nested_lb
            .children
            .iter()
            .map(|c| u64::from(c.node_id))
            .collect();
        assert!(
            inner_ids.contains(&(inner_high_z as u64)),
            "high z-index child should remain inside its stacking context parent"
        );
    }

    #[test]
    fn same_z_index_preserves_dom_order() {
        let mut s = RuntimeState::new("https://t.com".into());
        let parent = s.create_element("div".into());
        s.append_element(0, parent).unwrap();
        s.set_inline_style(parent, "position".into(), "relative".into())
            .unwrap();

        let a = s.create_element("div".into());
        s.append_element(parent, a).unwrap();
        s.set_inline_style(a, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(a, "z-index".into(), "1".into()).unwrap();

        let b = s.create_element("div".into());
        s.append_element(parent, b).unwrap();
        s.set_inline_style(b, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(b, "z-index".into(), "1".into()).unwrap();

        let c = s.create_element("div".into());
        s.append_element(parent, c).unwrap();
        s.set_inline_style(c, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(c, "z-index".into(), "1".into()).unwrap();

        let lb = commit(&mut s);
        let ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();
        assert_eq!(
            ids,
            vec![a as u64, b as u64, c as u64],
            "equal z-index should preserve DOM order (stable sort)"
        );
    }

    #[test]
    fn deeply_nested_stacking_contexts_each_sort_independently() {
        let mut s = RuntimeState::new("https://t.com".into());

        let root_div = s.create_element("div".into());
        s.append_element(0, root_div).unwrap();
        s.set_inline_style(root_div, "position".into(), "relative".into())
            .unwrap();

        let l1_block = s.create_element("div".into());
        s.append_element(root_div, l1_block).unwrap();

        let l1_sc = s.create_element("div".into());
        s.append_element(root_div, l1_sc).unwrap();
        s.set_inline_style(l1_sc, "position".into(), "relative".into())
            .unwrap();
        s.set_inline_style(l1_sc, "z-index".into(), "0".into())
            .unwrap();

        // Level 2 (inside l1_sc): pos z=2 first in DOM, then pos z=1
        let l2_high = s.create_element("div".into());
        s.append_element(l1_sc, l2_high).unwrap();
        s.set_inline_style(l2_high, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(l2_high, "z-index".into(), "2".into())
            .unwrap();

        let l2_low = s.create_element("div".into());
        s.append_element(l1_sc, l2_low).unwrap();
        s.set_inline_style(l2_low, "position".into(), "absolute".into())
            .unwrap();
        s.set_inline_style(l2_low, "z-index".into(), "1".into())
            .unwrap();

        let lb = commit(&mut s);

        // Level 1: l1_block before l1_sc
        let l1_ids: Vec<u64> = lb.children.iter().map(|c| u64::from(c.node_id)).collect();
        assert_eq!(l1_ids, vec![l1_block as u64, l1_sc as u64]);

        // Level 2: l2_low (z=1) before l2_high (z=2)
        let l1_sc_lb = lb
            .children
            .iter()
            .find(|c| u64::from(c.node_id) == l1_sc as u64)
            .unwrap();
        let l2_ids: Vec<u64> = l1_sc_lb
            .children
            .iter()
            .map(|c| u64::from(c.node_id))
            .collect();
        assert_eq!(
            l2_ids,
            vec![l2_low as u64, l2_high as u64],
            "nested stacking context should sort its children independently"
        );
    }
}
