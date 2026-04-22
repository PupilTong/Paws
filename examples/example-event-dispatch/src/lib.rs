//! Tests the full host ↔ guest event dispatch pipeline.
//!
//! 1. Creates a parent `<div>` with a child `<button>`.
//! 2. Registers a click listener on the button that creates a `<span>`
//!    sibling as a side effect (proof the callback fired).
//! 3. Dispatches a synthetic click event on the button.
//! 4. Returns 0 if the `<span>` was created (pipeline works), non-zero
//!    otherwise.

use rust_wasm_binding::*;

/// Callback invoked by the host during event dispatch. The `callback_id`
/// argument is unused here — we just create a `<span>` as proof of
/// execution.
fn on_click(_callback_id: i32) {
    // Side effect: create a span element and attach it to the parent.
    // The parent id (1) is the first element created by `run()`.
    let span = create_element("span").expect("create span");
    set_attribute(span, "data-clicked", "true").expect("set attr");
    set_inline_style(span, "display", "block").expect("span display");
    set_inline_style(span, "width", "200px").expect("span w");
    set_inline_style(span, "height", "32px").expect("span h");
    set_inline_style(span, "margin", "8px 0").expect("span m");
    set_inline_style(span, "background-color", "#30D158").expect("span bg");
    append_element(1, span).expect("append span");
}

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            // Create DOM structure: document(0) > div(1) > button(2)
            let parent = create_element("div")?;
            append_element(0, parent)?;
            set_inline_style(parent, "padding", "16px")?;
            set_inline_style(parent, "background-color", "#FF9500")?;

            let button = create_element("button")?;
            append_element(parent, button)?;
            set_inline_style(button, "display", "block")?;
            set_inline_style(button, "width", "200px")?;
            set_inline_style(button, "height", "44px")?;
            set_inline_style(button, "background-color", "#0A84FF")?;

            // Register the click listener on the button
            let callback_id = register_listener(on_click);
            add_event_listener(
                button,
                "click",
                callback_id as i32,
                EventListenerOptions::new(),
            )?;

            // Dispatch a click event on the button. The host will run
            // the W3C three-phase dispatch, find our listener, and
            // call `Guest::invoke_listener(callback_id)` which
            // delegates to `__dispatch_listener` and runs `on_click`.
            let _not_canceled = dispatch_event(button, "click", true, true, false)?;

            // Verify the side effect: the span should now exist as child of parent.
            let verify_result = match get_first_child(parent) {
                // First child is the button (id 2), next sibling should be the span
                Some(first) => match get_next_sibling(first) {
                    Some(span) => {
                        // Verify the span has the attribute we set in the callback
                        match has_attribute(span, "data-clicked") {
                            Ok(true) => Ok(0), // Success: event fired and handler ran
                            _ => Ok(-10),      // Span exists but attribute missing
                        }
                    }
                    None => Ok(-2), // No span sibling — callback didn't fire
                },
                None => Ok(-3), // No children at all
            };
            // Flush ops to the rendering backend so the styled
            // parent/button/span actually paint in the showcase app.
            commit()?;
            verify_result
        })();

        result.unwrap_or_else(|e| e)
    }
}
