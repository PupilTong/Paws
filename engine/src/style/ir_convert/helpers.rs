//! IR → Stylo value conversion helpers.
//!
//! This module provides two categories of helpers:
//!
//! 1. **Typed IR converters** — infallible conversions from the validated IR
//!    types (`ArchivedSizeIR`, `ArchivedMarginIR`, etc.) to Stylo specified
//!    values.  These are the primary path.
//!
//! 2. **Raw token fallback helpers** — used only for `PropertyValueIR::Raw`
//!    tokens that haven't been typed yet.  These retain the original
//!    string-matching logic from before the typed IR refactor.

use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::LengthPercentage;
use paws_style_ir::{
    ArchivedCssToken, ArchivedCssUnit, ArchivedGapIR, ArchivedInsetIR, ArchivedMarginIR,
    ArchivedMaxSizeIR, ArchivedSizeIR,
};

use super::length::{lp_ir_to_stylo, nn_lp_ir_to_stylo, no_calc_length};

// ═════════════════════════════════════════════════════════════════════
// Typed IR → Stylo converters (infallible)
// ═════════════════════════════════════════════════════════════════════

/// Converts an [`ArchivedSizeIR`] to a Stylo `Size`.
pub(crate) fn size_ir_to_stylo(ir: &ArchivedSizeIR) -> ::style::values::specified::Size {
    use ::style::values::specified::Size;
    match ir {
        ArchivedSizeIR::Auto => Size::Auto,
        ArchivedSizeIR::LengthPercentage(ref lp) => Size::LengthPercentage(nn_lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedMaxSizeIR`] to a Stylo `MaxSize`.
pub(crate) fn max_size_ir_to_stylo(ir: &ArchivedMaxSizeIR) -> ::style::values::specified::MaxSize {
    use ::style::values::specified::MaxSize;
    match ir {
        ArchivedMaxSizeIR::None => MaxSize::None,
        ArchivedMaxSizeIR::LengthPercentage(ref lp) => {
            MaxSize::LengthPercentage(nn_lp_ir_to_stylo(lp))
        }
    }
}

/// Converts an [`ArchivedMarginIR`] to a Stylo `Margin`.
pub(crate) fn margin_ir_to_stylo(
    ir: &ArchivedMarginIR,
) -> ::style::values::specified::length::Margin {
    use ::style::values::specified::length::Margin;
    match ir {
        ArchivedMarginIR::Auto => Margin::Auto,
        ArchivedMarginIR::LengthPercentage(ref lp) => Margin::LengthPercentage(lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedInsetIR`] to a Stylo `Inset`.
pub(crate) fn inset_ir_to_stylo(ir: &ArchivedInsetIR) -> ::style::values::specified::Inset {
    use ::style::values::specified::Inset;
    match ir {
        ArchivedInsetIR::Auto => Inset::Auto,
        ArchivedInsetIR::LengthPercentage(ref lp) => Inset::LengthPercentage(lp_ir_to_stylo(lp)),
    }
}

/// Converts an [`ArchivedGapIR`] to a Stylo `NonNegativeLengthPercentageOrNormal`.
pub(crate) fn gap_ir_to_stylo(
    ir: &ArchivedGapIR,
) -> ::style::values::specified::length::NonNegativeLengthPercentageOrNormal {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    match ir {
        ArchivedGapIR::Normal => NonNegativeLengthPercentageOrNormal::Normal,
        ArchivedGapIR::LengthPercentage(ref lp) => {
            NonNegativeLengthPercentageOrNormal::LengthPercentage(nn_lp_ir_to_stylo(lp))
        }
    }
}

// ═════════════════════════════════════════════════════════════════════
// Raw token fallback helpers (for PropertyValueIR::Raw)
// ═════════════════════════════════════════════════════════════════════

/// Extracts a keyword string from a single-value token list.
pub(crate) fn ir_keyword(values: &[ArchivedCssToken]) -> Option<&str> {
    match values {
        [ArchivedCssToken::Ident(ref kw)] => Some(kw.as_str()),
        _ => None,
    }
}

/// Extracts a unitless numeric value from a single-value token list.
pub(crate) fn ir_unitless(values: &[ArchivedCssToken]) -> Option<f32> {
    match values {
        [ArchivedCssToken::Number(val, ArchivedCssUnit::Unitless)] => Some((*val).into()),
        _ => None,
    }
}

/// Extracts the number and unit from a single `Number` token.
pub(crate) fn ir_single_number(values: &[ArchivedCssToken]) -> Option<(f32, &ArchivedCssUnit)> {
    match values {
        [ArchivedCssToken::Number(val, ref unit)] => Some(((*val).into(), unit)),
        _ => None,
    }
}

/// Converts a single-value token list to a Stylo [`LengthPercentage`] (Raw fallback).
fn lp_from_number(val: f32, unit: &ArchivedCssUnit) -> Option<LengthPercentage> {
    if matches!(unit, ArchivedCssUnit::Percent) {
        Some(LengthPercentage::Percentage(Percentage(val / 100.0)))
    } else {
        no_calc_length(val, unit).map(LengthPercentage::Length)
    }
}

/// Converts a single-value token list to a Stylo [`LengthPercentage`] (Raw fallback).
pub(crate) fn ir_to_lp(values: &[ArchivedCssToken]) -> Option<LengthPercentage> {
    let (val, unit) = ir_single_number(values)?;
    lp_from_number(val, unit)
}

/// Converts a single-value token list to a `NonNegative<LengthPercentage>` (Raw fallback).
pub(crate) fn ir_to_nn_lp(values: &[ArchivedCssToken]) -> Option<NonNegative<LengthPercentage>> {
    let (val, unit) = ir_single_number(values)?;
    if val < 0.0 {
        return None;
    }
    lp_from_number(val, unit).map(NonNegative)
}

/// Converts a token list to a Stylo `Size` (Raw fallback).
pub(crate) fn ir_to_size(values: &[ArchivedCssToken]) -> Option<::style::values::specified::Size> {
    use ::style::values::specified::Size;
    if ir_keyword(values) == Some("auto") {
        Some(Size::Auto)
    } else {
        ir_to_nn_lp(values).map(Size::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `MaxSize` (Raw fallback).
pub(crate) fn ir_to_max_size(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::MaxSize> {
    use ::style::values::specified::MaxSize;
    if ir_keyword(values) == Some("none") {
        Some(MaxSize::None)
    } else {
        ir_to_nn_lp(values).map(MaxSize::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `Margin` (Raw fallback).
pub(crate) fn ir_to_margin(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::length::Margin> {
    use ::style::values::specified::length::Margin;
    if ir_keyword(values) == Some("auto") {
        Some(Margin::Auto)
    } else {
        ir_to_lp(values).map(Margin::LengthPercentage)
    }
}

/// Converts a token list to a Stylo `Inset` (Raw fallback).
pub(crate) fn ir_to_inset(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::Inset> {
    use ::style::values::specified::Inset;
    if ir_keyword(values) == Some("auto") {
        Some(Inset::Auto)
    } else {
        ir_to_lp(values).map(Inset::LengthPercentage)
    }
}

/// Converts a keyword token list to a Stylo `BorderStyle` (Raw fallback).
pub(crate) fn ir_to_border_style(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::BorderStyle> {
    use ::style::values::specified::BorderStyle;
    match ir_keyword(values)? {
        "none" => Some(BorderStyle::None),
        "hidden" => Some(BorderStyle::Hidden),
        "solid" => Some(BorderStyle::Solid),
        "double" => Some(BorderStyle::Double),
        "dotted" => Some(BorderStyle::Dotted),
        "dashed" => Some(BorderStyle::Dashed),
        "groove" => Some(BorderStyle::Groove),
        "ridge" => Some(BorderStyle::Ridge),
        "inset" => Some(BorderStyle::Inset),
        "outset" => Some(BorderStyle::Outset),
        _ => None,
    }
}

/// Converts a keyword token list to a Stylo `BorderSideWidth` (Raw fallback).
pub(crate) fn ir_to_border_width(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::BorderSideWidth> {
    use ::style::values::specified::BorderSideWidth;
    if ir_keyword(values)? == "medium" {
        Some(BorderSideWidth::medium())
    } else {
        None
    }
}

/// Converts a token list to a `NonNegativeLengthPercentageOrNormal` (Raw fallback).
pub(crate) fn ir_to_gap(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::length::NonNegativeLengthPercentageOrNormal> {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    if ir_keyword(values) == Some("normal") {
        Some(NonNegativeLengthPercentageOrNormal::Normal)
    } else {
        ir_to_nn_lp(values).map(NonNegativeLengthPercentageOrNormal::LengthPercentage)
    }
}
