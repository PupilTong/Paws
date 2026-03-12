/// A placeholder interface for text layout.
/// Paws does not use `parley`, but instead relies on the host OS for text rendering and measurement.
pub trait TextMeasurer {
    /// Measures the dimensions of a text string given a font size and available width.
    fn measure_text(&self, text: &str, font_size: f32, max_width: Option<f32>) -> (f32, f32);
}

/// A mock text measurer for testing.
pub struct MockTextMeasurer;

impl TextMeasurer for MockTextMeasurer {
    fn measure_text(&self, text: &str, font_size: f32, _max_width: Option<f32>) -> (f32, f32) {
        let char_count = text.chars().count();
        let width = char_count as f32 * font_size * 0.6; // approx width per char
        let height = font_size; // line height approx
        (width, height)
    }
}
