//! Mounts a minimal yew counter component on the Paws host.
//!
//! Tests the full integration: yew virtual DOM reconciliation,
//! element creation via `rust_wasm_binding`, attribute/text diffing,
//! and event listener registration through the host dispatch pipeline.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

/// Stylesheet injected by [`run`] before the yew virtual DOM is rendered.
/// Purely for visual demo purposes: gives the button a tappable-looking
/// chrome and sizes the counter value so the component actually looks
/// like a counter when running on a rendering backend (iOS, wgpu, etc.).
/// Test fixtures that only care about DOM structure / state transitions
/// don't depend on any of these properties — stylo cascades the rules
/// but Taffy still computes a valid layout if the backend ignores them.
// Plain block layout — Paws' Taffy integration sizes text children from
// the block's inline context, which is the only path currently known to
// flow text widths correctly through to the iOS op buffer. Switching the
// `.counter` container to `display: flex` visually loses both the "+"
// and "0" text because the flex-item → text-node constraint path
// doesn't propagate an intrinsic width. Keep the simpler layout until
// that's fixed in the engine.
const COUNTER_CSS: &str = "
    .counter {
        padding: 24px;
        background-color: #f2f2f7;
    }
    button {
        background-color: #0A84FF;
        color: #ffffff;
        padding: 16px 32px;
        font-size: 28px;
        font-weight: bold;
    }
    span {
        font-size: 40px;
        color: #1c1c1e;
        font-weight: 600;
        padding: 8px 0;
    }
";

/// A minimal counter component with a "+" button and a count display.
#[function_component]
fn Counter() -> Html {
    let count = use_state(|| 0i32);

    let onclick = {
        let count = count.clone();
        Callback::from(move |_: ()| {
            count.set(*count + 1);
        })
    };

    html! {
        <div class="counter">
            <button {onclick}>{ "+" }</button>
            <span>{ format!("{}", *count) }</span>
        </div>
    }
}

// Entry point called by the Paws host.
//
// Creates a root `<div>` element, appends it to the document, and
// mounts the yew counter component into it. After `render()` returns
// the virtual DOM has been reconciled and the physical DOM tree is
// populated.
rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let root = match Element::new("div") {
            Ok(element) => Rc::new(element),
            Err(error_code) => return error_code,
        };

        if let Err(error_code) = rust_wasm_binding::append_element(0, root.id()) {
            return error_code;
        }

        // Inject the demo stylesheet *before* yew mounts so stylo has
        // it in the rule tree the first time it resolves styles.
        if let Err(error_code) = rust_wasm_binding::add_stylesheet(COUNTER_CSS) {
            return error_code;
        }

        let _app = yew::Renderer::<Counter>::with_root(root).render();

        0
    }
}
