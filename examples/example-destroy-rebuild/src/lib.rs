//! Creates two elements, destroys the first, then creates a replacement.
//!
//! Tests that destroyed elements are properly cleaned up and new elements
//! can be created and attached afterwards.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    // Create parent
    let parent = match create_element("div") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(0, parent) {
        return e;
    }

    // Create child, append, then destroy it
    let child_old = match create_element("span") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(parent, child_old) {
        return e;
    }
    if let Err(e) = destroy_element(child_old) {
        return e;
    }

    // Create replacement child
    let child_new = match create_element("p") {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = append_element(parent, child_new) {
        return e;
    }

    0
}
