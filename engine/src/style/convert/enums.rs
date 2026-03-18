//! Enum-to-enum converters for display, position, overflow, alignment, and box-sizing.

use super::stylo_types as st;

/// Converts a Stylo `Display` to a Taffy `Display`.
#[inline]
pub fn display(input: st::Display) -> taffy::Display {
    let mut display = match input.inside() {
        st::DisplayInside::None => taffy::Display::None,
        st::DisplayInside::Flex => taffy::Display::Flex,
        st::DisplayInside::Grid => taffy::Display::Grid,
        st::DisplayInside::Flow | st::DisplayInside::FlowRoot => taffy::Display::Block,
        st::DisplayInside::Table => taffy::Display::Grid,
        _ => taffy::Display::DEFAULT,
    };

    if matches!(input.outside(), st::DisplayOutside::None) {
        display = taffy::Display::None;
    }

    display
}

/// Converts a Stylo `BoxSizing` to a Taffy `BoxSizing`.
#[inline]
pub fn box_sizing(input: st::BoxSizing) -> taffy::BoxSizing {
    match input {
        st::BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
        st::BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
    }
}

/// Converts a Stylo `Position` to a Taffy `Position`.
#[inline]
pub fn position(input: st::Position) -> taffy::Position {
    match input {
        st::Position::Relative | st::Position::Static | st::Position::Sticky => {
            taffy::Position::Relative
        }
        st::Position::Absolute | st::Position::Fixed => taffy::Position::Absolute,
    }
}

/// Converts a Stylo `Overflow` to a Taffy `Overflow`.
#[inline]
pub fn overflow(input: st::Overflow) -> taffy::Overflow {
    match input {
        st::Overflow::Visible => taffy::Overflow::Visible,
        st::Overflow::Clip => taffy::Overflow::Clip,
        st::Overflow::Hidden => taffy::Overflow::Hidden,
        st::Overflow::Scroll => taffy::Overflow::Scroll,
        st::Overflow::Auto => taffy::Overflow::Scroll,
    }
}

/// Converts a Stylo `AspectRatio` to Taffy's `Option<f32>`.
#[inline]
pub fn aspect_ratio(input: st::GenericAspectRatio<st::NonNegative<f32>>) -> Option<f32> {
    use style::values::generics::position::PreferredRatio;
    match input.ratio {
        PreferredRatio::None => None,
        PreferredRatio::Ratio(val) => {
            let w = (val.0).0;
            let h = (val.1).0;
            if h != 0.0 {
                Some(w / h)
            } else {
                None
            }
        }
    }
}

/// Converts a Stylo `ContentDistribution` (align-content / justify-content) to Taffy.
#[inline]
pub fn content_alignment(input: st::ContentDistribution) -> Option<taffy::AlignContent> {
    match input.primary().value() {
        st::AlignFlags::NORMAL | st::AlignFlags::AUTO => None,
        st::AlignFlags::START | st::AlignFlags::LEFT => Some(taffy::AlignContent::Start),
        st::AlignFlags::END | st::AlignFlags::RIGHT => Some(taffy::AlignContent::End),
        st::AlignFlags::FLEX_START => Some(taffy::AlignContent::FlexStart),
        st::AlignFlags::FLEX_END => Some(taffy::AlignContent::FlexEnd),
        st::AlignFlags::CENTER => Some(taffy::AlignContent::Center),
        st::AlignFlags::STRETCH => Some(taffy::AlignContent::Stretch),
        st::AlignFlags::SPACE_BETWEEN => Some(taffy::AlignContent::SpaceBetween),
        st::AlignFlags::SPACE_AROUND => Some(taffy::AlignContent::SpaceAround),
        st::AlignFlags::SPACE_EVENLY => Some(taffy::AlignContent::SpaceEvenly),
        _ => None,
    }
}

/// Converts Stylo `AlignFlags` (align-items / align-self) to Taffy.
#[inline]
pub fn item_alignment(input: st::AlignFlags) -> Option<taffy::AlignItems> {
    match input.value() {
        st::AlignFlags::AUTO => None,
        st::AlignFlags::NORMAL | st::AlignFlags::STRETCH => Some(taffy::AlignItems::Stretch),
        st::AlignFlags::FLEX_START => Some(taffy::AlignItems::FlexStart),
        st::AlignFlags::FLEX_END => Some(taffy::AlignItems::FlexEnd),
        st::AlignFlags::SELF_START | st::AlignFlags::START | st::AlignFlags::LEFT => {
            Some(taffy::AlignItems::Start)
        }
        st::AlignFlags::SELF_END | st::AlignFlags::END | st::AlignFlags::RIGHT => {
            Some(taffy::AlignItems::End)
        }
        st::AlignFlags::CENTER => Some(taffy::AlignItems::Center),
        st::AlignFlags::BASELINE => Some(taffy::AlignItems::Baseline),
        _ => None,
    }
}

/// Converts a Stylo `TextAlign` to a Taffy `TextAlign`.
#[inline]
pub fn text_align(input: st::TextAlign) -> taffy::TextAlign {
    match input {
        st::TextAlign::MozLeft => taffy::TextAlign::LegacyLeft,
        st::TextAlign::MozRight => taffy::TextAlign::LegacyRight,
        st::TextAlign::MozCenter => taffy::TextAlign::LegacyCenter,
        _ => taffy::TextAlign::Auto,
    }
}
