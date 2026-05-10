//! Yew-flavor translation of the synchronous, HTML-document subset
//! of WPT `dom/nodes/Document-createElement.html`.
//!
//! Upstream: `wpt-reference/dom/nodes/Document-createElement.html`.
//! Spec:    <https://dom.spec.whatwg.org/#dom-document-createelement>
//!
//! ## Scope of this fixture
//!
//! The upstream test iterates over a list of valid and invalid tag
//! names across HTML, XML, and XHTML documents, asserting on
//! `localName`, `tagName`, `prefix`, and `namespaceURI` (plus an
//! `InvalidCharacterError` throw path for invalid names).
//!
//! This Yew translation covers the slice that is reachable through
//! the Yew API surface today:
//!
//! - **Input**: `html! { <div /> }` — equivalent to
//!   `document.createElement("div")` in an HTML document.
//! - **Assertions** (host-side): the resulting Paws element has
//!   `localName == "div"` and `namespaceURI == HTML_NS`.
//!
//! ## What is NOT translated, and why
//!
//! - **Mixed-case / special-character tag names** (e.g. `"FOO"`,
//!   `"f1oo"`): the `html! { ... }` macro only accepts a fixed,
//!   compile-time tag identifier, so each variant would require its
//!   own fixture crate. Tractable but tedious — defer until we have
//!   a parameterised fixture pattern.
//! - **XML / XHTML document contexts**: Paws has no concept of
//!   non-HTML documents at the engine level today; the WIT surface
//!   creates HTML elements unconditionally. This is a future engine
//!   capability rather than a Yew-fork gap.
//! - **`InvalidCharacterError` throw path**: Yew validates tags at
//!   compile time, so we cannot construct an invalid name through
//!   `html!`. Direct `rust_wasm_binding::create_element("")` would
//!   exercise this, but that variant is excluded by the
//!   "Yew-flavor only" policy in `agents.md`.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

/// The fixture component: renders a single bare `<div />` so the host
/// runner can inspect the element Paws produced for it.
#[function_component]
fn DocumentCreateElementFixture() -> Html {
    html! { <div /> }
}

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let root = match Element::new("div") {
            Ok(element) => Rc::new(element),
            Err(error_code) => return error_code,
        };

        if let Err(error_code) = rust_wasm_binding::append_element(0, root.id()) {
            return error_code;
        }

        // Yew renders the fixture into `root`. After render() returns,
        // `root.children[0]` is the `<div />` produced by the macro,
        // which the host-side test then asserts on.
        let _app = yew::Renderer::<DocumentCreateElementFixture>::with_root(root).render();

        0
    }
}
