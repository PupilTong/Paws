//! Full pipeline: creates elements with styles and calls `commit()`.
//!
//! Exercises the complete DOM → style → layout path in a single WASM module.
//! Adds a background color so the 300×150 rectangle is visible on a
//! rendering backend (the width/height dimensions are load-bearing for
//! the e2e test and stay unchanged).

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            set_inline_style(div_id, "width", "300px")?;
            set_inline_style(div_id, "height", "150px")?;
            set_inline_style(div_id, "background-color", "#00C7BE")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
