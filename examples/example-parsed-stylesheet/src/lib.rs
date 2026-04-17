//! Creates a `div` and applies a pre-parsed stylesheet via `css!()` + `apply_css()`.
//!
//! The stylesheet sets `display: flex` and `width: 200px` on all divs.

use rust_wasm_binding::*;

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        let div_id = create_element("div")?;
        append_element(0, div_id)?;

        apply_css(css!(
            r#"
            div {
                display: flex;
                width: 200px;
            }
            "#
        ));

        commit()?;
        Ok(0)
    })();

    result.unwrap_or_else(|e| e)
}
