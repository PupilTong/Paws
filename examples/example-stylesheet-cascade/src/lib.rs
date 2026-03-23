//! Creates a `div` and adds a stylesheet that sets `height: 77px` on all divs.

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

    if let Err(e) = add_stylesheet("div { height: 77px; }") {
        return e;
    }

    0
}
