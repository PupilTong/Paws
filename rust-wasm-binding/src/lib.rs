//! Rust WASM binding for Paws host functions.
//!
//! Provides safe wrappers around host-imported functions via the
//! WebAssembly component model. Internally uses `wit_bindgen::generate!`
//! over `../wit/paws.wit` (world `paws-guest`); downstream consumers
//! (examples, Yew fork) call the free functions and/or `paws_main!` —
//! they never see wit-bindgen or the WIT file directly.
//!
//! Targets `wasm32-wasip2` and is packaged as a component by
//! `wasm-component-ld`.

pub use view_macros::css;

// ---------------------------------------------------------------------------
// Generated bindings (wit-bindgen)
// ---------------------------------------------------------------------------
//
// `wit_bindgen::generate!` emits:
//   * `pub mod paws { pub mod host { pub mod dom, events, shadow, stylesheet { ... } } }`
//     — the host-import wrappers, one module per WIT `interface`.
//   * `pub trait Guest { fn run() -> i32; fn invoke_listener(callback_id: i32); }`
//     — the export surface the host calls into.
//   * `export_paws_app!` — macro that generates the component-model export
//     glue for a type implementing `Guest`.
//
// The `pub_export_macro` / `export_macro_name` options rename the default
// `export!` to `export_paws_app!` and make it re-exportable from this crate.

// The generate!() output is placed inside an inner module because
// wit-bindgen 0.45 additionally emits `pub use __export_world_*_cabi;`
// at the same module scope as a `#[macro_export]` macro of the same
// name, which collides at the *crate root* (E0255). Keeping the
// expansion in an inner module keeps the `pub use` scoped to the
// inner module while `#[macro_export]` still makes the outer macro
// reachable as `crate::...`.
pub mod bindings {
    //! Raw generated bindings. Downstream crates should not reach into
    //! this module directly — the crate root re-exports the public
    //! surface ([`Guest`](crate::Guest), [`paws`](crate::paws), and
    //! the [`export_paws_app!`](crate::export_paws_app) /
    //! [`paws_main!`](crate::paws_main) macros).
    wit_bindgen::generate!({
        path: "../wit",
        world: "paws-guest",
        pub_export_macro: true,
        export_macro_name: "export_paws_app",
        default_bindings_module: "rust_wasm_binding::bindings",
    });
}

pub use bindings::{paws, Guest};

// Also expose the wit-bindgen-generated export macro at the crate root.
// `paws_main!` emits `$crate::export_paws_app!(__PawsApp)`, and `$crate::X!`
// macro-path resolution only looks at the crate root — the `pub use` inside
// the `bindings` module alone is not enough. The underlying
// `__export_paws_guest_impl` macro is `#[macro_export]`'d by wit-bindgen so
// it lives at the crate root under that name; we alias it here so the
// `paws_main!` expansion can reach it via `$crate::export_paws_app!`.
#[doc(hidden)]
pub use bindings::export_paws_app;

// ---------------------------------------------------------------------------
// Scratch buffer compatibility shim
// ---------------------------------------------------------------------------

/// Compatibility shim. The old core-module FFI used a scratch buffer
/// for C-string marshalling; the component-model guest no longer has
/// one. Kept as a no-op so existing call sites keep compiling.
#[doc(hidden)]
#[deprecated(note = "no-op under the component-model binding; safe to remove")]
pub fn reset_scratch() {}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Converts a host return code to `Result`: 0 → `Ok(())`, negative → `Err(code)`.
#[inline]
fn check(code: i32) -> Result<(), i32> {
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

// ---------------------------------------------------------------------------
// Safe public wrappers — DOM mutation
// ---------------------------------------------------------------------------

/// Creates a new DOM element with the given tag name.
///
/// Returns the element's numeric ID on success, or a negative host error code.
pub fn create_element(name: &str) -> Result<i32, i32> {
    let id = paws::host::dom::create_element(name);
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Creates a new DOM element with the given namespace URI and tag name.
///
/// Returns the element's numeric ID on success, or a negative host error code.
pub fn create_element_ns(namespace: &str, tag: &str) -> Result<i32, i32> {
    let id = paws::host::dom::create_element_ns(namespace, tag);
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Returns the namespace URI of the given element.
///
/// Returns `Ok(Some(uri))` if the element has a namespace, `Ok(None)` if it
/// has none, or `Err(code)` on host error.
///
/// Note: the old signature `(id, &mut [u8]) -> Result<Option<usize>, i32>`
/// was replaced during the component-model migration — wit-bindgen marshals
/// strings directly, removing the need for caller-provided buffers.
pub fn get_namespace_uri(id: i32) -> Result<Option<String>, i32> {
    paws::host::dom::get_namespace_uri(id)
}

/// Creates a new DOM text node with the given content.
///
/// Returns the node's numeric ID on success, or a negative host error code.
pub fn create_text_node(text: &str) -> Result<i32, i32> {
    let id = paws::host::dom::create_text_node(text);
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Sets an inline CSS property on an element.
pub fn set_inline_style(id: i32, name: &str, value: &str) -> Result<(), i32> {
    check(paws::host::dom::set_inline_style(id, name, value))
}

/// Sets a DOM attribute on an element (e.g. `class`, `id`).
pub fn set_attribute(id: i32, name: &str, value: &str) -> Result<(), i32> {
    check(paws::host::dom::set_attribute(id, name, value))
}

/// Appends a child element to a parent element.
pub fn append_element(parent: i32, child: i32) -> Result<(), i32> {
    check(paws::host::dom::append_element(parent, child))
}

/// Appends multiple children to a parent element in one call.
pub fn append_elements(parent: i32, children: &[i32]) -> Result<(), i32> {
    check(paws::host::dom::append_elements(parent, children))
}

/// Destroys an element and all its descendants.
pub fn destroy_element(id: i32) -> Result<(), i32> {
    check(paws::host::dom::destroy_element(id))
}

/// Adds a CSS stylesheet from a string (parsed at runtime by the host).
pub fn add_stylesheet(css: &str) -> Result<(), i32> {
    check(paws::host::dom::add_stylesheet(css))
}

/// Triggers style resolution and layout computation.
///
/// Returns `Ok(())` on success.
pub fn commit() -> Result<(), i32> {
    check(paws::host::dom::commit())
}

/// Applies a pre-parsed CSS stylesheet (rkyv-encoded IR bytes) to the engine.
///
/// Use with the [`css!`] macro: `apply_css(css!(r#"div { color: red; }"#))`.
pub fn apply_css(css_bytes: &[u8]) {
    paws::host::stylesheet::add_parsed_stylesheet(css_bytes);
}

// ---------------------------------------------------------------------------
// DOM query wrappers
// ---------------------------------------------------------------------------

/// Returns the first child of the given node, or `None` if it has no children.
pub fn get_first_child(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_first_child(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the last child of the given node, or `None` if it has no children.
pub fn get_last_child(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_last_child(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the next sibling of the given node, or `None`.
pub fn get_next_sibling(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_next_sibling(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the previous sibling of the given node, or `None`.
pub fn get_previous_sibling(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_previous_sibling(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the parent element (Element type only), or `None`.
pub fn get_parent_element(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_parent_element(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the parent node (any type), or `None`.
pub fn get_parent_node(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_parent_node(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns whether the node is connected to the document tree.
pub fn is_connected(id: i32) -> Result<bool, i32> {
    match paws::host::dom::is_connected(id) {
        1 => Ok(true),
        0 => Ok(false),
        err => Err(err),
    }
}

/// Returns whether the element has the named attribute.
pub fn has_attribute(id: i32, name: &str) -> Result<bool, i32> {
    match paws::host::dom::has_attribute(id, name) {
        1 => Ok(true),
        0 => Ok(false),
        err => Err(err),
    }
}

/// Reads the value of the named attribute.
///
/// Returns `Ok(Some(value))` if the attribute is set, `Ok(None)` if the
/// attribute is not set, or `Err(code)` on host error.
///
/// Note: the old signature `(id, name, &mut [u8]) -> Result<Option<usize>, i32>`
/// was replaced during the component-model migration — wit-bindgen marshals
/// strings directly, removing the need for caller-provided buffers.
pub fn get_attribute(id: i32, name: &str) -> Result<Option<String>, i32> {
    paws::host::dom::get_attribute(id, name)
}

/// Removes the named attribute from the element.
pub fn remove_attribute(id: i32, name: &str) -> Result<(), i32> {
    check(paws::host::dom::remove_attribute(id, name))
}

/// Removes a child from its parent without deleting the child node.
pub fn remove_child(parent: i32, child: i32) -> Result<(), i32> {
    check(paws::host::dom::remove_child(parent, child))
}

/// Replaces an old child with a new child under the given parent.
pub fn replace_child(parent: i32, new_child: i32, old_child: i32) -> Result<(), i32> {
    check(paws::host::dom::replace_child(parent, new_child, old_child))
}

/// Inserts a new child before a reference child in the parent's children list.
pub fn insert_before(parent: i32, new_child: i32, ref_child: i32) -> Result<(), i32> {
    check(paws::host::dom::insert_before(parent, new_child, ref_child))
}

/// Clones a DOM node. If `deep` is true, all descendants are cloned recursively.
///
/// Returns the new node's ID on success, or a negative error code.
pub fn clone_node(id: i32, deep: bool) -> Result<i32, i32> {
    let result = paws::host::dom::clone_node(id, deep);
    if result < 0 {
        Err(result)
    } else {
        Ok(result)
    }
}

/// Sets the node's text value (for Text and Comment nodes).
///
/// For Element, Document, and ShadowRoot nodes, this is a no-op per the DOM spec.
pub fn set_node_value(id: i32, value: &str) -> Result<(), i32> {
    check(paws::host::dom::set_node_value(id, value))
}

/// Returns the W3C DOM `nodeType` constant for the given node.
///
/// Element=1, Text=3, Comment=8, Document=9, ShadowRoot(DocumentFragment)=11.
/// Returns `None` if the node does not exist.
pub fn get_node_type(id: i32) -> Option<i32> {
    let result = paws::host::dom::get_node_type(id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Event system wrappers
// ---------------------------------------------------------------------------

/// Options for `add_event_listener`.
///
/// Encoded as a bitfield for the host: bit 0 = capture, bit 1 = passive,
/// bit 2 = once. Use the builder methods to set flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventListenerOptions(i32);

impl EventListenerOptions {
    /// Creates default options (no capture, no passive, no once).
    pub fn new() -> Self {
        Self(0)
    }

    /// Registers the listener for the capture phase.
    pub fn capture(mut self) -> Self {
        self.0 |= 0b001;
        self
    }

    /// Marks the listener as passive (cannot call `preventDefault`).
    pub fn passive(mut self) -> Self {
        self.0 |= 0b010;
        self
    }

    /// Automatically removes the listener after its first invocation.
    pub fn once(mut self) -> Self {
        self.0 |= 0b100;
        self
    }
}

/// Registers an event listener on a DOM node.
///
/// `callback_id` is an opaque identifier managed by the guest. When the
/// event fires, the host calls `Guest::invoke_listener(callback_id)`.
pub fn add_event_listener(
    target_id: i32,
    event_type: &str,
    callback_id: i32,
    options: EventListenerOptions,
) -> Result<(), i32> {
    check(paws::host::events::add_event_listener(
        target_id,
        event_type,
        callback_id,
        options.0,
    ))
}

/// Removes an event listener from a DOM node.
///
/// Only the `capture` flag of `options` is used for matching (per W3C spec).
pub fn remove_event_listener(
    target_id: i32,
    event_type: &str,
    callback_id: i32,
    capture: bool,
) -> Result<(), i32> {
    let flags = if capture { 1 } else { 0 };
    check(paws::host::events::remove_event_listener(
        target_id,
        event_type,
        callback_id,
        flags,
    ))
}

/// Dispatches an event on a DOM node using the W3C three-phase algorithm.
///
/// Returns `Ok(true)` if the event was NOT canceled, `Ok(false)` if it was.
pub fn dispatch_event(
    target_id: i32,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
) -> Result<bool, i32> {
    let result =
        paws::host::events::dispatch_event(target_id, event_type, bubbles, cancelable, composed);
    match result {
        1 => Ok(true),  // not canceled
        0 => Ok(false), // canceled
        err => Err(err),
    }
}

/// Stops event propagation to ancestor/descendant nodes.
///
/// Must be called from within an event listener (during dispatch).
pub fn event_stop_propagation() -> Result<(), i32> {
    check(paws::host::events::stop_propagation())
}

/// Stops all remaining listeners, including on the current node.
///
/// Must be called from within an event listener (during dispatch).
pub fn event_stop_immediate_propagation() -> Result<(), i32> {
    check(paws::host::events::stop_immediate_propagation())
}

/// Cancels the event's default action.
///
/// No-op if the event is not cancelable or the listener is passive.
/// Must be called from within an event listener (during dispatch).
pub fn event_prevent_default() -> Result<(), i32> {
    check(paws::host::events::prevent_default())
}

/// Returns the target node ID of the current event, or `None` if no
/// event is being dispatched.
pub fn event_target() -> Option<i32> {
    let result = paws::host::events::target();
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the current target node ID during dispatch, or `None`.
pub fn event_current_target() -> Option<i32> {
    let result = paws::host::events::current_target();
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the current event phase (0=none, 1=capturing, 2=at-target, 3=bubbling).
pub fn event_phase() -> i32 {
    paws::host::events::phase()
}

/// Returns whether the current event bubbles.
pub fn event_bubbles() -> bool {
    paws::host::events::bubbles() == 1
}

/// Returns whether the current event is cancelable.
pub fn event_cancelable() -> bool {
    paws::host::events::cancelable() == 1
}

/// Returns whether `preventDefault()` was called on the current event.
pub fn event_default_prevented() -> bool {
    paws::host::events::default_prevented() == 1
}

/// Returns whether the current event is composed (crosses shadow boundaries).
pub fn event_composed() -> bool {
    paws::host::events::composed() == 1
}

/// Returns the timestamp of the current event in milliseconds.
pub fn event_timestamp() -> f64 {
    paws::host::events::timestamp()
}

// ---------------------------------------------------------------------------
// Listener callback infrastructure
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;

/// Maximum number of listeners that can be registered via [`register_listener`].
const MAX_LISTENERS: usize = 256;

/// Static listener table mapping callback IDs to function pointers.
///
/// Each callback receives its own `callback_id` as an `i32` argument,
/// allowing a single function pointer to serve as a multiplexing
/// dispatcher that looks up per-registration state in a side table.
type ListenerSlots = [Option<fn(i32)>; MAX_LISTENERS];

struct ListenerTable {
    table: UnsafeCell<ListenerSlots>,
    next_id: UnsafeCell<u32>,
}

// SAFETY: WASM is single-threaded; the listener table is never accessed
// concurrently. This impl is required for a static, but no actual sharing
// occurs.
unsafe impl Sync for ListenerTable {}

static LISTENERS: ListenerTable = ListenerTable {
    table: UnsafeCell::new([None; MAX_LISTENERS]),
    next_id: UnsafeCell::new(0),
};

/// Registers a function pointer as an event listener callback.
///
/// The callback receives its own `callback_id` as an argument so that a
/// single function can serve as a dispatcher for many registrations.
///
/// Returns the `callback_id` to pass to [`add_event_listener`].
///
/// # Panics
///
/// Panics if the listener table is full (`MAX_LISTENERS` reached).
pub fn register_listener(callback: fn(i32)) -> u32 {
    // SAFETY: Single-threaded WASM execution — no concurrent access to the
    // listener table. We obtain raw pointers from UnsafeCell and perform
    // bounded writes within the table.
    unsafe {
        let id_ptr = LISTENERS.next_id.get();
        let id = *id_ptr;
        assert!((id as usize) < MAX_LISTENERS, "listener table full");
        let table = &mut *LISTENERS.table.get();
        table[id as usize] = Some(callback);
        *id_ptr = id + 1;
        id
    }
}

/// Removes a previously registered listener, freeing its table slot.
///
/// After this call the `callback_id` is invalid and must not be passed
/// to [`add_event_listener`]. The slot is NOT reused by future
/// [`register_listener`] calls (IDs are monotonically increasing).
pub fn unregister_listener(id: u32) {
    // SAFETY: Single-threaded WASM execution — no concurrent access.
    unsafe {
        let table = &mut *LISTENERS.table.get();
        if let Some(slot) = table.get_mut(id as usize) {
            *slot = None;
        }
    }
}

/// Default `Guest::invoke_listener` dispatcher.
///
/// Looks up `callback_id` in the listener table and, if present, invokes
/// the registered callback with its own `callback_id` as the argument.
///
/// This is the body of the legacy `__paws_invoke_listener` export, now
/// exposed as a free function so a user's `impl Guest for _` can delegate
/// here. The [`paws_main!`] macro does this delegation automatically.
#[doc(hidden)]
pub fn __dispatch_listener(callback_id: i32) {
    // SAFETY: Single-threaded WASM execution — no concurrent access to the
    // listener table. We read a single entry at a bounded index.
    unsafe {
        let table = &*LISTENERS.table.get();
        if let Some(Some(callback)) = table.get(callback_id as usize) {
            callback(callback_id);
        }
    }
}

/// Default `Guest::dump_coverage` body used by [`paws_main!`].
///
/// Returns a fresh profraw snapshot via `minicov::capture_coverage`
/// when both `target_arch = "wasm32"` and the `coverage` Cargo feature
/// are active. Otherwise returns an empty `Vec<u8>` — the WIT export
/// still exists (every `paws-guest` component has it) but the host's
/// extraction step sees zero bytes and short-circuits.
///
/// The `target_arch = "wasm32"` gate is important: enabling `coverage`
/// for a host `cargo llvm-cov --all-features` run would otherwise
/// pull in minicov's `__llvm_profile_runtime`, which collides with
/// compiler-rt's copy. On host builds this function always returns
/// empty bytes regardless of the feature flag.
#[doc(hidden)]
pub fn __dump_coverage() -> Vec<u8> {
    #[cfg(all(target_arch = "wasm32", feature = "coverage"))]
    {
        let mut buffer = Vec::new();
        // SAFETY: `capture_coverage` requires single-threaded use and
        // that every static global-constructor has already run. Paws
        // guests are single-threaded WASM, and this helper is only
        // reachable from `Guest::dump_coverage`, which the host calls
        // AFTER `Guest::run()` returns — so every `#[ctor]`/`lazy_static`
        // has been initialised by then.
        unsafe {
            minicov::capture_coverage(&mut buffer).expect("capture_coverage");
        }
        buffer
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "coverage")))]
    {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Shadow DOM
// ---------------------------------------------------------------------------

/// Attaches a shadow root to the given host element.
///
/// `mode` must be `"open"` or `"closed"`. Returns the shadow root's
/// numeric ID on success, or a negative host error code.
pub fn attach_shadow(host_id: i32, mode: &str) -> Result<i32, i32> {
    let result = paws::host::shadow::attach_shadow(host_id, mode);
    if result >= 0 {
        Ok(result)
    } else {
        Err(result)
    }
}

/// Returns the shadow root ID for the given host element, or `None`.
pub fn get_shadow_root(host_id: i32) -> Option<i32> {
    let result = paws::host::shadow::get_shadow_root(host_id);
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Adds a CSS stylesheet scoped to a shadow root.
pub fn add_shadow_stylesheet(shadow_root_id: i32, css: &str) -> Result<(), i32> {
    check(paws::host::shadow::add_shadow_stylesheet(
        shadow_root_id,
        css,
    ))
}

// ---------------------------------------------------------------------------
// Guest entry-point macro
// ---------------------------------------------------------------------------

/// Ergonomic wrapper that generates a `Guest` implementation and the
/// component-model export glue from a single `run` body.
///
/// The default `invoke_listener` impl delegates to
/// [`__dispatch_listener`] — this is what virtually every guest wants.
///
/// # Example
///
/// ```ignore
/// rust_wasm_binding::paws_main! {
///     fn run() -> i32 {
///         let div_id = rust_wasm_binding::create_element("div")?;
///         rust_wasm_binding::append_element(0, div_id)?;
///         0
///     }
/// }
/// ```
#[macro_export]
macro_rules! paws_main {
    (fn run() -> i32 $body:block) => {
        struct __PawsApp;
        impl $crate::Guest for __PawsApp {
            fn run() -> i32 $body
            fn invoke_listener(callback_id: i32) {
                $crate::__dispatch_listener(callback_id);
            }
            fn dump_coverage() -> ::std::vec::Vec<u8> {
                $crate::__dump_coverage()
            }
        }
        $crate::export_paws_app!(__PawsApp);
    };
}

// ---------------------------------------------------------------------------
// RAII DOM wrapper types (mirrors web-sys naming)
// ---------------------------------------------------------------------------

mod dom;
pub use dom::{
    Element, ElementOps, NodeOps, PawsInputElement, PawsTextAreaElement, Text, NODE_TYPE_COMMENT,
    NODE_TYPE_DOCUMENT, NODE_TYPE_DOCUMENT_FRAGMENT, NODE_TYPE_ELEMENT, NODE_TYPE_TEXT,
};

// ---------------------------------------------------------------------------
// Tests (run on host, not wasm — only test the macro / IR round-trip)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use view_macros::css;

    #[test]
    fn test_css_macro_outputs_bytes() {
        let stylesheet_bytes = css!(
            r#"
            div {
                color: red;
                display: flex;
            }
            .classy {
                font-size: 16px;
            }
            "#
        );

        assert!(
            !stylesheet_bytes.is_empty(),
            "CSS macro should generate byte slice"
        );

        let ir =
            rkyv::from_bytes::<paws_style_ir::StyleSheetIR, rkyv::rancor::Error>(stylesheet_bytes)
                .unwrap();
        assert_eq!(ir.rules.len(), 2);

        match &ir.rules[0] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, "div");
                assert_eq!(s.declarations.len(), 2);
                assert_eq!(
                    s.declarations[0].name,
                    paws_style_ir::CssPropertyName::Color
                );
                match &s.declarations[0].value {
                    paws_style_ir::PropertyValueIR::Raw(tokens) => match &tokens[..] {
                        [paws_style_ir::CssToken::Ident(val)] => {
                            assert_eq!(val, "red");
                        }
                        other => panic!("Expected Raw Ident token, got: {other:?}"),
                    },
                    other => panic!("Expected Raw value for color, got: {other:?}"),
                }
            }
            _ => panic!("Expected Style rule"),
        }

        match &ir.rules[1] {
            paws_style_ir::CssRuleIR::Style(s) => {
                assert_eq!(s.selectors, ".classy");
            }
            _ => panic!("Expected Style rule"),
        }
    }
}
