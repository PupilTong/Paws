//! FFI boundary between Rust and Swift.
//!
//! - `exports`: `#[no_mangle] pub extern "C"` functions that Swift calls into Rust.
//! - `imports`: `extern "C"` declarations for functions Swift implements,
//!   allowing Rust to create and control UIKit objects.

pub(crate) mod exports;
pub(crate) mod imports;
