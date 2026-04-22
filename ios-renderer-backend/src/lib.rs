//! iOS renderer backend for Paws.
//!
//! The rendering pipeline runs on a background thread:
//! 1. WASM execution mutates the DOM via `RuntimeState`
//! 2. Stylo resolves CSS styles
//! 3. Taffy computes layout
//! 4. `ViewTree` generates minimal updating op-codes
//! 5. Op-codes are sent to Swift's main thread for UIKit execution
//!
//! The op-code buffer is a flat array of 32-byte slots passed via a
//! completion callback. Variable-length text content is passed alongside
//! in a separate string table buffer.

mod error;
pub(crate) mod ffi;
mod ops;
mod renderer;
mod thread;

/// Shared test utilities used by `thread::tests` and `ffi::exports::tests`.
#[cfg(test)]
pub(crate) mod test_util {
    /// No-op completion callback for tests.
    pub(crate) extern "C" fn noop_completion(
        _ops: *const u8,
        _ops_len: usize,
        _strings: *const u8,
        _strings_len: usize,
        _ctx: *mut std::ffi::c_void,
    ) {
    }

    /// WAT module that creates a styled div and commits.
    ///
    /// Creates a `<div>` with `width: 100px`, appends it to the document root,
    /// and calls `__commit` to trigger the rendering pipeline.
    pub(crate) fn make_wat_module() -> &'static str {
        r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "width\00")
  (data (i32.const 32) "100px\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $style (local.get $id) (i32.const 16) (i32.const 32)))
    (drop (call $commit))
    (i32.const 0)
  )
)
"#
    }
}
