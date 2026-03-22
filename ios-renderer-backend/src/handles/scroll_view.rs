//! Safe wrapper around an opaque `UIScrollView` pointer.

use std::ffi::c_void;

use super::{MainThreadOnly, RawHandle};
use crate::error::RendererError;
use crate::ffi::imports;

/// Typed handle to a `UIScrollView` on the Swift side.
///
/// `UIScrollView` is a `UIView` subclass. Use [`as_view_ptr`](Self::as_view_ptr)
/// to pass it to view-level FFI functions.
pub(crate) struct UIScrollViewHandle {
    raw: RawHandle,
    _not_send: MainThreadOnly,
}

impl UIScrollViewHandle {
    /// Creates a new `UIScrollView` via the Swift callback.
    pub(crate) fn new() -> Result<Self, RendererError> {
        // SAFETY: Calls the Swift-implemented create function.
        let ptr = unsafe { imports::swift_paws_scroll_view_create() };
        if ptr.is_null() {
            return Err(RendererError::CallbackFailed);
        }
        Ok(Self {
            // SAFETY: Non-null retained pointer from Swift.
            raw: unsafe { RawHandle::from_raw(ptr) },
            _not_send: MainThreadOnly::new(),
        })
    }

    /// Sets the `contentSize` of this scroll view.
    pub(crate) fn set_content_size(&self, w: f32, h: f32) {
        // SAFETY: self.raw holds a valid retained UIScrollView pointer.
        unsafe { imports::swift_paws_scroll_view_set_content_size(self.raw.as_ptr(), w, h) };
    }

    /// Sets the `contentOffset` of this scroll view.
    pub(crate) fn set_content_offset(&self, x: f32, y: f32, animated: bool) {
        // SAFETY: self.raw holds a valid retained UIScrollView pointer.
        unsafe {
            imports::swift_paws_scroll_view_set_content_offset(self.raw.as_ptr(), x, y, animated)
        };
    }

    /// Sets the `isScrollEnabled` property.
    pub(crate) fn set_scroll_enabled(&self, enabled: bool) {
        // SAFETY: self.raw holds a valid retained UIScrollView pointer.
        unsafe { imports::swift_paws_scroll_view_set_scroll_enabled(self.raw.as_ptr(), enabled) };
    }

    /// Sets the `bounces` property.
    pub(crate) fn set_bounces(&self, bounces: bool) {
        // SAFETY: self.raw holds a valid retained UIScrollView pointer.
        unsafe { imports::swift_paws_scroll_view_set_bounces(self.raw.as_ptr(), bounces) };
    }

    /// Returns the raw pointer, usable as a `UIView*` (UIScrollView is a UIView subclass).
    pub(crate) fn as_view_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
}

impl Drop for UIScrollViewHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: Releases the ARC reference on the Swift side.
            unsafe { imports::swift_paws_scroll_view_release(self.raw.as_ptr()) };
        }
    }
}
