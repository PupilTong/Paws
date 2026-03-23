//! Creates a `div` and applies a pre-parsed stylesheet via `css!()` + `apply_css()`.
//!
//! The stylesheet sets `display: flex` and `width: 200px` on all divs.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let div_id = match create_element("div") {
        Ok(id) => id,
        Err(e) => return e,
    };

    if let Err(e) = append_element(0, div_id) {
        return e;
    }

    apply_css(css!(
        r#"
        div {
            display: flex;
            width: 200px;
        }
        "#
    ));

    0
}
