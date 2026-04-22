//! End-to-end regression tests that load real wasm32-wasip2 component
//! examples through the iOS renderer FFI and assert the completion
//! callback fires with non-empty op buffers.
//!
//! Paws example WASMs are component-model binaries. The iOS backend
//! previously used the core-module entry point (`wasmtime::Module::new`)
//! which silently rejects components, so every guest was a no-op and the
//! host view ended up empty. These tests exercise the real FFI pipeline
//! so that regression can't recur without a unit-test failure.
//!
//! Runs against the `paws-examples` build artifacts (see
//! `Paws/examples/build.rs`).

use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ios_renderer_backend::{
    paws_renderer_create, paws_renderer_destroy, paws_renderer_post_run_wasm,
    paws_renderer_set_viewport,
};

/// Context shared with the C completion callback — counts invocations
/// and the total bytes delivered in the ops buffer.
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
    // SAFETY: `ctx` was constructed from `Arc::into_raw` and is kept alive
    // for the renderer's lifetime by the test (we leak it intentionally —
    // the Rust thread reads it and the test drops the renderer before
    // reading the counters).
    let state = unsafe { &*(ctx as *const CallbackState) };
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.total_ops_bytes.fetch_add(ops_len, Ordering::SeqCst);
}

fn run_example_via_ffi(resource_name: &str) -> Arc<CallbackState> {
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

    // `destroy` joins the background thread; by the time it returns the
    // guest has finished and every completion callback that will ever
    // fire has fired.
    paws_renderer_destroy(renderer);

    state
}

/// The yew counter produces a `<div><button>+</button><span>0</span></div>`
/// tree. The iOS backend must deliver at least one op buffer or the host
/// view stays empty — that was the regression this test guards against.
#[test]
fn yew_counter_component_delivers_ops_via_ffi() {
    let state = run_example_via_ffi("example_yew_counter");
    let calls = state.calls.load(Ordering::SeqCst);
    let bytes = state.total_ops_bytes.load(Ordering::SeqCst);
    assert!(
        calls >= 1,
        "completion callback should fire at least once for example_yew_counter, got {calls}"
    );
    assert!(
        bytes > 0,
        "completion callback should deliver non-empty ops buffer, got {bytes} bytes"
    );
}

/// Exercises the full commit path via a hand-written (non-yew) component.
/// A sibling to the yew test — catches regressions where run_component is
/// fine for yew but breaks for rust-wasm-binding + explicit `commit()`.
#[test]
fn commit_full_component_delivers_ops_via_ffi() {
    let state = run_example_via_ffi("example_commit_full");
    assert!(
        state.calls.load(Ordering::SeqCst) >= 1,
        "completion callback should fire for example_commit_full"
    );
    assert!(
        state.total_ops_bytes.load(Ordering::SeqCst) > 0,
        "completion callback should deliver a non-empty op buffer"
    );
}
