//! Host-side runner for translated WPT tests under `css/css-overflow/`.
//!
//! Each `#[test]` here corresponds to one (or a small group of) upstream
//! subtests from a `wpt-reference/css/css-overflow/parsing/*.html` file.
//! The corresponding Yew fixture lives under
//! `paws-wpt/fixtures/css-overflow-<test-name>/` and is compiled by
//! `paws-wpt/build.rs`.

use engine::{CSSStyleValue, NodeId, RuntimeState, StylePropertyMapReadOnly};
use paws_runner::Runner;
use paws_wpt::fixture_wasm_path;
use paws_wpt::testharness::assert_equals;

/// Loads a fixture's compiled `.wasm`, executes it through `paws_runner`,
/// and returns the [`Runner`] for inspection.
fn run_fixture(name: &str) -> Runner {
    let path = fixture_wasm_path(name);
    let wasm = std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let mut runner = Runner::builder().build();
    runner
        .run_component(&wasm, "run")
        .expect("wasm execution failed");
    runner
}

/// Reads a computed CSS property off a node and returns its keyword
/// representation. The typed-OM returns either `CSSStyleValue::Keyword`
/// or `CSSStyleValue::Unparsed(string)` for keyword-valued properties
/// depending on whether the property has a typed-value path; both are
/// accepted here, mirroring the engine's own `assert_keyword` helper.
fn computed_keyword(
    map: &StylePropertyMapReadOnly,
    state: &mut RuntimeState,
    property: &str,
) -> String {
    let value = map
        .get(property, &mut state.doc, &state.style_context)
        .unwrap_or_else(|| panic!("no computed value for '{property}'"));
    match value {
        CSSStyleValue::Keyword(kw) => kw.value,
        CSSStyleValue::Unparsed(s) => s,
        other => panic!("expected keyword for '{property}', got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// css/css-overflow/parsing/overflow-shorthand.html
//   + css/css-overflow/parsing/overflow-computed.html — Yew-flavor slice.
// ---------------------------------------------------------------------------

/// Asserts that the `overflow` shorthand expands correctly when authored
/// in a `css!()`-compiled stylesheet — i.e. the IR → Stylo pipeline now
/// produces both longhands instead of silently dropping the declaration.
///
/// Covers three representative shorthand cases:
/// 1. **Single-value form** (`overflow: hidden`): both axes are `hidden`.
///    Corresponds to upstream subtest
///    `test_shorthand_value("overflow", "hidden", { "overflow-x": "hidden", "overflow-y": "hidden" })`.
/// 2. **Two-value axis order** (`overflow: scroll hidden`): first → x,
///    second → y. Corresponds to upstream subtest
///    `test_shorthand_value("overflow", "scroll hidden", { "overflow-x": "scroll", "overflow-y": "hidden" })`.
///    Both values are non-`visible` to dodge the CSS Overflow 3 rule
///    "visible coerces to auto when the other axis is non-visible", which
///    would mask the axis-order check.
/// 3. **`clip` keyword preserved** (`overflow: clip`): both axes report
///    `clip`, distinct from `hidden`. Spec: clip is not a scroll
///    container and is therefore semantically different even though it
///    has the same visual effect today.
#[test]
fn overflow_shorthand_expands_to_longhands() {
    let mut runner = run_fixture("css_overflow_overflow_shorthand");
    let state = runner.state_mut();

    // The fixture mounts three classed <div>s under the root. Yew renders
    // them as the first three children of the root div (slab id 1). Find
    // them by class so the test is robust against Yew reordering.
    let root = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("root <div> from run()");
    let children: Vec<NodeId> = root.children.to_vec();
    let mut single = None;
    let mut two_values = None;
    let mut clip = None;
    for child_id in &children {
        let class = state
            .get_attribute(u64::from(*child_id) as u32, "class")
            .ok()
            .flatten()
            .unwrap_or_default();
        match class.as_str() {
            "single" => single = Some(*child_id),
            "two-values" => two_values = Some(*child_id),
            "clip" => clip = Some(*child_id),
            _ => {}
        }
    }
    let single = single.expect("rendered <div class=\"single\" />");
    let two_values = two_values.expect("rendered <div class=\"two-values\" />");
    let clip = clip.expect("rendered <div class=\"clip\" />");

    // Case 1: overflow: hidden → both axes = hidden.
    let map = state
        .computed_style_map(u64::from(single) as u32)
        .expect("computed style map for .single");
    assert_equals(
        computed_keyword(&map, state, "overflow-x"),
        "hidden".to_string(),
        "overflow: hidden — overflow-x",
    );
    assert_equals(
        computed_keyword(&map, state, "overflow-y"),
        "hidden".to_string(),
        "overflow: hidden — overflow-y",
    );

    // Case 2: overflow: scroll hidden → x = scroll, y = hidden.
    let map = state
        .computed_style_map(u64::from(two_values) as u32)
        .expect("computed style map for .two-values");
    assert_equals(
        computed_keyword(&map, state, "overflow-x"),
        "scroll".to_string(),
        "overflow: scroll hidden — overflow-x",
    );
    assert_equals(
        computed_keyword(&map, state, "overflow-y"),
        "hidden".to_string(),
        "overflow: scroll hidden — overflow-y",
    );

    // Case 3: overflow: clip — both axes = clip (distinct from hidden).
    let map = state
        .computed_style_map(u64::from(clip) as u32)
        .expect("computed style map for .clip");
    assert_equals(
        computed_keyword(&map, state, "overflow-x"),
        "clip".to_string(),
        "overflow: clip — overflow-x",
    );
    assert_equals(
        computed_keyword(&map, state, "overflow-y"),
        "clip".to_string(),
        "overflow: clip — overflow-y",
    );
}
