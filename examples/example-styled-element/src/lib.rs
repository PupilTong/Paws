//! Creates a `div` with inline width and height styles.

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

    if let Err(e) = set_inline_style(div_id, "width", "200px") {
        return e;
    }

    if let Err(e) = set_inline_style(div_id, "height", "100px") {
        return e;
    }

    0
}
