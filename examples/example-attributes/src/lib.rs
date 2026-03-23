//! Creates a `div` and sets `class` and `id` attributes on it.

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

    if let Err(e) = set_attribute(div_id, "class", "foo bar") {
        return e;
    }

    if let Err(e) = set_attribute(div_id, "id", "main") {
        return e;
    }

    0
}
