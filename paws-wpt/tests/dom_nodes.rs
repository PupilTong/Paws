//! Host-side runner for translated WPT tests under `dom/nodes/`.
//!
//! Each `#[test]` here corresponds to one (or a small group of)
//! upstream subtests from a `wpt-reference/dom/nodes/*.html` file.
//! The corresponding Yew fixture lives under
//! `paws-wpt/fixtures/dom-nodes-<test-name>/` and is compiled by
//! `paws-wpt/build.rs`.

use engine::NodeId;
use paws_runner::Runner;
use paws_wpt::fixture_wasm_path;
use paws_wpt::testharness::{assert_equals, HTML_NS};

/// Loads a fixture's compiled `.wasm`, executes it through
/// `paws_runner`, and returns the [`Runner`] for inspection.
fn run_fixture(name: &str) -> Runner {
    let path = fixture_wasm_path(name);
    let wasm = std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let mut runner = Runner::builder().build();
    runner
        .run_component(&wasm, "run")
        .expect("wasm execution failed");
    runner
}

// ---------------------------------------------------------------------------
// dom/nodes/Document-createElement.html — Yew-flavor translation.
// ---------------------------------------------------------------------------

/// Asserts that `html! { <div /> }` renders to a Paws element with
/// the spec-mandated `localName` and `namespaceURI`. Corresponds to
/// the `createElement("div") in HTML document` subtest in upstream
/// `dom/nodes/Document-createElement.html`.
#[test]
fn create_element_div_in_html_document() {
    let runner = run_fixture("dom_nodes_document_create_element");
    let state = runner.state();

    // The fixture creates a root <div> at slab id 1 and renders the
    // Yew component inside it. The Yew-rendered <div /> is the first
    // child of that root.
    let root = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("root <div> from run()");
    let yew_div_id = *root
        .children
        .first()
        .expect("Yew should have rendered a <div /> inside the root");
    let yew_div = state
        .doc
        .get_node(yew_div_id)
        .expect("rendered <div /> should be a real node");

    assert_equals(
        yew_div.is_element(),
        true,
        "createElement('div') in HTML document — node is an Element",
    );

    assert_equals(
        yew_div.local_name(),
        Some("div"),
        "createElement('div') in HTML document — localName",
    );

    let namespace = state
        .get_namespace_uri(u64::from(yew_div_id) as u32)
        .expect("namespace lookup should not return a host error");
    assert_equals(
        namespace.as_deref(),
        Some(HTML_NS),
        "createElement('div') in HTML document — namespaceURI",
    );
}
