//! FFI exports: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//!
//! These form the public C API exposed via the cbindgen-generated header.
//! Naming convention: `paws_renderer_*`.
//!
//! All engine work (WASM execution, style resolution, layout, ViewTree
//! processing) happens on a background thread. The FFI layer sends
//! [`Command`](crate::thread::Command)s via a channel and, for operations
//! that need a return value, blocks on a reply.

use std::ffi::{c_char, c_void, CStr};
use std::sync::mpsc;

use crate::error::RendererError;
use crate::thread::{Command, CompletionFn, EngineHandle};

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

/// Opaque handle to the Paws renderer.
///
/// Owns the background engine thread via [`EngineHandle`]. All engine
/// state lives on that thread — this struct only holds the channel sender.
///
/// Created by [`paws_renderer_create`] and destroyed by [`paws_renderer_destroy`].
pub struct PawsRenderer {
    engine: EngineHandle,
}

/// Creates a new `PawsRenderer` with a background engine thread.
///
/// - `base_url`: null-terminated UTF-8 string (document base URL).
///   Pass `null` to use `"about:blank"`.
/// - `completion`: called from the background thread each time ops are
///   ready after a commit. The `ops_ptr` and `ops_len` describe a buffer
///   of 32-byte op-code slots. The pointer is only valid for the duration
///   of the callback — copy or process before returning.
/// - `context`: opaque pointer forwarded to every `completion` call.
///   Typically an `Unmanaged<OpExecutor>` pointer on the Swift side.
///
/// Returns an opaque pointer. The caller (Swift) owns it and must call
/// [`paws_renderer_destroy`] to free it.
///
/// Returns `null` on failure.
#[no_mangle]
pub extern "C" fn paws_renderer_create(
    base_url: *const c_char,
    completion: CompletionFn,
    context: *mut c_void,
) -> *mut PawsRenderer {
    let url_str = if base_url.is_null() {
        "about:blank"
    } else {
        // SAFETY: Caller guarantees a valid null-terminated UTF-8 C string.
        match unsafe { CStr::from_ptr(base_url) }.to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let engine = EngineHandle::spawn(url_str.to_string(), completion, context);
    let renderer = PawsRenderer { engine };
    Box::into_raw(Box::new(renderer))
}

/// Destroys a `PawsRenderer`, shutting down the background thread.
///
/// After this call the pointer is invalid. Passing `null` is a no-op.
#[no_mangle]
pub extern "C" fn paws_renderer_destroy(renderer: *mut PawsRenderer) {
    if !renderer.is_null() {
        // SAFETY: Pointer was created by Box::into_raw in paws_renderer_create.
        drop(unsafe { Box::from_raw(renderer) });
    }
}

/// Asynchronously triggers style resolution + layout + op generation.
///
/// The completion callback (registered at creation) will be called from
/// the background thread once ops are ready. Returns `0` immediately.
#[no_mangle]
pub extern "C" fn paws_renderer_post_commit(renderer: *mut PawsRenderer) -> i32 {
    let renderer = get_renderer!(renderer);
    match renderer.engine.tx.send(Command::Commit) {
        Ok(()) => 0,
        Err(_) => RendererError::EngineFailed.as_i32(),
    }
}

/// Asynchronously compiles a WAT module and runs the named function,
/// then auto-commits.
///
/// The completion callback will be called from the background thread
/// once ops are ready. Returns `0` immediately.
#[no_mangle]
pub extern "C" fn paws_renderer_post_run_wat(
    renderer: *mut PawsRenderer,
    wat_text: *const c_char,
    func_name: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let wat_str = get_cstr!(wat_text);
    let func_str = get_cstr!(func_name);

    match renderer.engine.tx.send(Command::RunWat {
        wat: wat_str.to_string(),
        func_name: func_str.to_string(),
    }) {
        Ok(()) => 0,
        Err(_) => RendererError::EngineFailed.as_i32(),
    }
}

/// Creates a DOM element with the given tag name.
///
/// Blocks until the background thread processes the request.
/// Returns the element's node ID (>0) on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_create_element(
    renderer: *mut PawsRenderer,
    tag: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let tag_str = get_cstr!(tag);

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::CreateElement {
            tag: tag_str.to_string(),
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    match reply_rx.recv() {
        Ok(id) => id as i32,
        Err(_) => RendererError::EngineFailed.as_i32(),
    }
}

/// Creates a text node with the given content.
///
/// Blocks until the background thread processes the request.
/// Returns the node ID (>0) on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_create_text_node(
    renderer: *mut PawsRenderer,
    text: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let text_str = get_cstr!(text);

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::CreateTextNode {
            text: text_str.to_string(),
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    match reply_rx.recv() {
        Ok(id) => id as i32,
        Err(_) => RendererError::EngineFailed.as_i32(),
    }
}

/// Appends a child element to a parent element.
///
/// Blocks until the background thread processes the request.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_append_element(
    renderer: *mut PawsRenderer,
    parent: u32,
    child: u32,
) -> i32 {
    let renderer = get_renderer!(renderer);

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::AppendElement {
            parent,
            child,
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    reply_rx
        .recv()
        .unwrap_or(RendererError::EngineFailed.as_i32())
}

/// Sets an inline CSS property on an element.
///
/// Blocks until the background thread processes the request.
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

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::SetInlineStyle {
            id,
            name: name_str.to_string(),
            value: value_str.to_string(),
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    reply_rx
        .recv()
        .unwrap_or(RendererError::EngineFailed.as_i32())
}

/// Sets a DOM attribute on an element.
///
/// Blocks until the background thread processes the request.
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

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::SetAttribute {
            id,
            name: name_str.to_string(),
            value: value_str.to_string(),
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    reply_rx
        .recv()
        .unwrap_or(RendererError::EngineFailed.as_i32())
}

/// Adds a CSS stylesheet to the document.
///
/// Fire-and-forget — does not block for a reply.
#[no_mangle]
pub extern "C" fn paws_renderer_add_stylesheet(
    renderer: *mut PawsRenderer,
    css: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let css_str = get_cstr!(css);

    match renderer.engine.tx.send(Command::AddStylesheet {
        css: css_str.to_string(),
    }) {
        Ok(()) => 0,
        Err(_) => RendererError::EngineFailed.as_i32(),
    }
}

/// Destroys an element and removes it from the DOM.
///
/// Blocks until the background thread processes the request.
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_destroy_element(renderer: *mut PawsRenderer, id: u32) -> i32 {
    let renderer = get_renderer!(renderer);

    let (reply_tx, reply_rx) = mpsc::channel();
    if renderer
        .engine
        .tx
        .send(Command::DestroyElement {
            id,
            reply: reply_tx,
        })
        .is_err()
    {
        return RendererError::EngineFailed.as_i32();
    }

    reply_rx
        .recv()
        .unwrap_or(RendererError::EngineFailed.as_i32())
}

/// Converts a raw renderer pointer to a mutable reference.
///
/// Returns `None` if the pointer is null.
fn unsafe_renderer<'a>(ptr: *mut PawsRenderer) -> Option<&'a mut PawsRenderer> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: The pointer was created by Box::into_raw. Exclusive access
        // is guaranteed because the struct only holds a channel sender (which
        // is safe to use from multiple threads, though in practice Swift calls
        // FFI functions from a single thread).
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
    use std::ffi::CString;
    use std::sync::{Arc, Condvar, Mutex};

    use super::*;

    /// Test capture for the completion callback. Uses a condvar so tests
    /// can wait deterministically instead of sleeping.
    struct TestCapture {
        ops: Mutex<Vec<u8>>,
        condvar: Condvar,
    }

    impl TestCapture {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                ops: Mutex::new(Vec::new()),
                condvar: Condvar::new(),
            })
        }

        /// Blocks until the completion callback fires, with a timeout.
        fn wait_for_ops(&self) -> Vec<u8> {
            let guard = self.ops.lock().unwrap();
            let (guard, _timeout) = self
                .condvar
                .wait_timeout_while(guard, std::time::Duration::from_secs(5), |ops| {
                    ops.is_empty()
                })
                .unwrap();
            guard.clone()
        }
    }

    extern "C" fn test_completion(ptr: *const u8, len: usize, ctx: *mut c_void) {
        // SAFETY: ctx points to a valid Arc<TestCapture>.
        let capture = unsafe { &*(ctx as *const TestCapture) };
        let bytes = if len > 0 && !ptr.is_null() {
            // SAFETY: ptr is valid for len bytes during this callback.
            unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
        } else {
            Vec::new()
        };
        *capture.ops.lock().unwrap() = bytes;
        capture.condvar.notify_all();
    }

    fn create_test_renderer() -> (*mut PawsRenderer, Arc<TestCapture>) {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut c_void;
        let renderer = paws_renderer_create(std::ptr::null(), test_completion, ctx);
        assert!(!renderer.is_null());
        (renderer, capture)
    }

    #[test]
    fn test_create_renderer_null_url() {
        let (renderer, _capture) = create_test_renderer();
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_create_renderer_valid_url() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut c_void;
        let url = CString::new("https://example.com").unwrap();
        let renderer = paws_renderer_create(url.as_ptr(), test_completion, ctx);
        assert!(!renderer.is_null());
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_destroy_null_is_noop() {
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
        let (renderer, _capture) = create_test_renderer();
        let result = paws_renderer_create_element(renderer, std::ptr::null());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_create_element_valid() {
        let (renderer, _capture) = create_test_renderer();
        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0, "valid element should return positive node ID");
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_commit_produces_ops() {
        let (renderer, capture) = create_test_renderer();

        // Create an element and append to root.
        let tag = CString::new("div").unwrap();
        let node_id = paws_renderer_create_element(renderer, tag.as_ptr());
        assert!(node_id > 0);

        let result = paws_renderer_append_element(renderer, 0, node_id as u32);
        assert_eq!(result, 0);

        let name = CString::new("width").unwrap();
        let value = CString::new("100px").unwrap();
        let result =
            paws_renderer_set_inline_style(renderer, node_id as u32, name.as_ptr(), value.as_ptr());
        assert_eq!(result, 0);

        // Commit.
        let result = paws_renderer_post_commit(renderer);
        assert_eq!(result, 0);

        // Wait for the completion callback to fire (deterministic, no sleep).
        let ops_bytes = capture.wait_for_ops();
        assert!(
            !ops_bytes.is_empty(),
            "post_commit should produce a non-empty op buffer"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wat_produces_ops() {
        let (renderer, capture) = create_test_renderer();

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

        let result = paws_renderer_post_run_wat(renderer, wat.as_ptr(), func.as_ptr());
        assert_eq!(result, 0, "post_run_wat should return 0 immediately");

        // Wait for the completion callback to fire (deterministic, no sleep).
        let ops_bytes = capture.wait_for_ops();
        assert!(
            !ops_bytes.is_empty(),
            "post_run_wat should produce a non-empty op buffer"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wat_null_params() {
        let (renderer, _capture) = create_test_renderer();

        let func = CString::new("run").unwrap();
        let result = paws_renderer_post_run_wat(renderer, std::ptr::null(), func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        let wat = CString::new("(module)").unwrap();
        let result = paws_renderer_post_run_wat(renderer, wat.as_ptr(), std::ptr::null());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        let result = paws_renderer_post_run_wat(std::ptr::null_mut(), wat.as_ptr(), func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        paws_renderer_destroy(renderer);
    }
}
