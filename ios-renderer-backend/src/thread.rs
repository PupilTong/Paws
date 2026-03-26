//! Background engine thread that owns `RuntimeState` and `ViewTree`.
//!
//! Swift sends [`Command`]s via an `mpsc` channel. The thread processes
//! each command, and after any commit produces an [`OpBuffer`] which it
//! delivers to Swift via a completion callback.

use std::sync::mpsc;
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

/// Handle to the background engine thread.
///
/// Owned by `PawsRenderer` on the FFI side. Holds the channel sender
/// and the join handle for clean shutdown.
pub(crate) struct EngineHandle {
    pub(crate) tx: mpsc::Sender<Option<(Vec<u8>, String)>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    /// Spawns a new background engine thread.
    ///
    /// `base_url` is passed to `RuntimeState::new()`.
    /// `completion` is called from the background thread each time ops are ready.
    /// `context` is an opaque pointer forwarded to the completion callback.
    pub(crate) fn spawn(
        base_url: String,
        completion: CompletionFn,
        context: *mut std::ffi::c_void,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Option<(Vec<u8>, String)>>();

        // SAFETY: We wrap the callback + context in a single Send struct
        // so they can cross the thread boundary. The Swift side guarantees
        // the context pointer remains valid until paws_renderer_destroy.
        let callback = SendCallback {
            completion,
            context,
        };

        let handle = thread::Builder::new()
            .name("paws-engine".to_string())
            .spawn(move || {
                engine_loop(rx, base_url, callback);
            })
            .expect("failed to spawn paws-engine thread");

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Sends a shutdown command and waits for the thread to exit.
    pub(crate) fn shutdown(&mut self) {
        let _ = self.tx.send(None);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Bundles the completion callback and its context pointer for cross-thread
/// transfer.
///
/// # Safety
///
/// The caller must ensure the `context` pointer remains valid until the
/// engine thread is shut down. The completion callback must be safe to
/// call from any thread (typically it dispatches to the main queue).
struct SendCallback {
    completion: CompletionFn,
    context: *mut std::ffi::c_void,
}

// SAFETY: `completion` is a plain function pointer (code address) and
// `context` is forwarded to the callback which dispatches to the main
// queue before touching UIKit.
unsafe impl Send for SendCallback {}

/// The main loop of the background engine thread.
fn engine_loop(rx: mpsc::Receiver<Option<(Vec<u8>, String)>>, base_url: String, cb: SendCallback) {
    let mut state = RuntimeState::new(base_url);
    let mut view_tree = ViewTree::new();
    let mut ops = OpBuffer::new();

    // Receiving Some((wasm, func_name)) runs the module and auto-commits.
    // Receiving None or channel closure shuts down the thread.
    while let Ok(Some((wasm, func_name))) = rx.recv() {
        let taken = std::mem::replace(&mut state, RuntimeState::new("about:blank".to_string()));

        match wasmtime_engine::run_wasm(taken, &wasm, &func_name) {
            Ok(recovered) => {
                state = recovered;
                do_commit(&mut state, &mut view_tree, &mut ops, &cb);
            }
            Err(err) => {
                state = err.state;
                // TODO: surface error to Swift
            }
        }
    }
}

/// Runs the commit pipeline: style → layout → view tree → ops → callback.
fn do_commit(
    state: &mut RuntimeState,
    view_tree: &mut ViewTree,
    ops: &mut OpBuffer,
    cb: &SendCallback,
) {
    let layout = state.commit();
    view_tree.process(&layout, ops);

    // Deliver ops to Swift via the completion callback.
    // The callback is responsible for copying or processing the buffer
    // before returning, as the buffer will be reused on the next frame.
    (cb.completion)(ops.as_ptr(), ops.len(), cb.context);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};

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
    fn test_engine_handle_create_and_shutdown() {
        let capture = TestCapture::new();
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle =
            EngineHandle::spawn("https://example.com".to_string(), test_completion, ctx);

        handle.shutdown();
    }
}
