//! Background engine thread — the "WebWorker" of the iOS renderer.
//!
//! [`EngineHandle`] owns a single background thread that mirrors the browser
//! WebWorker model:
//!
//! 1. Swift calls [`EngineHandle::post_run_wasm`] **once** to start the
//!    rendering pipeline. The thread spawns, creates a fresh
//!    [`RuntimeState`], compiles the guest, and calls `run`.
//! 2. After `run` returns the thread keeps the wasmtime [`Store`] alive
//!    inside a [`ComponentSession`] and waits on a control channel for
//!    further messages — pointer events from the host land here as
//!    [`EngineMessage::Click`] entries and re-enter the guest's
//!    `invoke-listener` via
//!    [`ComponentSession::dispatch_pointer_event`]. This is what powers
//!    the FFI [`paws_renderer_dispatch_click`].
//! 3. When [`EngineHandle`] is dropped, an [`EngineMessage::Stop`] is
//!    posted and the thread is joined; all engine state (Store, DOM,
//!    ViewTree) drops with it, releasing the associated UIKit sub-tree.
//!
//! Core (non-component) WAT modules used by the unit tests skip the
//! session and run through the legacy one-shot
//! [`run_wasm_with_engine`](wasmtime_engine::run_wasm_with_engine) path —
//! they have no `invoke-listener` export to re-enter, so the message
//! loop simply sits and waits for `Stop`.

use std::sync::mpsc;
use std::thread;

use engine::{EngineRenderer, RuntimeState};

use crate::renderer::{IosNodeState, ViewTree};

/// Control messages sent from FFI threads to the engine thread.
///
/// Pointer events arrive as [`EngineMessage::Click`]; renderer teardown
/// posts an [`EngineMessage::Stop`] so the thread can exit its message
/// loop and drop the wasmtime [`Store`] cleanly.
pub(crate) enum EngineMessage {
    /// Host-driven click at a viewport-space (CSS-pixel, top-left origin)
    /// point. The engine thread runs hit-test against the laid-out
    /// document and dispatches `click` to the resolved element.
    Click { x: f32, y: f32 },
    /// Tear-down signal posted by [`EngineHandle::Drop`]. Causes the
    /// message loop to exit and the session to drop.
    Stop,
}

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
    /// Viewport (width, height) passed to Taffy for guest layout.
    /// `None` means `MAX_CONTENT` (content-sized layout, the historical
    /// default). Setting this to the host view's bounds makes unstyled
    /// block elements fill the available width instead of collapsing to
    /// their intrinsic content size.
    viewport: Option<(f32, f32)>,
    /// Sender half of the engine-thread control channel. `Some` once
    /// [`post_run_wasm`](Self::post_run_wasm) has been called. Pointer
    /// events posted via FFI land here.
    msg_sender: Option<mpsc::Sender<EngineMessage>>,
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
            viewport: None,
            msg_sender: None,
        }
    }

    /// Posts a click message to the engine thread. Returns `true` if the
    /// message was queued, `false` if the engine has not been started or
    /// the thread already exited (receiver hung up).
    pub(crate) fn post_click(&self, x: f32, y: f32) -> bool {
        match &self.msg_sender {
            Some(sender) => sender.send(EngineMessage::Click { x, y }).is_ok(),
            None => false,
        }
    }

    /// Sets the viewport that the engine will apply to `RuntimeState`
    /// before running the WASM module. The viewport is captured once when
    /// the background thread spawns in
    /// [`post_run_wasm`](Self::post_run_wasm), so calls after that return
    /// without mutating state.
    ///
    /// Both dimensions must be finite and strictly positive — Taffy
    /// treats NaN / infinite / non-positive values as layout bugs.
    /// Non-conforming inputs trigger a `debug_assert!` and are treated as
    /// "no viewport" in release builds.
    pub(crate) fn set_viewport(&mut self, width: f32, height: f32) {
        if self.handle.is_some() {
            // Engine already started — viewport is capture-once. Bail
            // rather than silently holding a value that will never be
            // read (see post_run_wasm below, which reads viewport once
            // at thread-spawn time).
            return;
        }
        let is_valid = width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0;
        debug_assert!(
            is_valid,
            "viewport dimensions must be finite and positive, got {width}×{height}"
        );
        self.viewport = if is_valid {
            Some((width, height))
        } else {
            None
        };
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
        let viewport = self.viewport;
        let (sender, receiver) = mpsc::channel::<EngineMessage>();

        let handle = thread::Builder::new()
            .name("paws-engine".to_string())
            .spawn(move || run_engine(base_url, wasm_bytes, func_name, cb, viewport, receiver))
            .expect("failed to spawn paws-engine thread");

        self.handle = Some(handle);
        self.msg_sender = Some(sender);
        true
    }
}

impl Drop for EngineHandle {
    /// Posts [`EngineMessage::Stop`] and joins the engine thread.
    ///
    /// All engine state (`RuntimeState`, `ViewTree`, `OpBuffer`,
    /// wasmtime [`Store`]) drops when the thread exits, releasing the
    /// associated UIKit sub-tree.
    fn drop(&mut self) {
        if let Some(sender) = self.msg_sender.take() {
            // If the engine thread already exited (e.g. WAT path with a
            // failure inside the run), send returns Err; that's fine —
            // the join below will still wait for the thread to finish.
            let _ = sender.send(EngineMessage::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// iOS renderer backend that implements [`EngineRenderer`].
///
/// Bundles the [`ViewTree`] diff engine and the completion callback.
/// On each commit the ViewTree walks the Document tree directly,
/// generates minimal ops, and delivers them to Swift via the callback.
struct IosRenderer {
    view_tree: ViewTree,
    callback: SendCallback,
}

// SAFETY: `SendCallback` is already `Send` (see its unsafe impl above).
// `ViewTree` is a plain data structure with no thread-affine pointers.
unsafe impl Send for IosRenderer {}

impl EngineRenderer for IosRenderer {
    type NodeState = IosNodeState;

    fn on_commit(
        &mut self,
        doc: &mut engine::dom::Document<IosNodeState>,
        resources: &dyn engine::ResourceResolver,
        root_element: Option<engine::NodeId>,
    ) {
        self.view_tree.process(doc, resources, root_element);
        let ops = self.view_tree.ops();
        if ops.len() > 0 {
            (self.callback.completion)(
                ops.as_ptr(),
                ops.len(),
                ops.strings_ptr(),
                ops.strings_len(),
                self.callback.context,
            );
        }
    }
}

/// Entry point for the background engine thread.
///
/// Creates engine state with an [`IosRenderer`], compiles + runs the
/// guest, then enters a small message loop: every
/// [`EngineMessage::Click`] is hit-tested and dispatched as a `click`
/// event back into the guest's `invoke-listener`. The WASM module drives
/// its initial DOM build inside `run`; subsequent mutations happen
/// inside listener callbacks fired here.
///
/// The thread exits when an [`EngineMessage::Stop`] is received (posted
/// by [`EngineHandle::Drop`]) or when the channel is hung up. All engine
/// state — including the wasmtime [`Store`] held by the
/// [`ComponentSession`] — drops at that point, releasing the UIKit
/// sub-tree.
fn run_engine(
    base_url: String,
    wasm_bytes: Vec<u8>,
    func_name: String,
    cb: SendCallback,
    viewport: Option<(f32, f32)>,
    receiver: mpsc::Receiver<EngineMessage>,
) {
    let renderer = IosRenderer {
        view_tree: ViewTree::new(),
        callback: cb,
    };
    let state = match viewport {
        Some((width, height)) => {
            RuntimeState::with_definite_viewport(base_url, renderer, (), width, height)
        }
        None => RuntimeState::with_renderer(base_url, renderer, ()),
    };

    // Paws examples are wasm32-wasip2 components (see
    // `Paws/examples/build.rs` — `WASM_TARGET = "wasm32-wasip2"`), but
    // the existing FFI also accepts hand-written core modules (the WAT
    // path used by `paws_renderer_post_run_wat` and the thread unit
    // tests). Dispatch on the wasm header layer byte so both survive:
    // core (layer 0) → `run_wasm` (one-shot, no pointer dispatch),
    // component (layer 1) → `ComponentSession::start` (live, supports
    // pointer dispatch).
    let engine = wasmtime_engine::create_engine();

    if is_wasm_component(&wasm_bytes) {
        let mut session =
            match wasmtime_engine::ComponentSession::start(&engine, state, &wasm_bytes) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("paws iOS engine: guest failed to start: {}", e.error);
                    return;
                }
            };

        // Pump messages until the channel closes or a Stop is received.
        // Each Click runs hit-test → dispatch_event_component, which
        // re-enters the guest's `invoke-listener`. Any DOM mutation +
        // commit() inside the listener flows ops through
        // IosRenderer::on_commit on this same thread.
        for msg in receiver {
            match msg {
                EngineMessage::Click { x, y } => {
                    if let Err(e) = session.dispatch_pointer_event(x, y, "click") {
                        eprintln!("paws iOS engine: pointer dispatch failed: {e}");
                    }
                }
                EngineMessage::Stop => break,
            }
        }
        // Session drops here; Store + RuntimeState + UIKit nodes go with it.
    } else {
        // Core-module path — used only by WAT unit tests. There is no
        // `invoke-listener` export, so pointer events are silently
        // ignored. Run the module once, then sit on the channel until
        // Stop / hang-up so the join contract still holds.
        if let Err(e) =
            wasmtime_engine::run_wasm_with_engine(&engine, state, &wasm_bytes, &func_name)
        {
            eprintln!("paws iOS engine: guest failed to run: {}", e.error);
            return;
        }
        for msg in receiver {
            if matches!(msg, EngineMessage::Stop) {
                break;
            }
        }
    }
}

/// Returns `true` when `bytes` look like a component-model binary
/// (`layer == 1` in the wasm header), `false` for a core module.
///
/// Every wasm binary starts with the 4-byte magic `\0asm`. The next
/// four bytes encode `(version: u16, layer: u16)` little-endian: core
/// modules use `layer = 0`, components use `layer = 1` (see the
/// component-model spec). Short or non-wasm inputs fall through as
/// "not a component"; wasmtime will then produce a parse error, which
/// surfaces the same way as before this dispatch existed.
fn is_wasm_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..4] == b"\0asm" && u16::from_le_bytes([bytes[6], bytes[7]]) == 1
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

    #[test]
    fn test_is_wasm_component_routes_by_header() {
        // Core module header: magic + (version=1, layer=0).
        let core = [
            b'\0', b'a', b's', b'm', // magic
            0x01, 0x00, // version = 1
            0x00, 0x00, // layer = 0 → core
        ];
        assert!(
            !is_wasm_component(&core),
            "core module must route to run_wasm"
        );

        // Component header: magic + (version=0x000D, layer=1). Byte 6–7
        // is the layer (little-endian u16); byte 4–5 is the preview
        // version and is free to change as the component spec evolves.
        let component = [
            b'\0', b'a', b's', b'm', // magic
            0x0D, 0x00, // version
            0x01, 0x00, // layer = 1 → component
        ];
        assert!(
            is_wasm_component(&component),
            "component must route to run_component"
        );

        // Too short / not wasm at all.
        assert!(!is_wasm_component(b""), "empty input is not a component");
        assert!(
            !is_wasm_component(b"(module)"),
            "WAT text is not a binary component"
        );
    }
}
