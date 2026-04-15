//! Creates a `div` and adds a stylesheet that sets `height: 77px` on all divs.

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let div_id = create_element("div")?;
        append_element(0, div_id)?;
        add_stylesheet("div { height: 77px; }")?;
        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
