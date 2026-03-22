//! FFI imports: functions implemented by Swift (via `@_cdecl`) that Rust calls
//! to create and control UIKit objects.
//!
//! Naming convention: `swift_paws_<object>_<action>`.
//!
//! All pointer parameters are opaque `*mut c_void` handles obtained from the
//! corresponding `swift_paws_<object>_create` function. The Swift side retains
//! objects via `Unmanaged.passRetained` and releases via the `_release` functions.

use std::ffi::{c_char, c_void};

extern "C" {
    // ── UIView ──────────────────────────────────────────────────────────

    /// Creates a new `UIView` and returns a retained opaque pointer.
    pub(crate) fn swift_paws_view_create() -> *mut c_void;

    /// Releases a retained `UIView` pointer.
    pub(crate) fn swift_paws_view_release(view: *mut c_void);

    /// Sets the frame (origin + size) of a `UIView`.
    pub(crate) fn swift_paws_view_set_frame(view: *mut c_void, x: f32, y: f32, w: f32, h: f32);

    /// Sets the background color of a `UIView` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_view_set_background_color(
        view: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Sets the alpha (opacity) of a `UIView`.
    pub(crate) fn swift_paws_view_set_alpha(view: *mut c_void, alpha: f32);

    /// Sets the `isHidden` property of a `UIView`.
    pub(crate) fn swift_paws_view_set_hidden(view: *mut c_void, hidden: bool);

    /// Sets the `clipsToBounds` property of a `UIView`.
    pub(crate) fn swift_paws_view_set_clips_to_bounds(view: *mut c_void, clips: bool);

    /// Adds `child` as a subview of `parent`.
    pub(crate) fn swift_paws_view_add_subview(parent: *mut c_void, child: *mut c_void);

    /// Removes a `UIView` from its superview.
    pub(crate) fn swift_paws_view_remove_from_superview(view: *mut c_void);

    /// Returns the `CALayer` associated with a `UIView`.
    /// The returned pointer is NOT additionally retained — it shares
    /// the lifetime of the view.
    pub(crate) fn swift_paws_view_get_layer(view: *mut c_void) -> *mut c_void;

    // ── UILabel ─────────────────────────────────────────────────────────

    /// Creates a new `UILabel` and returns a retained opaque pointer.
    pub(crate) fn swift_paws_label_create() -> *mut c_void;

    /// Releases a retained `UILabel` pointer.
    pub(crate) fn swift_paws_label_release(label: *mut c_void);

    /// Sets the text of a `UILabel`. `text` must be a null-terminated UTF-8 string.
    pub(crate) fn swift_paws_label_set_text(label: *mut c_void, text: *const c_char);

    /// Sets the font size of a `UILabel`.
    pub(crate) fn swift_paws_label_set_font_size(label: *mut c_void, size: f32);

    /// Sets the text color of a `UILabel` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_label_set_text_color(
        label: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Sets the `numberOfLines` property. Pass `0` for unlimited lines.
    pub(crate) fn swift_paws_label_set_number_of_lines(label: *mut c_void, lines: i32);

    /// Sets text alignment. Values follow `NSTextAlignment` raw values:
    /// 0 = left, 1 = center, 2 = right, 3 = justified, 4 = natural.
    pub(crate) fn swift_paws_label_set_text_alignment(label: *mut c_void, alignment: i32);

    // ── UITextView ──────────────────────────────────────────────────────

    /// Creates a new `UITextView` and returns a retained opaque pointer.
    pub(crate) fn swift_paws_text_view_create() -> *mut c_void;

    /// Releases a retained `UITextView` pointer.
    pub(crate) fn swift_paws_text_view_release(text_view: *mut c_void);

    /// Sets the text content of a `UITextView`. `text` must be a null-terminated UTF-8 string.
    pub(crate) fn swift_paws_text_view_set_text(text_view: *mut c_void, text: *const c_char);

    /// Sets the font size of a `UITextView`.
    pub(crate) fn swift_paws_text_view_set_font_size(text_view: *mut c_void, size: f32);

    /// Sets the text color of a `UITextView` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_text_view_set_text_color(
        text_view: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Sets the `isEditable` property of a `UITextView`.
    pub(crate) fn swift_paws_text_view_set_editable(text_view: *mut c_void, editable: bool);

    /// Sets the `isScrollEnabled` property of a `UITextView`.
    pub(crate) fn swift_paws_text_view_set_scrollable(text_view: *mut c_void, scrollable: bool);

    /// Sets text alignment. Values follow `NSTextAlignment` raw values.
    pub(crate) fn swift_paws_text_view_set_text_alignment(text_view: *mut c_void, alignment: i32);

    // ── UIScrollView ────────────────────────────────────────────────────

    /// Creates a new `UIScrollView` and returns a retained opaque pointer.
    pub(crate) fn swift_paws_scroll_view_create() -> *mut c_void;

    /// Releases a retained `UIScrollView` pointer.
    pub(crate) fn swift_paws_scroll_view_release(scroll_view: *mut c_void);

    /// Sets the `contentSize` of a `UIScrollView`.
    pub(crate) fn swift_paws_scroll_view_set_content_size(scroll_view: *mut c_void, w: f32, h: f32);

    /// Sets the `contentOffset` of a `UIScrollView`.
    pub(crate) fn swift_paws_scroll_view_set_content_offset(
        scroll_view: *mut c_void,
        x: f32,
        y: f32,
        animated: bool,
    );

    /// Sets the `isScrollEnabled` property of a `UIScrollView`.
    pub(crate) fn swift_paws_scroll_view_set_scroll_enabled(
        scroll_view: *mut c_void,
        enabled: bool,
    );

    /// Sets the `bounces` property of a `UIScrollView`.
    pub(crate) fn swift_paws_scroll_view_set_bounces(scroll_view: *mut c_void, bounces: bool);

    // ── CALayer ─────────────────────────────────────────────────────────

    /// Sets the `cornerRadius` of a `CALayer`.
    pub(crate) fn swift_paws_layer_set_corner_radius(layer: *mut c_void, radius: f32);

    /// Sets the `borderWidth` of a `CALayer`.
    pub(crate) fn swift_paws_layer_set_border_width(layer: *mut c_void, width: f32);

    /// Sets the `borderColor` of a `CALayer` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_layer_set_border_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Sets the `shadowColor` of a `CALayer` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_layer_set_shadow_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Sets the `shadowOffset` of a `CALayer`.
    pub(crate) fn swift_paws_layer_set_shadow_offset(layer: *mut c_void, dx: f32, dy: f32);

    /// Sets the `shadowRadius` of a `CALayer`.
    pub(crate) fn swift_paws_layer_set_shadow_radius(layer: *mut c_void, radius: f32);

    /// Sets the `shadowOpacity` of a `CALayer`.
    pub(crate) fn swift_paws_layer_set_shadow_opacity(layer: *mut c_void, opacity: f32);

    /// Sets the `transform` of a `CALayer` as a column-major 4x4 matrix (16 floats).
    pub(crate) fn swift_paws_layer_set_transform(layer: *mut c_void, matrix: *const f32);
}
