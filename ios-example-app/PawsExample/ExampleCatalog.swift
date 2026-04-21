import Foundation

struct ExampleEntry {
    let displayName: String
    let description: String
    let wasmResourceName: String
    let symbolName: String
}

struct ExampleSection {
    let title: String
    let footer: String?
    let entries: [ExampleEntry]
}

enum ExampleCatalog {
    static let sections: [ExampleSection] = [
        ExampleSection(
            title: "Core Primitives",
            footer: "Hand-written WASM components that exercise the rust-wasm-binding API directly.",
            entries: [
                ExampleEntry(
                    displayName: "Basic Element",
                    description: "Creates a single <div> on the document root.",
                    wasmResourceName: "example_basic_element",
                    symbolName: "square"
                ),
                ExampleEntry(
                    displayName: "Styled Element",
                    description: "<div> with inline width and height via setInlineStyle.",
                    wasmResourceName: "example_styled_element",
                    symbolName: "paintpalette"
                ),
                ExampleEntry(
                    displayName: "Nested Elements",
                    description: "Parent <div> with three <span> children via batch append.",
                    wasmResourceName: "example_nested_elements",
                    symbolName: "square.stack.3d.up"
                ),
                ExampleEntry(
                    displayName: "Stylesheet Cascade",
                    description: "Adds a stylesheet `div { height: 77px; }` via add_stylesheet.",
                    wasmResourceName: "example_stylesheet_cascade",
                    symbolName: "doc.text"
                ),
                ExampleEntry(
                    displayName: "Parsed Stylesheet",
                    description: "css!() macro drives a flexbox layout.",
                    wasmResourceName: "example_parsed_stylesheet",
                    symbolName: "curlybraces"
                ),
                ExampleEntry(
                    displayName: "Attributes",
                    description: "Sets class and id on a <div>.",
                    wasmResourceName: "example_attributes",
                    symbolName: "tag"
                ),
                ExampleEntry(
                    displayName: "Destroy & Rebuild",
                    description: "Creates, destroys, and recreates child elements.",
                    wasmResourceName: "example_destroy_rebuild",
                    symbolName: "arrow.triangle.2.circlepath"
                ),
                ExampleEntry(
                    displayName: "Full Commit Pipeline",
                    description: "DOM → style → layout with explicit commit().",
                    wasmResourceName: "example_commit_full",
                    symbolName: "checkmark.seal"
                ),
                ExampleEntry(
                    displayName: "Namespaces",
                    description: "SVG and MathML via create_element_ns.",
                    wasmResourceName: "example_namespace",
                    symbolName: "globe"
                ),
                ExampleEntry(
                    displayName: "Event Dispatch",
                    description: "Button with a click listener; dispatches a synthetic click.",
                    wasmResourceName: "example_event_dispatch",
                    symbolName: "hand.tap"
                ),
            ]
        ),
        ExampleSection(
            title: "Yew Framework",
            footer: "React-style components from the yew crate, running on Paws' virtual-DOM reconciler.",
            entries: [
                ExampleEntry(
                    displayName: "Counter",
                    description: "Classic button-plus-counter with use_state.",
                    wasmResourceName: "example_yew_counter",
                    symbolName: "plus.circle"
                ),
                ExampleEntry(
                    displayName: "use_state Counter",
                    description: "use_state with multiple setters and reads.",
                    wasmResourceName: "example_yew_use_state_counter",
                    symbolName: "number.circle"
                ),
                ExampleEntry(
                    displayName: "Multi-State Setters",
                    description: "Several setters mutating the same state in one frame.",
                    wasmResourceName: "example_yew_multi_state_setters",
                    symbolName: "square.grid.2x2"
                ),
                ExampleEntry(
                    displayName: "use_state_eq",
                    description: "Equality-gated state updates (no rerender on equal value).",
                    wasmResourceName: "example_yew_use_state_eq",
                    symbolName: "equal.circle"
                ),
                ExampleEntry(
                    displayName: "UB Deref Regression",
                    description: "Guards against use-after-free in state derefs.",
                    wasmResourceName: "example_yew_ub_deref",
                    symbolName: "exclamationmark.shield"
                ),
                ExampleEntry(
                    displayName: "Stale Read Regression",
                    description: "Guards against reading state after it was updated.",
                    wasmResourceName: "example_yew_stale_read",
                    symbolName: "clock.arrow.circlepath"
                ),
                ExampleEntry(
                    displayName: "Child Rerender",
                    description: "Parent state change that must rerender a child subtree.",
                    wasmResourceName: "example_yew_child_rerender",
                    symbolName: "arrow.down.forward.square"
                ),
            ]
        ),
    ]
}
