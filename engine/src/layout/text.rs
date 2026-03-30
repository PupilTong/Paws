/// Built-in text measurer using character-width approximation.
///
/// Uses a simple heuristic: each character is ~60% of `font_size` wide.
/// Paws does not use `parley`; this placeholder stands in until
/// platform-native text measurement is wired through from the host OS.
pub(crate) struct MockTextMeasurer;

impl MockTextMeasurer {
    /// Measures the dimensions of a text string given a font size and available width.
    pub(crate) fn measure_text(
        &self,
        text: &str,
        font_size: f32,
        _max_width: Option<f32>,
    ) -> (f32, f32) {
        let char_count = text.chars().count();
        let width = char_count as f32 * font_size * 0.6; // approx width per char
        let height = font_size; // line height approx
        (width, height)
    }
}
