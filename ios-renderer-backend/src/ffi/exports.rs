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
    /// The root `UIView*` to render into, set via [`paws_renderer_set_root_view`].
    root_view: Option<*mut c_void>,
}

impl PawsRenderer {
    /// Resolves styles, computes layout, and applies the resulting `LayoutBox`
    /// tree to the UIKit view hierarchy under the stored root view.
    ///
    /// No-op if no root view has been set.
    pub(crate) fn commit(&mut self) -> Result<(), RendererError> {
        let layout = self.state.commit();
        if let Some(root_view) = self.root_view {
            self.view_tree.apply(&layout, root_view)?;
        }
        Ok(())
    }
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
        root_view: None,
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

/// Sets the root `UIView` to render into.
///
/// `root_view` is an opaque pointer to the `UIView`. Pass `null` to clear.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_set_root_view(
    renderer: *mut PawsRenderer,
    root_view: *mut c_void,
) -> i32 {
    let renderer = get_renderer!(renderer);
    renderer.root_view = (!root_view.is_null()).then_some(root_view);
    0
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

/// Resolves styles, computes layout, and applies the resulting tree to UIKit.
///
/// No-op if no root view has been set via [`paws_renderer_set_root_view`].
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_commit(renderer: *mut PawsRenderer) -> i32 {
    let renderer = get_renderer!(renderer);
    match renderer.commit() {
        Ok(()) => 0,
        Err(e) => e.as_i32(),
    }
}

/// Compiles a WAT module and runs the named function against the renderer's
/// engine state, then commits the resulting layout to UIKit.
///
/// `wat_text` must be a null-terminated UTF-8 WAT string.
/// `func_name` must be a null-terminated UTF-8 string naming the export to call.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_run_wat(
    renderer: *mut PawsRenderer,
    wat_text: *const c_char,
    func_name: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let wat_str = get_cstr!(wat_text);
    let func_str = get_cstr!(func_name);

    // Move RuntimeState into wasmtime-engine for execution, then recover it.
    let state = std::mem::replace(
        &mut renderer.state,
        RuntimeState::new("about:blank".to_string()),
    );

    match wasmtime_engine::run_wat(state, wat_str, func_str) {
        Ok(state) => {
            renderer.state = state;
            // Auto-commit after WASM execution.
            match renderer.commit() {
                Ok(()) => 0,
                Err(e) => e.as_i32(),
            }
        }
        Err(err) => {
            renderer.state = err.state;
            RendererError::EngineFailed.as_i32()
        }
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

#[cfg(test)]
mod tests {
    use std::ffi::{c_void, CString};

    use super::*;
    use crate::ffi::imports::stubs::{clear_call_log, take_call_log, FfiCall};

    #[test]
    fn test_create_renderer_null_url() {
        let renderer = paws_renderer_create(std::ptr::null());
        assert!(
            !renderer.is_null(),
            "null URL should fall back to about:blank"
        );
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_create_renderer_valid_url() {
        let url = CString::new("https://example.com").unwrap();
        let renderer = paws_renderer_create(url.as_ptr());
        assert!(!renderer.is_null());
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_destroy_null_is_noop() {
        // Should not panic.
        paws_renderer_destroy(std::ptr::null_mut());
    }

    #[test]
    fn test_create_element_null_renderer() {
        let tag = CString::new("div").unwrap();
        let result = paws_renderer_create_element(std::ptr::null_mut(), tag.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());
    }

    #[test]
    fn test_create_element_null_tag() {
        let renderer = paws_renderer_create(std::ptr::null());
        let result = paws_renderer_create_element(renderer, std::ptr::null());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_create_element_valid() {
        let renderer = paws_renderer_create(std::ptr::null());
        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0, "valid element should return positive node ID");
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_set_root_view_null_renderer() {
        let root_view = 0x9000 as *mut c_void;
        let result = paws_renderer_set_root_view(std::ptr::null_mut(), root_view);
        assert_eq!(result, RendererError::InvalidHandle.as_i32());
    }

    #[test]
    fn test_set_root_view_null_clears() {
        let renderer = paws_renderer_create(std::ptr::null());
        let root_view = 0x9000 as *mut c_void;

        let result = paws_renderer_set_root_view(renderer, root_view);
        assert_eq!(result, 0);

        // Clearing with null should also succeed.
        let result = paws_renderer_set_root_view(renderer, std::ptr::null_mut());
        assert_eq!(result, 0);

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_commit_with_root_view_applies_layout() {
        clear_call_log();
        let renderer = paws_renderer_create(std::ptr::null());

        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0);

        let name = CString::new("width").unwrap();
        let value = CString::new("100px").unwrap();
        let style_result =
            paws_renderer_set_inline_style(renderer, node_id as u32, name.as_ptr(), value.as_ptr());
        assert_eq!(style_result, 0);

        // Set root view, then commit internally.
        let root_view = 0x9000 as *mut c_void;
        paws_renderer_set_root_view(renderer, root_view);

        // SAFETY: renderer is valid, created above.
        let r = unsafe { &mut *renderer };
        r.commit().unwrap();

        let log = take_call_log();

        // Commit should have created at least one view and set its frame.
        assert!(
            log.iter().any(|c| matches!(c, FfiCall::ViewCreate { .. })),
            "commit should create UIKit views"
        );
        assert!(
            log.iter()
                .any(|c| matches!(c, FfiCall::ViewSetFrame { .. })),
            "commit should set view frames"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_run_wat_success() {
        clear_call_log();
        let renderer = paws_renderer_create(std::ptr::null());
        let root_view = 0x9000 as *mut c_void;
        paws_renderer_set_root_view(renderer, root_view);

        let wat = CString::new(
            r#"
(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "width\00")
  (data (i32.const 32) "100px\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $style (local.get $id) (i32.const 16) (i32.const 32)))
    (i32.const 0)
  )
)
"#,
        )
        .unwrap();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_run_wat(renderer, wat.as_ptr(), func.as_ptr());
        assert_eq!(result, 0, "run_wat should succeed");

        let log = take_call_log();
        // run_wat auto-commits, so UIKit views should have been created.
        assert!(
            log.iter().any(|c| matches!(c, FfiCall::ViewCreate { .. })),
            "run_wat should auto-commit and create UIKit views"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_run_wat_invalid_wat() {
        let renderer = paws_renderer_create(std::ptr::null());
        let root_view = 0x9000 as *mut c_void;
        paws_renderer_set_root_view(renderer, root_view);

        let wat = CString::new("not valid wat!").unwrap();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_run_wat(renderer, wat.as_ptr(), func.as_ptr());
        assert_eq!(
            result,
            RendererError::EngineFailed.as_i32(),
            "invalid WAT should return EngineFailed"
        );

        // Renderer should still be usable after error.
        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0, "renderer should still work after WAT error");

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_run_wat_null_params() {
        let renderer = paws_renderer_create(std::ptr::null());

        // Null WAT text.
        let func = CString::new("run").unwrap();
        let result = paws_renderer_run_wat(renderer, std::ptr::null(), func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        // Null func name.
        let wat = CString::new("(module)").unwrap();
        let result = paws_renderer_run_wat(renderer, wat.as_ptr(), std::ptr::null());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        // Null renderer.
        let result = paws_renderer_run_wat(std::ptr::null_mut(), wat.as_ptr(), func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_commit_without_root_view_is_noop() {
        clear_call_log();
        let renderer = paws_renderer_create(std::ptr::null());

        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0);

        // Commit without setting root view — should not create any UIKit views.
        // SAFETY: renderer is valid, created above.
        let r = unsafe { &mut *renderer };
        r.commit().unwrap();

        let log = take_call_log();
        assert!(
            !log.iter().any(|c| matches!(c, FfiCall::ViewCreate { .. })),
            "commit without root view should not create UIKit views"
        );

        paws_renderer_destroy(renderer);
    }
}
