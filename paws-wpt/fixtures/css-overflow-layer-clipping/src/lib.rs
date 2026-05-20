//! Yew-flavor translation of WPT
//! `css/css-overflow/overflow-clip-rendering-001.html` (and the related
//! `overflow-hidden-rendering-001.html` reftest).
//!
//! Upstream:
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/>
//! Spec:
//!   - <https://drafts.csswg.org/css-overflow/#overflow-properties>
//!
//! ## Scope of this fixture
//!
//! Upstream WPT verifies `overflow: hidden | clip` painting via reftest
//! image comparison — Paws does not yet have a reftest framework. This
//! translation instead checks the **engine-side contract** that the
//! reftest depends on: the iOS renderer emits a `SetClipsToBounds` op
//! for a `ViewKind::Layer`-backed element when its computed
//! `overflow-x` or `overflow-y` is `hidden | clip`. The op tells the
//! Swift side to set `CALayer.masksToBounds = true`, which is the
//! mechanism by which Paws-on-iOS clips descendants the way the reftest
//! expects.
//!
//! The fixture mounts three classed `<div>`s under a single flex
//! parent (so each one is a non-root, non-scrollable child and renders
//! as `ViewKind::Layer`):
//! - `.hidden`  — `overflow-x: hidden; overflow-y: hidden`
//! - `.clip`    — `overflow-x: clip;   overflow-y: clip`
//! - `.visible` — no overflow set (defaults to `visible`)
//!
//! Longhand-only declarations are used because the `overflow` shorthand
//! does not yet flow through the IR pipeline (deferred to a future
//! PR). Longhand `overflow-x` / `overflow-y` already have direct
//! typed IR converters in the engine.
//!
//! The host-side runner (`paws-wpt/tests/css_overflow.rs`) inspects the
//! emitted op stream and asserts:
//! - The two clipped children produce `DeclareLayer` + `SetClipsToBounds`.
//! - The `.visible` child produces `DeclareLayer` but NO clip op.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

#[function_component]
fn LayerClippingFixture() -> Html {
    html! {
        <>
            <div class="hidden" />
            <div class="clip" />
            <div class="visible" />
        </>
    }
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

        // Make the root a flex container so its children are each
        // non-root, non-scrollable, and (when their overflow is not
        // scroll|auto) rendered as `ViewKind::Layer` by the iOS
        // renderer's `determine_kind` at
        // `ios-renderer-backend/src/renderer.rs :: 358-387`.
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#"div { display: flex; }"#
        ));

        // Apply overflow longhands separately on each classed child.
        // The longhand path uses the engine's direct token → Stylo
        // typed-value converters (`ir_to_overflow` at
        // `engine/src/style/ir_convert/keyword.rs`) — no string
        // round-trip or external parser.
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".hidden {
                overflow-x: hidden;
                overflow-y: hidden;
                width: 20px;
                height: 20px;
            }"#
        ));
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".clip {
                overflow-x: clip;
                overflow-y: clip;
                width: 20px;
                height: 20px;
            }"#
        ));
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".visible {
                width: 20px;
                height: 20px;
            }"#
        ));

        let _app = yew::Renderer::<LayerClippingFixture>::with_root(root).render();

        0
    }
}
