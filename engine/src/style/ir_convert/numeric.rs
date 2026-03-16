//! Numeric `PropertyDeclaration` converters.
//!
//! **Typed IR path**: infallible conversions from validated IR types.
//!
//! **Raw fallback path**: string-matching converters with runtime validation
//! for `PropertyValueIR::Raw` tokens.

use ::style::properties::PropertyDeclaration;
use paws_style_ir::{
    ArchivedCssToken, ArchivedFlexBasisIR, ArchivedIntegerIR, ArchivedNonNegativeNumberIR,
    ArchivedZIndexIR,
};

use super::helpers::{ir_keyword, ir_to_size, ir_unitless, size_ir_to_stylo};

// ═════════════════════════════════════════════════════════════════════
// Typed IR → Stylo (infallible)
// ═════════════════════════════════════════════════════════════════════

/// Converts an [`ArchivedNonNegativeNumberIR`] to a Stylo `NonNegativeNumber`.
pub(crate) fn nn_number_ir_to_stylo(
    ir: &ArchivedNonNegativeNumberIR,
) -> ::style::values::specified::NonNegativeNumber {
    ::style::values::specified::NonNegativeNumber::new(ir.0.into())
}

/// Converts an [`ArchivedIntegerIR`] to a Stylo `Integer`.
pub(crate) fn integer_ir_to_stylo(ir: &ArchivedIntegerIR) -> ::style::values::specified::Integer {
    ::style::values::specified::Integer::new(ir.0.into())
}

/// Converts an [`ArchivedZIndexIR`] to a Stylo `ZIndex`.
pub(crate) fn z_index_ir_to_stylo(
    ir: &ArchivedZIndexIR,
) -> ::style::values::generics::position::ZIndex<::style::values::specified::Integer> {
    use ::style::values::generics::position::ZIndex;
    match ir {
        ArchivedZIndexIR::Auto => ZIndex::Auto,
        ArchivedZIndexIR::Integer(ref i) => ZIndex::Integer(integer_ir_to_stylo(i)),
    }
}

/// Converts an [`ArchivedFlexBasisIR`] to a Stylo `FlexBasis`.
pub(crate) fn flex_basis_ir_to_stylo(
    ir: &ArchivedFlexBasisIR,
) -> ::style::values::generics::flex::GenericFlexBasis<::style::values::specified::Size> {
    use ::style::values::generics::flex::GenericFlexBasis;
    match ir {
        ArchivedFlexBasisIR::Content => GenericFlexBasis::Content,
        ArchivedFlexBasisIR::Size(ref s) => GenericFlexBasis::Size(size_ir_to_stylo(s)),
    }
}

// ═════════════════════════════════════════════════════════════════════
// Raw token fallback converters
// ═════════════════════════════════════════════════════════════════════

/// Converts a `flex-grow` value (Raw fallback).
pub(super) fn convert_flex_grow(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;
    let v = ir_unitless(values)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexGrow(NonNegativeNumber::new(v)))
}

/// Converts a `flex-shrink` value (Raw fallback).
pub(super) fn convert_flex_shrink(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;
    let v = ir_unitless(values)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexShrink(NonNegativeNumber::new(v)))
}

/// Converts a `flex-basis` value (Raw fallback).
pub(super) fn convert_flex_basis(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::generics::flex::GenericFlexBasis;
    if ir_keyword(values) == Some("content") {
        return Some(PropertyDeclaration::FlexBasis(Box::new(
            GenericFlexBasis::Content,
        )));
    }
    ir_to_size(values).map(|s| PropertyDeclaration::FlexBasis(Box::new(GenericFlexBasis::Size(s))))
}

/// Converts an `order` value (Raw fallback).
pub(super) fn convert_order(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::Integer;
    let v = ir_unitless(values)?;
    if v.fract() != 0.0 {
        return None;
    }
    Some(PropertyDeclaration::Order(Integer::new(v as i32)))
}

/// Converts a `z-index` value (Raw fallback).
pub(super) fn convert_z_index(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::generics::position::ZIndex;
    use ::style::values::specified::Integer;

    if ir_keyword(values) == Some("auto") {
        return Some(PropertyDeclaration::ZIndex(ZIndex::Auto));
    }
    let v = ir_unitless(values)?;
    if v.fract() != 0.0 {
        return None;
    }
    Some(PropertyDeclaration::ZIndex(ZIndex::Integer(Integer::new(
        v as i32,
    ))))
}
