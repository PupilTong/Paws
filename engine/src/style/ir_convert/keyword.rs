//! Keyword-based `PropertyDeclaration` converters.
//!
//! Each function in this module converts a single CSS property whose value
//! is always (or primarily) a CSS keyword.  Mixed-type properties that accept
//! both keywords and lengths live in [`super::helpers`] or
//! [`super::numeric`] as appropriate.

use ::style::properties::PropertyDeclaration;
use ::style::values::specified::Display;
use paws_style_ir::ArchivedCssComponentValue;

use super::helpers::ir_keyword;

// ─── Box model ───────────────────────────────────────────────────────

/// Converts a `display` keyword.
pub(super) fn convert_display(values: &[ArchivedCssComponentValue]) -> Option<PropertyDeclaration> {
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
        // `list-item` requires combining DisplayOutside + LIST_ITEM_MASK;
        // not yet expressible via Display's public API.
        _ => return None,
    };
    Some(PropertyDeclaration::Display(display))
}

/// Converts a `box-sizing` keyword.
pub(super) fn convert_box_sizing(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::computed_values::box_sizing::T as BoxSizing;
    let bs = match ir_keyword(values)? {
        "content-box" => BoxSizing::ContentBox,
        "border-box" => BoxSizing::BorderBox,
        _ => return None,
    };
    Some(PropertyDeclaration::BoxSizing(bs))
}

// ─── Positioning ─────────────────────────────────────────────────────

/// Converts a `position` keyword.
pub(super) fn convert_position(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
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

/// Converts a `float` keyword.
pub(super) fn convert_float(values: &[ArchivedCssComponentValue]) -> Option<PropertyDeclaration> {
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

/// Converts a `clear` keyword.
pub(super) fn convert_clear(values: &[ArchivedCssComponentValue]) -> Option<PropertyDeclaration> {
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

// ─── Visual ──────────────────────────────────────────────────────────

/// Converts a `visibility` keyword.
pub(super) fn convert_visibility(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::computed_values::visibility::T as Visibility;
    let v = match ir_keyword(values)? {
        "visible" => Visibility::Visible,
        "hidden" => Visibility::Hidden,
        "collapse" => Visibility::Collapse,
        _ => return None,
    };
    Some(PropertyDeclaration::Visibility(v))
}

/// Converts an `overflow-x` keyword.
pub(super) fn convert_overflow_x(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    ir_to_overflow(values).map(PropertyDeclaration::OverflowX)
}

/// Converts an `overflow-y` keyword.
pub(super) fn convert_overflow_y(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    ir_to_overflow(values).map(PropertyDeclaration::OverflowY)
}

/// Shared helper for `overflow-x` / `overflow-y`.
fn ir_to_overflow(
    values: &[ArchivedCssComponentValue],
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

/// Converts an `object-fit` keyword.
pub(super) fn convert_object_fit(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
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

// ─── Flexbox keywords ────────────────────────────────────────────────

/// Converts a `flex-direction` keyword.
pub(super) fn convert_flex_direction(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
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

/// Converts a `flex-wrap` keyword.
pub(super) fn convert_flex_wrap(
    values: &[ArchivedCssComponentValue],
) -> Option<PropertyDeclaration> {
    use ::style::computed_values::flex_wrap::T as FlexWrap;
    let fw = match ir_keyword(values)? {
        "nowrap" => FlexWrap::Nowrap,
        "wrap" => FlexWrap::Wrap,
        "wrap-reverse" => FlexWrap::WrapReverse,
        _ => return None,
    };
    Some(PropertyDeclaration::FlexWrap(fw))
}
