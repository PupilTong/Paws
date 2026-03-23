//! Creates a single `div` element and appends it to the document root.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    // Create a div element
    let div_id = match create_element("div") {
        Ok(id) => id,
        Err(e) => return e,
    };

    // Append to document root (id 0)
    if let Err(e) = append_element(0, div_id) {
        return e;
    }

    0
}
