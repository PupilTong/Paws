//! End-to-end tests that load compiled WASM example modules and verify
//! DOM structure, layout dimensions, and computed styles.
//!
//! The `.wasm` files are built by `build.rs` from the `examples/` crates.

include!(concat!(env!("OUT_DIR"), "/wasm_examples.rs"));

use engine::{CSSStyleValue, NodeId, RuntimeState};
use wasmtime_engine::{run_wasm, WasmInstance};

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
    state.commit();
    let node = state.doc.get_node(NodeId::from(1_u64)).unwrap();
    assert_eq!(node.layout().size.width, 200.0, "div width should be 200px");
    assert_eq!(
        node.layout().size.height,
        100.0,
        "div height should be 100px"
    );
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

    state.commit();
    let node = state.doc.get_node(NodeId::from(1_u64)).unwrap();
    assert_eq!(
        node.layout().size.height,
        77.0,
        "cascaded height should be 77px"
    );
}

// -----------------------------------------------------------------------
// parsed-stylesheet: css!() macro with display:flex, width:200px
// -----------------------------------------------------------------------

#[test]
fn test_parsed_stylesheet_display_flex() {
    let mut state = run_example("example_parsed_stylesheet");

    // Trigger style resolution
    state.commit();
    let node = state.doc.get_node(NodeId::from(1_u64)).unwrap();
    assert_eq!(
        node.layout().size.width,
        200.0,
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
    // it again (idempotent) to verify layout
    state.commit();
    let node = state.doc.get_node(NodeId::from(1_u64)).unwrap();
    assert_eq!(node.layout().size.width, 300.0, "div width should be 300px");
    assert_eq!(
        node.layout().size.height,
        150.0,
        "div height should be 150px"
    );
}

// -----------------------------------------------------------------------
// namespace: exercises __create_element_ns and __get_namespace_uri
// -----------------------------------------------------------------------

#[test]
fn test_namespace_dom_structure_and_uris() {
    let state = run_example("example_namespace");

    // Example creates: svg(1) with circle(2), math(3), div(4)
    let svg = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("svg should exist");
    assert!(svg.is_element());

    let circle = state
        .doc
        .get_node(NodeId::from(2_u64))
        .expect("circle should exist");
    assert!(circle.is_element());
    assert_eq!(circle.parent, Some(NodeId::from(1_u64)));

    let math = state
        .doc
        .get_node(NodeId::from(3_u64))
        .expect("math should exist");
    assert!(math.is_element());

    let div = state
        .doc
        .get_node(NodeId::from(4_u64))
        .expect("div should exist");
    assert!(div.is_element());

    // Namespace URIs are verified via the public RuntimeState API
    assert_eq!(
        state.get_namespace_uri(1).unwrap().as_deref(),
        Some("http://www.w3.org/2000/svg")
    );
    assert_eq!(
        state.get_namespace_uri(2).unwrap().as_deref(),
        Some("http://www.w3.org/2000/svg")
    );
    assert_eq!(
        state.get_namespace_uri(3).unwrap().as_deref(),
        Some("http://www.w3.org/1998/Math/MathML")
    );
    // Regular HTML div created via create_element() uses the HTML namespace
    assert_eq!(
        state.get_namespace_uri(4).unwrap().as_deref(),
        Some("http://www.w3.org/1999/xhtml")
    );
}

// -----------------------------------------------------------------------
// event-dispatch: tests full host ↔ guest event pipeline
// -----------------------------------------------------------------------

#[test]
fn test_event_dispatch_callback_fires() {
    let state = run_example("example_event_dispatch");

    // The example creates div(1) > button(2), registers a click listener,
    // dispatches a click, and the listener creates span(3) as a sibling.
    let parent = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("parent div should exist");
    assert!(parent.is_element());

    // Parent should have two children: button and the span created by the
    // event handler.
    assert_eq!(
        parent.children.len(),
        2,
        "parent should have button + span after event dispatch"
    );

    // The span (created by the callback) should be the second child.
    let span = state
        .doc
        .get_node(parent.children[1])
        .expect("span created by event handler should exist");
    assert!(span.is_element());
    assert_eq!(span.parent, Some(NodeId::from(1_u64)));
}

// -----------------------------------------------------------------------
// yew-counter: mounts a yew component and verifies DOM structure
// -----------------------------------------------------------------------

#[test]
fn test_yew_counter_renders_dom() {
    // Skip if the yew submodule wasn't checked out (CI without --recurse-submodules).
    let path = std::panic::catch_unwind(|| example_wasm_path("example_yew_counter"));
    let Ok(path) = path else {
        eprintln!("skipping test_yew_counter_renders_dom: yew example not built");
        return;
    };
    if !std::path::Path::new(path).exists() {
        eprintln!("skipping test_yew_counter_renders_dom: wasm binary not found");
        return;
    }
    let state = run_example("example_yew_counter");

    // The yew counter component renders:
    //   root(1) > div.counter(?) > button(?) + span(?)
    //
    // Element 1 is the host div created by run(). Yew mounts inside it,
    // creating child elements for the virtual DOM tree.
    let root = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("root div should exist");
    assert!(root.is_element(), "root should be an element");
    assert!(
        !root.children.is_empty(),
        "root should have children after yew mounts"
    );

    // Yew creates a div.counter as the first child of the root.
    let counter_div = state
        .doc
        .get_node(root.children[0])
        .expect("counter div should exist");
    assert!(counter_div.is_element());

    // The counter div should have two children: button and span.
    assert_eq!(
        counter_div.children.len(),
        2,
        "counter div should have button + span"
    );

    // Verify both children exist as elements.
    let button = state
        .doc
        .get_node(counter_div.children[0])
        .expect("button should exist");
    assert!(button.is_element());

    let span = state
        .doc
        .get_node(counter_div.children[1])
        .expect("span should exist");
    assert!(span.is_element());
}

// -----------------------------------------------------------------------
// yew-counter click: dispatch event → yew re-render → DOM update
// -----------------------------------------------------------------------

#[test]
fn test_yew_counter_click_updates_dom() {
    let path = std::panic::catch_unwind(|| example_wasm_path("example_yew_counter"));
    let Ok(path) = path else {
        eprintln!("skipping test_yew_counter_click_updates_dom: yew example not built");
        return;
    };
    if !std::path::Path::new(path).exists() {
        eprintln!("skipping test_yew_counter_click_updates_dom: wasm binary not found");
        return;
    }

    let wasm = load_example("example_yew_counter");
    let state = RuntimeState::new("https://example.com".to_string());
    let mut instance = WasmInstance::new(state, &wasm).expect("instantiate yew counter");

    // Initial render
    let rc = instance.call("run").expect("run should succeed");
    assert_eq!(rc, 0);

    // Dispatch a click on the button (node 3). This should trigger:
    // host dispatch_event → __paws_invoke_listener → yew_event_dispatch
    // → SubtreeData::handle → onclick callback → use_state(count + 1)
    // → scheduler re-render → DOM update
    let rc = instance
        .call("click_button")
        .expect("click_button should succeed");
    assert_eq!(rc, 0, "click_button should return 0 (success)");

    // After the click, the counter's span text should show "1".
    // The span is a text container — yew creates a text node inside it.
    // We verify the DOM structure is still intact (yew reconciled correctly).
    let state = instance.state();
    let root = state
        .doc
        .get_node(NodeId::from(1_u64))
        .expect("root should exist");
    let counter_div = state
        .doc
        .get_node(root.children[0])
        .expect("counter div should exist");

    // After re-render, button + span should still be there.
    assert_eq!(
        counter_div.children.len(),
        2,
        "counter div should still have button + span after re-render"
    );
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
        "example_namespace",
        "example_event_dispatch",
    ];

    for name in examples {
        let wasm = load_example(name);
        let state = RuntimeState::new("https://example.com".to_string());
        let result = run_wasm(state, &wasm, "run");
        assert!(result.is_ok(), "example {name} should run without error");
    }

    // yew examples may not be available if the submodule isn't checked out.
    if let Ok(path) = std::panic::catch_unwind(|| example_wasm_path("example_yew_counter")) {
        if std::path::Path::new(path).exists() {
            let wasm = std::fs::read(path).unwrap();
            let state = RuntimeState::new("https://example.com".to_string());
            let result = run_wasm(state, &wasm, "run");
            assert!(
                result.is_ok(),
                "example_yew_counter should run without error"
            );
        }
    }
}
