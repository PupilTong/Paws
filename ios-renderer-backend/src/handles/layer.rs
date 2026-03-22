//! Safe wrapper around an opaque `CALayer` pointer.

use std::ffi::c_void;

use super::MainThreadOnly;
use crate::ffi::imports;

/// Typed handle to a `CALayer` on the Swift side.
///
/// Unlike other handles, `CALayer` pointers are NOT independently retained —
/// they share the lifetime of their owning `UIView`. Callers must ensure the
/// parent `UIViewHandle` outlives any `CALayerHandle` obtained from it.
pub(crate) struct CALayerHandle {
    ptr: *mut c_void,
    _not_send: MainThreadOnly,
}

impl CALayerHandle {
    /// Wraps a layer pointer obtained from [`UIViewHandle::layer`].
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid `CALayer` pointer whose owning `UIView` is still alive.
    pub(crate) unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self {
            ptr,
            _not_send: MainThreadOnly::new(),
        }
    }

    /// Sets the `cornerRadius`.
    pub(crate) fn set_corner_radius(&self, radius: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_corner_radius(self.ptr, radius) };
    }

    /// Sets the `borderWidth`.
    pub(crate) fn set_border_width(&self, width: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_border_width(self.ptr, width) };
    }

    /// Sets the `borderColor` (RGBA, 0.0–1.0).
    pub(crate) fn set_border_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_border_color(self.ptr, r, g, b, a) };
    }

    /// Sets the `shadowColor` (RGBA, 0.0–1.0).
    pub(crate) fn set_shadow_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_shadow_color(self.ptr, r, g, b, a) };
    }

    /// Sets the `shadowOffset`.
    pub(crate) fn set_shadow_offset(&self, dx: f32, dy: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_shadow_offset(self.ptr, dx, dy) };
    }

    /// Sets the `shadowRadius`.
    pub(crate) fn set_shadow_radius(&self, radius: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_shadow_radius(self.ptr, radius) };
    }

    /// Sets the `shadowOpacity`.
    pub(crate) fn set_shadow_opacity(&self, opacity: f32) {
        // SAFETY: ptr is a valid CALayer while the owning UIView is alive.
        unsafe { imports::swift_paws_layer_set_shadow_opacity(self.ptr, opacity) };
    }

    /// Sets the `transform` as a column-major 4x4 matrix (`CATransform3D`).
    pub(crate) fn set_transform(&self, matrix: &[f32; 16]) {
        // SAFETY: ptr is a valid CALayer, matrix points to 16 contiguous f32s.
        unsafe { imports::swift_paws_layer_set_transform(self.ptr, matrix.as_ptr()) };
    }
}

// No Drop impl — the layer is not independently retained.
// Its lifetime is tied to the owning UIView.
