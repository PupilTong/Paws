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

/// Commands sent from the main thread (Swift FFI) to the background engine thread.
pub(crate) enum Command {
    /// Trigger style resolution + layout + op generation.
    Commit,

    /// Compile and run a WAT module, then auto-commit.
    RunWat { wat: String, func_name: String },

    /// Create a DOM element, reply with its node ID.
    CreateElement {
        tag: String,
        reply: mpsc::Sender<u32>,
    },

    /// Create a text node, reply with its node ID.
    CreateTextNode {
        text: String,
        reply: mpsc::Sender<u32>,
    },

    /// Append a child to a parent. Reply with 0 or error code.
    AppendElement {
        parent: u32,
        child: u32,
        reply: mpsc::Sender<i32>,
    },

    /// Set an inline CSS property. Reply with 0 or error code.
    SetInlineStyle {
        id: u32,
        name: String,
        value: String,
        reply: mpsc::Sender<i32>,
    },

    /// Set a DOM attribute. Reply with 0 or error code.
    SetAttribute {
        id: u32,
        name: String,
        value: String,
        reply: mpsc::Sender<i32>,
    },

    /// Add a CSS stylesheet.
    AddStylesheet { css: String },

    /// Destroy an element. Reply with 0 or error code.
    DestroyElement { id: u32, reply: mpsc::Sender<i32> },

    /// Shut down the background thread.
    Shutdown,
}

// SAFETY: Command is Send because all its fields are Send.
// The mpsc::Sender<T> and String types are all Send.
unsafe impl Send for Command {}

/// Handle to the background engine thread.
///
/// Owned by `PawsRenderer` on the FFI side. Holds the channel sender
/// and the join handle for clean shutdown.
pub(crate) struct EngineHandle {
    pub(crate) tx: mpsc::Sender<Command>,
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
        let (tx, rx) = mpsc::channel::<Command>();

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
        let _ = self.tx.send(Command::Shutdown);
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
fn engine_loop(rx: mpsc::Receiver<Command>, base_url: String, cb: SendCallback) {
    let mut state = RuntimeState::new(base_url);
    let mut view_tree = ViewTree::new();
    let mut ops = OpBuffer::new();

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Commit => {
                do_commit(&mut state, &mut view_tree, &mut ops, &cb);
            }

            Command::RunWat { wat, func_name } => {
                // Move state into wasmtime for execution, then recover it.
                let taken =
                    std::mem::replace(&mut state, RuntimeState::new("about:blank".to_string()));

                match wasmtime_engine::run_wat(taken, &wat, &func_name) {
                    Ok(recovered) => {
                        state = recovered;
                        do_commit(&mut state, &mut view_tree, &mut ops, &cb);
                    }
                    Err(err) => {
                        // Recover state even on error.
                        state = err.state;
                        // TODO: surface error to Swift via a separate error callback
                    }
                }
            }

            Command::CreateElement { tag, reply } => {
                let id = state.create_element(tag);
                let _ = reply.send(id);
            }

            Command::CreateTextNode { text, reply } => {
                let id = state.create_text_node(text);
                let _ = reply.send(id);
            }

            Command::AppendElement {
                parent,
                child,
                reply,
            } => {
                let result = match state.append_element(parent, child) {
                    Ok(()) => 0,
                    Err(code) => code.as_i32(),
                };
                let _ = reply.send(result);
            }

            Command::SetInlineStyle {
                id,
                name,
                value,
                reply,
            } => {
                let result = match state.set_inline_style(id, name, value) {
                    Ok(()) => 0,
                    Err(code) => code.as_i32(),
                };
                let _ = reply.send(result);
            }

            Command::SetAttribute {
                id,
                name,
                value,
                reply,
            } => {
                let result = match state.set_attribute(id, name, value) {
                    Ok(()) => 0,
                    Err(code) => code.as_i32(),
                };
                let _ = reply.send(result);
            }

            Command::AddStylesheet { css } => {
                state.add_stylesheet(css);
            }

            Command::DestroyElement { id, reply } => {
                let result = match state.destroy_element(id) {
                    Ok(()) => 0,
                    Err(code) => code.as_i32(),
                };
                let _ = reply.send(result);
            }

            Command::Shutdown => break,
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
    use std::sync::{Arc, Mutex};

    /// A test completion callback that captures the op buffer contents.
    struct TestCapture {
        ops: Mutex<Vec<u8>>,
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
    }

    #[test]
    fn test_engine_handle_create_and_shutdown() {
        let capture = Arc::new(TestCapture {
            ops: Mutex::new(Vec::new()),
        });
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let mut handle =
            EngineHandle::spawn("https://example.com".to_string(), test_completion, ctx);

        handle.shutdown();
    }

    #[test]
    fn test_create_element_via_channel() {
        let capture = Arc::new(TestCapture {
            ops: Mutex::new(Vec::new()),
        });
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::spawn("https://example.com".to_string(), test_completion, ctx);

        let (reply_tx, reply_rx) = mpsc::channel();
        handle
            .tx
            .send(Command::CreateElement {
                tag: "div".to_string(),
                reply: reply_tx,
            })
            .unwrap();

        let node_id = reply_rx.recv().unwrap();
        assert!(
            node_id > 0,
            "createElement should return a positive node ID"
        );

        drop(handle);
    }

    #[test]
    fn test_commit_produces_ops() {
        let capture = Arc::new(TestCapture {
            ops: Mutex::new(Vec::new()),
        });
        let ctx = Arc::as_ptr(&capture) as *mut std::ffi::c_void;

        let handle = EngineHandle::spawn("https://example.com".to_string(), test_completion, ctx);

        // Create an element and append it to root.
        let (reply_tx, reply_rx) = mpsc::channel();
        handle
            .tx
            .send(Command::CreateElement {
                tag: "div".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        let node_id = reply_rx.recv().unwrap();

        let (reply_tx, reply_rx) = mpsc::channel();
        handle
            .tx
            .send(Command::AppendElement {
                parent: 0,
                child: node_id,
                reply: reply_tx,
            })
            .unwrap();
        reply_rx.recv().unwrap();

        // Set inline style so it has dimensions.
        let (reply_tx, reply_rx) = mpsc::channel();
        handle
            .tx
            .send(Command::SetInlineStyle {
                id: node_id,
                name: "width".to_string(),
                value: "100px".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        reply_rx.recv().unwrap();

        // Commit should invoke the callback with ops.
        handle.tx.send(Command::Commit).unwrap();

        // Give the background thread a moment to process.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let ops_bytes = capture.ops.lock().unwrap();
        assert!(
            !ops_bytes.is_empty(),
            "commit should produce a non-empty op buffer"
        );

        drop(handle);
    }
}
