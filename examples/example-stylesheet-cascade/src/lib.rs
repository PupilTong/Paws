//! Creates a `div` and adds a stylesheet that sets `height: 77px` on all divs.
//!
//! The stylesheet also adds a width and background color so the element
//! is visible on rendering backends; the `height: 77px` rule — the
//! fixture's reason for existing — is preserved unchanged.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            add_stylesheet(
                "div { height: 77px; width: 240px; background-color: #FF2D55; }",
            )?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
