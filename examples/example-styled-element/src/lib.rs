//! Creates a `div` with inline width, height, and background-color
//! styles — the smallest example that actually paints something
//! visible on a rendering backend.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let div_id = create_element("div")?;
            append_element(0, div_id)?;
            set_inline_style(div_id, "width", "200px")?;
            set_inline_style(div_id, "height", "100px")?;
            // Without a paint property (background-color / border / text)
            // the resulting CALayer is transparent, so every backend that
            // doesn't add UA default styling renders it as empty space.
            // Set an explicit color so the example is visually verifiable.
            set_inline_style(div_id, "background-color", "#0A84FF")?;
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
