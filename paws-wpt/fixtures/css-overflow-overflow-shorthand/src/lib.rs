//! Yew-flavor translation of WPT
//! `css/css-overflow/parsing/overflow-shorthand.html`.
//!
//! Upstream:
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/parsing/overflow-shorthand.html>
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/parsing/overflow-computed.html>
//! Spec:
//!   - <https://drafts.csswg.org/css-overflow/#overflow-properties>
//!
//! ## Scope of this fixture
//!
//! The upstream `overflow-shorthand.html` test asserts that the `overflow`
//! shorthand correctly populates the `overflow-x` and `overflow-y`
//! longhands for every combination of one- and two-value forms.
//! `overflow-computed.html` covers the same surface from the computed-value
//! direction.
//!
//! This Yew translation covers the slice that is reachable through the
//! Yew API surface and the `css!()` compile-time CSS pipeline:
//!
//! - **Input**: an `html! { <div /> }` mounted as a child of the fixture
//!   root, plus a stylesheet that exercises three representative shorthand
//!   forms via the compile-time `css!()` macro:
//!   - Single-value form on a class selector (`overflow: hidden`).
//!   - Two-value form on a class selector (`overflow: scroll hidden`).
//!   - `clip` keyword preserved through the shorthand
//!     (distinct from `hidden` per CSS Overflow 3).
//! - **Assertions** (host-side): the three classed `<div>`s end up with the
//!   computed `overflow-x` and `overflow-y` values the spec mandates.
//!
//! ## What is NOT translated, and why
//!
//! - **All upstream value combinations.** Upstream enumerates 5×5 + 5
//!   = 30 value combinations; covering them all here would be `O(n)`
//!   fixtures (no parameterised fixture pattern yet). We cover the three
//!   representative cases that exercise the three failure modes the
//!   previous code path had: shorthand-dropped, two-value axis order, and
//!   `clip`-vs-`hidden` distinction. Engine-level tests at
//!   `engine/src/runtime.rs :: test_ir_overflow_shorthand_*` exhaustively
//!   cover the keyword cross-product through the same pipeline.
//! - **`overflow: inherit | initial | unset | revert`.** CSS-wide keywords
//!   on shorthands are dropped at the IR layer today
//!   (`PropertyValueIR::CssWide` is returned by `extract_css_wide` first
//!   and falls through without expansion). Tracked as a pre-existing gap;
//!   skip for the shorthand fixture.
//! - **The `<overflow>` value `overlay`** (legacy alias for `auto`).
//!   Modern WPT pins `overlay` as out-of-scope; we follow.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

/// Renders three classed `<div>`s — one per shorthand case the host-side
/// runner inspects. Yew does not expose a stable
/// `style="overflow: ..."` parsing path through the IR pipeline (inline
/// styles go via Stylo's own parser, which trivially expands shorthands),
/// so the values are applied via class selectors backed by `css!()`
/// stylesheets installed in `run()` below.
#[function_component]
fn OverflowShorthandFixture() -> Html {
    html! {
        <>
            <div class="single" />
            <div class="two-values" />
            <div class="clip" />
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

        // Install three compile-time-parsed stylesheets that route through
        // the IR pipeline. The `overflow` shorthand has no typed IR
        // variant, so each declaration lands as `PropertyValueIR::Raw` and
        // exercises the engine's direct token → typed-value shorthand
        // expander at `engine/src/style/ir_convert/keyword.rs ::
        // convert_overflow_shorthand_into_block`. No string round-trip,
        // no Stylo parser invocation on this path.
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".single { overflow: hidden; }"#
        ));
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".two-values { overflow: scroll hidden; }"#
        ));
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".clip { overflow: clip; }"#
        ));

        let _app = yew::Renderer::<OverflowShorthandFixture>::with_root(root).render();

        0
    }
}
