//! iOS renderer backend for Paws.
//!
//! Bridges the Paws engine's [`LayoutBox`](engine::LayoutBox) output to UIKit
//! via C FFI. Rust owns and controls UIView, UILabel, UITextView, UIScrollView,
//! and CALayer objects through opaque pointer handles.

// Handle wrappers and FFI types are part of the public API surface but not all
// are consumed internally yet. Suppress dead_code warnings for the crate.
#![allow(dead_code)]
// In test mode, the Swift FFI imports are replaced with plain Rust stub
// functions, making `unsafe` blocks around them unnecessary. Allow this
// crate-wide so callers don't need per-site annotations.
#![cfg_attr(test, allow(unused_unsafe))]

mod error;
pub(crate) mod ffi;
pub(crate) mod handles;
mod renderer;
