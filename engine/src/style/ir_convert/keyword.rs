//! Keyword-based `PropertyDeclaration` converters.
//!
//! **Typed IR path**: infallible enum-to-enum maps from `Archived*IR` →
//! Stylo types.
//!
//! **Raw fallback path**: string-matching converters for
//! `PropertyValueIR::Raw` tokens.

use ::style::properties::PropertyDeclaration;
use ::style::values::specified::Display;
use paws_style_ir::{
    ArchivedBorderStyleIR, ArchivedBoxSizingIR, ArchivedClearIR, ArchivedCssToken,
    ArchivedDisplayIR, ArchivedFloatIR, ArchivedObjectFitIR, ArchivedOverflowIR,
    ArchivedPositionIR, ArchivedVisibilityIR,
};

use super::helpers::ir_keyword;

// ═════════════════════════════════════════════════════════════════════
// Typed IR → Stylo (infallible)
// ═════════════════════════════════════════════════════════════════════

/// Converts an [`ArchivedDisplayIR`] to a Stylo `Display`.
pub(crate) fn display_ir_to_stylo(ir: &ArchivedDisplayIR) -> Display {
    match ir {
        ArchivedDisplayIR::Block => Display::Block,
        ArchivedDisplayIR::Inline => Display::Inline,
        ArchivedDisplayIR::InlineBlock => Display::InlineBlock,
        ArchivedDisplayIR::None => Display::None,
        ArchivedDisplayIR::Flex => Display::Flex,
        ArchivedDisplayIR::Grid => Display::Grid,
        ArchivedDisplayIR::Table => Display::Table,
        ArchivedDisplayIR::InlineFlex => Display::InlineFlex,
        ArchivedDisplayIR::InlineGrid => Display::InlineGrid,
        ArchivedDisplayIR::InlineTable => Display::InlineTable,
        ArchivedDisplayIR::TableRow => Display::TableRow,
        ArchivedDisplayIR::TableCell => Display::TableCell,
        ArchivedDisplayIR::TableColumn => Display::TableColumn,
        ArchivedDisplayIR::TableRowGroup => Display::TableRowGroup,
        ArchivedDisplayIR::TableHeaderGroup => Display::TableHeaderGroup,
        ArchivedDisplayIR::TableFooterGroup => Display::TableFooterGroup,
        ArchivedDisplayIR::TableColumnGroup => Display::TableColumnGroup,
        ArchivedDisplayIR::TableCaption => Display::TableCaption,
        ArchivedDisplayIR::Contents => Display::Contents,
    }
}

/// Converts an [`ArchivedBoxSizingIR`] to a Stylo `BoxSizing`.
pub(crate) fn box_sizing_ir_to_stylo(
    ir: &ArchivedBoxSizingIR,
) -> ::style::computed_values::box_sizing::T {
    use ::style::computed_values::box_sizing::T as BoxSizing;
    match ir {
        ArchivedBoxSizingIR::ContentBox => BoxSizing::ContentBox,
        ArchivedBoxSizingIR::BorderBox => BoxSizing::BorderBox,
    }
}

/// Converts an [`ArchivedPositionIR`] to a Stylo `PositionProperty`.
pub(crate) fn position_ir_to_stylo(
    ir: &ArchivedPositionIR,
) -> ::style::values::specified::box_::PositionProperty {
    use ::style::values::specified::box_::PositionProperty;
    match ir {
        ArchivedPositionIR::Static => PositionProperty::Static,
        ArchivedPositionIR::Relative => PositionProperty::Relative,
        ArchivedPositionIR::Absolute => PositionProperty::Absolute,
        ArchivedPositionIR::Fixed => PositionProperty::Fixed,
        ArchivedPositionIR::Sticky => PositionProperty::Sticky,
    }
}

/// Converts an [`ArchivedFloatIR`] to a Stylo `Float`.
pub(crate) fn float_ir_to_stylo(ir: &ArchivedFloatIR) -> ::style::values::specified::box_::Float {
    use ::style::values::specified::box_::Float;
    match ir {
        ArchivedFloatIR::None => Float::None,
        ArchivedFloatIR::Left => Float::Left,
        ArchivedFloatIR::Right => Float::Right,
        ArchivedFloatIR::InlineStart => Float::InlineStart,
        ArchivedFloatIR::InlineEnd => Float::InlineEnd,
    }
}

/// Converts an [`ArchivedClearIR`] to a Stylo `Clear`.
pub(crate) fn clear_ir_to_stylo(ir: &ArchivedClearIR) -> ::style::values::specified::box_::Clear {
    use ::style::values::specified::box_::Clear;
    match ir {
        ArchivedClearIR::None => Clear::None,
        ArchivedClearIR::Left => Clear::Left,
        ArchivedClearIR::Right => Clear::Right,
        ArchivedClearIR::Both => Clear::Both,
        ArchivedClearIR::InlineStart => Clear::InlineStart,
        ArchivedClearIR::InlineEnd => Clear::InlineEnd,
    }
}

/// Converts an [`ArchivedVisibilityIR`] to a Stylo `Visibility`.
pub(crate) fn visibility_ir_to_stylo(
    ir: &ArchivedVisibilityIR,
) -> ::style::computed_values::visibility::T {
    use ::style::computed_values::visibility::T as Visibility;
    match ir {
        ArchivedVisibilityIR::Visible => Visibility::Visible,
        ArchivedVisibilityIR::Hidden => Visibility::Hidden,
        ArchivedVisibilityIR::Collapse => Visibility::Collapse,
    }
}

/// Converts an [`ArchivedOverflowIR`] to a Stylo `Overflow`.
pub(crate) fn overflow_ir_to_stylo(
    ir: &ArchivedOverflowIR,
) -> ::style::values::specified::box_::Overflow {
    use ::style::values::specified::box_::Overflow;
    match ir {
        ArchivedOverflowIR::Visible => Overflow::Visible,
        ArchivedOverflowIR::Hidden => Overflow::Hidden,
        ArchivedOverflowIR::Scroll => Overflow::Scroll,
        ArchivedOverflowIR::Auto => Overflow::Auto,
        ArchivedOverflowIR::Clip => Overflow::Clip,
    }
}

/// Converts an [`ArchivedObjectFitIR`] to a Stylo `ObjectFit`.
pub(crate) fn object_fit_ir_to_stylo(
    ir: &ArchivedObjectFitIR,
) -> ::style::computed_values::object_fit::T {
    use ::style::computed_values::object_fit::T as ObjectFit;
    match ir {
        ArchivedObjectFitIR::Fill => ObjectFit::Fill,
        ArchivedObjectFitIR::Contain => ObjectFit::Contain,
        ArchivedObjectFitIR::Cover => ObjectFit::Cover,
        ArchivedObjectFitIR::None => ObjectFit::None,
        ArchivedObjectFitIR::ScaleDown => ObjectFit::ScaleDown,
    }
}

/// Converts an [`ArchivedBorderStyleIR`] to a Stylo `BorderStyle`.
pub(crate) fn border_style_ir_to_stylo(
    ir: &ArchivedBorderStyleIR,
) -> ::style::values::specified::BorderStyle {
    use ::style::values::specified::BorderStyle;
    match ir {
        ArchivedBorderStyleIR::None => BorderStyle::None,
        ArchivedBorderStyleIR::Hidden => BorderStyle::Hidden,
        ArchivedBorderStyleIR::Solid => BorderStyle::Solid,
        ArchivedBorderStyleIR::Double => BorderStyle::Double,
        ArchivedBorderStyleIR::Dotted => BorderStyle::Dotted,
        ArchivedBorderStyleIR::Dashed => BorderStyle::Dashed,
        ArchivedBorderStyleIR::Groove => BorderStyle::Groove,
        ArchivedBorderStyleIR::Ridge => BorderStyle::Ridge,
        ArchivedBorderStyleIR::Inset => BorderStyle::Inset,
        ArchivedBorderStyleIR::Outset => BorderStyle::Outset,
    }
}

/// Converts an [`ArchivedFlexDirectionIR`] to a Stylo `FlexDirection`.
pub(crate) fn flex_direction_ir_to_stylo(
    ir: &paws_style_ir::ArchivedFlexDirectionIR,
) -> ::style::computed_values::flex_direction::T {
    use ::style::computed_values::flex_direction::T as FlexDirection;
    match ir {
        paws_style_ir::ArchivedFlexDirectionIR::Row => FlexDirection::Row,
        paws_style_ir::ArchivedFlexDirectionIR::RowReverse => FlexDirection::RowReverse,
        paws_style_ir::ArchivedFlexDirectionIR::Column => FlexDirection::Column,
        paws_style_ir::ArchivedFlexDirectionIR::ColumnReverse => FlexDirection::ColumnReverse,
    }
}

/// Converts an [`ArchivedFlexWrapIR`] to a Stylo `FlexWrap`.
pub(crate) fn flex_wrap_ir_to_stylo(
    ir: &paws_style_ir::ArchivedFlexWrapIR,
) -> ::style::computed_values::flex_wrap::T {
    use ::style::computed_values::flex_wrap::T as FlexWrap;
    match ir {
        paws_style_ir::ArchivedFlexWrapIR::Nowrap => FlexWrap::Nowrap,
        paws_style_ir::ArchivedFlexWrapIR::Wrap => FlexWrap::Wrap,
        paws_style_ir::ArchivedFlexWrapIR::WrapReverse => FlexWrap::WrapReverse,
    }
}

// ═════════════════════════════════════════════════════════════════════
// Raw token fallback converters
// ═════════════════════════════════════════════════════════════════════

/// Converts a `display` keyword (Raw fallback).
pub(super) fn convert_display(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    let display = match ir_keyword(values)? {
        "block" => Display::Block,
        "inline" => Display::Inline,
        "inline-block" => Display::InlineBlock,
        "none" => Display::None,
        "flex" => Display::Flex,
        "grid" => Display::Grid,
        "table" => Display::Table,
        "inline-flex" => Display::InlineFlex,
        "inline-grid" => Display::InlineGrid,
        "inline-table" => Display::InlineTable,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "table-column" => Display::TableColumn,
        "table-row-group" => Display::TableRowGroup,
        "table-header-group" => Display::TableHeaderGroup,
        "table-footer-group" => Display::TableFooterGroup,
        "table-column-group" => Display::TableColumnGroup,
        "table-caption" => Display::TableCaption,
        "contents" => Display::Contents,
        _ => return None,
    };
    Some(PropertyDeclaration::Display(display))
}

/// Converts a `box-sizing` keyword (Raw fallback).
pub(super) fn convert_box_sizing(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::computed_values::box_sizing::T as BoxSizing;
    let bs = match ir_keyword(values)? {
        "content-box" => BoxSizing::ContentBox,
        "border-box" => BoxSizing::BorderBox,
        _ => return None,
    };
    Some(PropertyDeclaration::BoxSizing(bs))
}

/// Converts a `position` keyword (Raw fallback).
pub(super) fn convert_position(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::PositionProperty;
    let pos = match ir_keyword(values)? {
        "static" => PositionProperty::Static,
        "relative" => PositionProperty::Relative,
        "absolute" => PositionProperty::Absolute,
        "fixed" => PositionProperty::Fixed,
        "sticky" => PositionProperty::Sticky,
        _ => return None,
    };
    Some(PropertyDeclaration::Position(pos))
}

/// Converts a `float` keyword (Raw fallback).
pub(super) fn convert_float(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::Float;
    let f = match ir_keyword(values)? {
        "none" => Float::None,
        "left" => Float::Left,
        "right" => Float::Right,
        "inline-start" => Float::InlineStart,
        "inline-end" => Float::InlineEnd,
        _ => return None,
    };
    Some(PropertyDeclaration::Float(f))
}

/// Converts a `clear` keyword (Raw fallback).
pub(super) fn convert_clear(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::Clear;
    let c = match ir_keyword(values)? {
        "none" => Clear::None,
        "left" => Clear::Left,
        "right" => Clear::Right,
        "both" => Clear::Both,
        "inline-start" => Clear::InlineStart,
        "inline-end" => Clear::InlineEnd,
        _ => return None,
    };
    Some(PropertyDeclaration::Clear(c))
}

/// Converts a `visibility` keyword (Raw fallback).
pub(super) fn convert_visibility(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::computed_values::visibility::T as Visibility;
    let v = match ir_keyword(values)? {
        "visible" => Visibility::Visible,
        "hidden" => Visibility::Hidden,
        "collapse" => Visibility::Collapse,
        _ => return None,
    };
    Some(PropertyDeclaration::Visibility(v))
}

/// Converts an `overflow-x` keyword (Raw fallback).
pub(super) fn convert_overflow_x(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    ir_to_overflow(values).map(PropertyDeclaration::OverflowX)
}

/// Converts an `overflow-y` keyword (Raw fallback).
pub(super) fn convert_overflow_y(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    ir_to_overflow(values).map(PropertyDeclaration::OverflowY)
}

fn ir_to_overflow(
    values: &[ArchivedCssToken],
) -> Option<::style::values::specified::box_::Overflow> {
    use ::style::values::specified::box_::Overflow;
    let o = match ir_keyword(values)? {
        "visible" => Overflow::Visible,
        "hidden" => Overflow::Hidden,
        "scroll" => Overflow::Scroll,
        "auto" => Overflow::Auto,
        "clip" => Overflow::Clip,
        _ => return None,
    };
    Some(o)
}

/// Converts an `object-fit` keyword (Raw fallback).
pub(super) fn convert_object_fit(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::computed_values::object_fit::T as ObjectFit;
    let of = match ir_keyword(values)? {
        "fill" => ObjectFit::Fill,
        "contain" => ObjectFit::Contain,
        "cover" => ObjectFit::Cover,
        "none" => ObjectFit::None,
        "scale-down" => ObjectFit::ScaleDown,
        _ => return None,
    };
    Some(PropertyDeclaration::ObjectFit(of))
}

/// Converts a `flex-direction` keyword (Raw fallback).
pub(super) fn convert_flex_direction(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::computed_values::flex_direction::T as FlexDirection;
    let fd = match ir_keyword(values)? {
        "row" => FlexDirection::Row,
        "row-reverse" => FlexDirection::RowReverse,
        "column" => FlexDirection::Column,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => return None,
    };
    Some(PropertyDeclaration::FlexDirection(fd))
}

/// Converts a `flex-wrap` keyword (Raw fallback).
pub(super) fn convert_flex_wrap(values: &[ArchivedCssToken]) -> Option<PropertyDeclaration> {
    use ::style::computed_values::flex_wrap::T as FlexWrap;
    let fw = match ir_keyword(values)? {
        "nowrap" => FlexWrap::Nowrap,
        "wrap" => FlexWrap::Wrap,
        "wrap-reverse" => FlexWrap::WrapReverse,
        _ => return None,
    };
    Some(PropertyDeclaration::FlexWrap(fw))
}
