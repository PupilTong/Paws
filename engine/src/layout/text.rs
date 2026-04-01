use std::cell::RefCell;

use parley::style::StyleProperty;
use parley::{FontContext, FontWeight, Layout, LayoutContext};

/// Parley-backed text layout context.
///
/// Wraps `FontContext` (system font database) and `LayoutContext` (reusable
/// scratch buffers). Created once per engine lifetime, reused across frames.
///
/// Uses `RefCell` because Parley needs `&mut` access internally but the layout
/// adapter borrows us via `&self`.
pub struct TextLayoutContext {
    font_cx: RefCell<FontContext>,
    layout_cx: RefCell<LayoutContext<()>>,
}

impl Default for TextLayoutContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLayoutContext {
    /// Creates a new text layout context with system fonts loaded.
    pub fn new() -> Self {
        Self {
            font_cx: RefCell::new(FontContext::new()),
            layout_cx: RefCell::new(LayoutContext::new()),
        }
    }

    /// Measures text with the given font size, font weight, and optional width constraint.
    ///
    /// Returns `(width, height)` in CSS pixels. When `max_width` is `Some`,
    /// Parley performs line breaking within that constraint.
    pub fn measure_text(
        &self,
        text: &str,
        font_size: f32,
        font_weight: f32,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let mut font_cx = self.font_cx.borrow_mut();
        let mut layout_cx = self.layout_cx.borrow_mut();
        let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(font_weight)));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_width);
        (layout.width(), layout.height())
    }
}

// SAFETY: Only accessed from the single engine thread. RefCell is !Sync
// but the engine is single-threaded.
unsafe impl Sync for TextLayoutContext {}
unsafe impl Send for TextLayoutContext {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_text_returns_nonzero_dimensions() {
        let ctx = TextLayoutContext::new();
        let (w, h) = ctx.measure_text("hello", 16.0, 400.0, None);
        assert!(w > 0.0, "width should be positive, got {w}");
        assert!(h > 0.0, "height should be positive, got {h}");
    }

    #[test]
    fn measure_text_empty_string() {
        let ctx = TextLayoutContext::new();
        let (w, _h) = ctx.measure_text("", 16.0, 400.0, None);
        assert_eq!(w, 0.0, "empty string should have zero width");
    }

    #[test]
    fn measure_text_with_max_width_wraps() {
        let ctx = TextLayoutContext::new();
        let long_text = "hello world this is a long string that should wrap";
        let (_w_unconstrained, h_unconstrained) = ctx.measure_text(long_text, 16.0, 400.0, None);
        let (_w_narrow, h_narrow) = ctx.measure_text(long_text, 16.0, 400.0, Some(50.0));
        assert!(
            h_narrow >= h_unconstrained,
            "narrow constraint should produce equal or taller layout: \
             unconstrained={h_unconstrained}, narrow={h_narrow}"
        );
    }

    #[test]
    fn measure_text_larger_font_is_taller() {
        let ctx = TextLayoutContext::new();
        let (_, h_small) = ctx.measure_text("hello", 12.0, 400.0, None);
        let (_, h_large) = ctx.measure_text("hello", 48.0, 400.0, None);
        assert!(
            h_large > h_small,
            "larger font should be taller: 12px={h_small}, 48px={h_large}"
        );
    }

    #[test]
    fn measure_text_reuse_context() {
        let ctx = TextLayoutContext::new();
        let (w1, h1) = ctx.measure_text("test", 16.0, 400.0, None);
        let (w2, h2) = ctx.measure_text("test", 16.0, 400.0, None);
        assert_eq!(w1, w2, "same input should give same width");
        assert_eq!(h1, h2, "same input should give same height");
    }

    #[test]
    fn default_creates_valid_context() {
        let ctx = TextLayoutContext::default();
        let (w, h) = ctx.measure_text("x", 16.0, 400.0, None);
        assert!(w > 0.0);
        assert!(h > 0.0);
    }
}
