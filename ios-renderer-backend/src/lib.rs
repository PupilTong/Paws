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
//! completion callback. Swift's `OpExecutor` decodes and executes them.

mod error;
pub(crate) mod ffi;
// TODO: ops and renderer are temporarily unused at the crate root because
// op delivery is not yet wired into __commit. They are still tested
// independently and will be re-integrated when __commit delivers ops.
#[allow(dead_code)]
mod ops;
#[allow(dead_code)]
mod renderer;
mod thread;
