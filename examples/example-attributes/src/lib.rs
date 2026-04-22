//! Creates a `div` and sets `class` and `id` attributes on it.
//!
//! Adds a stylesheet that targets both the class and the id selectors so
//! the element has visible dimensions + colour on a rendering backend —
//! proves the attribute pipeline feeds into stylo's selector matching.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            set_attribute(div_id, "class", "foo bar")?;
            set_attribute(div_id, "id", "main")?;
            // `.foo` contributes the size, `#main` contributes the colour —
            // demonstrates that both selectors resolve against the same
            // element.
            add_stylesheet(
                ".foo { width: 220px; height: 100px; } #main { background-color: #AF52DE; }",
            )?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
