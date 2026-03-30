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
