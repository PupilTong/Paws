//! Full pipeline: creates elements with styles and calls `commit()`.
//!
//! Exercises the complete DOM → style → layout path in a single WASM module.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            set_inline_style(div_id, "width", "300px")?;
            set_inline_style(div_id, "height", "150px")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
