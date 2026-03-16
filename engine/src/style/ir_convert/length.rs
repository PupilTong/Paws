//! Typed IR → Stylo length conversion primitives.
//!
//! These functions convert the validated IR length types into their Stylo
//! equivalents.  Since validation happens at compile time in the `css!()`
//! macro, these conversions are infallible.

use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::{
    AbsoluteLength, ContainerRelativeLength, FontRelativeLength, LengthPercentage, NoCalcLength,
    ViewportPercentageLength,
};
use paws_style_ir::{ArchivedCssUnit, ArchivedLengthPercentageIR, ArchivedNonNegativeLPIR};

/// Converts an IR unit + value to a Stylo [`NoCalcLength`].
///
/// Returns `None` for non-length units (percentage, angle, time, etc.).
pub(crate) fn no_calc_length(val: f32, unit: &ArchivedCssUnit) -> Option<NoCalcLength> {
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
        // Not a length unit
        _ => None,
    }
}

/// Converts an [`ArchivedLengthPercentageIR`] to a Stylo [`LengthPercentage`].
pub(crate) fn lp_ir_to_stylo(ir: &ArchivedLengthPercentageIR) -> LengthPercentage {
    match ir {
        ArchivedLengthPercentageIR::Percentage(p) => {
            LengthPercentage::Percentage(Percentage(Into::<f32>::into(*p) / 100.0))
        }
        ArchivedLengthPercentageIR::Length(val, ref unit) => {
            let v: f32 = (*val).into();
            // If the unit isn't a valid length, fall back to 0px.
            let len =
                no_calc_length(v, unit).unwrap_or(NoCalcLength::Absolute(AbsoluteLength::Px(0.0)));
            LengthPercentage::Length(len)
        }
    }
}

/// Converts an [`ArchivedNonNegativeLPIR`] to a Stylo `NonNegative<LengthPercentage>`.
pub(crate) fn nn_lp_ir_to_stylo(ir: &ArchivedNonNegativeLPIR) -> NonNegative<LengthPercentage> {
    match ir {
        ArchivedNonNegativeLPIR::Percentage(p) => NonNegative(LengthPercentage::Percentage(
            Percentage(Into::<f32>::into(*p) / 100.0),
        )),
        ArchivedNonNegativeLPIR::Length(val, ref unit) => {
            let v: f32 = (*val).into();
            let len =
                no_calc_length(v, unit).unwrap_or(NoCalcLength::Absolute(AbsoluteLength::Px(0.0)));
            NonNegative(LengthPercentage::Length(len))
        }
    }
}
