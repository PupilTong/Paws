//! Host-driven click target.
//!
//! Lays out a single styled button at a known position/size and registers
//! a click listener that mutates the DOM as a visible side effect: sets
//! `data-clicked="true"` on the button **and** appends a coloured marker
//! `<span>` whose creation/style flows through the iOS renderer's op
//! buffer (`DeclareView` + `SetBgColor`). The attribute is the
//! data-binding signal the wasmtime-engine integration test reads from
//! `RuntimeState`; the appended span is the visual signal the iOS-FFI
//! integration test reads from the ops buffer.
//!
//! Unlike [`example-event-dispatch`], this guest never dispatches the
//! event itself: `run()` returns immediately after wiring the listener
//! and committing the initial DOM. The host (e.g. an iOS UIKit tap, the
//! `paws_renderer_dispatch_click` FFI integration test) is what fires
//! the click. If the listener's side effect appears in the post-click
//! op stream, the entire host → hit-test → dispatch → guest re-entry
//! pipeline is healthy.

use rust_wasm_binding::*;

/// Click handler. The `callback_id` is unused — we just produce two
/// post-conditions the host can observe.
fn on_click(_callback_id: i32) {
    // Element id 2 is the button (id 1 is the wrapper, id 0 is the
    // document root). The attribute is checked by tests that inspect
    // `RuntimeState` directly.
    set_attribute(2, "data-clicked", "true").expect("set data-clicked");

    // Visible side effect for renderer-level tests: append a marker span
    // with a distinctive background colour as a child of the wrapper
    // (id 1) so it lives inside the iOS renderer's render-root subtree.
    // The iOS view-tree diff turns this into DeclareView + SetBgColor
    // in the next commit's op buffer — the smallest reliable signal the
    // FFI test can grep for.
    let marker = create_element("span").expect("create marker");
    set_inline_style(marker, "display", "block").expect("marker display");
    set_inline_style(marker, "width", "32px").expect("marker w");
    set_inline_style(marker, "height", "32px").expect("marker h");
    set_inline_style(marker, "background-color", "#30D158").expect("marker bg");
    append_element(1, marker).expect("append marker to wrapper");

    commit().expect("commit after click");
}

rust_wasm_binding::paws_main! {
    fn run() -> i32 {
        let result: Result<i32, i32> = (|| {
            // Wrapper div is the iOS renderer's render-root (the first
            // element child of the document). All hit-testable content
            // — including the post-click marker — lives inside it so the
            // view-tree diff actually walks them.
            let wrapper = create_element("div")?;
            append_element(0, wrapper)?;
            set_inline_style(wrapper, "position", "relative")?;
            set_inline_style(wrapper, "width", "300px")?;
            set_inline_style(wrapper, "height", "300px")?;

            // Button at (10, 10), 200×44 inside the wrapper.
            let button = create_element("button")?;
            append_element(wrapper, button)?;
            set_inline_style(button, "position", "absolute")?;
            set_inline_style(button, "left", "10px")?;
            set_inline_style(button, "top", "10px")?;
            set_inline_style(button, "width", "200px")?;
            set_inline_style(button, "height", "44px")?;
            set_inline_style(button, "background-color", "#0A84FF")?;

            let callback_id = register_listener(on_click);
            add_event_listener(
                button,
                "click",
                callback_id as i32,
                EventListenerOptions::new(),
            )?;

            // Flush the initial DOM so the host paints the button before
            // any pointer event arrives.
            commit()?;
            Ok(0)
        })();

        result.unwrap_or_else(|e| e)
    }
}
