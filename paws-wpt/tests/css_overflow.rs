//! Host-side runner for translated WPT tests under `css/css-overflow/`.
//!
//! Each `#[test]` here corresponds to one (or a small group of) upstream
//! subtests. The corresponding Yew fixture lives under
//! `paws-wpt/fixtures/css-overflow-<test-name>/` and is compiled by
//! `paws-wpt/build.rs`.

use engine::{CSSStyleValue, NodeId, RuntimeState};
use ios_renderer_backend::test_support::{
    process_into_op_tags, IosNodeState, OP_TAG_DECLARE_LAYER, OP_TAG_DECLARE_SCROLL_VIEW,
    OP_TAG_SET_CLIPS_TO_BOUNDS,
};
use paws_runner::Runner;
use paws_wpt::fixture_wasm_path;
use paws_wpt::testharness::assert_equals;

/// No-op `EngineRenderer` whose only purpose is to type the `Runner` so
/// that the underlying `Document` carries the iOS render state. Style
/// resolution + layout still run inside `commit`; this type just
/// declines to do any per-commit painting. The host test invokes the
/// iOS renderer explicitly via `process_into_op_tags` after the wasm
/// guest finishes.
struct IosRenderStateRenderer;

// SAFETY: `IosRenderStateRenderer` is a zero-sized unit struct with no
// thread-affine pointers; it is trivially `Send`.
unsafe impl Send for IosRenderStateRenderer {}

impl engine::EngineRenderer for IosRenderStateRenderer {
    type NodeState = IosNodeState;
    fn on_commit(
        &mut self,
        _doc: &mut engine::dom::Document<IosNodeState>,
        _resources: &dyn engine::ResourceResolver,
        _root: Option<engine::NodeId>,
    ) {
    }
}

/// Loads a fixture's compiled `.wasm`, executes it through `paws_runner`
/// configured with `IosNodeState` as the render state, and returns the
/// resulting [`Runner`] for inspection.
fn run_fixture_for_ios(name: &str) -> Runner<IosRenderStateRenderer> {
    let path = fixture_wasm_path(name);
    let wasm = std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let mut runner = Runner::builder().renderer(IosRenderStateRenderer).build();
    runner
        .run_component(&wasm, "run")
        .expect("wasm execution failed");
    runner
}

// ---------------------------------------------------------------------------
// css/css-overflow/ — Yew-flavor renderer-side verification.
// ---------------------------------------------------------------------------

/// Asserts the iOS renderer honours `overflow: hidden | clip` on
/// `ViewKind::Layer`-backed (CALayer) elements.
///
/// Upstream WPT covers this via reftest image comparison (e.g.
/// `css/css-overflow/overflow-clip-rendering-001.html`); Paws does not
/// yet have a reftest framework, so the translation verifies the
/// engine-side contract the reftests depend on: the renderer emits a
/// `SetClipsToBounds` op for the clipped Layer children but not for an
/// `overflow: visible` Layer child. The op drives
/// `CALayer.masksToBounds` on the Swift side, which is the mechanism
/// for the visual clipping the upstream reftest measures.
///
/// Spec reference:
/// <https://drafts.csswg.org/css-overflow/#overflow-properties>
#[test]
fn overflow_hidden_and_clip_emit_layer_mask_ops() {
    let mut runner = run_fixture_for_ios("css_overflow_layer_clipping");

    // Sanity: the three classed children are present under the root.
    // The fixture's `apply_css` and `Renderer::with_root` build the
    // following tree:
    //   root <div>                          (id 1 — rendered as View)
    //     <div class="hidden" />            (Layer)
    //     <div class="clip"   />            (Layer)
    //     <div class="visible"/>            (Layer)
    let state = runner.state_mut();
    let root = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("root <div> from run()");
    assert_equals(
        root.children.len(),
        3,
        "fixture should mount three classed children under the root",
    );

    // Drive the iOS renderer's ViewTree over the fully styled +
    // laid-out document and collect the op-tag stream.
    let root_id = state.doc.root_element_id();
    let tags = process_into_op_tags(&mut state.doc, &engine::NoopResourceResolver, root_id);

    // None of the three children specify `overflow: scroll | auto`, so
    // none should end up as scroll containers. (If they did, the test
    // premise — that they're Layer-kind — would fail.)
    let scroll_decl_count = tags
        .iter()
        .filter(|&&t| t == OP_TAG_DECLARE_SCROLL_VIEW)
        .count();
    assert_equals(
        scroll_decl_count,
        0,
        "no DeclareScrollView ops — the three children should all be Layer-kind",
    );

    let layer_decl_count = tags.iter().filter(|&&t| t == OP_TAG_DECLARE_LAYER).count();
    assert_equals(
        layer_decl_count,
        3,
        "three Layer-kind children should be declared",
    );

    // Clip-op accounting:
    //   - root View emits its own baseline `SetClipsToBounds(false)`
    //     unconditionally (existing behaviour for View / ScrollView).
    //   - The `.hidden` Layer child emits one (overflow: hidden).
    //   - The `.clip`   Layer child emits one (overflow: clip).
    //   - The `.visible` Layer child must NOT emit one — `emit_full_node`
    //     skips the `clips == false` case for Layer because
    //     `CALayer.masksToBounds` defaults to `false`.
    // Total expected: 1 (root) + 1 (hidden) + 1 (clip) = 3.
    let clip_op_count = tags
        .iter()
        .filter(|&&t| t == OP_TAG_SET_CLIPS_TO_BOUNDS)
        .count();
    assert_equals(
        clip_op_count,
        3,
        "expected exactly 3 SetClipsToBounds ops: root View baseline + \
         two clipped Layer children. The overflow:visible Layer child \
         must not emit a clip op.",
    );
}

// ---------------------------------------------------------------------------
// css/css-overflow/parsing/ — longhand subset of `overflow-computed.html`,
// `overflow-valid.html`, `overflow-invalid.html`.
// ---------------------------------------------------------------------------

/// Returns the computed CSS keyword for `property` on the element with
/// class `class_name` (first child match under the root). Mirrors the
/// engine-level `ir_pipeline_get` + `assert_keyword` helpers from
/// `engine/src/runtime.rs` tests but adapted to the WPT runner.
fn computed_keyword_for_class(
    runner: &mut Runner<IosRenderStateRenderer>,
    class_name: &str,
    property: &str,
) -> String {
    let state: &mut RuntimeState<IosRenderStateRenderer> = runner.state_mut();
    let root = state.doc.get_node(NodeId::from(1_u64)).expect("root <div>");
    let children: Vec<NodeId> = root.children.to_vec();
    let target = children
        .iter()
        .find(|child_id| {
            state
                .get_attribute(u64::from(**child_id) as u32, "class")
                .ok()
                .flatten()
                .as_deref()
                == Some(class_name)
        })
        .copied()
        .unwrap_or_else(|| panic!("no <div class=\"{class_name}\" /> under root"));
    let map = state
        .computed_style_map(u64::from(target) as u32)
        .unwrap_or_else(|_| panic!("computed style map for .{class_name}"));
    let value = map
        .get(property, &mut state.doc, &state.style_context)
        .unwrap_or_else(|| panic!("no computed value for '{property}' on .{class_name}"));
    match value {
        CSSStyleValue::Keyword(kw) => kw.value,
        CSSStyleValue::Unparsed(s) => s,
        other => panic!("expected keyword for '{property}' on .{class_name}, got: {other:?}"),
    }
}

/// Runs every `test_computed_value("overflow-x", v)` and
/// `test_computed_value("overflow-y", v)` longhand subtest from
/// upstream `parsing/overflow-computed.html` for `v ∈ {visible, hidden,
/// scroll, auto, clip}`. Each subtest asserts the computed-style
/// keyword matches the specified value verbatim — `overflow-x: scroll`
/// must read back as `"scroll"`, etc.
///
/// Also covers the `overflow-x: <value>` /  `overflow-y: <value>`
/// subset of upstream `parsing/overflow-valid.html` — `test_valid_value`
/// reduces to "specified parses + computes to the same keyword", which
/// is what these assertions check.
///
/// Cross-axis interactions (the "visible coerces to auto when the other
/// axis is hidden / scroll / auto" rule from the same upstream file)
/// are exercised separately by
/// `overflow_visible_coerces_to_auto_when_other_axis_is_scrollable`.
#[test]
fn overflow_longhand_computed_values_match_spec() {
    let mut runner = run_fixture_for_ios("css_overflow_overflow_longhand_computed");

    for keyword in &["visible", "hidden", "scroll", "auto", "clip"] {
        assert_equals(
            computed_keyword_for_class(&mut runner, &format!("x-{keyword}"), "overflow-x"),
            (*keyword).to_string(),
            "overflow-x computed value matches specified keyword",
        );
        assert_equals(
            computed_keyword_for_class(&mut runner, &format!("y-{keyword}"), "overflow-y"),
            (*keyword).to_string(),
            "overflow-y computed value matches specified keyword",
        );
    }
}

/// Translates the cross-axis coercion subset of upstream
/// `parsing/overflow-computed.html`:
/// ```
/// test_computed_value("overflow", 'hidden visible', 'hidden auto');
/// test_computed_value("overflow", 'scroll visible', 'scroll auto');
/// test_computed_value("overflow", 'clip visible',   'clip visible');  // clip is exempt
/// ```
///
/// Spec rule (CSS Overflow 3 §2.1): "visible" on one axis computes to
/// "auto" when the other axis is set to anything other than `visible`
/// or `clip` (i.e. `hidden`, `scroll`, or `auto`).
///
/// `clip` is exempt from this coercion: a `clip` + `visible` pair stays
/// `clip visible` because neither axis introduces a scroll container.
///
/// Stylo implements this coercion when consuming the computed values,
/// so the longhand path through the IR pipeline (no shorthand
/// expansion here) inherits the rule for free.
#[test]
fn overflow_visible_coerces_to_auto_when_other_axis_is_scrollable() {
    let mut runner = run_fixture_for_ios("css_overflow_overflow_longhand_computed");

    // Triggers coercion: `hidden`, `scroll`, and `auto` on overflow-x
    // each force the otherwise-`visible` overflow-y to `auto`.
    for triggering in &["hidden", "scroll", "auto"] {
        assert_equals(
            computed_keyword_for_class(&mut runner, &format!("x-{triggering}"), "overflow-y"),
            "auto".to_string(),
            "overflow-y (default visible) coerces to auto when overflow-x is non-visible / non-clip",
        );
    }
    // Symmetric: overflow-y triggers coercion on overflow-x.
    for triggering in &["hidden", "scroll", "auto"] {
        assert_equals(
            computed_keyword_for_class(&mut runner, &format!("y-{triggering}"), "overflow-x"),
            "auto".to_string(),
            "overflow-x (default visible) coerces to auto when overflow-y is non-visible / non-clip",
        );
    }
    // Exempt: `clip` does not trigger coercion. The other axis's
    // implicit `visible` stays `visible`.
    assert_equals(
        computed_keyword_for_class(&mut runner, "x-clip", "overflow-y"),
        "visible".to_string(),
        "overflow-y stays visible when overflow-x is clip (clip does not establish a scroll container)",
    );
    assert_equals(
        computed_keyword_for_class(&mut runner, "y-clip", "overflow-x"),
        "visible".to_string(),
        "overflow-x stays visible when overflow-y is clip",
    );
    // Symmetric baseline: `visible` does not trigger coercion either.
    assert_equals(
        computed_keyword_for_class(&mut runner, "x-visible", "overflow-y"),
        "visible".to_string(),
        "overflow-y stays visible when overflow-x is visible (no coercion)",
    );
    assert_equals(
        computed_keyword_for_class(&mut runner, "y-visible", "overflow-x"),
        "visible".to_string(),
        "overflow-x stays visible when overflow-y is visible (no coercion)",
    );
}

/// Translates the longhand assertions in upstream
/// `parsing/overflow-invalid.html`:
/// `test_invalid_value("overflow-x", 'visible clip')` and
/// `test_invalid_value("overflow-y", 'clip hidden')`. The longhand
/// `overflow-x` / `overflow-y` accept only a single keyword — a
/// two-value list is invalid and the declaration must be dropped, so
/// the initial `visible` value is preserved on both axes.
///
/// On the Paws side this is exercised by the bespoke `ir_to_overflow`
/// converter in `engine/src/style/ir_convert/keyword.rs`, which pattern
/// matches `[ArchivedCssToken::Ident(_)]` (single-element slice) and
/// returns `None` for any other shape, dropping the declaration at
/// `engine/src/style/ir_convert/mod.rs :: convert_raw_declaration`.
#[test]
fn overflow_longhand_two_value_form_is_invalid_and_drops_declaration() {
    let mut runner = run_fixture_for_ios("css_overflow_overflow_longhand_computed");

    // overflow-x: visible clip — invalid; element stays at the
    // initial overflow-x = visible.
    assert_equals(
        computed_keyword_for_class(&mut runner, "x-two-values-invalid", "overflow-x"),
        "visible".to_string(),
        "overflow-x: 'visible clip' is invalid — overflow-x stays visible",
    );
    assert_equals(
        computed_keyword_for_class(&mut runner, "x-two-values-invalid", "overflow-y"),
        "visible".to_string(),
        "overflow-x: 'visible clip' must not bleed into overflow-y",
    );

    // overflow-y: clip hidden — same shape.
    assert_equals(
        computed_keyword_for_class(&mut runner, "y-two-values-invalid", "overflow-y"),
        "visible".to_string(),
        "overflow-y: 'clip hidden' is invalid — overflow-y stays visible",
    );
    assert_equals(
        computed_keyword_for_class(&mut runner, "y-two-values-invalid", "overflow-x"),
        "visible".to_string(),
        "overflow-y: 'clip hidden' must not bleed into overflow-x",
    );
}
