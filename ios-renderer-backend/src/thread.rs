//! Background engine thread that owns `RuntimeState` and `ViewTree`.
//!
//! Each [`EngineHandle`] owns at most one background thread at a time.
//! [`run_wasm`](EngineHandle::run_wasm) spawns a fresh thread that creates
//! its own `RuntimeState`, runs the WASM module, commits, and delivers
//! op-codes via the completion callback before exiting.
//!
//! Multiple `EngineHandle` instances can coexist — each owns an independent
//! engine thread and UIKit area. Swift creates a new UI node + engine for
//! each WASM file without needing to stop existing ones.

use std::sync::Arc;
use std::thread;

use engine::RuntimeState;

use crate::ops::OpBuffer;
use crate::renderer::ViewTree;

/// Completion callback type.
///
/// Called from the background thread with a pointer to the op buffer and
/// its byte length. The Swift side is responsible for dispatching to the
/// main queue before touching UIKit.
///
/// # Safety
///
/// The `ops_ptr` is only valid for the duration of the callback invocation.
/// The callback must not store the pointer beyond that scope.
pub(crate) type CompletionFn =
    extern "C" fn(ops_ptr: *const u8, ops_len: usize, ctx: *mut std::ffi::c_void);

/// Bundles the completion callback and its context pointer for cross-thread
/// transfer.
///
/// # Safety
///
/// The caller must ensure the `context` pointer remains valid until the
/// engine is stopped or destroyed. The completion callback must be safe to
/// call from any thread (typically it dispatches to the main queue).
pub(crate) struct SendCallback {
    completion: CompletionFn,
    context: *mut std::ffi::c_void,
}

// SAFETY: `completion` is a plain function pointer (code address) and
// `context` is forwarded to the callback which dispatches to the main
// queue before touching UIKit.
unsafe impl Send for SendCallback {}
// SAFETY: The function pointer is immutable and the context pointer is
// only read (forwarded to the callback), never mutated through &SendCallback.
unsafe impl Sync for SendCallback {}

/// Handle to the background engine thread.
///
/// Owned by `PawsRenderer` on the FFI side. Manages the lifecycle of
/// a single background thread that runs WASM modules.
///
/// Multiple handles can coexist independently — each manages its own
/// engine thread and associated UIKit area.
pub(crate) struct EngineHandle {
    handle: Option<thread::JoinHandle<()>>,
    callback: Arc<SendCallback>,
    base_url: String,
}

impl EngineHandle {
    /// Creates a new engine handle without spawning a thread.
    ///
    /// `base_url` is passed to `RuntimeState::new()` when a thread is spawned.
    /// `completion` is called from the background thread each time ops are ready.
    /// `context` is an opaque pointer forwarded to the completion callback.
    pub(crate) fn new(
        base_url: String,
        completion: CompletionFn,
        context: *mut std::ffi::c_void,
    ) -> Self {
        Self {
            handle: None,
            callback: Arc::new(SendCallback {
                completion,
                context,
            }),
            base_url,
        }
    }

    /// Spawns a fresh background thread to run a WASM module.
    ///
    /// If a previous thread is still running, joins it first (waits for
    /// completion). The new thread creates its own `RuntimeState`, `ViewTree`,
    /// and `OpBuffer`, runs the WASM module, commits the rendering pipeline,
    /// and delivers op-codes via the completion callback before exiting.
    pub(crate) fn run_wasm(&mut self, wasm_bytes: Vec<u8>, func_name: String) {
        // Join any previous thread before spawning a new one.
        if let Some(prev) = self.handle.take() {
            let _ = prev.join();
        }

        let base_url = self.base_url.clone();
        let cb = Arc::clone(&self.callback);

        let handle = thread::Builder::new()
            .name("paws-engine".to_string())
            .spawn(move || {
                run_engine(base_url, &wasm_bytes, &func_name, &cb);
            })
            .expect("failed to spawn paws-engine thread");

        self.handle = Some(handle);
    }

    /// Stops the engine thread if one is running.
    ///
    /// Joins the thread and waits for it to finish. After this call,
    /// no background thread is active for this engine.
    pub(crate) fn stop_engine(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop_engine();
    }
}

/// Runs the full engine pipeline on the background thread:
/// WASM execution → style resolution → layout → view tree → ops → callback.
fn run_engine(base_url: String, wasm_bytes: &[u8], func_name: &str, cb: &SendCallback) {
    let state = RuntimeState::new(base_url);
    let mut view_tree = ViewTree::new();
    let mut ops = OpBuffer::new();

    match wasmtime_engine::run_wasm(state, wasm_bytes, func_name) {
        Ok(mut state) => {
            let layout = state.commit();
            view_tree.process(&layout, &mut ops);

            // Deliver ops to Swift via the completion callback.
            // The callback is responsible for copying or processing the buffer
            // before returning, as the buffer will not persist after this call.
            (cb.completion)(ops.as_ptr(), ops.len(), cb.context);
        }
        Err(_err) => {
            // TODO: surface error to Swift
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};

    /// A test completion callback that captures the op buffer contents
    /// and signals a condvar so tests can wait deterministically.
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

        /// Resets the captured ops for a new round.
        fn reset(&self) {
            *self.ops.lock().unwrap() = Vec::new();
        }
    }

    extern "C" fn test_completion(ptr: *const u8, len: usize, ctx: *mut std::ffi::c_void) {
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

    #[test]
    fn test_engine_handle_new_no_thread() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        // No thread should be running at creation time.
        assert!(handle.handle.is_none());
    }

    #[test]
    fn test_stop_engine_no_thread_is_noop() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        // Stopping without a thread should not panic.
        handle.stop_engine();
        assert!(handle.handle.is_none());
    }

    #[test]
    fn test_run_wasm_produces_ops() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        let wat = r#"
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
"#;
        handle.run_wasm(wat.as_bytes().to_vec(), "run".to_string());

        let ops_bytes = capture.wait_for_ops();
        assert!(
            !ops_bytes.is_empty(),
            "run_wasm should produce a non-empty op buffer"
        );

        handle.stop_engine();
    }

    #[test]
    fn test_sequential_run_wasm_calls() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (i32.const 0)
  )
)
"#;

        // First call
        handle.run_wasm(wat.as_bytes().to_vec(), "run".to_string());
        let ops1 = capture.wait_for_ops();
        assert!(!ops1.is_empty());

        // Reset capture for second call
        capture.reset();

        // Second call — joins the first thread, then spawns a new one
        handle.run_wasm(wat.as_bytes().to_vec(), "run".to_string());
        let ops2 = capture.wait_for_ops();
        assert!(!ops2.is_empty());

        handle.stop_engine();
    }
}
