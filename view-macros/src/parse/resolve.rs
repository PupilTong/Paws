//! Compile-time resolution of CSS tokens into typed [`PropertyValueIR`] values.
//!
//! This module is invoked after token-level parsing is complete.  It inspects
//! the property name and token list, producing a typed IR value when possible
//! or falling back to [`PropertyValueIR::Raw`] for unrecognised /
//! not-yet-typed properties.
//!
//! Value validation (non-negative checks, integrality) happens here at
//! compile time rather than at runtime in the engine.

use paws_style_ir::values::*;
use paws_style_ir::{CssPropertyName, CssToken, CssWideKeyword, PropertyValueIR};

/// Resolves a parsed token list into a typed [`PropertyValueIR`].
///
/// Falls back to [`PropertyValueIR::Raw`] when:
/// - The property is not yet typed (color, typography, grid, etc.)
/// - The token list does not match the expected shape
/// - The property name is `Custom` or `Other`
pub fn resolve_typed_value(name: &CssPropertyName, tokens: Vec<CssToken>) -> PropertyValueIR {
    // Handle CSS-wide keywords first — they apply to all properties.
    if let Some(wide) = extract_css_wide(&tokens) {
        return PropertyValueIR::CssWide(wide);
    }

    match name {
        // ── Sizing ──────────────────────────────────────────────
        CssPropertyName::Width
        | CssPropertyName::Height
        | CssPropertyName::MinWidth
        | CssPropertyName::MinHeight => resolve_size(&tokens)
            .map(PropertyValueIR::Size)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::MaxWidth | CssPropertyName::MaxHeight => resolve_max_size(&tokens)
            .map(PropertyValueIR::MaxSize)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Margin ──────────────────────────────────────────────
        CssPropertyName::MarginTop
        | CssPropertyName::MarginRight
        | CssPropertyName::MarginBottom
        | CssPropertyName::MarginLeft => resolve_margin(&tokens)
            .map(PropertyValueIR::Margin)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Padding ─────────────────────────────────────────────
        CssPropertyName::PaddingTop
        | CssPropertyName::PaddingRight
        | CssPropertyName::PaddingBottom
        | CssPropertyName::PaddingLeft => resolve_nn_lp(&tokens)
            .map(PropertyValueIR::Padding)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Border style ────────────────────────────────────────
        CssPropertyName::BorderTopStyle
        | CssPropertyName::BorderRightStyle
        | CssPropertyName::BorderBottomStyle
        | CssPropertyName::BorderLeftStyle => resolve_border_style(&tokens)
            .map(PropertyValueIR::BorderStyle)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Positioning ─────────────────────────────────────────
        CssPropertyName::Top
        | CssPropertyName::Right
        | CssPropertyName::Bottom
        | CssPropertyName::Left => resolve_inset(&tokens)
            .map(PropertyValueIR::Inset)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::ZIndex => resolve_z_index(&tokens)
            .map(PropertyValueIR::ZIndex)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::Position => resolve_position(&tokens)
            .map(PropertyValueIR::Position)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Display & box model ─────────────────────────────────
        CssPropertyName::Display => resolve_display(&tokens)
            .map(PropertyValueIR::Display)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::BoxSizing => resolve_box_sizing(&tokens)
            .map(PropertyValueIR::BoxSizing)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::Float => resolve_float(&tokens)
            .map(PropertyValueIR::Float)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::Clear => resolve_clear(&tokens)
            .map(PropertyValueIR::Clear)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Visual ──────────────────────────────────────────────
        CssPropertyName::Visibility => resolve_visibility(&tokens)
            .map(PropertyValueIR::Visibility)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::OverflowX | CssPropertyName::OverflowY => resolve_overflow(&tokens)
            .map(PropertyValueIR::Overflow)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::ObjectFit => resolve_object_fit(&tokens)
            .map(PropertyValueIR::ObjectFit)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Flexbox ─────────────────────────────────────────────
        CssPropertyName::FlexDirection => resolve_flex_direction(&tokens)
            .map(PropertyValueIR::FlexDirection)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::FlexWrap => resolve_flex_wrap(&tokens)
            .map(PropertyValueIR::FlexWrap)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::FlexGrow => resolve_nn_number(&tokens)
            .map(PropertyValueIR::FlexGrow)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::FlexShrink => resolve_nn_number(&tokens)
            .map(PropertyValueIR::FlexShrink)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::FlexBasis => resolve_flex_basis(&tokens)
            .map(PropertyValueIR::FlexBasis)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        CssPropertyName::Order => resolve_integer(&tokens)
            .map(PropertyValueIR::Order)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Gap ─────────────────────────────────────────────────
        CssPropertyName::ColumnGap | CssPropertyName::RowGap => resolve_gap(&tokens)
            .map(PropertyValueIR::Gap)
            .unwrap_or(PropertyValueIR::Raw(tokens)),

        // ── Everything else → Raw ───────────────────────────────
        _ => PropertyValueIR::Raw(tokens),
    }
}

// ─── CSS-wide keyword extraction ────────────────────────────────────

fn extract_css_wide(tokens: &[CssToken]) -> Option<CssWideKeyword> {
    match tokens {
        [CssToken::Ident(ref kw)] => CssWideKeyword::parse(kw),
        _ => None,
    }
}

// ─── Token extractors ───────────────────────────────────────────────

/// Extracts a single keyword identifier.
fn keyword(tokens: &[CssToken]) -> Option<&str> {
    match tokens {
        [CssToken::Ident(ref s)] => Some(s.as_str()),
        _ => None,
    }
}

// ─── Length / percentage helpers ─────────────────────────────────────

fn resolve_lp(tokens: &[CssToken]) -> Option<LengthPercentageIR> {
    match tokens {
        [CssToken::Percentage(val)] => Some(LengthPercentageIR::Percentage(*val)),
        [CssToken::Dimension(val, ref unit)] if is_length_unit(unit) => {
            Some(LengthPercentageIR::Length(*val, *unit))
        }
        // Bare zero is a valid zero-length.
        [CssToken::Number(val)] if *val == 0.0 => {
            Some(LengthPercentageIR::Length(0.0, paws_style_ir::CssUnit::Px))
        }
        _ => None,
    }
}

fn resolve_nn_lp(tokens: &[CssToken]) -> Option<NonNegativeLPIR> {
    match tokens {
        [CssToken::Percentage(val)] => NonNegativeLPIR::new_percentage(*val),
        [CssToken::Dimension(val, ref unit)] if is_length_unit(unit) => {
            NonNegativeLPIR::new_length(*val, *unit)
        }
        [CssToken::Number(val)] if *val == 0.0 => {
            NonNegativeLPIR::new_length(0.0, paws_style_ir::CssUnit::Px)
        }
        _ => None,
    }
}

fn is_length_unit(unit: &paws_style_ir::CssUnit) -> bool {
    use paws_style_ir::CssUnit;
    !matches!(
        unit,
        CssUnit::Fr
            | CssUnit::Deg
            | CssUnit::Rad
            | CssUnit::Grad
            | CssUnit::Turn
            | CssUnit::S
            | CssUnit::Ms
            | CssUnit::Dpi
            | CssUnit::Dpcm
            | CssUnit::Dppx
    )
}

// ─── Typed property resolvers ───────────────────────────────────────

fn resolve_size(tokens: &[CssToken]) -> Option<SizeIR> {
    if keyword(tokens) == Some("auto") {
        Some(SizeIR::Auto)
    } else {
        resolve_nn_lp(tokens).map(SizeIR::LengthPercentage)
    }
}

fn resolve_max_size(tokens: &[CssToken]) -> Option<MaxSizeIR> {
    if keyword(tokens) == Some("none") {
        Some(MaxSizeIR::None)
    } else {
        resolve_nn_lp(tokens).map(MaxSizeIR::LengthPercentage)
    }
}

fn resolve_margin(tokens: &[CssToken]) -> Option<MarginIR> {
    if keyword(tokens) == Some("auto") {
        Some(MarginIR::Auto)
    } else {
        resolve_lp(tokens).map(MarginIR::LengthPercentage)
    }
}

fn resolve_inset(tokens: &[CssToken]) -> Option<InsetIR> {
    if keyword(tokens) == Some("auto") {
        Some(InsetIR::Auto)
    } else {
        resolve_lp(tokens).map(InsetIR::LengthPercentage)
    }
}

fn resolve_gap(tokens: &[CssToken]) -> Option<GapIR> {
    if keyword(tokens) == Some("normal") {
        Some(GapIR::Normal)
    } else {
        resolve_nn_lp(tokens).map(GapIR::LengthPercentage)
    }
}

fn resolve_border_style(tokens: &[CssToken]) -> Option<BorderStyleIR> {
    match keyword(tokens)? {
        "none" => Some(BorderStyleIR::None),
        "hidden" => Some(BorderStyleIR::Hidden),
        "solid" => Some(BorderStyleIR::Solid),
        "double" => Some(BorderStyleIR::Double),
        "dotted" => Some(BorderStyleIR::Dotted),
        "dashed" => Some(BorderStyleIR::Dashed),
        "groove" => Some(BorderStyleIR::Groove),
        "ridge" => Some(BorderStyleIR::Ridge),
        "inset" => Some(BorderStyleIR::Inset),
        "outset" => Some(BorderStyleIR::Outset),
        _ => None,
    }
}

fn resolve_display(tokens: &[CssToken]) -> Option<DisplayIR> {
    match keyword(tokens)? {
        "block" => Some(DisplayIR::Block),
        "inline" => Some(DisplayIR::Inline),
        "inline-block" => Some(DisplayIR::InlineBlock),
        "none" => Some(DisplayIR::None),
        "flex" => Some(DisplayIR::Flex),
        "grid" => Some(DisplayIR::Grid),
        "table" => Some(DisplayIR::Table),
        "inline-flex" => Some(DisplayIR::InlineFlex),
        "inline-grid" => Some(DisplayIR::InlineGrid),
        "inline-table" => Some(DisplayIR::InlineTable),
        "table-row" => Some(DisplayIR::TableRow),
        "table-cell" => Some(DisplayIR::TableCell),
        "table-column" => Some(DisplayIR::TableColumn),
        "table-row-group" => Some(DisplayIR::TableRowGroup),
        "table-header-group" => Some(DisplayIR::TableHeaderGroup),
        "table-footer-group" => Some(DisplayIR::TableFooterGroup),
        "table-column-group" => Some(DisplayIR::TableColumnGroup),
        "table-caption" => Some(DisplayIR::TableCaption),
        "contents" => Some(DisplayIR::Contents),
        _ => None,
    }
}

fn resolve_position(tokens: &[CssToken]) -> Option<PositionIR> {
    match keyword(tokens)? {
        "static" => Some(PositionIR::Static),
        "relative" => Some(PositionIR::Relative),
        "absolute" => Some(PositionIR::Absolute),
        "fixed" => Some(PositionIR::Fixed),
        "sticky" => Some(PositionIR::Sticky),
        _ => None,
    }
}

fn resolve_box_sizing(tokens: &[CssToken]) -> Option<BoxSizingIR> {
    match keyword(tokens)? {
        "content-box" => Some(BoxSizingIR::ContentBox),
        "border-box" => Some(BoxSizingIR::BorderBox),
        _ => None,
    }
}

fn resolve_float(tokens: &[CssToken]) -> Option<FloatIR> {
    match keyword(tokens)? {
        "none" => Some(FloatIR::None),
        "left" => Some(FloatIR::Left),
        "right" => Some(FloatIR::Right),
        "inline-start" => Some(FloatIR::InlineStart),
        "inline-end" => Some(FloatIR::InlineEnd),
        _ => None,
    }
}

fn resolve_clear(tokens: &[CssToken]) -> Option<ClearIR> {
    match keyword(tokens)? {
        "none" => Some(ClearIR::None),
        "left" => Some(ClearIR::Left),
        "right" => Some(ClearIR::Right),
        "both" => Some(ClearIR::Both),
        "inline-start" => Some(ClearIR::InlineStart),
        "inline-end" => Some(ClearIR::InlineEnd),
        _ => None,
    }
}

fn resolve_visibility(tokens: &[CssToken]) -> Option<VisibilityIR> {
    match keyword(tokens)? {
        "visible" => Some(VisibilityIR::Visible),
        "hidden" => Some(VisibilityIR::Hidden),
        "collapse" => Some(VisibilityIR::Collapse),
        _ => None,
    }
}

fn resolve_overflow(tokens: &[CssToken]) -> Option<OverflowIR> {
    match keyword(tokens)? {
        "visible" => Some(OverflowIR::Visible),
        "hidden" => Some(OverflowIR::Hidden),
        "scroll" => Some(OverflowIR::Scroll),
        "auto" => Some(OverflowIR::Auto),
        "clip" => Some(OverflowIR::Clip),
        _ => None,
    }
}

fn resolve_object_fit(tokens: &[CssToken]) -> Option<ObjectFitIR> {
    match keyword(tokens)? {
        "fill" => Some(ObjectFitIR::Fill),
        "contain" => Some(ObjectFitIR::Contain),
        "cover" => Some(ObjectFitIR::Cover),
        "none" => Some(ObjectFitIR::None),
        "scale-down" => Some(ObjectFitIR::ScaleDown),
        _ => None,
    }
}

fn resolve_flex_direction(tokens: &[CssToken]) -> Option<FlexDirectionIR> {
    match keyword(tokens)? {
        "row" => Some(FlexDirectionIR::Row),
        "row-reverse" => Some(FlexDirectionIR::RowReverse),
        "column" => Some(FlexDirectionIR::Column),
        "column-reverse" => Some(FlexDirectionIR::ColumnReverse),
        _ => None,
    }
}

fn resolve_flex_wrap(tokens: &[CssToken]) -> Option<FlexWrapIR> {
    match keyword(tokens)? {
        "nowrap" => Some(FlexWrapIR::Nowrap),
        "wrap" => Some(FlexWrapIR::Wrap),
        "wrap-reverse" => Some(FlexWrapIR::WrapReverse),
        _ => None,
    }
}

fn resolve_flex_basis(tokens: &[CssToken]) -> Option<FlexBasisIR> {
    if keyword(tokens) == Some("content") {
        Some(FlexBasisIR::Content)
    } else {
        resolve_size(tokens).map(FlexBasisIR::Size)
    }
}

fn resolve_nn_number(tokens: &[CssToken]) -> Option<NonNegativeNumberIR> {
    match tokens {
        [CssToken::Number(val)] => NonNegativeNumberIR::new(*val),
        _ => None,
    }
}

fn resolve_integer(tokens: &[CssToken]) -> Option<IntegerIR> {
    match tokens {
        [CssToken::Number(val)] => IntegerIR::from_f32(*val),
        _ => None,
    }
}

fn resolve_z_index(tokens: &[CssToken]) -> Option<ZIndexIR> {
    if keyword(tokens) == Some("auto") {
        Some(ZIndexIR::Auto)
    } else {
        resolve_integer(tokens).map(ZIndexIR::Integer)
    }
}
