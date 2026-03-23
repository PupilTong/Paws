//! End-to-end tests that load compiled WASM example modules and verify
//! DOM structure, layout dimensions, and computed styles.
//!
//! The `.wasm` files are built by `build.rs` from the `examples/` crates.

include!(concat!(env!("OUT_DIR"), "/wasm_examples.rs"));

use engine::{CSSStyleValue, NodeId, RuntimeState};
use wasmtime_engine::run_wasm;

/// Loads an example WASM binary by name.
fn load_example(name: &str) -> Vec<u8> {
    let path = example_wasm_path(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

/// Runs an example and returns the RuntimeState.
fn run_example(name: &str) -> RuntimeState {
    let wasm = load_example(name);
    let state = RuntimeState::new("https://example.com".to_string());
    run_wasm(state, &wasm, "run").expect("wasm execution failed")
}

// -----------------------------------------------------------------------
// basic-element: creates div, appends to root
// -----------------------------------------------------------------------

#[test]
fn test_basic_element_dom_structure() {
    let state = run_example("example_basic_element");

    // Element 1 (the div) should exist
    let div = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("div should exist");
    assert!(div.is_element());

    // Its parent should be the document root (id 0)
    assert_eq!(div.parent, Some(NodeId::from(0_u64)));

    // The root should have the div as a child
    let root = state.doc.get_node(NodeId::from(0_u64)).expect("root");
    assert!(root.children.contains(&NodeId::from(1_u64)));
}

// -----------------------------------------------------------------------
// styled-element: div with width=200px, height=100px
// -----------------------------------------------------------------------

#[test]
fn test_styled_element_layout_dimensions() {
    let mut state = run_example("example_styled_element");

    // Resolve styles + compute layout
    let layout = state.commit();
    assert_eq!(layout.width, 200.0, "div width should be 200px");
    assert_eq!(layout.height, 100.0, "div height should be 100px");
}

// -----------------------------------------------------------------------
// nested-elements: parent div with 3 span children
// -----------------------------------------------------------------------

#[test]
fn test_nested_elements_parent_child() {
    let state = run_example("example_nested_elements");

    // Parent is id 1, children are ids 2, 3, 4
    let parent = state.doc.get_node(NodeId::from(1_u64)).expect("parent div");
    assert!(parent.is_element());
    assert_eq!(
        parent.children,
        vec![
            NodeId::from(2_u64),
            NodeId::from(3_u64),
            NodeId::from(4_u64),
        ],
        "parent should have 3 children in order"
    );

    // Each child should reference parent
    for child_id in 2..=4_u64 {
        let child = state
            .doc
            .get_node(NodeId::from(child_id))
            .expect("child span");
        assert!(child.is_element());
        assert_eq!(child.parent, Some(NodeId::from(1_u64)));
    }
}

// -----------------------------------------------------------------------
// stylesheet-cascade: div { height: 77px; }
// -----------------------------------------------------------------------

#[test]
fn test_stylesheet_cascade_height() {
    let mut state = run_example("example_stylesheet_cascade");

    let layout = state.commit();
    assert_eq!(layout.height, 77.0, "cascaded height should be 77px");
}

// -----------------------------------------------------------------------
// parsed-stylesheet: css!() macro with display:flex, width:200px
// -----------------------------------------------------------------------

#[test]
fn test_parsed_stylesheet_display_flex() {
    let mut state = run_example("example_parsed_stylesheet");

    // Trigger style resolution
    let layout = state.commit();
    assert_eq!(
        layout.width, 200.0,
        "width should be 200px from parsed stylesheet"
    );

    // Check computed display value
    let map = state
        .computed_style_map(1)
        .expect("computed style map for div");
    let display = map
        .get("display", &mut state.doc, &state.style_context)
        .expect("should have computed display");
    match display {
        CSSStyleValue::Keyword(kw) => assert_eq!(kw.value, "flex"),
        CSSStyleValue::Unparsed(s) => assert!(s.contains("flex"), "expected flex in {s}"),
        other => panic!("unexpected display value: {other:?}"),
    }
}

// -----------------------------------------------------------------------
// attributes: class="foo bar", id="main"
// -----------------------------------------------------------------------

#[test]
fn test_attributes_set_successfully() {
    // The wasm module returns 0 on success (all set_attribute calls passed)
    let state = run_example("example_attributes");

    // Verify the element exists and is attached
    let div = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("div should exist");
    assert!(div.is_element());
    assert_eq!(div.parent, Some(NodeId::from(0_u64)));
}

// -----------------------------------------------------------------------
// destroy-rebuild: create, destroy, recreate
// -----------------------------------------------------------------------

#[test]
fn test_destroy_element_cleanup() {
    let state = run_example("example_destroy_rebuild");

    // Parent (id 1) should exist
    let parent = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("parent should exist");
    assert!(parent.is_element());

    // After destroy + recreate, the slab may reuse slot 2 for the new "p"
    // element. The parent should have exactly one child (the replacement).
    assert_eq!(
        parent.children.len(),
        1,
        "parent should have exactly one child after destroy+rebuild"
    );

    // The replacement child should be attached to parent
    let replacement_id = parent.children[0];
    let replacement = state
        .doc
        .get_node(replacement_id)
        .expect("replacement child should exist");
    assert!(replacement.is_element());
    assert_eq!(replacement.parent, Some(NodeId::from(1_u64)));
}

// -----------------------------------------------------------------------
// commit-full: complete pipeline with width=300, height=150
// -----------------------------------------------------------------------

#[test]
fn test_commit_full_pipeline() {
    let mut state = run_example("example_commit_full");

    // commit() was already called inside the wasm module, but we can call
    // it again (idempotent) to get the LayoutBox
    let layout = state.commit();
    assert_eq!(layout.width, 300.0, "div width should be 300px");
    assert_eq!(layout.height, 150.0, "div height should be 150px");
}

// -----------------------------------------------------------------------
// Additional: verify all examples run without error
// -----------------------------------------------------------------------

#[test]
fn test_all_examples_run_successfully() {
    let examples = [
        "example_basic_element",
        "example_styled_element",
        "example_nested_elements",
        "example_stylesheet_cascade",
        "example_parsed_stylesheet",
        "example_attributes",
        "example_destroy_rebuild",
        "example_commit_full",
    ];

    for name in examples {
        let wasm = load_example(name);
        let state = RuntimeState::new("https://example.com".to_string());
        let result = run_wasm(state, &wasm, "run");
        assert!(result.is_ok(), "example {name} should run without error");
    }
}
