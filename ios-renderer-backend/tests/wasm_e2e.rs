//! End-to-end test: WAT → WASM bridge → engine → renderer pipeline → LayerCmd output.
//!
//! Validates the complete path from a WASM module through the push-model FFI.

use ios_renderer_backend::ffi;
use ios_renderer_backend::types::LayerCmd;

/// A simple WAT module that creates a div, sets its height to 100px, and
/// appends it to the document root.
const SIMPLE_WAT: &str = r#"
(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "height\00")
  (data (i32.const 32) "100px\00")
  (data (i32.const 48) "width\00")
  (data (i32.const 64) "200px\00")
  (data (i32.const 80) "display\00")
  (data (i32.const 96) "block\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $id))
    (drop)
    (call $set_style (local.get $id) (i32.const 80) (i32.const 96))
    (drop)
    (call $set_style (local.get $id) (i32.const 16) (i32.const 32))
    (drop)
    (call $set_style (local.get $id) (i32.const 48) (i32.const 64))
    (drop)
    (local.get $id)
  )
)
"#;

#[test]
fn wasm_app_produces_layer_commands() {
    let handle = ffi::rb_create(1024);
    assert_ne!(handle, 0);

    let wat_bytes = SIMPLE_WAT.as_bytes();

    // SAFETY: handle is valid, wat_bytes points to valid memory.
    let result = unsafe { ffi::rb_run_wasm_app(handle, wat_bytes.as_ptr(), wat_bytes.len()) };
    assert_eq!(result, 0, "rb_run_wasm_app should succeed");

    // Now pull-render to verify commands were produced.
    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count: u32 = 0;

    // SAFETY: handle is valid, cmds is a valid buffer.
    unsafe {
        ffi::rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count);
    }

    assert!(count > 0, "should produce at least one LayerCmd");

    ffi::rb_destroy(handle);
}

#[test]
fn wasm_app_with_push_callback() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let handle = ffi::rb_create(1024);
    let cmd_count = Arc::new(AtomicU32::new(0));

    // Set up push callback.
    unsafe extern "C" fn callback(
        _cmds: *const LayerCmd,
        count: u32,
        user_data: *mut std::ffi::c_void,
    ) {
        // SAFETY: user_data points to an Arc<AtomicU32> that outlives the callback.
        let counter = unsafe { &*(user_data as *const AtomicU32) };
        counter.store(count, Ordering::Release);
    }

    let user_data = Arc::as_ptr(&cmd_count) as *mut std::ffi::c_void;
    ffi::rb_set_render_callback(handle, Some(callback), user_data);

    let wat_bytes = SIMPLE_WAT.as_bytes();
    // SAFETY: handle valid, wat_bytes valid.
    let result = unsafe { ffi::rb_run_wasm_app(handle, wat_bytes.as_ptr(), wat_bytes.len()) };
    assert_eq!(result, 0);

    let received = cmd_count.load(Ordering::Acquire);
    assert!(received > 0, "push callback should have received commands");

    ffi::rb_destroy(handle);
}

#[test]
fn invalid_wasm_returns_error() {
    let handle = ffi::rb_create(1024);
    let garbage = b"not valid wasm";

    // SAFETY: handle valid, garbage is valid bytes.
    let result = unsafe { ffi::rb_run_wasm_app(handle, garbage.as_ptr(), garbage.len()) };
    assert!(result < 0, "invalid WASM should return negative error");

    ffi::rb_destroy(handle);
}
