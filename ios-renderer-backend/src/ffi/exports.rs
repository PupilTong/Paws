//! FFI exports: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//!
//! These form the public C API exposed via the cbindgen-generated header.
//! Naming convention: `paws_renderer_*`.

use std::ffi::{c_char, c_void, CStr};

use engine::RuntimeState;

use crate::error::RendererError;
use crate::renderer::ViewTree;

/// Extracts a mutable reference to `PawsRenderer` from a raw pointer,
/// returning the given error code if the pointer is null.
macro_rules! get_renderer {
    ($renderer:expr) => {
        match unsafe_renderer($renderer) {
            Some(r) => r,
            None => return RendererError::InvalidHandle.as_i32(),
        }
    };
}

/// Reads a null-terminated C string from a raw pointer,
/// returning the given error code if the pointer is null or not valid UTF-8.
macro_rules! get_cstr {
    ($ptr:expr) => {
        match read_cstr($ptr) {
            Some(s) => s,
            None => return RendererError::InvalidHandle.as_i32(),
        }
    };
}

/// Opaque handle to the Paws renderer state.
///
/// Owns the engine's `RuntimeState` and the UIKit view tree mapping.
/// Created by [`paws_renderer_create`] and destroyed by [`paws_renderer_destroy`].
pub struct PawsRenderer {
    state: RuntimeState,
    view_tree: ViewTree,
}

/// Creates a new `PawsRenderer`.
///
/// `base_url` must be a null-terminated UTF-8 string (used as the document base URL).
/// Returns an opaque pointer. The caller (Swift) owns this and must call
/// [`paws_renderer_destroy`] to free it.
///
/// Returns `null` on failure.
#[no_mangle]
pub extern "C" fn paws_renderer_create(base_url: *const c_char) -> *mut PawsRenderer {
    let url_str = if base_url.is_null() {
        "about:blank"
    } else {
        // SAFETY: Caller guarantees a valid null-terminated UTF-8 C string.
        match unsafe { CStr::from_ptr(base_url) }.to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let renderer = PawsRenderer {
        state: RuntimeState::new(url_str.to_string()),
        view_tree: ViewTree::new(),
    };

    Box::into_raw(Box::new(renderer))
}

/// Destroys a `PawsRenderer` and frees all associated memory.
///
/// After this call the pointer is invalid. Passing `null` is a no-op.
#[no_mangle]
pub extern "C" fn paws_renderer_destroy(renderer: *mut PawsRenderer) {
    if !renderer.is_null() {
        // SAFETY: Pointer was created by Box::into_raw in paws_renderer_create.
        drop(unsafe { Box::from_raw(renderer) });
    }
}

/// Creates a DOM element with the given tag name.
///
/// `tag` must be a null-terminated UTF-8 string.
/// Returns the element's node ID (>0) on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_create_element(
    renderer: *mut PawsRenderer,
    tag: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let tag_str = get_cstr!(tag);
    renderer.state.create_element(tag_str.to_string()) as i32
}

/// Creates a text node with the given content.
///
/// `text` must be a null-terminated UTF-8 string.
/// Returns the node ID (>0) on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_create_text_node(
    renderer: *mut PawsRenderer,
    text: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let text_str = get_cstr!(text);
    renderer.state.create_text_node(text_str.to_string()) as i32
}

/// Appends a child element to a parent element.
///
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_append_element(
    renderer: *mut PawsRenderer,
    parent: u32,
    child: u32,
) -> i32 {
    let renderer = get_renderer!(renderer);
    match renderer.state.append_element(parent, child) {
        Ok(()) => 0,
        Err(code) => code.as_i32(),
    }
}

/// Sets an inline CSS property on an element.
///
/// `name` and `value` must be null-terminated UTF-8 strings.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_set_inline_style(
    renderer: *mut PawsRenderer,
    id: u32,
    name: *const c_char,
    value: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let name_str = get_cstr!(name);
    let value_str = get_cstr!(value);
    match renderer
        .state
        .set_inline_style(id, name_str.to_string(), value_str.to_string())
    {
        Ok(()) => 0,
        Err(code) => code.as_i32(),
    }
}

/// Sets a DOM attribute on an element.
///
/// `name` and `value` must be null-terminated UTF-8 strings.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_set_attribute(
    renderer: *mut PawsRenderer,
    id: u32,
    name: *const c_char,
    value: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let name_str = get_cstr!(name);
    let value_str = get_cstr!(value);
    match renderer
        .state
        .set_attribute(id, name_str.to_string(), value_str.to_string())
    {
        Ok(()) => 0,
        Err(code) => code.as_i32(),
    }
}

/// Adds a CSS stylesheet to the document.
///
/// `css` must be a null-terminated UTF-8 string containing CSS source.
#[no_mangle]
pub extern "C" fn paws_renderer_add_stylesheet(
    renderer: *mut PawsRenderer,
    css: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let css_str = get_cstr!(css);

    renderer.state.add_stylesheet(css_str.to_string());
    0
}

/// Triggers a commit: resolves styles, computes layout, and applies the
/// resulting `LayoutBox` tree to the UIKit view hierarchy.
///
/// `root_view` is the `UIView` to render into (an opaque pointer).
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_commit(renderer: *mut PawsRenderer, root_view: *mut c_void) -> i32 {
    let renderer = get_renderer!(renderer);

    if root_view.is_null() {
        return RendererError::InvalidHandle.as_i32();
    }

    let layout = renderer.state.commit();
    match renderer.view_tree.apply(&layout, root_view) {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Destroys an element and removes it from the DOM.
///
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_destroy_element(renderer: *mut PawsRenderer, id: u32) -> i32 {
    let renderer = get_renderer!(renderer);

    match renderer.state.destroy_element(id) {
        Ok(()) => 0,
        Err(code) => code.as_i32(),
    }
}

/// Converts a raw renderer pointer to a mutable reference.
///
/// Returns `None` if the pointer is null.
fn unsafe_renderer<'a>(ptr: *mut PawsRenderer) -> Option<&'a mut PawsRenderer> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: The pointer was created by Box::into_raw and the caller
        // guarantees exclusive access (single-threaded UIKit requirement).
        Some(unsafe { &mut *ptr })
    }
}

/// Reads a null-terminated C string, returning `None` if the pointer is null
/// or the string is not valid UTF-8.
fn read_cstr<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees a valid null-terminated C string.
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}
