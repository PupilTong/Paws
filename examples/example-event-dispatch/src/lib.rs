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
/// argument (passed through from `__paws_invoke_listener`) is unused
/// here — we just create a `<span>` as proof of execution.
fn on_click(_callback_id: i32) {
    // Side effect: create a span element and attach it to the parent.
    // The parent id (1) is the first element created by `run()`.
    let span = create_element("span").expect("create span");
    set_attribute(span, "data-clicked", "true").expect("set attr");
    append_element(1, span).expect("append span");
}

#[no_mangle]
pub extern "C" fn run() -> i32 {
    reset_scratch();

    let result: Result<i32, i32> = (|| {
        // Create DOM structure: document(0) > div(1) > button(2)
        let parent = create_element("div")?;
        append_element(0, parent)?;

        let button = create_element("button")?;
        append_element(parent, button)?;

        // Register the click listener on the button
        let callback_id = register_listener(on_click);
        add_event_listener(
            button,
            "click",
            callback_id as i32,
            EventListenerOptions::new(),
        )?;

        // Dispatch a click event on the button. The host will run the
        // W3C three-phase dispatch, find our listener, and call
        // __paws_invoke_listener(callback_id) which invokes on_click.
        let _not_canceled = dispatch_event(button, "click", true, true, false)?;

        // Verify the side effect: the span should now exist as child of parent.
        match get_first_child(parent) {
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
        }
    })();

    result.unwrap_or_else(|e| e)
}
