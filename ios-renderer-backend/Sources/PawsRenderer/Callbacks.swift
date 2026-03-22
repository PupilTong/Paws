/// Swift implementations of the `swift_paws_*` C functions that Rust calls
/// to create and control UIKit objects.
///
/// Each function is exported via `@_cdecl` to produce a stable C symbol.
/// Objects are bridged using `Unmanaged` for manual ARC retain/release.

#if canImport(UIKit)
import UIKit

// MARK: - UIView

@_cdecl("swift_paws_view_create")
func swiftPawsViewCreate() -> UnsafeMutableRawPointer {
    let view = UIView()
    return Unmanaged.passRetained(view).toOpaque()
}

@_cdecl("swift_paws_view_release")
func swiftPawsViewRelease(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<UIView>.fromOpaque(ptr).release()
}

@_cdecl("swift_paws_view_set_frame")
func swiftPawsViewSetFrame(
    _ ptr: UnsafeMutableRawPointer,
    _ x: Float, _ y: Float, _ w: Float, _ h: Float
) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.frame = CGRect(x: CGFloat(x), y: CGFloat(y), width: CGFloat(w), height: CGFloat(h))
}

@_cdecl("swift_paws_view_set_background_color")
func swiftPawsViewSetBackgroundColor(
    _ ptr: UnsafeMutableRawPointer,
    _ r: Float, _ g: Float, _ b: Float, _ a: Float
) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.backgroundColor = UIColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: CGFloat(a))
}

@_cdecl("swift_paws_view_set_alpha")
func swiftPawsViewSetAlpha(_ ptr: UnsafeMutableRawPointer, _ alpha: Float) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.alpha = CGFloat(alpha)
}

@_cdecl("swift_paws_view_set_hidden")
func swiftPawsViewSetHidden(_ ptr: UnsafeMutableRawPointer, _ hidden: Bool) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.isHidden = hidden
}

@_cdecl("swift_paws_view_set_clips_to_bounds")
func swiftPawsViewSetClipsToBounds(_ ptr: UnsafeMutableRawPointer, _ clips: Bool) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.clipsToBounds = clips
}

@_cdecl("swift_paws_view_add_subview")
func swiftPawsViewAddSubview(_ parentPtr: UnsafeMutableRawPointer, _ childPtr: UnsafeMutableRawPointer) {
    let parent = Unmanaged<UIView>.fromOpaque(parentPtr).takeUnretainedValue()
    let child = Unmanaged<UIView>.fromOpaque(childPtr).takeUnretainedValue()
    parent.addSubview(child)
}

@_cdecl("swift_paws_view_remove_from_superview")
func swiftPawsViewRemoveFromSuperview(_ ptr: UnsafeMutableRawPointer) {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    view.removeFromSuperview()
}

@_cdecl("swift_paws_view_get_layer")
func swiftPawsViewGetLayer(_ ptr: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    let view = Unmanaged<UIView>.fromOpaque(ptr).takeUnretainedValue()
    return Unmanaged.passUnretained(view.layer).toOpaque()
}

// MARK: - UILabel

@_cdecl("swift_paws_label_create")
func swiftPawsLabelCreate() -> UnsafeMutableRawPointer {
    let label = UILabel()
    return Unmanaged.passRetained(label).toOpaque()
}

@_cdecl("swift_paws_label_release")
func swiftPawsLabelRelease(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<UILabel>.fromOpaque(ptr).release()
}

@_cdecl("swift_paws_label_set_text")
func swiftPawsLabelSetText(_ ptr: UnsafeMutableRawPointer, _ text: UnsafePointer<CChar>) {
    let label = Unmanaged<UILabel>.fromOpaque(ptr).takeUnretainedValue()
    label.text = String(cString: text)
}

@_cdecl("swift_paws_label_set_font_size")
func swiftPawsLabelSetFontSize(_ ptr: UnsafeMutableRawPointer, _ size: Float) {
    let label = Unmanaged<UILabel>.fromOpaque(ptr).takeUnretainedValue()
    label.font = UIFont.systemFont(ofSize: CGFloat(size))
}

@_cdecl("swift_paws_label_set_text_color")
func swiftPawsLabelSetTextColor(
    _ ptr: UnsafeMutableRawPointer,
    _ r: Float, _ g: Float, _ b: Float, _ a: Float
) {
    let label = Unmanaged<UILabel>.fromOpaque(ptr).takeUnretainedValue()
    label.textColor = UIColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: CGFloat(a))
}

@_cdecl("swift_paws_label_set_number_of_lines")
func swiftPawsLabelSetNumberOfLines(_ ptr: UnsafeMutableRawPointer, _ lines: Int32) {
    let label = Unmanaged<UILabel>.fromOpaque(ptr).takeUnretainedValue()
    label.numberOfLines = Int(lines)
}

@_cdecl("swift_paws_label_set_text_alignment")
func swiftPawsLabelSetTextAlignment(_ ptr: UnsafeMutableRawPointer, _ alignment: Int32) {
    let label = Unmanaged<UILabel>.fromOpaque(ptr).takeUnretainedValue()
    label.textAlignment = NSTextAlignment(rawValue: Int(alignment)) ?? .natural
}

// MARK: - UITextView

@_cdecl("swift_paws_text_view_create")
func swiftPawsTextViewCreate() -> UnsafeMutableRawPointer {
    let textView = UITextView()
    return Unmanaged.passRetained(textView).toOpaque()
}

@_cdecl("swift_paws_text_view_release")
func swiftPawsTextViewRelease(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<UITextView>.fromOpaque(ptr).release()
}

@_cdecl("swift_paws_text_view_set_text")
func swiftPawsTextViewSetText(_ ptr: UnsafeMutableRawPointer, _ text: UnsafePointer<CChar>) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.text = String(cString: text)
}

@_cdecl("swift_paws_text_view_set_font_size")
func swiftPawsTextViewSetFontSize(_ ptr: UnsafeMutableRawPointer, _ size: Float) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.font = UIFont.systemFont(ofSize: CGFloat(size))
}

@_cdecl("swift_paws_text_view_set_text_color")
func swiftPawsTextViewSetTextColor(
    _ ptr: UnsafeMutableRawPointer,
    _ r: Float, _ g: Float, _ b: Float, _ a: Float
) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.textColor = UIColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: CGFloat(a))
}

@_cdecl("swift_paws_text_view_set_editable")
func swiftPawsTextViewSetEditable(_ ptr: UnsafeMutableRawPointer, _ editable: Bool) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.isEditable = editable
}

@_cdecl("swift_paws_text_view_set_scrollable")
func swiftPawsTextViewSetScrollable(_ ptr: UnsafeMutableRawPointer, _ scrollable: Bool) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.isScrollEnabled = scrollable
}

@_cdecl("swift_paws_text_view_set_text_alignment")
func swiftPawsTextViewSetTextAlignment(_ ptr: UnsafeMutableRawPointer, _ alignment: Int32) {
    let textView = Unmanaged<UITextView>.fromOpaque(ptr).takeUnretainedValue()
    textView.textAlignment = NSTextAlignment(rawValue: Int(alignment)) ?? .natural
}

// MARK: - UIScrollView

@_cdecl("swift_paws_scroll_view_create")
func swiftPawsScrollViewCreate() -> UnsafeMutableRawPointer {
    let scrollView = UIScrollView()
    return Unmanaged.passRetained(scrollView).toOpaque()
}

@_cdecl("swift_paws_scroll_view_release")
func swiftPawsScrollViewRelease(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<UIScrollView>.fromOpaque(ptr).release()
}

@_cdecl("swift_paws_scroll_view_set_content_size")
func swiftPawsScrollViewSetContentSize(_ ptr: UnsafeMutableRawPointer, _ w: Float, _ h: Float) {
    let sv = Unmanaged<UIScrollView>.fromOpaque(ptr).takeUnretainedValue()
    sv.contentSize = CGSize(width: CGFloat(w), height: CGFloat(h))
}

@_cdecl("swift_paws_scroll_view_set_content_offset")
func swiftPawsScrollViewSetContentOffset(
    _ ptr: UnsafeMutableRawPointer,
    _ x: Float, _ y: Float, _ animated: Bool
) {
    let sv = Unmanaged<UIScrollView>.fromOpaque(ptr).takeUnretainedValue()
    sv.setContentOffset(CGPoint(x: CGFloat(x), y: CGFloat(y)), animated: animated)
}

@_cdecl("swift_paws_scroll_view_set_scroll_enabled")
func swiftPawsScrollViewSetScrollEnabled(_ ptr: UnsafeMutableRawPointer, _ enabled: Bool) {
    let sv = Unmanaged<UIScrollView>.fromOpaque(ptr).takeUnretainedValue()
    sv.isScrollEnabled = enabled
}

@_cdecl("swift_paws_scroll_view_set_bounces")
func swiftPawsScrollViewSetBounces(_ ptr: UnsafeMutableRawPointer, _ bounces: Bool) {
    let sv = Unmanaged<UIScrollView>.fromOpaque(ptr).takeUnretainedValue()
    sv.bounces = bounces
}

// MARK: - CALayer

@_cdecl("swift_paws_layer_set_corner_radius")
func swiftPawsLayerSetCornerRadius(_ ptr: UnsafeMutableRawPointer, _ radius: Float) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.cornerRadius = CGFloat(radius)
}

@_cdecl("swift_paws_layer_set_border_width")
func swiftPawsLayerSetBorderWidth(_ ptr: UnsafeMutableRawPointer, _ width: Float) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.borderWidth = CGFloat(width)
}

@_cdecl("swift_paws_layer_set_border_color")
func swiftPawsLayerSetBorderColor(
    _ ptr: UnsafeMutableRawPointer,
    _ r: Float, _ g: Float, _ b: Float, _ a: Float
) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.borderColor = UIColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: CGFloat(a)).cgColor
}

@_cdecl("swift_paws_layer_set_shadow_color")
func swiftPawsLayerSetShadowColor(
    _ ptr: UnsafeMutableRawPointer,
    _ r: Float, _ g: Float, _ b: Float, _ a: Float
) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.shadowColor = UIColor(red: CGFloat(r), green: CGFloat(g), blue: CGFloat(b), alpha: CGFloat(a)).cgColor
}

@_cdecl("swift_paws_layer_set_shadow_offset")
func swiftPawsLayerSetShadowOffset(_ ptr: UnsafeMutableRawPointer, _ dx: Float, _ dy: Float) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.shadowOffset = CGSize(width: CGFloat(dx), height: CGFloat(dy))
}

@_cdecl("swift_paws_layer_set_shadow_radius")
func swiftPawsLayerSetShadowRadius(_ ptr: UnsafeMutableRawPointer, _ radius: Float) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.shadowRadius = CGFloat(radius)
}

@_cdecl("swift_paws_layer_set_shadow_opacity")
func swiftPawsLayerSetShadowOpacity(_ ptr: UnsafeMutableRawPointer, _ opacity: Float) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    layer.shadowOpacity = opacity
}

@_cdecl("swift_paws_layer_set_transform")
func swiftPawsLayerSetTransform(_ ptr: UnsafeMutableRawPointer, _ matrix: UnsafePointer<Float>) {
    let layer = Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()
    // Column-major 4x4 matrix → CATransform3D
    var t = CATransform3DIdentity
    t.m11 = CGFloat(matrix[0]);  t.m12 = CGFloat(matrix[1])
    t.m13 = CGFloat(matrix[2]);  t.m14 = CGFloat(matrix[3])
    t.m21 = CGFloat(matrix[4]);  t.m22 = CGFloat(matrix[5])
    t.m23 = CGFloat(matrix[6]);  t.m24 = CGFloat(matrix[7])
    t.m31 = CGFloat(matrix[8]);  t.m32 = CGFloat(matrix[9])
    t.m33 = CGFloat(matrix[10]); t.m34 = CGFloat(matrix[11])
    t.m41 = CGFloat(matrix[12]); t.m42 = CGFloat(matrix[13])
    t.m43 = CGFloat(matrix[14]); t.m44 = CGFloat(matrix[15])
    layer.transform = t
}

#endif
