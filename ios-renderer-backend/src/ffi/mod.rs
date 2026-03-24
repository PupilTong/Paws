//! FFI boundary between Rust and Swift.
//!
//! - `exports`: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//!
//! The old `imports` module (Swift → Rust callbacks for UIKit control) has been
//! removed. UIKit mutations are now driven by the op-code buffer that Swift's
//! `OpExecutor` processes on the main thread.

pub(crate) mod exports;
