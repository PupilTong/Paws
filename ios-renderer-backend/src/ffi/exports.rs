//! FFI exports: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//!
//! These form the public C API exposed via the cbindgen-generated header.
//! Naming convention: `paws_renderer_*`.
//!
//! The engine thread is spawned on the first [`paws_renderer_post_run_wasm`]
//! call and stays alive until [`paws_renderer_destroy`] is called (or until
//! the WASM module's own internal loop exits). A renderer accepts only one
//! WASM module — subsequent calls to `post_run_wasm` on the same renderer
//! return [`RendererError::EngineFailed`].

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

/// Creates a new `PawsRenderer`.
///
/// No background thread is spawned yet — that happens on the first
/// [`paws_renderer_post_run_wasm`] call. The renderer owns all engine state
/// (DOM, styles, layout, view snapshots) for its full lifetime.
///
/// - `base_url`: null-terminated UTF-8 string (document base URL).
///   Pass `null` to use `"about:blank"`.
/// - `completion`: called from the background thread each time ops are ready
///   after a commit. `ops_ptr` and `ops_len` describe a buffer of 32-byte
///   op-code slots valid only for the duration of the call — copy or process
///   before returning. Swift must dispatch UIKit mutations to the main queue.
/// - `context`: opaque pointer forwarded to every `completion` call.
///   Typically an `Unmanaged<OpExecutor>` on the Swift side.
///
/// Returns an opaque pointer owned by the caller. Must be freed with
/// [`paws_renderer_destroy`]. Returns `null` on failure.
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

/// Starts the rendering pipeline by loading and running a WASM module.
///
/// Spawns the background engine thread, which compiles the module and calls
/// the named export. The WASM module is expected to run its own internal
/// event loop — it drives all DOM mutations and op delivery from within.
///
/// This is a **one-shot** call per renderer. Calling it again on the same
/// renderer returns [`RendererError::EngineFailed`] — create a new renderer
/// to run a different module.
///
/// Returns `0` on success, or a negative error code.
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

    if renderer
        .engine
        .post_run_wasm(wasm_vec, func_str.to_string())
    {
        0
    } else {
        RendererError::EngineFailed.as_i32()
    }
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

    use super::*;
    use crate::test_util::noop_completion;

    fn create_test_renderer() -> *mut PawsRenderer {
        let renderer =
            paws_renderer_create(std::ptr::null(), noop_completion, std::ptr::null_mut());
        assert!(!renderer.is_null());
        renderer
    }

    #[test]
    fn test_create_renderer_null_url() {
        let renderer = create_test_renderer();
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_create_renderer_valid_url() {
        let url = CString::new("https://example.com").unwrap();
        let renderer = paws_renderer_create(url.as_ptr(), noop_completion, std::ptr::null_mut());
        assert!(!renderer.is_null());
        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_destroy_null_is_noop() {
        paws_renderer_destroy(std::ptr::null_mut());
    }

    #[test]
    fn test_post_run_wasm_missing_export() {
        let renderer = create_test_renderer();

        let wasm_bytes = b"(module)";
        let func = CString::new("nonexistent").unwrap();

        // Thread spawns and WASM fails internally (missing export), but the
        // FFI call itself succeeds — returns 0 because the thread was started.
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, 0, "first call should succeed (thread spawned)");

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wasm_second_call_returns_engine_failed() {
        let renderer = create_test_renderer();

        let wasm_bytes = b"(module)";
        let func = CString::new("nonexistent").unwrap();

        // First call — thread spawns.
        paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );

        // Second call on the same renderer — one-shot, should fail.
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(
            result,
            RendererError::EngineFailed.as_i32(),
            "second call on same renderer should return EngineFailed"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wasm_null_params() {
        let renderer = create_test_renderer();

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
