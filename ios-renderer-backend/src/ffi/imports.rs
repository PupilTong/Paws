//! FFI imports: functions implemented by Swift (via `@_cdecl`) that Rust calls
//! to create and control UIKit objects.
//!
//! Naming convention: `swift_paws_<object>_<action>`.
//!
//! All pointer parameters are opaque `*mut c_void` handles obtained from the
//! corresponding `swift_paws_<object>_create` function. The Swift side retains
//! objects via `Unmanaged.passRetained` and releases via the `_release` functions.
//!
//! When compiled under `#[cfg(test)]`, the real `extern "C"` block is replaced
//! with Rust stub functions that record every call to a thread-local log,
//! enabling precise UIKit assertions on Linux (where Swift/UIKit is unavailable).

#[cfg(not(test))]
use std::ffi::{c_char, c_void};

#[cfg(not(test))]
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

    // ── CALayer (standalone lifecycle) ─────────────────────────────────

    /// Creates a new standalone `CALayer` and returns a retained opaque pointer.
    pub(crate) fn swift_paws_layer_create() -> *mut c_void;

    /// Releases a retained standalone `CALayer` pointer.
    pub(crate) fn swift_paws_layer_release(layer: *mut c_void);

    /// Sets the frame (origin + size) of a standalone `CALayer`.
    pub(crate) fn swift_paws_layer_set_frame(layer: *mut c_void, x: f32, y: f32, w: f32, h: f32);

    /// Sets the background color of a standalone `CALayer` (RGBA, 0.0–1.0).
    pub(crate) fn swift_paws_layer_set_background_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );

    /// Adds `child` as a sublayer of `parent` (both `CALayer`).
    pub(crate) fn swift_paws_layer_add_sublayer(parent: *mut c_void, child: *mut c_void);

    /// Removes a `CALayer` from its superlayer.
    pub(crate) fn swift_paws_layer_remove_from_superlayer(layer: *mut c_void);

    /// Adds a standalone `CALayer` as a sublayer of a `UIView`'s layer.
    pub(crate) fn swift_paws_view_add_sublayer(view: *mut c_void, layer: *mut c_void);

    // ── CALayer (property setters) ──────────────────────────────────────

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

// ─── Test stubs ────────────────────────────────────────────────────────────
//
// When compiling for tests, we replace the `extern "C"` declarations with
// plain Rust functions that record every call to a thread-local log. This
// lets us run tests on Linux without Swift/UIKit and assert the exact
// sequence of UIKit operations the renderer performs.

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
pub(crate) mod stubs {
    use std::cell::RefCell;
    use std::ffi::{c_char, c_void};

    /// A recorded FFI call with its arguments.
    ///
    /// Used by tests to assert the exact UIKit operations the renderer performs.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum FfiCall {
        // ── UIView ──────────────────────────────────────────────────────
        ViewCreate {
            ret: *mut c_void,
        },
        ViewRelease {
            ptr: *mut c_void,
        },
        ViewSetFrame {
            ptr: *mut c_void,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
        },
        ViewSetBackgroundColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        ViewSetAlpha {
            ptr: *mut c_void,
            alpha: f32,
        },
        ViewSetHidden {
            ptr: *mut c_void,
            hidden: bool,
        },
        ViewSetClipsToBounds {
            ptr: *mut c_void,
            clips: bool,
        },
        ViewAddSubview {
            parent: *mut c_void,
            child: *mut c_void,
        },
        ViewRemoveFromSuperview {
            ptr: *mut c_void,
        },
        ViewGetLayer {
            ptr: *mut c_void,
            ret: *mut c_void,
        },

        // ── UILabel ─────────────────────────────────────────────────────
        LabelCreate {
            ret: *mut c_void,
        },
        LabelRelease {
            ptr: *mut c_void,
        },
        LabelSetText {
            ptr: *mut c_void,
        },
        LabelSetFontSize {
            ptr: *mut c_void,
            size: f32,
        },
        LabelSetTextColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        LabelSetNumberOfLines {
            ptr: *mut c_void,
            lines: i32,
        },
        LabelSetTextAlignment {
            ptr: *mut c_void,
            alignment: i32,
        },

        // ── UITextView ──────────────────────────────────────────────────
        TextViewCreate {
            ret: *mut c_void,
        },
        TextViewRelease {
            ptr: *mut c_void,
        },
        TextViewSetText {
            ptr: *mut c_void,
        },
        TextViewSetFontSize {
            ptr: *mut c_void,
            size: f32,
        },
        TextViewSetTextColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        TextViewSetEditable {
            ptr: *mut c_void,
            editable: bool,
        },
        TextViewSetScrollable {
            ptr: *mut c_void,
            scrollable: bool,
        },
        TextViewSetTextAlignment {
            ptr: *mut c_void,
            alignment: i32,
        },

        // ── UIScrollView ────────────────────────────────────────────────
        ScrollViewCreate {
            ret: *mut c_void,
        },
        ScrollViewRelease {
            ptr: *mut c_void,
        },
        ScrollViewSetContentSize {
            ptr: *mut c_void,
            w: f32,
            h: f32,
        },
        ScrollViewSetContentOffset {
            ptr: *mut c_void,
            x: f32,
            y: f32,
            animated: bool,
        },
        ScrollViewSetScrollEnabled {
            ptr: *mut c_void,
            enabled: bool,
        },
        ScrollViewSetBounces {
            ptr: *mut c_void,
            bounces: bool,
        },

        // ── CALayer (standalone lifecycle) ──────────────────────────────
        LayerCreate {
            ret: *mut c_void,
        },
        LayerRelease {
            ptr: *mut c_void,
        },
        LayerSetFrame {
            ptr: *mut c_void,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
        },
        LayerSetBackgroundColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        LayerAddSublayer {
            parent: *mut c_void,
            child: *mut c_void,
        },
        LayerRemoveFromSuperlayer {
            ptr: *mut c_void,
        },
        ViewAddSublayer {
            view: *mut c_void,
            layer: *mut c_void,
        },

        // ── CALayer (property setters) ─────────────────────────────────
        LayerSetCornerRadius {
            ptr: *mut c_void,
            radius: f32,
        },
        LayerSetBorderWidth {
            ptr: *mut c_void,
            width: f32,
        },
        LayerSetBorderColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        LayerSetShadowColor {
            ptr: *mut c_void,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        },
        LayerSetShadowOffset {
            ptr: *mut c_void,
            dx: f32,
            dy: f32,
        },
        LayerSetShadowRadius {
            ptr: *mut c_void,
            radius: f32,
        },
        LayerSetShadowOpacity {
            ptr: *mut c_void,
            opacity: f32,
        },
        LayerSetTransform {
            ptr: *mut c_void,
        },
    }

    // SAFETY: FfiCall contains *mut c_void which is !Send by default.
    // In tests, the pointers are Box::into_raw sentinels that are never
    // dereferenced — they only serve as unique identifiers. We need Send
    // so that the test harness can move FfiCall values across threads.
    unsafe impl Send for FfiCall {}

    thread_local! {
        static CALL_LOG: RefCell<Vec<FfiCall>> = const { RefCell::new(Vec::new()) };
    }

    fn log(call: FfiCall) {
        CALL_LOG.with(|log| log.borrow_mut().push(call));
    }

    /// Returns and clears the call log.
    pub(crate) fn take_call_log() -> Vec<FfiCall> {
        CALL_LOG.with(|log| log.borrow_mut().drain(..).collect())
    }

    /// Clears the call log without returning entries.
    pub(crate) fn clear_call_log() {
        CALL_LOG.with(|log| log.borrow_mut().clear());
    }

    /// Allocates a unique non-null pointer for test stubs.
    ///
    /// These pointers are never dereferenced — they serve only as unique
    /// identifiers for tracking view identity across calls.
    fn alloc_ptr() -> *mut c_void {
        Box::into_raw(Box::new(())) as *mut c_void
    }

    // ── UIView stubs ────────────────────────────────────────────────────

    pub(crate) fn swift_paws_view_create() -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::ViewCreate { ret });
        ret
    }

    pub(crate) fn swift_paws_view_release(view: *mut c_void) {
        log(FfiCall::ViewRelease { ptr: view });
    }

    pub(crate) fn swift_paws_view_set_frame(view: *mut c_void, x: f32, y: f32, w: f32, h: f32) {
        log(FfiCall::ViewSetFrame {
            ptr: view,
            x,
            y,
            w,
            h,
        });
    }

    pub(crate) fn swift_paws_view_set_background_color(
        view: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::ViewSetBackgroundColor {
            ptr: view,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_view_set_alpha(view: *mut c_void, alpha: f32) {
        log(FfiCall::ViewSetAlpha { ptr: view, alpha });
    }

    pub(crate) fn swift_paws_view_set_hidden(view: *mut c_void, hidden: bool) {
        log(FfiCall::ViewSetHidden { ptr: view, hidden });
    }

    pub(crate) fn swift_paws_view_set_clips_to_bounds(view: *mut c_void, clips: bool) {
        log(FfiCall::ViewSetClipsToBounds { ptr: view, clips });
    }

    pub(crate) fn swift_paws_view_add_subview(parent: *mut c_void, child: *mut c_void) {
        log(FfiCall::ViewAddSubview { parent, child });
    }

    pub(crate) fn swift_paws_view_remove_from_superview(view: *mut c_void) {
        log(FfiCall::ViewRemoveFromSuperview { ptr: view });
    }

    pub(crate) fn swift_paws_view_get_layer(view: *mut c_void) -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::ViewGetLayer { ptr: view, ret });
        ret
    }

    // ── UILabel stubs ───────────────────────────────────────────────────

    pub(crate) fn swift_paws_label_create() -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::LabelCreate { ret });
        ret
    }

    pub(crate) fn swift_paws_label_release(label: *mut c_void) {
        log(FfiCall::LabelRelease { ptr: label });
    }

    pub(crate) fn swift_paws_label_set_text(label: *mut c_void, _text: *const c_char) {
        log(FfiCall::LabelSetText { ptr: label });
    }

    pub(crate) fn swift_paws_label_set_font_size(label: *mut c_void, size: f32) {
        log(FfiCall::LabelSetFontSize { ptr: label, size });
    }

    pub(crate) fn swift_paws_label_set_text_color(
        label: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::LabelSetTextColor {
            ptr: label,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_label_set_number_of_lines(label: *mut c_void, lines: i32) {
        log(FfiCall::LabelSetNumberOfLines { ptr: label, lines });
    }

    pub(crate) fn swift_paws_label_set_text_alignment(label: *mut c_void, alignment: i32) {
        log(FfiCall::LabelSetTextAlignment {
            ptr: label,
            alignment,
        });
    }

    // ── UITextView stubs ────────────────────────────────────────────────

    pub(crate) fn swift_paws_text_view_create() -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::TextViewCreate { ret });
        ret
    }

    pub(crate) fn swift_paws_text_view_release(text_view: *mut c_void) {
        log(FfiCall::TextViewRelease { ptr: text_view });
    }

    pub(crate) fn swift_paws_text_view_set_text(text_view: *mut c_void, _text: *const c_char) {
        log(FfiCall::TextViewSetText { ptr: text_view });
    }

    pub(crate) fn swift_paws_text_view_set_font_size(text_view: *mut c_void, size: f32) {
        log(FfiCall::TextViewSetFontSize {
            ptr: text_view,
            size,
        });
    }

    pub(crate) fn swift_paws_text_view_set_text_color(
        text_view: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::TextViewSetTextColor {
            ptr: text_view,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_text_view_set_editable(text_view: *mut c_void, editable: bool) {
        log(FfiCall::TextViewSetEditable {
            ptr: text_view,
            editable,
        });
    }

    pub(crate) fn swift_paws_text_view_set_scrollable(text_view: *mut c_void, scrollable: bool) {
        log(FfiCall::TextViewSetScrollable {
            ptr: text_view,
            scrollable,
        });
    }

    pub(crate) fn swift_paws_text_view_set_text_alignment(text_view: *mut c_void, alignment: i32) {
        log(FfiCall::TextViewSetTextAlignment {
            ptr: text_view,
            alignment,
        });
    }

    // ── UIScrollView stubs ──────────────────────────────────────────────

    pub(crate) fn swift_paws_scroll_view_create() -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::ScrollViewCreate { ret });
        ret
    }

    pub(crate) fn swift_paws_scroll_view_release(scroll_view: *mut c_void) {
        log(FfiCall::ScrollViewRelease { ptr: scroll_view });
    }

    pub(crate) fn swift_paws_scroll_view_set_content_size(
        scroll_view: *mut c_void,
        w: f32,
        h: f32,
    ) {
        log(FfiCall::ScrollViewSetContentSize {
            ptr: scroll_view,
            w,
            h,
        });
    }

    pub(crate) fn swift_paws_scroll_view_set_content_offset(
        scroll_view: *mut c_void,
        x: f32,
        y: f32,
        animated: bool,
    ) {
        log(FfiCall::ScrollViewSetContentOffset {
            ptr: scroll_view,
            x,
            y,
            animated,
        });
    }

    pub(crate) fn swift_paws_scroll_view_set_scroll_enabled(
        scroll_view: *mut c_void,
        enabled: bool,
    ) {
        log(FfiCall::ScrollViewSetScrollEnabled {
            ptr: scroll_view,
            enabled,
        });
    }

    pub(crate) fn swift_paws_scroll_view_set_bounces(scroll_view: *mut c_void, bounces: bool) {
        log(FfiCall::ScrollViewSetBounces {
            ptr: scroll_view,
            bounces,
        });
    }

    // ── CALayer standalone stubs ──────────────────────────────────────

    pub(crate) fn swift_paws_layer_create() -> *mut c_void {
        let ret = alloc_ptr();
        log(FfiCall::LayerCreate { ret });
        ret
    }

    pub(crate) fn swift_paws_layer_release(layer: *mut c_void) {
        log(FfiCall::LayerRelease { ptr: layer });
    }

    pub(crate) fn swift_paws_layer_set_frame(layer: *mut c_void, x: f32, y: f32, w: f32, h: f32) {
        log(FfiCall::LayerSetFrame {
            ptr: layer,
            x,
            y,
            w,
            h,
        });
    }

    pub(crate) fn swift_paws_layer_set_background_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::LayerSetBackgroundColor {
            ptr: layer,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_layer_add_sublayer(parent: *mut c_void, child: *mut c_void) {
        log(FfiCall::LayerAddSublayer { parent, child });
    }

    pub(crate) fn swift_paws_layer_remove_from_superlayer(layer: *mut c_void) {
        log(FfiCall::LayerRemoveFromSuperlayer { ptr: layer });
    }

    pub(crate) fn swift_paws_view_add_sublayer(view: *mut c_void, layer: *mut c_void) {
        log(FfiCall::ViewAddSublayer { view, layer });
    }

    // ── CALayer property stubs ──────────────────────────────────────────

    pub(crate) fn swift_paws_layer_set_corner_radius(layer: *mut c_void, radius: f32) {
        log(FfiCall::LayerSetCornerRadius { ptr: layer, radius });
    }

    pub(crate) fn swift_paws_layer_set_border_width(layer: *mut c_void, width: f32) {
        log(FfiCall::LayerSetBorderWidth { ptr: layer, width });
    }

    pub(crate) fn swift_paws_layer_set_border_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::LayerSetBorderColor {
            ptr: layer,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_layer_set_shadow_color(
        layer: *mut c_void,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        log(FfiCall::LayerSetShadowColor {
            ptr: layer,
            r,
            g,
            b,
            a,
        });
    }

    pub(crate) fn swift_paws_layer_set_shadow_offset(layer: *mut c_void, dx: f32, dy: f32) {
        log(FfiCall::LayerSetShadowOffset { ptr: layer, dx, dy });
    }

    pub(crate) fn swift_paws_layer_set_shadow_radius(layer: *mut c_void, radius: f32) {
        log(FfiCall::LayerSetShadowRadius { ptr: layer, radius });
    }

    pub(crate) fn swift_paws_layer_set_shadow_opacity(layer: *mut c_void, opacity: f32) {
        log(FfiCall::LayerSetShadowOpacity {
            ptr: layer,
            opacity,
        });
    }

    pub(crate) fn swift_paws_layer_set_transform(layer: *mut c_void, _matrix: *const f32) {
        log(FfiCall::LayerSetTransform { ptr: layer });
    }
}

#[cfg(test)]
pub(crate) use stubs::*;
