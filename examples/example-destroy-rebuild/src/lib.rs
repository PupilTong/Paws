//! Creates two elements, destroys the first, then creates a replacement.
//!
//! Tests that destroyed elements are properly cleaned up and new elements
//! can be created and attached afterwards.

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let parent = create_element("div")?;
        append_element(0, parent)?;

        // Create child, append, then destroy it
        let child_old = create_element("span")?;
        append_element(parent, child_old)?;
        destroy_element(child_old)?;

        // Create replacement child
        let child_new = create_element("p")?;
        append_element(parent, child_new)?;

        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
