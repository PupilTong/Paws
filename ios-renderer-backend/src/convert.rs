//! Conversion from engine's [`LayoutBoxFull`] to the renderer's [`LayoutNode`].
//!
//! Bridges the gap between the engine's DOM-centric layout output and the
//! renderer's flat, FFI-safe input types. Extracts computed visual properties
//! from Stylo's [`ComputedValues`] and maps overflow to [`ScrollProps`].

use crate::types::*;
use engine::layout::LayoutBoxFull;
use style::color::AbsoluteColor;
use style::properties::ComputedValues;
use style::values::computed::Overflow as StyloOverflow;

/// Convert an engine layout tree into a renderer [`LayoutNode`] tree.
///
/// `generation` is a monotonically increasing counter used by the diff
/// stage to detect unchanged subtrees in O(1).
pub(crate) fn layout_box_to_layout_node(
    doc: &engine::dom::Document,
    layout: &LayoutBoxFull,
    generation: u64,
) -> LayoutNode {
    let node = doc.get_node(layout.node_id);

    let (style, scroll) = node
        .and_then(|n| {
            n.get_computed_values()
                .map(|cv| extract_style_and_scroll(cv, layout))
        })
        .unwrap_or_else(|| (default_style(), None));

    let children = layout
        .children
        .iter()
        .map(|child| layout_box_to_layout_node(doc, child, generation))
        .collect();

    LayoutNode {
        id: layout.node_id as u64,
        frame: Rect {
            x: layout.x,
            y: layout.y,
            width: layout.width,
            height: layout.height,
        },
        children,
        scroll,
        style,
        generation,
    }
}

/// Extract visual properties from Stylo's `ComputedValues`.
fn extract_style_and_scroll(
    cv: &ComputedValues,
    layout: &LayoutBoxFull,
) -> (ComputedStyle, Option<ScrollProps>) {
    let opacity = cv.clone_opacity();

    // Background color — resolve to absolute sRGB.
    let bg_color = cv.clone_background_color();
    let background =
        absolute_color_to_color(&bg_color.resolve_to_absolute(&AbsoluteColor::TRANSPARENT_BLACK));

    // Border radius — use the top-left corner, resolve percentage against width.
    let br = &cv.get_border().border_top_left_radius;
    let border_radius = resolve_length_percentage(&br.0.width.0, layout.width);

    let style = ComputedStyle {
        opacity,
        transform: None,
        clip: None,
        background,
        border_radius,
        will_change: false,
    };

    // Detect scroll containers via overflow.
    let overflow_x = cv.clone_overflow_x();
    let overflow_y = cv.clone_overflow_y();

    let scroll = if overflow_y != StyloOverflow::Visible || overflow_x != StyloOverflow::Visible {
        let content_height = layout
            .children
            .iter()
            .map(|c| c.y + c.height)
            .fold(0.0f32, f32::max);
        let content_width = layout
            .children
            .iter()
            .map(|c| c.x + c.width)
            .fold(0.0f32, f32::max);

        Some(ScrollProps {
            content_size: Size {
                width: content_width.max(layout.width),
                height: content_height.max(layout.height),
            },
            overflow_x: map_overflow(overflow_x),
            overflow_y: map_overflow(overflow_y),
        })
    } else {
        None
    };

    (style, scroll)
}

/// Convert Stylo's `AbsoluteColor` (sRGB) to our flat `Color`.
fn absolute_color_to_color(c: &AbsoluteColor) -> Color {
    let srgb = c.to_color_space(style::color::ColorSpace::Srgb);
    Color {
        r: srgb.components.0,
        g: srgb.components.1,
        b: srgb.components.2,
        a: srgb.alpha,
    }
}

/// Resolve a Stylo `LengthPercentage` to `f32` pixels, given a
/// reference length for percentage resolution.
fn resolve_length_percentage(
    lp: &style::values::computed::LengthPercentage,
    reference: f32,
) -> f32 {
    lp.to_percentage()
        .map(|p| p.0 * reference)
        .unwrap_or_else(|| lp.to_length().map(|l| l.px()).unwrap_or(0.0))
}

fn map_overflow(o: StyloOverflow) -> Overflow {
    match o {
        StyloOverflow::Visible => Overflow::Visible,
        StyloOverflow::Hidden | StyloOverflow::Clip => Overflow::Hidden,
        StyloOverflow::Scroll => Overflow::Scroll,
        StyloOverflow::Auto => Overflow::Auto,
    }
}

fn default_style() -> ComputedStyle {
    ComputedStyle {
        opacity: 1.0,
        transform: None,
        clip: None,
        background: Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        },
        border_radius: 0.0,
        will_change: false,
    }
}
