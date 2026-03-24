//! Creates a single `div` element and appends it to the document root.

#![no_std]

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let div_id = create_element("div")?;
        append_element(0, div_id)?;
        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
