//! Shared IR → Stylo value conversion helpers.
//!
//! These low-level primitives translate [`ArchivedCssPropertyIR`] and
//! [`ArchivedCssUnit`] values into the Stylo specified-value types consumed
//! by [`PropertyDeclaration`] constructors.  They are `pub(crate)` so that
//! sibling sub-modules (`keyword`, `numeric`) can import them directly.

use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::{
    AbsoluteLength, ContainerRelativeLength, FontRelativeLength, LengthPercentage, NoCalcLength,
    ViewportPercentageLength,
};
use paws_style_ir::{ArchivedCssPropertyIR, ArchivedCssUnit};

// ─── Primitive extractors ────────────────────────────────────────────

/// Extracts a keyword string from an IR value.
///
/// Returns `None` when the value is not the `Keyword` variant.
pub(crate) fn ir_keyword(value: &ArchivedCssPropertyIR) -> Option<&str> {
    if let ArchivedCssPropertyIR::Keyword(ref kw) = value {
        Some(kw.as_str())
    } else {
        None
    }
}

/// Extracts a unitless numeric value from an IR value.
///
/// Returns `None` for any variant other than `Unit(_, Unitless)`.
pub(crate) fn ir_unitless(value: &ArchivedCssPropertyIR) -> Option<f32> {
    if let ArchivedCssPropertyIR::Unit(val, ArchivedCssUnit::Unitless) = value {
        Some((*val).into())
    } else {
        None
    }
}

// ─── Length conversion ───────────────────────────────────────────────

/// Converts an IR `(value, unit)` pair to a Stylo [`NoCalcLength`].
///
/// Handles absolute, font-relative, viewport-relative, and
/// container-relative units.  Returns `None` for non-length units
/// (e.g. percentage, angle, time, or unitless).
pub(crate) fn ir_to_no_calc_length(val: f32, unit: &ArchivedCssUnit) -> Option<NoCalcLength> {
    match unit {
        // Absolute lengths
        ArchivedCssUnit::Px => Some(NoCalcLength::Absolute(AbsoluteLength::Px(val))),
        ArchivedCssUnit::Cm => Some(NoCalcLength::Absolute(AbsoluteLength::Cm(val))),
        ArchivedCssUnit::Mm => Some(NoCalcLength::Absolute(AbsoluteLength::Mm(val))),
        ArchivedCssUnit::In => Some(NoCalcLength::Absolute(AbsoluteLength::In(val))),
        ArchivedCssUnit::Pt => Some(NoCalcLength::Absolute(AbsoluteLength::Pt(val))),
        ArchivedCssUnit::Pc => Some(NoCalcLength::Absolute(AbsoluteLength::Pc(val))),
        ArchivedCssUnit::Q => Some(NoCalcLength::Absolute(AbsoluteLength::Q(val))),
        // Font-relative lengths
        ArchivedCssUnit::Em => Some(NoCalcLength::FontRelative(FontRelativeLength::Em(val))),
        ArchivedCssUnit::Rem => Some(NoCalcLength::FontRelative(FontRelativeLength::Rem(val))),
        ArchivedCssUnit::Ex => Some(NoCalcLength::FontRelative(FontRelativeLength::Ex(val))),
        ArchivedCssUnit::Ch => Some(NoCalcLength::FontRelative(FontRelativeLength::Ch(val))),
        // Viewport-relative lengths
        ArchivedCssUnit::Vw => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Vw(val),
        )),
        ArchivedCssUnit::Vh => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Vh(val),
        )),
        ArchivedCssUnit::Vmin => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Vmin(val),
        )),
        ArchivedCssUnit::Vmax => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Vmax(val),
        )),
        ArchivedCssUnit::Svw => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Svw(val),
        )),
        ArchivedCssUnit::Svh => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Svh(val),
        )),
        ArchivedCssUnit::Lvw => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Lvw(val),
        )),
        ArchivedCssUnit::Lvh => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Lvh(val),
        )),
        ArchivedCssUnit::Dvw => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Dvw(val),
        )),
        ArchivedCssUnit::Dvh => Some(NoCalcLength::ViewportPercentage(
            ViewportPercentageLength::Dvh(val),
        )),
        // Container-relative lengths
        ArchivedCssUnit::Cqw => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqw(val),
        )),
        ArchivedCssUnit::Cqh => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqh(val),
        )),
        ArchivedCssUnit::Cqi => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqi(val),
        )),
        ArchivedCssUnit::Cqb => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqb(val),
        )),
        ArchivedCssUnit::Cqmin => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqmin(val),
        )),
        ArchivedCssUnit::Cqmax => Some(NoCalcLength::ContainerRelative(
            ContainerRelativeLength::Cqmax(val),
        )),
        // Not a length unit (percent, fr, angle, time, resolution, unitless)
        _ => None,
    }
}

/// Converts an IR value to a Stylo specified [`LengthPercentage`].
///
/// Accepts `Unit(_, Px|Em|Rem|…)` for lengths and `Unit(_, Percent)` for
/// percentages.  Returns `None` for keywords or other non-LP values.
pub(crate) fn ir_to_lp(value: &ArchivedCssPropertyIR) -> Option<LengthPercentage> {
    if let ArchivedCssPropertyIR::Unit(val, ref unit) = value {
        let v: f32 = (*val).into();
        if matches!(unit, ArchivedCssUnit::Percent) {
            Some(LengthPercentage::Percentage(Percentage(v / 100.0)))
        } else {
            ir_to_no_calc_length(v, unit).map(LengthPercentage::Length)
        }
    } else {
        None
    }
}

/// Converts an IR value to a `NonNegative<LengthPercentage>`.
///
/// Returns `None` for negative values so the fallback string parser can
/// correctly reject them per the CSS spec.
pub(crate) fn ir_to_nn_lp(value: &ArchivedCssPropertyIR) -> Option<NonNegative<LengthPercentage>> {
    if let ArchivedCssPropertyIR::Unit(val, _) = value {
        let v: f32 = (*val).into();
        if v < 0.0 {
            return None;
        }
    }
    ir_to_lp(value).map(NonNegative)
}

// ─── Typed dimension helpers ─────────────────────────────────────────

/// Converts an IR value to a Stylo `Size` (used by `width`, `height`, `min-*`).
///
/// Handles the `auto` keyword and non-negative length-percentage values.
pub(crate) fn ir_to_size(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::Size> {
    use ::style::values::specified::Size;
    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Size::Auto),
        _ => ir_to_nn_lp(value).map(Size::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `MaxSize` (used by `max-width`, `max-height`).
///
/// Handles the `none` keyword and non-negative length-percentage values.
pub(crate) fn ir_to_max_size(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::MaxSize> {
    use ::style::values::specified::MaxSize;
    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "none" => Some(MaxSize::None),
        _ => ir_to_nn_lp(value).map(MaxSize::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `Margin` (`auto` or length-percentage).
pub(crate) fn ir_to_margin(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::length::Margin> {
    use ::style::values::specified::length::Margin;
    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Margin::Auto),
        _ => ir_to_lp(value).map(Margin::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `Inset` (`auto` or length-percentage).
pub(crate) fn ir_to_inset(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::Inset> {
    use ::style::values::specified::Inset;
    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Inset::Auto),
        _ => ir_to_lp(value).map(Inset::LengthPercentage),
    }
}

/// Converts an IR keyword value to a Stylo `BorderStyle`.
pub(crate) fn ir_to_border_style(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::BorderStyle> {
    use ::style::values::specified::BorderStyle;
    match ir_keyword(value)? {
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

/// Converts an IR keyword value to a Stylo `BorderSideWidth`.
///
/// Supports the `medium` keyword only.  `thin` and `thick` are not
/// constructable from outside Stylo because `BorderSideWidth`'s inner
/// `LineWidth` field is module-private.
pub(crate) fn ir_to_border_width(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::BorderSideWidth> {
    use ::style::values::specified::BorderSideWidth;
    if ir_keyword(value)? == "medium" {
        Some(BorderSideWidth::medium())
    } else {
        None
    }
}

/// Converts an IR value to a `NonNegativeLengthPercentageOrNormal`
/// (used by `column-gap` and `row-gap`).
pub(crate) fn ir_to_gap(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::length::NonNegativeLengthPercentageOrNormal> {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "normal" => {
            Some(NonNegativeLengthPercentageOrNormal::Normal)
        }
        _ => ir_to_nn_lp(value).map(NonNegativeLengthPercentageOrNormal::LengthPercentage),
    }
}
