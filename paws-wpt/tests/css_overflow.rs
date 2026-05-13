//! Host-side runner for translated WPT tests under `css/css-overflow/`.
//!
//! Each `#[test]` here corresponds to one (or a small group of) upstream
//! subtests. The corresponding Yew fixture lives under
//! `paws-wpt/fixtures/css-overflow-<test-name>/` and is compiled by
//! `paws-wpt/build.rs`.

use engine::NodeId;
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
