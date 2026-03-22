//! Typed wrappers around opaque UIKit object pointers.
//!
//! Each handle holds a `*mut c_void` pointing to a retained UIKit object
//! on the Swift side. The `PhantomData<*mut ()>` marker makes all handles
//! `!Send` and `!Sync`, enforcing UIKit's main-thread requirement at
//! compile time.

pub(crate) mod label;
pub(crate) mod layer;
pub(crate) mod scroll_view;
pub(crate) mod text_view;
pub(crate) mod view;

use std::ffi::c_void;
use std::marker::PhantomData;

/// Marker ensuring a type is `!Send` and `!Sync`.
///
/// UIKit objects must only be accessed from the main thread.
pub(crate) struct MainThreadOnly(PhantomData<*mut ()>);

impl MainThreadOnly {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

/// An opaque handle to a UIKit object held by the Swift side.
///
/// The Swift side is responsible for retaining (ARC strong reference)
/// the object for the lifetime of this handle. The Rust side MUST call
/// the corresponding `swift_paws_*_release` callback when dropping.
#[repr(transparent)]
pub(crate) struct RawHandle {
    ptr: *mut c_void,
}

impl RawHandle {
    /// Creates a new handle from a raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be a valid, retained Objective-C/Swift object pointer
    /// obtained via `Unmanaged.passRetained(...).toOpaque()`.
    pub(crate) unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self { ptr }
    }

    /// Returns the raw pointer for passing back to Swift FFI functions.
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }

    /// Returns `true` if the underlying pointer is null.
    pub(crate) fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}
