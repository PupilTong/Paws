//! Primitive length / dimension converters from Stylo to Taffy.
//!
//! All functions in this module convert Stylo `LengthPercentage` and related
//! types into their Taffy equivalents, handling calc() resolution.

use super::stylo_types as st;

// ─── Core LP resolution ─────────────────────────────────────────────

/// An intermediate representation for a resolved `LengthPercentage`.
pub(crate) enum ResolvedLP {
    Length(f32),
    Percent(f32),
}

/// Resolves a Stylo `LengthPercentage` to a [`ResolvedLP`].
///
/// Calc values are resolved against a zero basis (best-effort — drops
/// percentage terms since Taffy 0.4 has no calc representation).
#[inline]
fn resolve_lp(val: &st::LengthPercentage) -> ResolvedLP {
    match val.unpack() {
        st::UnpackedLP::Length(len) => ResolvedLP::Length(len.px()),
        st::UnpackedLP::Percentage(pct) => ResolvedLP::Percent(pct.0),
        st::UnpackedLP::Calc(calc) => {
            let resolved = calc.resolve(style::values::computed::Length::new(0.0));
            ResolvedLP::Length(resolved.px())
        }
    }
}

// ─── Primitive converters ────────────────────────────────────────────

/// Converts a Stylo `LengthPercentage` to a Taffy `LengthPercentage`.
#[inline]
pub fn length_percentage(val: &st::LengthPercentage) -> taffy::LengthPercentage {
    match resolve_lp(val) {
        ResolvedLP::Percent(v) => taffy::LengthPercentage::Percent(v),
        ResolvedLP::Length(v) => taffy::LengthPercentage::Length(v),
    }
}

/// Converts a Stylo `LengthPercentage` to a Taffy `Dimension`.
#[inline]
fn lp_to_dimension(val: &st::LengthPercentage) -> taffy::Dimension {
    match resolve_lp(val) {
        ResolvedLP::Percent(v) => taffy::Dimension::Percent(v),
        ResolvedLP::Length(v) => taffy::Dimension::Length(v),
    }
}

/// Converts a Stylo `LengthPercentage` to a Taffy `LengthPercentageAuto`.
#[inline]
fn lp_to_lpa(val: &st::LengthPercentage) -> taffy::prelude::LengthPercentageAuto {
    match resolve_lp(val) {
        ResolvedLP::Percent(v) => taffy::prelude::LengthPercentageAuto::Percent(v),
        ResolvedLP::Length(v) => taffy::prelude::LengthPercentageAuto::Length(v),
    }
}

// ─── Dimension converters ────────────────────────────────────────────

/// Converts a Stylo `Size` (width/height) to a Taffy `Dimension`.
#[inline]
pub fn dimension(val: &st::Size) -> taffy::Dimension {
    match val {
        st::Size::LengthPercentage(val) => lp_to_dimension(&val.0),
        st::Size::Auto => taffy::Dimension::Auto,
        // Taffy 0.4 lacks intrinsic sizing keywords; fall back to Auto.
        st::Size::MaxContent
        | st::Size::MinContent
        | st::Size::FitContent
        | st::Size::FitContentFunction(_)
        | st::Size::Stretch
        | st::Size::WebkitFillAvailable => taffy::Dimension::Auto,
        // Anchor positioning is not supported.
        st::Size::AnchorSizeFunction(_) | st::Size::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::Auto
        }
    }
}

/// Converts a Stylo `MaxSize` to a Taffy `Dimension`.
#[inline]
pub fn max_size_dimension(val: &st::MaxSize) -> taffy::Dimension {
    match val {
        st::MaxSize::LengthPercentage(val) => lp_to_dimension(&val.0),
        st::MaxSize::None => taffy::Dimension::Auto,
        st::MaxSize::MaxContent
        | st::MaxSize::MinContent
        | st::MaxSize::FitContent
        | st::MaxSize::FitContentFunction(_)
        | st::MaxSize::Stretch
        | st::MaxSize::WebkitFillAvailable => taffy::Dimension::Auto,
        st::MaxSize::AnchorSizeFunction(_) | st::MaxSize::AnchorContainingCalcFunction(_) => {
            taffy::Dimension::Auto
        }
    }
}

/// Converts a Stylo margin value to a Taffy `LengthPercentageAuto`.
#[inline]
pub fn margin(val: &st::MarginVal) -> taffy::prelude::LengthPercentageAuto {
    match val {
        st::MarginVal::Auto => taffy::prelude::LengthPercentageAuto::Auto,
        st::MarginVal::LengthPercentage(val) => lp_to_lpa(val),
        // Anchor positioning not supported.
        st::MarginVal::AnchorSizeFunction(_) | st::MarginVal::AnchorContainingCalcFunction(_) => {
            taffy::prelude::LengthPercentageAuto::Auto
        }
    }
}

/// Converts a Stylo border width + style to a Taffy `LengthPercentage`.
///
/// Hidden/none borders resolve to zero width.
#[inline]
pub fn border(width: &st::BorderSideWidth, style: st::BorderStyle) -> taffy::LengthPercentage {
    if style.none_or_hidden() {
        return taffy::LengthPercentage::Length(0.0);
    }
    taffy::LengthPercentage::Length(width.0.to_f32_px())
}

/// Converts a Stylo inset (top/right/bottom/left) to Taffy `LengthPercentageAuto`.
#[inline]
pub fn inset(val: &st::InsetVal) -> taffy::prelude::LengthPercentageAuto {
    match val {
        st::InsetVal::Auto => taffy::prelude::LengthPercentageAuto::Auto,
        st::InsetVal::LengthPercentage(val) => lp_to_lpa(val),
        // Anchor positioning not supported.
        st::InsetVal::AnchorSizeFunction(_)
        | st::InsetVal::AnchorFunction(_)
        | st::InsetVal::AnchorContainingCalcFunction(_) => {
            taffy::prelude::LengthPercentageAuto::Auto
        }
    }
}

/// Converts a Stylo gap value to Taffy `LengthPercentage`.
#[inline]
pub fn gap(input: &st::Gap) -> taffy::LengthPercentage {
    match input {
        st::Gap::Normal => taffy::LengthPercentage::Length(0.0),
        st::Gap::LengthPercentage(val) => length_percentage(&val.0),
    }
}
