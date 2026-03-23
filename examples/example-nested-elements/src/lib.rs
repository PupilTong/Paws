//! Creates a parent `div` with three child `span` elements.
//!
//! Uses both `append_element` (first child) and `append_elements` (batch for
//! the remaining two) to exercise both APIs.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    // Parent
    let parent = match create_element("div") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(0, parent) {
        return e;
    }

    // First child — single append
    let child1 = match create_element("span") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(parent, child1) {
        return e;
    }

    // Second and third children — batch append
    let child2 = match create_element("span") {
        Ok(id) => id,
        Err(e) => return e,
    };
    let child3 = match create_element("span") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_elements(parent, &[child2, child3]) {
        return e;
    }

    0
}
