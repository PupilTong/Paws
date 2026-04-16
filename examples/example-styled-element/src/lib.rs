//! Creates a `div` with inline width and height styles.

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let div_id = create_element("div")?;
        append_element(0, div_id)?;
        set_inline_style(div_id, "width", "200px")?;
        set_inline_style(div_id, "height", "100px")?;
        commit()?;
        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
