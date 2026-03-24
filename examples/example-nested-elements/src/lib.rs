//! Creates a parent `div` with three child `span` elements.
//!
//! Uses both `append_element` (first child) and `append_elements` (batch for
//! the remaining two) to exercise both APIs.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let parent = create_element("div")?;
        append_element(0, parent)?;

        // First child — single append
        let child1 = create_element("span")?;
        append_element(parent, child1)?;

        // Second and third children — batch append
        let child2 = create_element("span")?;
        let child3 = create_element("span")?;
        append_elements(parent, &[child2, child3])?;

        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
