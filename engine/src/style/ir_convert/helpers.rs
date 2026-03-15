//! Shared IR → Stylo value conversion helpers.
//!
//! These low-level primitives translate [`ArchivedCssComponentValue`] and
//! [`ArchivedCssUnit`] values into the Stylo specified-value types consumed
//! by [`PropertyDeclaration`] constructors.  They are `pub(crate)` so that
//! sibling sub-modules (`keyword`, `numeric`) can import them directly.

use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::{
    AbsoluteLength, ContainerRelativeLength, FontRelativeLength, LengthPercentage, NoCalcLength,
    ViewportPercentageLength,
};
use paws_style_ir::{ArchivedCssComponentValue, ArchivedCssUnit};

// ─── Primitive extractors ────────────────────────────────────────────

/// Extracts a keyword string from a single-value component list.
///
/// Returns `None` when the list does not contain exactly one `Ident` value.
pub(crate) fn ir_keyword(values: &[ArchivedCssComponentValue]) -> Option<&str> {
    match values {
        [ArchivedCssComponentValue::Ident(ref kw)] => Some(kw.as_str()),
        _ => None,
    }
}

/// Extracts a unitless numeric value from a single-value component list.
///
/// Returns `None` unless the list is exactly `[Number(_, Unitless)]`.
pub(crate) fn ir_unitless(values: &[ArchivedCssComponentValue]) -> Option<f32> {
    match values {
        [ArchivedCssComponentValue::Number(val, ArchivedCssUnit::Unitless)] => Some((*val).into()),
        _ => None,
    }
}

/// Extracts the number and unit from a single `Number` component value.
///
/// Returns `None` unless the list is exactly one `Number` value.
pub(crate) fn ir_single_number(
    values: &[ArchivedCssComponentValue],
) -> Option<(f32, &ArchivedCssUnit)> {
    match values {
        [ArchivedCssComponentValue::Number(val, ref unit)] => Some(((*val).into(), unit)),
        _ => None,
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

/// Converts a single-value component list to a Stylo [`LengthPercentage`].
///
/// Accepts `Number(_, Px|Em|Rem|…)` for lengths and `Number(_, Percent)` for
/// percentages.  Returns `None` for keywords or multi-value lists.
pub(crate) fn ir_to_lp(values: &[ArchivedCssComponentValue]) -> Option<LengthPercentage> {
    let (val, unit) = ir_single_number(values)?;
    if matches!(unit, ArchivedCssUnit::Percent) {
        Some(LengthPercentage::Percentage(Percentage(val / 100.0)))
    } else {
        ir_to_no_calc_length(val, unit).map(LengthPercentage::Length)
    }
}

/// Converts a single-value component list to a `NonNegative<LengthPercentage>`.
///
/// Returns `None` for negative values so the fallback parser can
/// correctly reject them per the CSS spec.
pub(crate) fn ir_to_nn_lp(
    values: &[ArchivedCssComponentValue],
) -> Option<NonNegative<LengthPercentage>> {
    let (val, _) = ir_single_number(values)?;
    if val < 0.0 {
        return None;
    }
    ir_to_lp(values).map(NonNegative)
}

// ─── Typed dimension helpers ─────────────────────────────────────────

/// Converts a component value list to a Stylo `Size` (used by `width`, `height`, `min-*`).
///
/// Handles the `auto` keyword and non-negative length-percentage values.
pub(crate) fn ir_to_size(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::Size> {
    use ::style::values::specified::Size;
    if ir_keyword(values) == Some("auto") {
        Some(Size::Auto)
    } else {
        ir_to_nn_lp(values).map(Size::LengthPercentage)
    }
}

/// Converts a component value list to a Stylo `MaxSize` (used by `max-width`, `max-height`).
///
/// Handles the `none` keyword and non-negative length-percentage values.
pub(crate) fn ir_to_max_size(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::MaxSize> {
    use ::style::values::specified::MaxSize;
    if ir_keyword(values) == Some("none") {
        Some(MaxSize::None)
    } else {
        ir_to_nn_lp(values).map(MaxSize::LengthPercentage)
    }
}

/// Converts a component value list to a Stylo `Margin` (`auto` or length-percentage).
pub(crate) fn ir_to_margin(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::length::Margin> {
    use ::style::values::specified::length::Margin;
    if ir_keyword(values) == Some("auto") {
        Some(Margin::Auto)
    } else {
        ir_to_lp(values).map(Margin::LengthPercentage)
    }
}

/// Converts a component value list to a Stylo `Inset` (`auto` or length-percentage).
pub(crate) fn ir_to_inset(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::Inset> {
    use ::style::values::specified::Inset;
    if ir_keyword(values) == Some("auto") {
        Some(Inset::Auto)
    } else {
        ir_to_lp(values).map(Inset::LengthPercentage)
    }
}

/// Converts a keyword component value list to a Stylo `BorderStyle`.
pub(crate) fn ir_to_border_style(
    values: &[ArchivedCssComponentValue],
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

/// Converts a keyword component value list to a Stylo `BorderSideWidth`.
///
/// Supports the `medium` keyword only.  `thin` and `thick` are not
/// constructable from outside Stylo because `BorderSideWidth`'s inner
/// `LineWidth` field is module-private.
pub(crate) fn ir_to_border_width(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::BorderSideWidth> {
    use ::style::values::specified::BorderSideWidth;
    if ir_keyword(values)? == "medium" {
        Some(BorderSideWidth::medium())
    } else {
        None
    }
}

/// Converts a component value list to a `NonNegativeLengthPercentageOrNormal`
/// (used by `column-gap` and `row-gap`).
pub(crate) fn ir_to_gap(
    values: &[ArchivedCssComponentValue],
) -> Option<::style::values::specified::length::NonNegativeLengthPercentageOrNormal> {
    use ::style::values::specified::length::NonNegativeLengthPercentageOrNormal;
    if ir_keyword(values) == Some("normal") {
        Some(NonNegativeLengthPercentageOrNormal::Normal)
    } else {
        ir_to_nn_lp(values).map(NonNegativeLengthPercentageOrNormal::LengthPercentage)
    }
}
