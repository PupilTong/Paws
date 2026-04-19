//! Audited `unsafe` island for SharedMemory bulk access.
//!
//! wasmtime 43's [`SharedMemory::data`] returns `&[UnsafeCell<u8>]` and the
//! crate exposes no safe bulk-read / bulk-write primitive. Paws' invariant —
//! host functions always run while no other wasm thread writes to linear
//! memory — makes raw reads safe in practice but is not encodable in Rust's
//! type system.
//!
//! This module is the *only* place in `wasmtime-engine` allowed to use
//! `unsafe`. Every other file in the crate carries `#![forbid(unsafe_code)]`.
//! The functions here expose **safe** signatures and document the load-bearing
//! invariant internally.
//!
//! The Paws invariant is upheld by two structural properties:
//!
//! 1. Host functions execute with the calling wasm thread suspended. For WAT
//!    tests and non-threaded modules there is only one wasm thread; the
//!    invariant is trivial.
//! 2. Under `wasm32-wasip1-threads` + wasi-threads, *any* host function that
//!    reaches into `RuntimeState` requires a `MainThreadToken` (defined in
//!    `store_data.rs`). The token is `!Send` and cannot be smuggled to worker
//!    threads, so worker threads never enter code paths that read or write
//!    SharedMemory on behalf of the host. Workers use SharedMemory only for
//!    their own internal data, and message-pass to main.
//!
//! If either invariant is violated (e.g. future work adds re-entrant host
//! callbacks, or a host function bypasses the token), the SAFETY justification
//! below is broken and this module must be revisited.

use wasmtime::SharedMemory;

/// Exposes the full linear-memory region of a [`SharedMemory`] as `&[u8]`.
///
/// Returns the closure's result. Safe to call because the Paws single-writer
/// invariant documented at the module level guarantees no concurrent writes
/// to the memory region during `f`'s execution.
#[allow(unsafe_code)]
pub fn with_shared_bytes<T>(shared: &SharedMemory, f: impl FnOnce(&[u8]) -> T) -> T {
    let raw = shared.data();
    // SAFETY: Paws invariant — host functions always run with the calling
    // wasm thread suspended, and worker threads cannot obtain a
    // `MainThreadToken`, so they never enter this code path. No concurrent
    // writes are possible for the duration of `f`.
    let data = unsafe { std::slice::from_raw_parts(raw.as_ptr() as *const u8, raw.len()) };
    f(data)
}

/// Exposes the full linear-memory region of a [`SharedMemory`] as `&mut [u8]`.
///
/// Returns the closure's result. Safe to call under the same invariant as
/// [`with_shared_bytes`].
#[allow(unsafe_code)]
pub fn with_shared_bytes_mut<T>(shared: &SharedMemory, f: impl FnOnce(&mut [u8]) -> T) -> T {
    let raw = shared.data();
    // SAFETY: Paws invariant — see module-level docs. No other reader or
    // writer can access the memory during `f`.
    let data = unsafe { std::slice::from_raw_parts_mut(raw.as_ptr() as *mut u8, raw.len()) };
    f(data)
}
