//! Minimal component-model twin of `example-basic-element`.
//!
//! Imports host functions via `wit_bindgen::generate!` over
//! `../../wit/paws.wit` and exports `run` / `invoke-listener` to satisfy
//! the `paws-guest` world. The body does the same thing the original
//! core-module example does: create a `<div>`, append it under the
//! document root (id 0), return 0.

wit_bindgen::generate!({
    path: "../../wit",
    world: "paws-guest",
});

struct App;

impl Guest for App {
    fn run() -> i32 {
        let div_id = paws::host::dom::create_element("div");
        if div_id < 0 {
            return div_id;
        }
        let result = paws::host::dom::append_element(0, div_id);
        if result < 0 {
            return result;
        }
        0
    }

    /// Required by the world; this example registers no listeners, so
    /// the host will never actually call it.
    fn invoke_listener(_callback_id: i32) {}
}

export!(App);
