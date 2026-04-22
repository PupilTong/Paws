//! Creates a `div` and applies a pre-parsed stylesheet via `css!()` + `apply_css()`.
//!
//! The stylesheet sets `display: flex` and `width: 200px` on all divs;
//! we also add an explicit `height` and `background-color` so the
//! element is visible on rendering backends.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;

            apply_css(css!(
                r#"
                div {
                    display: flex;
                    width: 200px;
                    height: 100px;
                    background-color: #FFD60A;
                }
                "#
            ));

            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
