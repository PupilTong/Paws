//! Safe wrapper around an opaque `UITextView` pointer.

use std::ffi::{c_char, c_void};

use super::{MainThreadOnly, RawHandle};
use crate::error::RendererError;
use crate::ffi::imports;

/// Typed handle to a `UITextView` on the Swift side.
///
/// `UITextView` is a `UIScrollView` subclass (which is a `UIView` subclass).
/// Use [`as_view_ptr`](Self::as_view_ptr) to pass it to view-level FFI functions.
pub(crate) struct UITextViewHandle {
    raw: RawHandle,
    _not_send: MainThreadOnly,
}

impl UITextViewHandle {
    /// Creates a new `UITextView` via the Swift callback.
    pub(crate) fn new() -> Result<Self, RendererError> {
        // SAFETY: Calls the Swift-implemented create function.
        let ptr = unsafe { imports::swift_paws_text_view_create() };
        if ptr.is_null() {
            return Err(RendererError::CallbackFailed);
        }
        Ok(Self {
            // SAFETY: Non-null retained pointer from Swift.
            raw: unsafe { RawHandle::from_raw(ptr) },
            _not_send: MainThreadOnly::new(),
        })
    }

    /// Sets the text content. `text` must be a null-terminated UTF-8 C string.
    pub(crate) fn set_text(&self, text: *const c_char) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_text(self.raw.as_ptr(), text) };
    }

    /// Sets the font size in points.
    pub(crate) fn set_font_size(&self, size: f32) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_font_size(self.raw.as_ptr(), size) };
    }

    /// Sets the text color (RGBA, 0.0–1.0).
    pub(crate) fn set_text_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_text_color(self.raw.as_ptr(), r, g, b, a) };
    }

    /// Sets the `isEditable` property.
    pub(crate) fn set_editable(&self, editable: bool) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_editable(self.raw.as_ptr(), editable) };
    }

    /// Sets the `isScrollEnabled` property.
    pub(crate) fn set_scrollable(&self, scrollable: bool) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_scrollable(self.raw.as_ptr(), scrollable) };
    }

    /// Sets text alignment using `NSTextAlignment` raw values:
    /// 0 = left, 1 = center, 2 = right, 3 = justified, 4 = natural.
    pub(crate) fn set_text_alignment(&self, alignment: i32) {
        // SAFETY: self.raw holds a valid retained UITextView pointer.
        unsafe { imports::swift_paws_text_view_set_text_alignment(self.raw.as_ptr(), alignment) };
    }

    /// Returns the raw pointer, usable as a `UIView*` (UITextView is a UIView subclass).
    pub(crate) fn as_view_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
}

impl Drop for UITextViewHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: Releases the ARC reference on the Swift side.
            unsafe { imports::swift_paws_text_view_release(self.raw.as_ptr()) };
        }
    }
}
