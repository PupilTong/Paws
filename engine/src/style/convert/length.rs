//! Primitive length / dimension converters from Stylo to Taffy.
//!
//! Uses `CompactLength` tagged pointers and `style_helpers` constructors
//! for efficient representation. `calc()` values are passed through as raw
//! pointers for layout-time resolution (no lossy zero-basis resolve).

use super::stylo_types as st;
use taffy::style_helpers::*;
use taffy::CompactLength;

// ─── Core LP resolution ─────────────────────────────────────────────

/// Converts a Stylo `LengthPercentage` to a Taffy `LengthPercentage`.
///
/// `calc()` expressions are passed through as raw pointers to `CompactLength`,
/// preserving percentage terms for layout-time resolution.
#[inline]
pub fn length_percentage(val: &st::LengthPercentage) -> taffy::LengthPercentage {
    match val.unpack() {
        st::UnpackedLP::Calc(calc_ptr) => {
            let val = CompactLength::calc(calc_ptr as *const st::CalcLengthPercentage as *const ());
            // SAFETY: `CompactLength::calc` produces a valid tagged-pointer value for
            // `LengthPercentage`. The pointee (`CalcLengthPercentage`) remains live for the
            // duration of the `ComputedValues` borrow that the caller holds.
            unsafe { taffy::LengthPercentage::from_raw(val) }
        }
        st::UnpackedLP::Length(len) => length(len.px()),
        st::UnpackedLP::Percentage(pct) => percent(pct.0),
    }
}

// ─── Dimension converters ────────────────────────────────────────────

/// Converts a Stylo `Size` (width/height) to a Taffy `Dimension`.
#[inline]
pub fn dimension(val: &st::Size) -> taffy::Dimension {
    match val {
        st::Size::LengthPercentage(val) => length_percentage(&val.0).into(),
        st::Size::Auto => taffy::Dimension::AUTO,
        st::Size::MaxContent
        | st::Size::MinContent
        | st::Size::FitContent
        | st::Size::FitContentFunction(_)
        | st::Size::Stretch
        | st::Size::WebkitFillAvailable => taffy::Dimension::AUTO,
        st::Size::AnchorSizeFunction(_) | st::Size::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::AUTO
        }
    }
}

/// Converts a Stylo `MaxSize` to a Taffy `Dimension`.
#[inline]
pub fn max_size_dimension(val: &st::MaxSize) -> taffy::Dimension {
    match val {
        st::MaxSize::LengthPercentage(val) => length_percentage(&val.0).into(),
        st::MaxSize::None => taffy::Dimension::AUTO,
        st::MaxSize::MaxContent
        | st::MaxSize::MinContent
        | st::MaxSize::FitContent
        | st::MaxSize::FitContentFunction(_)
        | st::MaxSize::Stretch
        | st::MaxSize::WebkitFillAvailable => taffy::Dimension::AUTO,
        st::MaxSize::AnchorSizeFunction(_) | st::MaxSize::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::AUTO
        }
    }
}

/// Converts a Stylo margin value to a Taffy `LengthPercentageAuto`.
#[inline]
pub fn margin(val: &st::MarginVal) -> taffy::LengthPercentageAuto {
    match val {
        st::MarginVal::Auto => taffy::LengthPercentageAuto::AUTO,
        st::MarginVal::LengthPercentage(val) => length_percentage(val).into(),
        st::MarginVal::AnchorSizeFunction(_) | st::MarginVal::AnchorContainingCalcFunction(_) => {
            taffy::LengthPercentageAuto::AUTO
        }
    }
}

/// Converts a Stylo border width + style to a Taffy `LengthPercentage`.
///
/// Hidden/none borders resolve to zero width.
#[inline]
pub fn border(width: &st::BorderSideWidth, style: st::BorderStyle) -> taffy::LengthPercentage {
    if style.none_or_hidden() {
        return zero();
    }
    length(width.0.to_f32_px())
}

/// Converts a Stylo inset (top/right/bottom/left) to Taffy `LengthPercentageAuto`.
#[inline]
pub fn inset(val: &st::InsetVal) -> taffy::LengthPercentageAuto {
    match val {
        st::InsetVal::Auto => taffy::LengthPercentageAuto::AUTO,
        st::InsetVal::LengthPercentage(val) => length_percentage(val).into(),
        st::InsetVal::AnchorSizeFunction(_)
        | st::InsetVal::AnchorFunction(_)
        | st::InsetVal::AnchorContainingCalcFunction(_) => taffy::LengthPercentageAuto::AUTO,
    }
}

/// Converts a Stylo gap value to Taffy `LengthPercentage`.
#[inline]
pub fn gap(input: &st::Gap) -> taffy::LengthPercentage {
    match input {
        st::Gap::Normal => taffy::LengthPercentage::ZERO,
        st::Gap::LengthPercentage(val) => length_percentage(&val.0),
    }
}
