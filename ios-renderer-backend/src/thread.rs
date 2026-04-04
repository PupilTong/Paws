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

use engine::{EngineRenderer, RuntimeState};

use crate::ops::OpBuffer;
use crate::renderer::ViewTree;

/// Completion callback type.
///
/// Called from the background thread each time the engine commits a frame.
/// - `ops_ptr` / `ops_len`: buffer of 32-byte op-code slots
/// - `strings_ptr` / `strings_len`: UTF-8 string table referenced by text ops
/// - `ctx`: opaque pointer passed from Swift (typically `Unmanaged<OpExecutor>`)
///
/// Both buffers are valid only for the duration of the call — Swift must copy
/// or process them before returning and must dispatch all UIKit mutations to
/// the main queue.
pub(crate) type CompletionFn = extern "C" fn(
    ops_ptr: *const u8,
    ops_len: usize,
    strings_ptr: *const u8,
    strings_len: usize,
    ctx: *mut std::ffi::c_void,
);

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

/// iOS renderer backend that implements [`EngineRenderer`].
///
/// Bundles the [`ViewTree`] diff engine, an [`OpBuffer`], and the completion
/// callback together. On each commit the ViewTree generates minimal ops
/// and delivers them to Swift via the callback.
struct IosRenderer {
    view_tree: ViewTree,
    op_buffer: OpBuffer,
    callback: SendCallback,
}

// SAFETY: `SendCallback` is already `Send` (see its unsafe impl above).
// `ViewTree` and `OpBuffer` are plain data structures with no thread-affine
// pointers — they are trivially `Send`.
unsafe impl Send for IosRenderer {}

impl EngineRenderer for IosRenderer {
    type NodeState = ();

    fn on_commit(
        &mut self,
        doc: &mut engine::dom::Document<()>,
        root_element: Option<engine::NodeId>,
    ) {
        let Some(root_id) = root_element else {
            return;
        };

        // Extract a LayoutBox tree for the ViewTree to process.
        // TODO: refactor ViewTree to walk Document directly and remove this.
        let Some(layout) = engine::compute_layout(doc, root_id) else {
            return;
        };

        self.view_tree.process(&layout, &mut self.op_buffer);
        if self.op_buffer.len() > 0 {
            (self.callback.completion)(
                self.op_buffer.as_ptr(),
                self.op_buffer.len(),
                self.op_buffer.strings_ptr(),
                self.op_buffer.strings_len(),
                self.callback.context,
            );
        }
    }
}

/// Entry point for the background engine thread.
///
/// Creates engine state with an [`IosRenderer`] and runs the WASM module.
/// The WASM drives its own internal event loop — all commits and op delivery
/// happen from within via the `__commit` host function, which calls
/// [`EngineRenderer::on_commit`]. When the WASM loop exits, all engine state
/// drops and the UIKit sub-tree is released.
fn run_engine(base_url: String, wasm_bytes: Vec<u8>, func_name: String, cb: SendCallback) {
    let renderer = IosRenderer {
        view_tree: ViewTree::new(),
        op_buffer: OpBuffer::new(),
        callback: cb,
    };
    let state = RuntimeState::with_renderer(base_url, renderer);

    // Blocks until WASM's own event loop exits.
    let _ = wasmtime_engine::run_wasm(state, &wasm_bytes, &func_name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{make_wat_module, noop_completion};

    #[test]
    fn test_engine_handle_new_no_thread() {
        let handle = EngineHandle::new(
            "https://example.com".to_string(),
            noop_completion,
            std::ptr::null_mut(),
        );

        // No thread spawned until post_run_wasm is called.
        assert!(handle.handle.is_none());
    }

    #[test]
    fn test_post_run_wasm_spawns_thread() {
        let mut handle = EngineHandle::new(
            "https://example.com".to_string(),
            noop_completion,
            std::ptr::null_mut(),
        );

        let started =
            handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        assert!(started, "first call should spawn the thread");
        assert!(handle.handle.is_some());
    }

    #[test]
    fn test_post_run_wasm_second_call_is_noop() {
        let mut handle = EngineHandle::new(
            "https://example.com".to_string(),
            noop_completion,
            std::ptr::null_mut(),
        );

        let first = handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());
        let second = handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());

        assert!(first, "first call should start the engine");
        assert!(!second, "second call on same handle should be a no-op");
    }

    #[test]
    fn test_drop_joins_thread() {
        let mut handle = EngineHandle::new(
            "https://example.com".to_string(),
            noop_completion,
            std::ptr::null_mut(),
        );
        handle.post_run_wasm(make_wat_module().as_bytes().to_vec(), "run".to_string());

        // Drop joins the thread — should not hang or panic.
        drop(handle);
    }
}
