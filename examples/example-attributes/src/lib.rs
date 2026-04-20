//! Creates a `div` and sets `class` and `id` attributes on it.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            set_attribute(div_id, "class", "foo bar")?;
            set_attribute(div_id, "id", "main")?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
