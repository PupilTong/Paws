//! FFI exports: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//!
//! These form the public C API exposed via the cbindgen-generated header.
//! Naming convention: `paws_renderer_*`.
//!
//! All engine work (WASM execution, style resolution, layout, ViewTree
//! processing) happens on a background thread spawned per
//! [`paws_renderer_post_run_wasm`] call.

use std::ffi::{c_char, c_void, CStr};

use crate::error::RendererError;
use crate::thread::{CompletionFn, EngineHandle};

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
/// state lives on that thread.
///
/// Multiple instances can coexist — each manages an independent engine
/// and UIKit area.
///
/// Created by [`paws_renderer_create`] and destroyed by [`paws_renderer_destroy`].
pub struct PawsRenderer {
    engine: EngineHandle,
}

/// Creates a new `PawsRenderer`. No background thread is spawned yet —
/// that happens on the first [`paws_renderer_post_run_wasm`] call.
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

    let engine = EngineHandle::new(url_str.to_string(), completion, context);
    let renderer = PawsRenderer { engine };
    Box::into_raw(Box::new(renderer))
}

/// Destroys a `PawsRenderer`, stopping the background thread if running.
///
/// After this call the pointer is invalid. Passing `null` is a no-op.
#[no_mangle]
pub extern "C" fn paws_renderer_destroy(renderer: *mut PawsRenderer) {
    if !renderer.is_null() {
        // SAFETY: Pointer was created by Box::into_raw in paws_renderer_create.
        drop(unsafe { Box::from_raw(renderer) });
    }
}

/// Stops the engine's background thread if one is running.
///
/// Waits for the thread to finish before returning. After this call,
/// no background work is active for this renderer. A subsequent
/// [`paws_renderer_post_run_wasm`] call will spawn a new thread.
///
/// Returns `0` on success.
#[no_mangle]
pub extern "C" fn paws_renderer_stop_engine(renderer: *mut PawsRenderer) -> i32 {
    let renderer = get_renderer!(renderer);
    renderer.engine.stop_engine();
    0
}

/// Spawns a fresh background thread to compile and run a WASM module,
/// then auto-commits.
///
/// If a previous thread is still running, it is joined first (waited
/// for completion). The new thread creates fresh engine state, runs the
/// module, and delivers op-codes via the completion callback.
///
/// Returns `0` on success (thread spawned).
#[no_mangle]
pub extern "C" fn paws_renderer_post_run_wasm(
    renderer: *mut PawsRenderer,
    wasm_bytes: *const u8,
    wasm_len: usize,
    func_name: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    if wasm_bytes.is_null() {
        return RendererError::InvalidHandle.as_i32();
    }

    // SAFETY: wasm_bytes is a valid pointer to wasm_len bytes.
    let wasm_slice = unsafe { std::slice::from_raw_parts(wasm_bytes, wasm_len) };
    let wasm_vec = wasm_slice.to_vec();
    let func_str = get_cstr!(func_name);

    renderer.engine.run_wasm(wasm_vec, func_str.to_string());
    0
}

/// Converts a raw renderer pointer to a mutable reference.
///
/// Returns `None` if the pointer is null.
fn unsafe_renderer<'a>(ptr: *mut PawsRenderer) -> Option<&'a mut PawsRenderer> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: The pointer was created by Box::into_raw. Exclusive access
        // is guaranteed because Swift calls FFI functions from a single thread.
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
    fn test_stop_engine_null_renderer() {
        let result = paws_renderer_stop_engine(std::ptr::null_mut());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());
    }

    #[test]
    fn test_stop_engine_no_thread() {
        let (renderer, _capture) = create_test_renderer();
        let result = paws_renderer_stop_engine(renderer);
        assert_eq!(result, 0);
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wasm_missing_export() {
        let (renderer, _capture) = create_test_renderer();

        let wasm_bytes = b"(module)";
        let func = CString::new("nonexistent").unwrap();

        // Will fail because "nonexistent" export is missing, but it shouldn't crash.
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, 0, "post_run_wasm should return 0 immediately");

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wasm_produces_ops_with_wat_bytes() {
        let (renderer, capture) = create_test_renderer();

        let wat = CString::new(
            r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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
        let wasm_bytes = wat.as_bytes();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, 0, "post_run_wasm should return 0");

        // Wait for the completion callback to fire (deterministic, no sleep).
        let ops_bytes = capture.wait_for_ops();
        assert!(
            !ops_bytes.is_empty(),
            "post_run_wasm should produce a non-empty op buffer"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wasm_null_params() {
        let (renderer, _capture) = create_test_renderer();

        let func = CString::new("run").unwrap();
        let result = paws_renderer_post_run_wasm(renderer, std::ptr::null(), 0, func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        let wat = CString::new("(module)").unwrap();
        let wasm_bytes = wat.as_bytes();
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            std::ptr::null(),
        );
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        let result = paws_renderer_post_run_wasm(
            std::ptr::null_mut(),
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        paws_renderer_destroy(renderer);
    }
}
