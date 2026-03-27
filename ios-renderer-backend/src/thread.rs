//! Background engine thread — the "WebWorker" of the iOS renderer.
//!
//! Each [`EngineHandle`] owns one long-running background thread that behaves
//! like a browser WebWorker:
//!
//! 1. Thread starts immediately when [`EngineHandle::new`] is called.
//! 2. It waits in an event loop for [`EngineCommand`]s sent via an `mpsc` channel.
//! 3. [`EngineCommand::RunWasm`] runs a WASM function against the engine's persistent
//!    [`RuntimeState`], then commits and delivers op-codes to Swift.
//! 4. The [`RuntimeState`], [`ViewTree`], and [`OpBuffer`] live for the thread's full
//!    lifetime — they are **not** recreated between calls.
//! 5. The thread exits (and drops all engine state) only when [`EngineHandle`] is
//!    dropped, which sends [`EngineCommand::Stop`] and joins the thread.
//!
//! This mirrors the browser model: Swift delegates a UIKit sub-tree to the engine.
//! The engine fully owns and controls that area from start until stop. A new
//! `EngineHandle` (and therefore a new thread + fresh `RuntimeState`) is created
//! for each new WASM module.

use std::sync::mpsc;
use std::thread;

use engine::RuntimeState;

use crate::ops::OpBuffer;
use crate::renderer::ViewTree;

/// Completion callback type.
///
/// Called from the background thread each time the engine commits a new frame.
/// The `ops_ptr` points to a buffer of 32-byte op-code slots valid only for
/// the duration of the callback. Swift must copy or process it before returning,
/// and must dispatch UIKit mutations to the main queue.
pub(crate) type CompletionFn =
    extern "C" fn(ops_ptr: *const u8, ops_len: usize, ctx: *mut std::ffi::c_void);

/// Bundles the completion callback and its opaque context pointer for
/// cross-thread transfer.
///
/// # Safety
///
/// `context` must remain valid until the engine is stopped. The callback
/// must be safe to call from any thread (Swift dispatches to main queue internally).
struct SendCallback {
    completion: CompletionFn,
    context: *mut std::ffi::c_void,
}

// SAFETY: `completion` is a plain function pointer (code address, not heap data).
// `context` is forwarded to a callback that dispatches to the main queue —
// the Rust thread never dereferences it directly.
unsafe impl Send for SendCallback {}

/// Commands sent to the background engine thread via [`mpsc::Sender`].
enum EngineCommand {
    /// Run the named export of a WASM module against the persistent engine state,
    /// then commit and deliver op-codes.
    RunWasm {
        wasm_bytes: Vec<u8>,
        func_name: String,
    },
    /// Signal the thread to exit. Sent by [`EngineHandle`]'s `Drop` impl.
    Stop,
}

/// Handle to the long-running background engine thread.
///
/// Modelled after a browser WebWorker: the thread starts on construction,
/// owns all engine state for its lifetime, and shuts down when this handle
/// is dropped.
///
/// Multiple handles can coexist independently — each manages its own
/// thread and UIKit sub-tree.
pub(crate) struct EngineHandle {
    tx: mpsc::Sender<EngineCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    /// Creates a new engine handle and spawns the background thread.
    ///
    /// The thread starts immediately and waits for commands. `base_url` is used
    /// to initialise [`RuntimeState`]. `completion` is called from the background
    /// thread after each commit; `context` is forwarded to every call.
    pub(crate) fn new(
        base_url: String,
        completion: CompletionFn,
        context: *mut std::ffi::c_void,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<EngineCommand>();
        let cb = SendCallback {
            completion,
            context,
        };

        let handle = thread::Builder::new()
            .name("paws-engine".to_string())
            .spawn(move || engine_loop(base_url, rx, cb))
            .expect("failed to spawn paws-engine thread");

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Sends a WASM module to the engine thread for execution.
    ///
    /// The command is delivered asynchronously. The completion callback fires
    /// from the background thread once the commit is done. Returns `false` if
    /// the thread has already exited.
    pub(crate) fn post_run_wasm(&self, wasm_bytes: Vec<u8>, func_name: String) -> bool {
        self.tx
            .send(EngineCommand::RunWasm {
                wasm_bytes,
                func_name,
            })
            .is_ok()
    }
}

impl Drop for EngineHandle {
    /// Signals the engine thread to stop and waits for it to finish.
    ///
    /// All engine state (`RuntimeState`, `ViewTree`, `OpBuffer`) is dropped
    /// when the thread exits, releasing the associated UIKit sub-tree.
    fn drop(&mut self) {
        // Best-effort: thread may have already exited on error.
        let _ = self.tx.send(EngineCommand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// The engine event loop — runs on the background thread for its full lifetime.
///
/// Owns `RuntimeState`, `ViewTree`, and `OpBuffer`. These persist across
/// multiple `RunWasm` commands, so DOM state and view snapshots accumulate
/// between calls (enabling incremental updates).
fn engine_loop(base_url: String, rx: mpsc::Receiver<EngineCommand>, cb: SendCallback) {
    let mut state = RuntimeState::new(base_url);
    let mut view_tree = ViewTree::new();
    let mut ops = OpBuffer::new();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            EngineCommand::Stop => break,

            EngineCommand::RunWasm {
                wasm_bytes,
                func_name,
            } => {
                // `run_wasm` takes ownership of `RuntimeState` (wasmtime stores it
                // inside the `Store`). We swap it out with a blank placeholder for
                // the duration of the call, then recover the real state afterward.
                // The placeholder is never committed or diffed.
                let taken =
                    std::mem::replace(&mut state, RuntimeState::new("about:blank".to_string()));

                match wasmtime_engine::run_wasm(taken, &wasm_bytes, &func_name) {
                    Ok(recovered) => {
                        state = recovered;
                        let layout = state.commit();
                        view_tree.process(&layout, &mut ops);
                        (cb.completion)(ops.as_ptr(), ops.len(), cb.context);
                    }
                    Err(err) => {
                        // Recover state so subsequent calls still work.
                        state = err.state;
                        // TODO: surface error code to Swift
                    }
                }
            }
        }
    }
    // `state`, `view_tree`, and `ops` drop here, releasing all engine resources.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};

    /// Captures op buffer contents from the completion callback.
    /// Uses a condvar so tests can wait deterministically without sleeping.
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

        /// Blocks until the completion callback fires once, then returns the ops.
        fn wait_for_ops(&self) -> Vec<u8> {
            let guard = self.ops.lock().unwrap();
            let (guard, _) = self
                .condvar
                .wait_timeout_while(guard, std::time::Duration::from_secs(5), |o| o.is_none())
                .unwrap();
            guard.clone().unwrap_or_default()
        }

        /// Resets the capture so the next `wait_for_ops` waits for a new callback.
        fn reset(&self) {
            *self.ops.lock().unwrap() = None;
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
    fn test_engine_handle_new_spawns_thread() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        // Thread is spawned on construction.
        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);
        assert!(handle.handle.is_some());

        // Drop sends Stop and joins cleanly.
        drop(handle);
    }

    #[test]
    fn test_run_wasm_produces_ops() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);
        let sent = handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        assert!(sent, "post_run_wasm should succeed while thread is alive");

        let ops = capture.wait_for_ops();
        assert!(!ops.is_empty(), "should produce a non-empty op buffer");
    }

    /// A WAT module that mutates node 1 (the layout-root div created in the
    /// first [`make_wat_module`] run) by adding a `height` style.
    ///
    /// Calling `set_inline_style` on an existing node marks it dirty and
    /// propagates dirtiness up to the document root, so `ensure_styles_resolved`
    /// re-runs and the updated computed values reach the layout and ViewTree.
    ///
    /// This is used to verify that DOM state accumulated from a previous WASM run
    /// is visible to subsequent runs on the same long-running engine thread.
    fn make_wat_mutate_module() -> &'static str {
        r#"
(module
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "height\00")
  (data (i32.const 16) "50px\00")
  (func (export "run") (result i32)
    ;; Node 1 is the layout-root div created in the first run.
    ;; Mutate its height — this marks it style-dirty and propagates dirtiness
    ;; to the document root so styles get re-resolved on the next commit.
    (drop (call $style (i32.const 1) (i32.const 0) (i32.const 16)))
    (i32.const 0)
  )
)
"#
    }

    #[test]
    fn test_sequential_run_wasm_accumulates_state() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);

        // Run 1: creates div1 (node 1, width:100px), appended to doc root.
        // ViewTree sees a new node → emits Declare + SetFrame + Attach ops.
        handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        let ops1 = capture.wait_for_ops();
        assert!(
            !ops1.is_empty(),
            "first run should produce ops for the new div"
        );

        // Run 2: mutates div1 (node 1) by adding height:50px.
        // The engine thread reuses its RuntimeState — div1 is still in the DOM.
        // set_inline_style propagates dirtiness to root → styles re-resolved →
        // layout height changes → ViewTree emits SetViewFrame op.
        capture.reset();
        handle.post_run_wasm(
            make_wat_mutate_module().as_bytes().to_vec(),
            "run".to_string(),
        );
        let ops2 = capture.wait_for_ops();
        assert!(
            !ops2.is_empty(),
            "second run should produce ops reflecting the height change on div1"
        );
    }

    #[test]
    fn test_post_run_wasm_returns_false_after_drop() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::new("https://example.com".to_string(), test_completion, ctx);
        // Clone the sender before dropping the handle.
        let tx_clone = handle.tx.clone();
        drop(handle); // Stop is sent, thread exits.

        // Sending after thread exit should fail.
        let result = tx_clone.send(EngineCommand::RunWasm {
            wasm_bytes: vec![],
            func_name: String::new(),
        });
        assert!(result.is_err());
    }
}
