//! Background engine thread — the "WebWorker" of the iOS renderer.
//!
//! [`EngineHandle`] owns a single background thread that mirrors the browser
//! WebWorker model:
//!
//! 1. Swift calls [`EngineHandle::post_run_wasm`] **once** to start the rendering
//!    pipeline. The thread spawns, creates a fresh [`RuntimeState`], and calls
//!    into the WASM module.
//! 2. The WASM module runs **its own internal event loop** — it never returns
//!    until the engine is stopped. All DOM mutations, commits, and op deliveries
//!    are driven from inside that loop via host functions.
//! 3. When [`EngineHandle`] is dropped, the thread is joined (waiting for WASM
//!    to finish, which happens when WASM's loop exits or the process tears down).
//!
//! There is intentionally **no Rust-side event loop**: a single `post_run_wasm`
//! call is the only entry point. Calling it again on the same handle is a no-op
//! — a new engine must be created to run a different WASM module.

use std::thread;

use engine::RuntimeState;

use crate::ops::OpBuffer;
use crate::renderer::ViewTree;

/// Completion callback type.
///
/// Called from the background thread each time the engine commits a frame.
/// `ops_ptr` points to a buffer of 32-byte op-code slots valid only for the
/// duration of the call — Swift must copy or process it before returning and
/// must dispatch all UIKit mutations to the main queue.
pub(crate) type CompletionFn =
    extern "C" fn(ops_ptr: *const u8, ops_len: usize, ctx: *mut std::ffi::c_void);

/// Bundles the completion callback and its opaque context pointer for
/// cross-thread transfer.
///
/// # Safety
///
/// `context` must remain valid until the engine thread exits. The callback must
/// be safe to call from any thread (Swift dispatches to the main queue internally).
pub(crate) struct SendCallback {
    pub(crate) completion: CompletionFn,
    pub(crate) context: *mut std::ffi::c_void,
}

// SAFETY: `completion` is a plain function pointer (code address, not heap data).
// `context` is forwarded to a callback that dispatches to the main queue —
// the Rust thread never dereferences it directly.
unsafe impl Send for SendCallback {}

/// Handle to the background engine thread.
///
/// Modelled after a browser WebWorker: one thread, one WASM module, started
/// once via [`post_run_wasm`](Self::post_run_wasm) and alive until dropped.
///
/// Multiple handles can coexist — each manages its own independent thread and
/// UIKit sub-tree. There is no shared state between handles.
pub(crate) struct EngineHandle {
    /// Background thread handle. `Some` while the thread is alive,
    /// `None` before `post_run_wasm` is called.
    handle: Option<thread::JoinHandle<()>>,
    base_url: String,
    callback: SendCallback,
}

impl EngineHandle {
    /// Creates a new engine handle without spawning a thread.
    ///
    /// The thread is spawned on the first (and only) call to
    /// [`post_run_wasm`](Self::post_run_wasm).
    pub(crate) fn new(
        base_url: String,
        completion: CompletionFn,
        context: *mut std::ffi::c_void,
    ) -> Self {
        Self {
            handle: None,
            base_url,
            callback: SendCallback {
                completion,
                context,
            },
        }
    }

    /// Starts the engine by spawning a background thread that runs the WASM module.
    ///
    /// This is a **one-shot** operation — the WASM module is expected to run its
    /// own internal event loop and never return until the engine is stopped.
    /// Calling this a second time on the same handle is a no-op; create a new
    /// [`EngineHandle`] to run a different module.
    ///
    /// Returns `true` if the thread was spawned, `false` if already running.
    pub(crate) fn post_run_wasm(&mut self, wasm_bytes: Vec<u8>, func_name: String) -> bool {
        if self.handle.is_some() {
            // Already running — one engine, one WASM module.
            return false;
        }

        let base_url = self.base_url.clone();
        // SAFETY: SendCallback is Send; see its impl above.
        let cb = SendCallback {
            completion: self.callback.completion,
            context: self.callback.context,
        };

        let handle = thread::Builder::new()
            .name("paws-engine".to_string())
            .spawn(move || run_engine(base_url, wasm_bytes, func_name, cb))
            .expect("failed to spawn paws-engine thread");

        self.handle = Some(handle);
        true
    }
}

impl Drop for EngineHandle {
    /// Joins the engine thread, waiting for the WASM module to exit.
    ///
    /// All engine state (`RuntimeState`, `ViewTree`, `OpBuffer`) drops when the
    /// thread exits, releasing the associated UIKit sub-tree.
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Entry point for the background engine thread.
///
/// Creates engine state, runs the WASM module once (the WASM drives its own
/// internal loop), and delivers one final commit when the module exits.
///
/// All resources drop at the end of this function, releasing the UIKit sub-tree.
fn run_engine(base_url: String, wasm_bytes: Vec<u8>, func_name: String, cb: SendCallback) {
    let state = RuntimeState::new(base_url);
    let mut view_tree = ViewTree::new();
    let mut ops = OpBuffer::new();

    // Single call — blocks until WASM's own event loop exits.
    // The WASM drives commits from within via host functions (future: __commit()).
    match wasmtime_engine::run_wasm(state, &wasm_bytes, &func_name) {
        Ok(mut state) => {
            // Deliver a final commit when the WASM module exits cleanly.
            let layout = state.commit();
            view_tree.process(&layout, &mut ops);
            (cb.completion)(ops.as_ptr(), ops.len(), cb.context);
        }
        Err(_err) => {
            // TODO: surface error code to Swift
        }
    }
    // `state`, `view_tree`, `ops` drop here — UIKit sub-tree released.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};

    struct TestCapture {
        ops: Mutex<Option<Vec<u8>>>,
        condvar: Condvar,
    }

    impl TestCapture {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                ops: Mutex::new(None),
                condvar: Condvar::new(),
            })
        }

        fn wait_for_ops(&self) -> Vec<u8> {
            let guard = self.ops.lock().unwrap();
            let (guard, _) = self
                .condvar
                .wait_timeout_while(guard, std::time::Duration::from_secs(5), |o| o.is_none())
                .unwrap();
            guard.clone().unwrap_or_default()
        }
    }

    extern "C" fn test_completion(ptr: *const u8, len: usize, ctx: *mut std::ffi::c_void) {
        // SAFETY: ctx points to a valid Arc<TestCapture> kept alive by the test.
        let capture = unsafe { &*(ctx as *const TestCapture) };
        let bytes = if len > 0 && !ptr.is_null() {
            // SAFETY: ptr is valid for `len` bytes for the duration of this callback.
            unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
        } else {
            Vec::new()
        };
        *capture.ops.lock().unwrap() = Some(bytes);
        capture.condvar.notify_all();
    }

    fn make_wat_module() -> &'static str {
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
"#
    }

    #[test]
    fn test_engine_handle_new_no_thread() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        // No thread spawned until post_run_wasm is called.
        assert!(handle.handle.is_none());
    }

    #[test]
    fn test_post_run_wasm_spawns_thread_and_produces_ops() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        let started =
            handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        assert!(started, "first call should spawn the thread");

        let ops = capture.wait_for_ops();
        assert!(!ops.is_empty(), "should produce a non-empty op buffer");
    }

    #[test]
    fn test_post_run_wasm_second_call_is_noop() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        let first = handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        let second = handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());

        assert!(first, "first call should start the engine");
        assert!(!second, "second call on same handle should be a no-op");
    }

    #[test]
    fn test_drop_joins_thread() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);
        handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());

        // Drop joins the thread — should not hang or panic.
        drop(handle);
    }
}
