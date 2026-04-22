//! Creates two elements, destroys the first, then creates a replacement.
//!
//! Tests that destroyed elements are properly cleaned up and new elements
//! can be created and attached afterwards.
//!
//! Parent and the replacement get distinct background colors so the
//! tree is visible on a rendering backend — the destroyed element is
//! never attached by the time commit runs, so it never paints.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let parent = create_element("div")?;
            append_element(0, parent)?;
            set_inline_style(parent, "padding", "16px")?;
            set_inline_style(parent, "background-color", "#32ADE6")?;

            // Create child, append, then destroy it
            let child_old = create_element("span")?;
            append_element(parent, child_old)?;
            destroy_element(child_old)?;

            // Create replacement child
            let child_new = create_element("p")?;
            append_element(parent, child_new)?;
            set_inline_style(child_new, "width", "200px")?;
            set_inline_style(child_new, "height", "60px")?;
            set_inline_style(child_new, "background-color", "#FF3B30")?;

            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
