//! Yew-flavor translation of the longhand `overflow-x` / `overflow-y`
//! slice of WPT
//! `css/css-overflow/parsing/overflow-computed.html` (plus the
//! companion `overflow-valid.html` and `overflow-invalid.html` longhand
//! assertions).
//!
//! Upstream:
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/parsing/overflow-computed.html>
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/parsing/overflow-valid.html>
//!   - <https://github.com/web-platform-tests/wpt/blob/master/css/css-overflow/parsing/overflow-invalid.html>
//! Spec:
//!   - <https://drafts.csswg.org/css-overflow/#overflow-properties>
//!
//! ## Scope of this fixture
//!
//! Upstream `overflow-computed.html` exercises:
//! ```
//! test_computed_value("overflow-x", 'scroll' | 'hidden' | 'visible');
//! test_computed_value("overflow-y", 'clip'   | 'auto'   | 'visible');
//! test_computed_value("overflow-block",  'hidden' | 'clip' | 'visible');
//! test_computed_value("overflow-inline", 'scroll' | 'visible');
//! ```
//! plus 20+ subtests of the `overflow` shorthand. This translation
//! covers the **longhand-axis subset** (`overflow-x` / `overflow-y`)
//! for all five spec keywords each. Out of scope:
//!
//! - **`overflow` shorthand**: deferred to a future PR (the IR pipeline
//!   has no typed shorthand variant yet, and a bespoke
//!   `convert_overflow_shorthand_into_block` was rolled back in favour
//!   of a generic mechanism that will land later).
//! - **`overflow-block` / `overflow-inline`**: logical-axis properties
//!   not yet wired through `paws-style-ir`'s typed `CssPropertyName`
//!   enum. They fall through to `Other(String)` today and have no
//!   downstream effect; revisit after writing-modes support lands.
//!
//! Upstream `overflow-invalid.html` longhand assertions:
//! ```
//! test_invalid_value("overflow-x", 'visible clip');     // two-value
//! test_invalid_value("overflow-y", 'clip hidden');      // two-value
//! ```
//! Translated below: a two-token value on a longhand drops the
//! declaration and the initial `visible` value is preserved.

use std::rc::Rc;

use rust_wasm_binding::{Element, NodeOps};
use yew::prelude::*;

#[function_component]
fn OverflowLonghandComputedFixture() -> Html {
    html! {
        <>
            // overflow-x cases — one child per spec keyword.
            <div class="x-visible" />
            <div class="x-hidden"  />
            <div class="x-scroll"  />
            <div class="x-auto"    />
            <div class="x-clip"    />
            // overflow-y cases — one child per spec keyword.
            <div class="y-visible" />
            <div class="y-hidden"  />
            <div class="y-scroll"  />
            <div class="y-auto"    />
            <div class="y-clip"    />
            // Invalid two-value form on a longhand — declaration must
            // be dropped at the IR layer (the bespoke `ir_to_overflow`
            // matches only a single `Ident` token slice, so a 2-token
            // list returns `None` and the rule contributes nothing).
            <div class="x-two-values-invalid" />
            <div class="y-two-values-invalid" />
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

        // Each rule sets exactly one longhand declaration via the
        // `css!()` compile-time IR pipeline — the engine path
        // exercised is `paws-style-ir` typed enum → engine
        // `ir_to_overflow` keyword table at
        // `engine/src/style/ir_convert/keyword.rs`. No string round
        // trip, no Stylo parser invocation on this path.
        rust_wasm_binding::apply_css(rust_wasm_binding::css!(
            r#".x-visible { overflow-x: visible; }
               .x-hidden  { overflow-x: hidden;  }
               .x-scroll  { overflow-x: scroll;  }
               .x-auto    { overflow-x: auto;    }
               .x-clip    { overflow-x: clip;    }

               .y-visible { overflow-y: visible; }
               .y-hidden  { overflow-y: hidden;  }
               .y-scroll  { overflow-y: scroll;  }
               .y-auto    { overflow-y: auto;    }
               .y-clip    { overflow-y: clip;    }

               .x-two-values-invalid { overflow-x: visible clip; }
               .y-two-values-invalid { overflow-y: clip hidden;  }
            "#
        ));

        let _app = yew::Renderer::<OverflowLonghandComputedFixture>::with_root(root).render();

        0
    }
}
