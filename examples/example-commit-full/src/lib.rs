//! Full pipeline: creates elements with styles and calls `commit()`.
//!
//! Exercises the complete DOM → style → layout path in a single WASM module.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    // Create root div with explicit dimensions
    let div_id = match create_element("div") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(0, div_id) {
        return e;
    }
    if let Err(e) = set_inline_style(div_id, "width", "300px") {
        return e;
    }
    if let Err(e) = set_inline_style(div_id, "height", "150px") {
        return e;
    }

    // Trigger full style resolution + layout
    if let Err(e) = commit() {
        return e;
    }

    0
}
