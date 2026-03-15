//! Numeric `PropertyDeclaration` converters.
//!
//! Covers CSS properties whose values are unitless numbers (`<number>`,
//! `<integer>`) or mixed-type values that include a numeric component
//! (e.g. `flex-basis`).  All converters validate ranges / integrality
//! before constructing Stylo types.

use ::style::properties::PropertyDeclaration;
use paws_style_ir::ArchivedCssComponentValue;

use super::helpers::{ir_keyword, ir_to_size, ir_unitless};

// ─── Flexbox numerics ────────────────────────────────────────────────

/// Converts a `flex-grow` value (non-negative `<number>`).
///
/// Rejects negative values so the fallback parser correctly rejects
/// invalid CSS rather than silently wrapping in `NonNegativeNumber`.
pub(super) fn convert_flex_grow(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;
    let v = ir_unitless(values)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexGrow(NonNegativeNumber::new(v)))
}

/// Converts a `flex-shrink` value (non-negative `<number>`).
///
/// Rejects negative values so the fallback parser correctly rejects
/// invalid CSS rather than silently wrapping in `NonNegativeNumber`.
pub(super) fn convert_flex_shrink(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;
    let v = ir_unitless(values)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexShrink(NonNegativeNumber::new(v)))
}

/// Converts a `flex-basis` value (`auto`, `content`, or a size).
///
/// `content` is special-cased; `auto` and length-percentage values are
/// delegated to [`ir_to_size`] which handles both.
pub(super) fn convert_flex_basis(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::values::generics::flex::GenericFlexBasis;
    if ir_keyword(values) == Some("content") {
        return Some(PropertyDeclaration::FlexBasis(Box::new(
            GenericFlexBasis::Content,
        )));
    }
    ir_to_size(values).map(|s| PropertyDeclaration::FlexBasis(Box::new(GenericFlexBasis::Size(s))))
}

// ─── Integer properties ──────────────────────────────────────────────

/// Converts an `order` value (CSS `<integer>`).
///
/// Rejects non-integer floats (e.g. `1.5`) — a CSS `<integer>` must not
/// have a fractional part and a string parser would reject them.
pub(super) fn convert_order(values: &[ArchivedCssComponentValue]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::Integer;
    let v = ir_unitless(values)?;
    if v.fract() != 0.0 {
        return None;
    }
    Some(PropertyDeclaration::Order(Integer::new(v as i32)))
}

/// Converts a `z-index` value (`auto` or CSS `<integer>`).
///
/// Rejects non-integer floats before casting to `i32`.
pub(super) fn convert_z_index(values: &[ArchivedCssComponentValue]) -> Option<PropertyDeclaration> {
    use ::style::values::generics::position::ZIndex;
    use ::style::values::specified::Integer;

    if ir_keyword(values) == Some("auto") {
        return Some(PropertyDeclaration::ZIndex(ZIndex::Auto));
    }
    let v = ir_unitless(values)?;
    // CSS `<integer>` must not have a fractional part.
    if v.fract() != 0.0 {
        return None;
    }
    Some(PropertyDeclaration::ZIndex(ZIndex::Integer(Integer::new(
        v as i32,
    ))))
}
