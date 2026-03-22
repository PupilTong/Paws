//! Safe wrapper around an opaque `UIView` pointer.

use std::ffi::c_void;

use super::{MainThreadOnly, RawHandle};
use crate::error::RendererError;
use crate::ffi::imports;

/// Typed handle to a `UIView` on the Swift side.
///
/// Dropping this handle releases the Swift-side ARC reference.
pub(crate) struct UIViewHandle {
    pub(crate) raw: RawHandle,
    _not_send: MainThreadOnly,
}

impl UIViewHandle {
    /// Creates a new `UIView` via the Swift callback.
    pub(crate) fn new() -> Result<Self, RendererError> {
        // SAFETY: Calls the Swift-implemented create function which returns
        // a retained UIView pointer via Unmanaged.passRetained().toOpaque().
        let ptr = unsafe { imports::swift_paws_view_create() };
        if ptr.is_null() {
            return Err(RendererError::CallbackFailed);
        }
        Ok(Self {
            // SAFETY: Pointer was just validated as non-null and is a retained
            // UIView from the Swift side.
            raw: unsafe { RawHandle::from_raw(ptr) },
            _not_send: MainThreadOnly::new(),
        })
    }

    /// Wraps an existing retained `UIView` pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid, retained `UIView` pointer.
    pub(crate) unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self {
            raw: RawHandle::from_raw(ptr),
            _not_send: MainThreadOnly::new(),
        }
    }

    /// Sets the frame (origin + size) of this view.
    pub(crate) fn set_frame(&self, x: f32, y: f32, w: f32, h: f32) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_set_frame(self.raw.as_ptr(), x, y, w, h) };
    }

    /// Sets the background color (RGBA, 0.0–1.0).
    pub(crate) fn set_background_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_set_background_color(self.raw.as_ptr(), r, g, b, a) };
    }

    /// Sets the alpha (opacity) of this view.
    pub(crate) fn set_alpha(&self, alpha: f32) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_set_alpha(self.raw.as_ptr(), alpha) };
    }

    /// Sets the `isHidden` property.
    pub(crate) fn set_hidden(&self, hidden: bool) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_set_hidden(self.raw.as_ptr(), hidden) };
    }

    /// Sets the `clipsToBounds` property.
    pub(crate) fn set_clips_to_bounds(&self, clips: bool) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_set_clips_to_bounds(self.raw.as_ptr(), clips) };
    }

    /// Adds `child` as a subview of this view.
    pub(crate) fn add_subview(&self, child: &UIViewHandle) {
        // SAFETY: Both pointers are valid retained UIView handles.
        unsafe { imports::swift_paws_view_add_subview(self.raw.as_ptr(), child.raw.as_ptr()) };
    }

    /// Removes this view from its superview.
    pub(crate) fn remove_from_superview(&self) {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_remove_from_superview(self.raw.as_ptr()) };
    }

    /// Returns the `CALayer` associated with this view.
    ///
    /// The returned layer shares the lifetime of this view — it is NOT
    /// additionally retained.
    pub(crate) fn layer(&self) -> *mut c_void {
        // SAFETY: self.raw holds a valid retained UIView pointer.
        unsafe { imports::swift_paws_view_get_layer(self.raw.as_ptr()) }
    }

    /// Returns the raw opaque pointer.
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
}

impl Drop for UIViewHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: Releases the ARC reference on the Swift side.
            // This matches the retain from swift_paws_view_create().
            unsafe { imports::swift_paws_view_release(self.raw.as_ptr()) };
        }
    }
}
