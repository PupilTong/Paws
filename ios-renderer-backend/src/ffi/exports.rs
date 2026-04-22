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

/// Sets the viewport size that the engine will apply to the guest's
/// `RuntimeState` layout. Must be called before `paws_renderer_post_run_wasm`
/// to take effect — the viewport is read once when the engine thread starts.
///
/// `width` / `height` must be finite and strictly positive. Passing
/// non-conforming values resets the viewport to `MAX_CONTENT` (content-sized
/// layout), which is the default when this function is not called.
///
/// Without a viewport, Taffy lays every block element out at its intrinsic
/// content size — unstyled `<div>`s collapse to the width of whatever text
/// they contain (often under 10 pixels), making them effectively invisible
/// inside a normal-sized host view.
///
/// # Safety
///
/// `renderer` must be a valid pointer returned by `paws_renderer_create`.
#[no_mangle]
pub extern "C" fn paws_renderer_set_viewport(
    renderer: *mut PawsRenderer,
    width: f32,
    height: f32,
) -> i32 {
    let renderer = get_renderer!(renderer);
    renderer.engine.set_viewport(width, height);
    0
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

/// Starts the rendering pipeline using WAT (WebAssembly Text) source.
///
/// Compiles the WAT text to WASM bytes and runs the module. This is a
/// convenience function for testing — production code should use
/// [`paws_renderer_post_run_wasm`] with pre-compiled WASM bytes.
///
/// Returns `0` on success, or a negative error code.
#[no_mangle]
pub extern "C" fn paws_renderer_post_run_wat(
    renderer: *mut PawsRenderer,
    wat_text: *const c_char,
    func_name: *const c_char,
) -> i32 {
    let renderer = get_renderer!(renderer);
    let wat_str = get_cstr!(wat_text);
    let func_str = get_cstr!(func_name);

    let wasm_bytes = match wat::parse_str(wat_str) {
        Ok(bytes) => bytes,
        Err(_) => return RendererError::EngineFailed.as_i32(),
    };

    if renderer
        .engine
        .post_run_wasm(wasm_bytes.to_vec(), func_str.to_string())
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

    #[test]
    fn test_post_run_wat_success() {
        let renderer = create_test_renderer();
        let wat = CString::new(crate::test_util::make_wat_module()).unwrap();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_post_run_wat(renderer, wat.as_ptr(), func.as_ptr());
        assert_eq!(result, 0, "post_run_wat should succeed");

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wat_invalid_syntax() {
        let renderer = create_test_renderer();
        let bad_wat = CString::new("not valid wat").unwrap();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_post_run_wat(renderer, bad_wat.as_ptr(), func.as_ptr());
        assert_eq!(
            result,
            RendererError::EngineFailed.as_i32(),
            "invalid WAT should return EngineFailed"
        );

        paws_renderer_destroy(renderer);
    }

    #[test]
    fn test_post_run_wat_null_params() {
        let renderer = create_test_renderer();
        let func = CString::new("run").unwrap();

        let result = paws_renderer_post_run_wat(renderer, std::ptr::null(), func.as_ptr());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        let wat = CString::new("(module)").unwrap();
        let result = paws_renderer_post_run_wat(renderer, wat.as_ptr(), std::ptr::null());
        assert_eq!(result, RendererError::InvalidHandle.as_i32());

        paws_renderer_destroy(renderer);
    }

    // ── Component-model integration regression guards ────────────────
    //
    // Paws example WASMs are wasm32-wasip2 components. The iOS backend
    // previously used the core-module loader (`wasmtime::Module::new`)
    // which silently rejects components, so every guest was a no-op
    // and the host view ended up empty. These tests drive the real C
    // FFI pipeline end-to-end and assert the completion callback fires
    // with a non-empty op buffer — exactly the signal the old
    // `let _ = run_wasm(...)` was suppressing.

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Context shared with the recording completion callback below.
    struct CallbackState {
        calls: AtomicUsize,
        total_ops_bytes: AtomicUsize,
    }

    extern "C" fn recording_completion(
        _ops_ptr: *const u8,
        ops_len: usize,
        _strings_ptr: *const u8,
        _strings_len: usize,
        ctx: *mut c_void,
    ) {
        // SAFETY: ctx is an `&CallbackState` handed in via Arc::as_ptr
        // below; the Arc is kept alive for the renderer's full lifetime
        // by the test (we drop the renderer — which joins the engine
        // thread — before reading the counters).
        let state = unsafe { &*(ctx as *const CallbackState) };
        state.calls.fetch_add(1, Ordering::SeqCst);
        state.total_ops_bytes.fetch_add(ops_len, Ordering::SeqCst);
    }

    fn run_component_example_via_ffi(resource_name: &str) -> Arc<CallbackState> {
        let wasm_path = paws_examples::example_wasm_path(resource_name);
        let wasm_bytes =
            std::fs::read(wasm_path).unwrap_or_else(|e| panic!("failed to read {wasm_path}: {e}"));

        let state = Arc::new(CallbackState {
            calls: AtomicUsize::new(0),
            total_ops_bytes: AtomicUsize::new(0),
        });
        let ctx_ptr = Arc::as_ptr(&state) as *mut c_void;

        let url = CString::new("https://test.paws").unwrap();
        let renderer = paws_renderer_create(url.as_ptr(), recording_completion, ctx_ptr);
        assert!(!renderer.is_null(), "paws_renderer_create returned null");

        // Match the iOS app's wiring: set a viewport before posting the wasm.
        assert_eq!(paws_renderer_set_viewport(renderer, 375.0, 667.0), 0);

        let func = CString::new("run").unwrap();
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, 0, "post_run_wasm returned error code {result}");

        // destroy joins the engine thread, so every callback that is
        // ever going to fire has fired by the time it returns.
        paws_renderer_destroy(renderer);

        state
    }

    /// yew auto-commits after `render()`, so its `<div><button>+</button>
    /// <span>0</span></div>` tree must deliver at least one non-empty
    /// op buffer through the FFI. This fails on a core-module loader.
    #[test]
    fn test_yew_counter_component_delivers_ops_via_ffi() {
        let state = run_component_example_via_ffi("example_yew_counter");
        assert!(
            state.calls.load(Ordering::SeqCst) >= 1,
            "completion callback should fire for example_yew_counter"
        );
        assert!(
            state.total_ops_bytes.load(Ordering::SeqCst) > 0,
            "completion callback should deliver non-empty ops buffer"
        );
    }

    /// Hand-written component (non-yew) that calls `commit()` explicitly.
    /// Guards against regressions that would pass yew-specific paths but
    /// break the rust-wasm-binding + explicit-commit flow.
    #[test]
    fn test_commit_full_component_delivers_ops_via_ffi() {
        let state = run_component_example_via_ffi("example_commit_full");
        assert!(
            state.calls.load(Ordering::SeqCst) >= 1,
            "completion callback should fire for example_commit_full"
        );
        assert!(
            state.total_ops_bytes.load(Ordering::SeqCst) > 0,
            "completion callback should deliver non-empty ops buffer"
        );
    }

    /// The styled-element example sets width, height, and now
    /// background-color — the minimum paint triad for a visible
    /// rectangle. Assert that the op buffer carries a SetBgColor slot;
    /// if a future change drops the paint property, the iOS host view
    /// goes empty (which is exactly the regression this guards).
    #[test]
    fn test_styled_element_emits_bg_color_op() {
        // Capture the raw ops buffer by moving the recording context to
        // accumulate bytes into a shared Vec.
        struct CollectingState {
            ops: std::sync::Mutex<Vec<u8>>,
        }
        extern "C" fn collect(
            ops_ptr: *const u8,
            ops_len: usize,
            _strings_ptr: *const u8,
            _strings_len: usize,
            ctx: *mut c_void,
        ) {
            // SAFETY: ctx is kept alive by the test until
            // `paws_renderer_destroy` returns (which joins the engine
            // thread). The ops buffer is only valid for the duration
            // of this call — we copy before returning.
            let state = unsafe { &*(ctx as *const CollectingState) };
            if ops_len > 0 {
                // SAFETY: Rust side guarantees `ops_ptr` points to
                // `ops_len` bytes of valid u8 data.
                let bytes = unsafe { std::slice::from_raw_parts(ops_ptr, ops_len) };
                state.ops.lock().unwrap().extend_from_slice(bytes);
            }
        }

        let state = std::sync::Arc::new(CollectingState {
            ops: std::sync::Mutex::new(Vec::new()),
        });
        let ctx_ptr = std::sync::Arc::as_ptr(&state) as *mut c_void;

        let wasm_path = paws_examples::example_wasm_path("example_styled_element");
        let wasm_bytes = std::fs::read(wasm_path).unwrap();

        let url = CString::new("https://test.paws").unwrap();
        let renderer = paws_renderer_create(url.as_ptr(), collect, ctx_ptr);
        assert!(!renderer.is_null());
        assert_eq!(paws_renderer_set_viewport(renderer, 375.0, 667.0), 0);

        let func = CString::new("run").unwrap();
        let result = paws_renderer_post_run_wasm(
            renderer,
            wasm_bytes.as_ptr(),
            wasm_bytes.len(),
            func.as_ptr(),
        );
        assert_eq!(result, 0);
        paws_renderer_destroy(renderer);

        // Walk the 32-byte op slots looking for SetBgColor (tag 0x06).
        // Matches the wire format in `src/ops.rs`.
        const SLOT_SIZE: usize = 32;
        const OP_SET_BG_COLOR: u8 = 0x06;
        let ops = state.ops.lock().unwrap();
        assert!(
            ops.len() >= SLOT_SIZE,
            "styled element should emit at least one op slot, got {} bytes",
            ops.len()
        );
        let has_bg = ops.chunks(SLOT_SIZE).any(|slot| slot[0] == OP_SET_BG_COLOR);
        assert!(
            has_bg,
            "styled element must emit a SetBgColor op or the iOS \
             PawsRendererView renders an invisible transparent box"
        );
    }
}
