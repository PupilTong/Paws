//! Rust WASM binding for Paws host functions.
//!
//! Provides safe wrappers around all host-imported functions that WASM guests
//! can call to manipulate the DOM, set styles, and trigger layout.
//!
//! Targets `wasm32-wasip1` and links `std` (wasi-libc). Earlier revisions
//! were `#![no_std]`, but minicov coverage instrumentation and the yew fork
//! both needed `std` anyway — keeping two build modes added complexity for
//! no real binary-size savings.

pub use view_macros::css;

// ---------------------------------------------------------------------------
// Raw extern declarations (private)
// ---------------------------------------------------------------------------

#[link(wasm_import_module = "env")]
extern "C" {
    fn __create_element(name_ptr: *const u8) -> i32;
    fn __create_element_ns(ns_ptr: *const u8, tag_ptr: *const u8) -> i32;
    fn __get_namespace_uri(id: i32, buf_ptr: *mut u8, buf_len: i32) -> i32;
    fn __create_text_node(text_ptr: *const u8) -> i32;
    fn __set_inline_style(id: i32, name_ptr: *const u8, value_ptr: *const u8) -> i32;
    fn __set_attribute(id: i32, name_ptr: *const u8, value_ptr: *const u8) -> i32;
    fn __append_element(parent: i32, child: i32) -> i32;
    fn __append_elements(parent: i32, ptr: *const i32, len: i32) -> i32;
    fn __destroy_element(id: i32) -> i32;
    fn __add_stylesheet(css_ptr: *const u8) -> i32;
    fn __commit() -> i32;
    fn __get_first_child(id: i32) -> i32;
    fn __get_last_child(id: i32) -> i32;
    fn __get_next_sibling(id: i32) -> i32;
    fn __get_previous_sibling(id: i32) -> i32;
    fn __get_parent_element(id: i32) -> i32;
    fn __get_parent_node(id: i32) -> i32;
    fn __is_connected(id: i32) -> i32;
    fn __has_attribute(id: i32, name_ptr: *const u8) -> i32;
    fn __get_attribute(id: i32, name_ptr: *const u8, buf_ptr: *mut u8, buf_len: i32) -> i32;
    fn __remove_attribute(id: i32, name_ptr: *const u8) -> i32;
    fn __remove_child(parent: i32, child: i32) -> i32;
    fn __replace_child(parent: i32, new_child: i32, old_child: i32) -> i32;
    fn __insert_before(parent: i32, new_child: i32, ref_child: i32) -> i32;
    fn __clone_node(id: i32, deep: i32) -> i32;
    fn __set_node_value(id: i32, value_ptr: *const u8) -> i32;
    fn __get_node_type(id: i32) -> i32;

    // Event system
    fn __add_event_listener(
        target_id: i32,
        type_ptr: *const u8,
        callback_id: i32,
        options_flags: i32,
    ) -> i32;
    fn __remove_event_listener(
        target_id: i32,
        type_ptr: *const u8,
        callback_id: i32,
        options_flags: i32,
    ) -> i32;
    fn __dispatch_event(
        target_id: i32,
        type_ptr: *const u8,
        bubbles: i32,
        cancelable: i32,
        composed: i32,
    ) -> i32;
    fn __event_stop_propagation() -> i32;
    fn __event_stop_immediate_propagation() -> i32;
    fn __event_prevent_default() -> i32;
    fn __event_target() -> i32;
    fn __event_current_target() -> i32;
    fn __event_phase() -> i32;
    fn __event_bubbles() -> i32;
    fn __event_cancelable() -> i32;
    fn __event_default_prevented() -> i32;
    fn __event_composed() -> i32;
    fn __event_timestamp() -> f64;

    // Shadow DOM
    fn __attach_shadow(host_id: i32, mode_ptr: *const u8) -> i32;
    fn __get_shadow_root(host_id: i32) -> i32;
    fn __add_shadow_stylesheet(shadow_root_id: i32, css_ptr: *const u8) -> i32;
}

#[link(wasm_import_module = "paws")]
extern "C" {
    fn paws_add_parsed_stylesheet(ptr: *const u8, len: usize);
}

// ---------------------------------------------------------------------------
// Scratch buffer for C-string passing
// ---------------------------------------------------------------------------

const SCRATCH_SIZE: usize = 8192;

use core::cell::UnsafeCell;

struct ScratchBuffer {
    buf: UnsafeCell<[u8; SCRATCH_SIZE]>,
    offset: UnsafeCell<usize>,
}

// SAFETY: WASM is single-threaded; the scratch buffer is never accessed
// concurrently. This impl is required for a static, but no actual sharing
// occurs.
unsafe impl Sync for ScratchBuffer {}

static SCRATCH: ScratchBuffer = ScratchBuffer {
    buf: UnsafeCell::new([0; SCRATCH_SIZE]),
    offset: UnsafeCell::new(0),
};

/// Writes a Rust `&str` into the scratch buffer as a null-terminated C-string.
///
/// Returns a pointer into WASM linear memory that the host can read.
///
/// # Panics
///
/// Panics if the scratch buffer does not have enough space for `s.len() + 1`
/// bytes. Call [`reset_scratch`] to reclaim space.
fn write_cstr(s: &str) -> *const u8 {
    let needed = s.len() + 1; // +1 for null terminator

    // SAFETY: Single-threaded WASM execution — no concurrent access to the
    // scratch buffer. We obtain raw pointers from UnsafeCell and perform
    // bounded writes within the buffer.
    unsafe {
        let offset_ptr = SCRATCH.offset.get();
        let off = *offset_ptr;
        assert!(off + needed <= SCRATCH_SIZE, "scratch buffer overflow");
        let buf_ptr = SCRATCH.buf.get() as *mut u8;
        let dst = buf_ptr.add(off);
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
        *dst.add(s.len()) = 0; // null terminator
        *offset_ptr = off + needed;
        dst as *const u8
    }
}

/// Resets the scratch buffer offset to zero, reclaiming all space.
///
/// Call this at the start of each frame or operation batch.
pub fn reset_scratch() {
    // SAFETY: Single-threaded WASM execution — no concurrent access.
    unsafe {
        *SCRATCH.offset.get() = 0;
    }
}

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
// Safe public wrappers
// ---------------------------------------------------------------------------

/// Creates a new DOM element with the given tag name.
///
/// Returns the element's numeric ID on success, or a negative host error code.
pub fn create_element(name: &str) -> Result<i32, i32> {
    let ptr = write_cstr(name);
    // SAFETY: `ptr` points to a null-terminated string in WASM linear memory.
    // The host reads from this memory region during the call.
    let id = unsafe { __create_element(ptr) };
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
    let ns_ptr = write_cstr(namespace);
    let tag_ptr = write_cstr(tag);
    // SAFETY: Both pointers point to null-terminated strings in WASM linear memory.
    // The host reads from these memory regions during the call.
    let id = unsafe { __create_element_ns(ns_ptr, tag_ptr) };
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Returns the namespace URI of the given element.
///
/// Returns `Ok(Some(len))` with the namespace written to `buf` if it fits,
/// `Ok(Some(len))` with `len > buf.len()` if the buffer is too small (namespace
/// not written), `Ok(None)` if the element has no namespace, or `Err(code)` on
/// error.
pub fn get_namespace_uri(id: i32, buf: &mut [u8]) -> Result<Option<usize>, i32> {
    // SAFETY: `buf` is a valid mutable slice in WASM linear memory.
    // The host writes into this region during the call.
    let result = unsafe { __get_namespace_uri(id, buf.as_mut_ptr(), buf.len() as i32) };
    if result == -1 {
        Ok(None)
    } else if result < -1 {
        Err(result)
    } else {
        Ok(Some(result as usize))
    }
}

/// Creates a new DOM text node with the given content.
///
/// Returns the node's numeric ID on success, or a negative host error code.
pub fn create_text_node(text: &str) -> Result<i32, i32> {
    let ptr = write_cstr(text);
    // SAFETY: `ptr` points to a null-terminated string in WASM linear memory.
    // The host reads from this memory region during the call.
    let id = unsafe { __create_text_node(ptr) };
    if id < 0 {
        Err(id)
    } else {
        Ok(id)
    }
}

/// Sets an inline CSS property on an element.
pub fn set_inline_style(id: i32, name: &str, value: &str) -> Result<(), i32> {
    let name_ptr = write_cstr(name);
    let value_ptr = write_cstr(value);
    // SAFETY: Both pointers are null-terminated strings in WASM linear memory.
    let code = unsafe { __set_inline_style(id, name_ptr, value_ptr) };
    check(code)
}

/// Sets a DOM attribute on an element (e.g. `class`, `id`).
pub fn set_attribute(id: i32, name: &str, value: &str) -> Result<(), i32> {
    let name_ptr = write_cstr(name);
    let value_ptr = write_cstr(value);
    // SAFETY: Both pointers are null-terminated strings in WASM linear memory.
    let code = unsafe { __set_attribute(id, name_ptr, value_ptr) };
    check(code)
}

/// Appends a child element to a parent element.
pub fn append_element(parent: i32, child: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let code = unsafe { __append_element(parent, child) };
    check(code)
}

/// Appends multiple children to a parent element in one call.
///
/// The `children` slice is passed as a contiguous i32 array in WASM linear memory.
pub fn append_elements(parent: i32, children: &[i32]) -> Result<(), i32> {
    // SAFETY: `children.as_ptr()` points to a valid i32 slice in WASM linear
    // memory. The host reads `len` i32 values starting from this pointer.
    let code = unsafe { __append_elements(parent, children.as_ptr(), children.len() as i32) };
    check(code)
}

/// Destroys an element and all its descendants.
pub fn destroy_element(id: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let code = unsafe { __destroy_element(id) };
    check(code)
}

/// Adds a CSS stylesheet from a string (parsed at runtime by the host).
pub fn add_stylesheet(css: &str) -> Result<(), i32> {
    let ptr = write_cstr(css);
    // SAFETY: `ptr` points to a null-terminated CSS string in WASM linear memory.
    let code = unsafe { __add_stylesheet(ptr) };
    check(code)
}

/// Triggers style resolution and layout computation.
///
/// Returns `Ok(())` on success.
pub fn commit() -> Result<(), i32> {
    // SAFETY: No arguments — triggers host-side style+layout pass.
    let code = unsafe { __commit() };
    check(code)
}

/// Applies a pre-parsed CSS stylesheet (rkyv-encoded IR bytes) to the engine.
///
/// Use with the [`css!`] macro: `apply_css(css!(r#"div { color: red; }"#))`.
pub fn apply_css(css_bytes: &[u8]) {
    // SAFETY: `css_bytes` is a valid byte slice in WASM linear memory.
    // The host reads `len` bytes starting from `ptr`.
    unsafe {
        paws_add_parsed_stylesheet(css_bytes.as_ptr(), css_bytes.len());
    }
}

// ---------------------------------------------------------------------------
// DOM query wrappers
// ---------------------------------------------------------------------------

/// Returns the first child of the given node, or `None` if it has no children.
pub fn get_first_child(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_first_child(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the last child of the given node, or `None` if it has no children.
pub fn get_last_child(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_last_child(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the next sibling of the given node, or `None`.
pub fn get_next_sibling(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_next_sibling(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the previous sibling of the given node, or `None`.
pub fn get_previous_sibling(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_previous_sibling(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the parent element (Element type only), or `None`.
pub fn get_parent_element(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_parent_element(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the parent node (any type), or `None`.
pub fn get_parent_node(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_parent_node(id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns whether the node is connected to the document tree.
pub fn is_connected(id: i32) -> Result<bool, i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __is_connected(id) };
    match result {
        1 => Ok(true),
        0 => Ok(false),
        err => Err(err),
    }
}

/// Returns whether the element has the named attribute.
pub fn has_attribute(id: i32, name: &str) -> Result<bool, i32> {
    let name_ptr = write_cstr(name);
    // SAFETY: `name_ptr` points to a null-terminated string in WASM linear memory.
    let result = unsafe { __has_attribute(id, name_ptr) };
    match result {
        1 => Ok(true),
        0 => Ok(false),
        err => Err(err),
    }
}

/// Reads the value of the named attribute into `buf`.
///
/// Returns `Ok(Some(len))` with the byte length of the attribute value on
/// success. If `buf` is large enough the value is written into it; otherwise
/// only the needed length is returned (no write). Returns `Ok(None)` if the
/// attribute does not exist.
pub fn get_attribute(id: i32, name: &str, buf: &mut [u8]) -> Result<Option<usize>, i32> {
    let name_ptr = write_cstr(name);
    // SAFETY: `name_ptr` is a null-terminated string. `buf` is a valid mutable
    // byte slice in WASM linear memory.
    let result = unsafe { __get_attribute(id, name_ptr, buf.as_mut_ptr(), buf.len() as i32) };
    if result >= 0 {
        Ok(Some(result as usize))
    } else if result == -1 {
        Ok(None)
    } else {
        Err(result)
    }
}

/// Removes the named attribute from the element.
pub fn remove_attribute(id: i32, name: &str) -> Result<(), i32> {
    let name_ptr = write_cstr(name);
    // SAFETY: `name_ptr` points to a null-terminated string in WASM linear memory.
    let code = unsafe { __remove_attribute(id, name_ptr) };
    check(code)
}

/// Removes a child from its parent without deleting the child node.
pub fn remove_child(parent: i32, child: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let code = unsafe { __remove_child(parent, child) };
    check(code)
}

/// Replaces an old child with a new child under the given parent.
pub fn replace_child(parent: i32, new_child: i32, old_child: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let code = unsafe { __replace_child(parent, new_child, old_child) };
    check(code)
}

/// Inserts a new child before a reference child in the parent's children list.
pub fn insert_before(parent: i32, new_child: i32, ref_child: i32) -> Result<(), i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let code = unsafe { __insert_before(parent, new_child, ref_child) };
    check(code)
}

/// Clones a DOM node. If `deep` is true, all descendants are cloned recursively.
///
/// Returns the new node's ID on success, or a negative error code.
pub fn clone_node(id: i32, deep: bool) -> Result<i32, i32> {
    // SAFETY: No memory pointers involved — only integer IDs.
    let result = unsafe { __clone_node(id, if deep { 1 } else { 0 }) };
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
    let value_ptr = write_cstr(value);
    // SAFETY: `value_ptr` points to a null-terminated string in WASM linear memory.
    let code = unsafe { __set_node_value(id, value_ptr) };
    check(code)
}

/// Returns the W3C DOM `nodeType` constant for the given node.
///
/// Element=1, Text=3, Comment=8, Document=9, ShadowRoot(DocumentFragment)=11.
/// Returns `None` if the node does not exist.
pub fn get_node_type(id: i32) -> Option<i32> {
    // SAFETY: No memory pointers involved — only integer ID.
    let result = unsafe { __get_node_type(id) };
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
/// event fires, the host calls `__paws_invoke_listener(callback_id)`.
pub fn add_event_listener(
    target_id: i32,
    event_type: &str,
    callback_id: i32,
    options: EventListenerOptions,
) -> Result<(), i32> {
    let type_ptr = write_cstr(event_type);
    // SAFETY: `type_ptr` points to a null-terminated string in WASM linear memory.
    let code = unsafe { __add_event_listener(target_id, type_ptr, callback_id, options.0) };
    check(code)
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
    let type_ptr = write_cstr(event_type);
    let flags = if capture { 1 } else { 0 };
    // SAFETY: `type_ptr` points to a null-terminated string in WASM linear memory.
    let code = unsafe { __remove_event_listener(target_id, type_ptr, callback_id, flags) };
    check(code)
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
    let type_ptr = write_cstr(event_type);
    // SAFETY: `type_ptr` points to a null-terminated string in WASM linear memory.
    let result = unsafe {
        __dispatch_event(
            target_id,
            type_ptr,
            bubbles as i32,
            cancelable as i32,
            composed as i32,
        )
    };
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
    // SAFETY: No memory pointers involved.
    let code = unsafe { __event_stop_propagation() };
    check(code)
}

/// Stops all remaining listeners, including on the current node.
///
/// Must be called from within an event listener (during dispatch).
pub fn event_stop_immediate_propagation() -> Result<(), i32> {
    // SAFETY: No memory pointers involved.
    let code = unsafe { __event_stop_immediate_propagation() };
    check(code)
}

/// Cancels the event's default action.
///
/// No-op if the event is not cancelable or the listener is passive.
/// Must be called from within an event listener (during dispatch).
pub fn event_prevent_default() -> Result<(), i32> {
    // SAFETY: No memory pointers involved.
    let code = unsafe { __event_prevent_default() };
    check(code)
}

/// Returns the target node ID of the current event, or `None` if no
/// event is being dispatched.
pub fn event_target() -> Option<i32> {
    // SAFETY: No memory pointers involved.
    let result = unsafe { __event_target() };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the current target node ID during dispatch, or `None`.
pub fn event_current_target() -> Option<i32> {
    // SAFETY: No memory pointers involved.
    let result = unsafe { __event_current_target() };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Returns the current event phase (0=none, 1=capturing, 2=at-target, 3=bubbling).
pub fn event_phase() -> i32 {
    // SAFETY: No memory pointers involved.
    unsafe { __event_phase() }
}

/// Returns whether the current event bubbles.
pub fn event_bubbles() -> bool {
    // SAFETY: No memory pointers involved.
    unsafe { __event_bubbles() == 1 }
}

/// Returns whether the current event is cancelable.
pub fn event_cancelable() -> bool {
    // SAFETY: No memory pointers involved.
    unsafe { __event_cancelable() == 1 }
}

/// Returns whether `preventDefault()` was called on the current event.
pub fn event_default_prevented() -> bool {
    // SAFETY: No memory pointers involved.
    unsafe { __event_default_prevented() == 1 }
}

/// Returns whether the current event is composed (crosses shadow boundaries).
pub fn event_composed() -> bool {
    // SAFETY: No memory pointers involved.
    unsafe { __event_composed() == 1 }
}

/// Returns the timestamp of the current event in milliseconds.
pub fn event_timestamp() -> f64 {
    // SAFETY: No memory pointers involved.
    unsafe { __event_timestamp() }
}

// ---------------------------------------------------------------------------
// Listener callback infrastructure
// ---------------------------------------------------------------------------

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

/// WASM export called by the host during event dispatch to invoke a listener.
///
/// The host calls this function for each matching listener during the
/// three-phase dispatch algorithm. The `callback_id` maps to a function
/// pointer registered via [`register_listener`] and is passed through
/// to the callback as its sole argument.
#[export_name = "__paws_invoke_listener"]
pub extern "C" fn paws_invoke_listener(callback_id: i32) {
    // SAFETY: Single-threaded WASM execution — no concurrent access to the
    // listener table. We read a single entry at a bounded index.
    unsafe {
        let table = &*LISTENERS.table.get();
        if let Some(Some(callback)) = table.get(callback_id as usize) {
            callback(callback_id);
        }
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
    let mode_ptr = write_cstr(mode);
    let result = unsafe { __attach_shadow(host_id, mode_ptr) };
    if result >= 0 {
        Ok(result)
    } else {
        Err(result)
    }
}

/// Returns the shadow root ID for the given host element, or `None`.
pub fn get_shadow_root(host_id: i32) -> Option<i32> {
    let result = unsafe { __get_shadow_root(host_id) };
    if result >= 0 {
        Some(result)
    } else {
        None
    }
}

/// Adds a CSS stylesheet scoped to a shadow root.
pub fn add_shadow_stylesheet(shadow_root_id: i32, css: &str) -> Result<(), i32> {
    let css_ptr = write_cstr(css);
    let code = unsafe { __add_shadow_stylesheet(shadow_root_id, css_ptr) };
    check(code)
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
// Coverage instrumentation (opt-in via `coverage` feature)
// ---------------------------------------------------------------------------

/// WASM exports for extracting LLVM coverage data from the guest.
///
/// When the `coverage` feature is enabled and the guest is compiled with
/// `RUSTFLAGS="-Cinstrument-coverage -Zno-profiler-runtime"`, minicov
/// collects profraw data at runtime. The host can call these exports after
/// the guest `run()` function returns:
///
/// 1. `__paws_dump_coverage()` → serialises profraw into a buffer, returns
///    its byte length.
/// 2. `__paws_coverage_ptr()` → returns the pointer to that buffer in WASM
///    linear memory so the host can read the bytes.
#[cfg(all(feature = "coverage", target_arch = "wasm32"))]
mod coverage_export {
    use std::cell::UnsafeCell;

    struct CoverageBuffer {
        data: UnsafeCell<Option<Vec<u8>>>,
    }

    // SAFETY: WASM is single-threaded; no concurrent access occurs.
    unsafe impl Sync for CoverageBuffer {}

    static COVERAGE_BUFFER: CoverageBuffer = CoverageBuffer {
        data: UnsafeCell::new(None),
    };

    /// Captures LLVM coverage data and stores it in a static buffer.
    ///
    /// Returns the byte length of the profraw data, or 0 if capture fails.
    #[export_name = "__paws_dump_coverage"]
    pub extern "C" fn dump_coverage() -> i32 {
        // SAFETY: Single-threaded WASM execution — no concurrent access.
        unsafe {
            let mut buffer = Vec::new();
            if minicov::capture_coverage(&mut buffer).is_err() {
                return 0;
            }
            let length = buffer.len() as i32;
            *COVERAGE_BUFFER.data.get() = Some(buffer);
            length
        }
    }

    /// Returns the pointer to the profraw buffer in WASM linear memory.
    ///
    /// Must be called after [`dump_coverage`]. Returns 0 if no data is
    /// available.
    #[export_name = "__paws_coverage_ptr"]
    pub extern "C" fn coverage_ptr() -> i32 {
        // SAFETY: Single-threaded WASM execution — no concurrent access.
        unsafe {
            match &*COVERAGE_BUFFER.data.get() {
                Some(buffer) => buffer.as_ptr() as i32,
                None => 0,
            }
        }
    }
}

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
