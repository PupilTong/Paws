//! Creates a parent `div` with three child `span` elements.
//!
//! Uses both `append_element` (first child) and `append_elements` (batch for
//! the remaining two) to exercise both APIs.
//!
//! Each element gets a distinct background color so the nested tree is
//! visible on rendering backends; `display: block` on the spans forces
//! them into their own line so all four rectangles show up stacked.

use rust_wasm_binding::*;

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            let parent = create_element("div")?;
            append_element(0, parent)?;
            set_inline_style(parent, "padding", "12px")?;
            set_inline_style(parent, "background-color", "#5AC8FA")?;

            // First child — single append
            let child1 = create_element("span")?;
            append_element(parent, child1)?;
            set_inline_style(child1, "display", "block")?;
            set_inline_style(child1, "width", "200px")?;
            set_inline_style(child1, "height", "40px")?;
            set_inline_style(child1, "margin", "6px 0")?;
            set_inline_style(child1, "background-color", "#FF9500")?;

            // Second and third children — batch append
            let child2 = create_element("span")?;
            let child3 = create_element("span")?;
            append_elements(parent, &[child2, child3])?;
            set_inline_style(child2, "display", "block")?;
            set_inline_style(child2, "width", "200px")?;
            set_inline_style(child2, "height", "40px")?;
            set_inline_style(child2, "margin", "6px 0")?;
            set_inline_style(child2, "background-color", "#30D158")?;
            set_inline_style(child3, "display", "block")?;
            set_inline_style(child3, "width", "200px")?;
            set_inline_style(child3, "height", "40px")?;
            set_inline_style(child3, "margin", "6px 0")?;
            set_inline_style(child3, "background-color", "#AF52DE")?;

            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
