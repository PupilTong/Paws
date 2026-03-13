//! Converts pre-parsed CSS IR (from `paws-style-ir`) into Stylo types.
//!
//! This module bridges the zero-copy `ArchivedStyleSheetIR` and Stylo's
//! `CssRule` tree. Known properties are dispatched by enum discriminant,
//! eliminating runtime string comparisons.

use ::style::properties::{Importance, PropertyDeclaration, PropertyDeclarationBlock};
use ::style::servo_arc::Arc;
use ::style::shared_lock::SharedRwLock;
use ::style::stylesheets::{CssRule, CssRules, StyleRule, UrlExtraData};
use ::style::values::computed::Percentage;
use ::style::values::generics::NonNegative;
use ::style::values::specified::length::{
    AbsoluteLength, ContainerRelativeLength, FontRelativeLength, LengthPercentage, NoCalcLength,
    ViewportPercentageLength,
};
use ::style::values::specified::Display;
use paws_style_ir::{ArchivedCssPropertyIR, ArchivedCssPropertyName, ArchivedCssUnit};

// ─── Shared helpers ──────────────────────────────────────────────────

/// Converts an IR (value, unit) pair to a Stylo `NoCalcLength`.
///
/// Handles absolute, font-relative, viewport-relative, and container-relative
/// units. Returns `None` for non-length units (e.g. percentage, time, angle).
fn ir_to_no_calc_length(val: f32, unit: &ArchivedCssUnit) -> Option<NoCalcLength> {
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

/// Converts an IR value to a Stylo specified `LengthPercentage`.
///
/// Accepts `Unit(_, Px|Em|Rem|…)` for lengths and `Unit(_, Percent)` for
/// percentages. Returns `None` for keywords or other non-LP values.
fn ir_to_lp(value: &ArchivedCssPropertyIR) -> Option<LengthPercentage> {
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
fn ir_to_nn_lp(value: &ArchivedCssPropertyIR) -> Option<NonNegative<LengthPercentage>> {
    if let ArchivedCssPropertyIR::Unit(val, _) = value {
        let v: f32 = (*val).into();
        if v < 0.0 {
            return None;
        }
    }
    ir_to_lp(value).map(NonNegative)
}

/// Converts an IR value to a Stylo `Size` (for width/height/min-*).
///
/// Handles the `auto` keyword and length-percentage values.
fn ir_to_size(value: &ArchivedCssPropertyIR) -> Option<::style::values::specified::Size> {
    use ::style::values::specified::Size;

    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Size::Auto),
        _ => ir_to_nn_lp(value).map(Size::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `MaxSize` (for max-width/max-height).
///
/// Handles the `none` keyword and length-percentage values.
fn ir_to_max_size(value: &ArchivedCssPropertyIR) -> Option<::style::values::specified::MaxSize> {
    use ::style::values::specified::MaxSize;

    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "none" => Some(MaxSize::None),
        _ => ir_to_nn_lp(value).map(MaxSize::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `Margin` (`auto` or length-percentage).
fn ir_to_margin(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::length::Margin> {
    use ::style::values::specified::length::Margin;

    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Margin::Auto),
        _ => ir_to_lp(value).map(Margin::LengthPercentage),
    }
}

/// Converts an IR value to a Stylo `Inset` (`auto` or length-percentage).
fn ir_to_inset(value: &ArchivedCssPropertyIR) -> Option<::style::values::specified::Inset> {
    use ::style::values::specified::Inset;

    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => Some(Inset::Auto),
        _ => ir_to_lp(value).map(Inset::LengthPercentage),
    }
}

/// Converts an IR keyword value to a Stylo `BorderStyle`.
fn ir_to_border_style(
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
/// Supports the `medium` keyword. Other keywords (`thin`, `thick`) and length
/// values are not yet supported because `BorderSideWidth`'s inner field is
/// module-private in Stylo.
fn ir_to_border_width(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::BorderSideWidth> {
    use ::style::values::specified::BorderSideWidth;

    if ir_keyword(value)? == "medium" {
        Some(BorderSideWidth::medium())
    } else {
        None
    }
}

/// Converts an IR value to a Stylo `NonNegativeLengthPercentageOrNormal` (gap).
fn ir_to_gap(
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

/// Extracts a keyword string from an IR value.
fn ir_keyword(value: &ArchivedCssPropertyIR) -> Option<&str> {
    if let ArchivedCssPropertyIR::Keyword(ref kw) = value {
        Some(kw.as_str())
    } else {
        None
    }
}

/// Extracts a unitless numeric value from an IR value.
fn ir_unitless(value: &ArchivedCssPropertyIR) -> Option<f32> {
    if let ArchivedCssPropertyIR::Unit(val, ArchivedCssUnit::Unitless) = value {
        Some((*val).into())
    } else {
        None
    }
}

// ─── Top-level conversion ────────────────────────────────────────────

/// Converts a slice of archived CSS rules into Stylo `CssRule` values.
pub(crate) fn construct_stylo_rules(
    rules_ir: &rkyv::vec::ArchivedVec<paws_style_ir::ArchivedCssRuleIR>,
    lock: &SharedRwLock,
    url_data: &UrlExtraData,
    _context: &::style::parser::ParserContext,
) -> Vec<CssRule> {
    let mut stylo_rules = Vec::new();
    for rule_ir in rules_ir.iter() {
        match rule_ir {
            paws_style_ir::ArchivedCssRuleIR::Style(s) => {
                if let Some(rule) = convert_style_rule(s, lock, url_data) {
                    stylo_rules.push(rule);
                }
            }
            paws_style_ir::ArchivedCssRuleIR::AtRule(_) => {
                // At-rules not yet supported in the typed path
            }
        }
    }
    stylo_rules
}

/// Converts a single archived style rule into a Stylo `CssRule::Style`.
fn convert_style_rule(
    s: &paws_style_ir::ArchivedStyleRuleIR,
    lock: &SharedRwLock,
    url_data: &UrlExtraData,
) -> Option<CssRule> {
    let sel_str = s.selectors.as_str();
    let selectors = ::style::selector_parser::SelectorParser::parse_author_origin_no_namespace(
        sel_str, url_data,
    )
    .ok()?;

    let mut block = PropertyDeclarationBlock::new();
    for decl in s.declarations.iter() {
        if let Some(prop_decl) = convert_declaration(&decl.name, &decl.value) {
            block.push(prop_decl, Importance::Normal);
        }
    }

    let nested_rules = if s.rules.is_empty() {
        None
    } else {
        let children = construct_stylo_rules(&s.rules, lock, url_data, &{
            // Build a minimal parser context for nested rules
            ::style::parser::ParserContext::new(
                ::style::stylesheets::Origin::Author,
                url_data,
                Some(::style::stylesheets::CssRuleType::Style),
                ::stylo_traits::ParsingMode::DEFAULT,
                ::style::context::QuirksMode::NoQuirks,
                Default::default(),
                None,
                None,
            )
        });
        Some(Arc::new(lock.wrap(CssRules(children))))
    };

    let style_rule = StyleRule {
        selectors,
        block: Arc::new(lock.wrap(block)),
        rules: nested_rules,
        source_location: ::style::values::SourceLocation { line: 0, column: 0 },
    };
    Some(CssRule::Style(Arc::new(lock.wrap(style_rule))))
}

// ─── Per-property dispatch ───────────────────────────────────────────

/// Converts a single IR property declaration to a Stylo `PropertyDeclaration`.
///
/// Known property names are dispatched by enum variant (integer comparison).
/// `Other(s)` falls back to string-based `PropertyId::parse_unchecked`.
fn convert_declaration(
    name: &ArchivedCssPropertyName,
    value: &ArchivedCssPropertyIR,
) -> Option<PropertyDeclaration> {
    match name {
        // ── Display & box model ──────────────────────────────────
        ArchivedCssPropertyName::Display => convert_display(value),
        ArchivedCssPropertyName::BoxSizing => convert_box_sizing(value),

        // ── Sizing ───────────────────────────────────────────────
        ArchivedCssPropertyName::Width => ir_to_size(value).map(PropertyDeclaration::Width),
        ArchivedCssPropertyName::Height => ir_to_size(value).map(PropertyDeclaration::Height),
        ArchivedCssPropertyName::MinWidth => ir_to_size(value).map(PropertyDeclaration::MinWidth),
        ArchivedCssPropertyName::MinHeight => ir_to_size(value).map(PropertyDeclaration::MinHeight),
        ArchivedCssPropertyName::MaxWidth => {
            ir_to_max_size(value).map(PropertyDeclaration::MaxWidth)
        }
        ArchivedCssPropertyName::MaxHeight => {
            ir_to_max_size(value).map(PropertyDeclaration::MaxHeight)
        }

        // ── Margin ───────────────────────────────────────────────
        ArchivedCssPropertyName::MarginTop => {
            ir_to_margin(value).map(PropertyDeclaration::MarginTop)
        }
        ArchivedCssPropertyName::MarginRight => {
            ir_to_margin(value).map(PropertyDeclaration::MarginRight)
        }
        ArchivedCssPropertyName::MarginBottom => {
            ir_to_margin(value).map(PropertyDeclaration::MarginBottom)
        }
        ArchivedCssPropertyName::MarginLeft => {
            ir_to_margin(value).map(PropertyDeclaration::MarginLeft)
        }

        // ── Padding ──────────────────────────────────────────────
        ArchivedCssPropertyName::PaddingTop => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingTop)
        }
        ArchivedCssPropertyName::PaddingRight => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingRight)
        }
        ArchivedCssPropertyName::PaddingBottom => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingBottom)
        }
        ArchivedCssPropertyName::PaddingLeft => {
            ir_to_nn_lp(value).map(PropertyDeclaration::PaddingLeft)
        }

        // ── Border width ─────────────────────────────────────────
        ArchivedCssPropertyName::BorderTopWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderTopWidth)
        }
        ArchivedCssPropertyName::BorderRightWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderRightWidth)
        }
        ArchivedCssPropertyName::BorderBottomWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderBottomWidth)
        }
        ArchivedCssPropertyName::BorderLeftWidth => {
            ir_to_border_width(value).map(PropertyDeclaration::BorderLeftWidth)
        }

        // ── Border style ─────────────────────────────────────────
        ArchivedCssPropertyName::BorderTopStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderTopStyle)
        }
        ArchivedCssPropertyName::BorderRightStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderRightStyle)
        }
        ArchivedCssPropertyName::BorderBottomStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderBottomStyle)
        }
        ArchivedCssPropertyName::BorderLeftStyle => {
            ir_to_border_style(value).map(PropertyDeclaration::BorderLeftStyle)
        }

        // ── Border color (not yet supported — needs color parsing) ──
        ArchivedCssPropertyName::BorderTopColor
        | ArchivedCssPropertyName::BorderRightColor
        | ArchivedCssPropertyName::BorderBottomColor
        | ArchivedCssPropertyName::BorderLeftColor => None,

        // ── Border radius (not yet supported — needs two-component LP) ──
        ArchivedCssPropertyName::BorderTopLeftRadius
        | ArchivedCssPropertyName::BorderTopRightRadius
        | ArchivedCssPropertyName::BorderBottomLeftRadius
        | ArchivedCssPropertyName::BorderBottomRightRadius => None,

        // ── Positioning ──────────────────────────────────────────
        ArchivedCssPropertyName::Position => convert_position(value),
        ArchivedCssPropertyName::Top => ir_to_inset(value).map(PropertyDeclaration::Top),
        ArchivedCssPropertyName::Right => ir_to_inset(value).map(PropertyDeclaration::Right),
        ArchivedCssPropertyName::Bottom => ir_to_inset(value).map(PropertyDeclaration::Bottom),
        ArchivedCssPropertyName::Left => ir_to_inset(value).map(PropertyDeclaration::Left),
        ArchivedCssPropertyName::ZIndex => convert_z_index(value),
        ArchivedCssPropertyName::Float => convert_float(value),
        ArchivedCssPropertyName::Clear => convert_clear(value),

        // ── Flexbox ──────────────────────────────────────────────
        ArchivedCssPropertyName::FlexDirection => convert_flex_direction(value),
        ArchivedCssPropertyName::FlexWrap => convert_flex_wrap(value),
        ArchivedCssPropertyName::FlexGrow => convert_flex_grow(value),
        ArchivedCssPropertyName::FlexShrink => convert_flex_shrink(value),
        ArchivedCssPropertyName::FlexBasis => convert_flex_basis(value),
        ArchivedCssPropertyName::Order => convert_order(value),

        // ── Alignment (not yet supported — needs AlignFlags parsing) ──
        ArchivedCssPropertyName::AlignItems
        | ArchivedCssPropertyName::AlignSelf
        | ArchivedCssPropertyName::AlignContent
        | ArchivedCssPropertyName::JustifyContent
        | ArchivedCssPropertyName::JustifyItems
        | ArchivedCssPropertyName::JustifySelf => None,

        // ── Grid (not yet supported — complex value types) ──
        ArchivedCssPropertyName::GridTemplateColumns
        | ArchivedCssPropertyName::GridTemplateRows
        | ArchivedCssPropertyName::GridAutoFlow
        | ArchivedCssPropertyName::GridAutoColumns
        | ArchivedCssPropertyName::GridAutoRows
        | ArchivedCssPropertyName::GridColumnStart
        | ArchivedCssPropertyName::GridColumnEnd
        | ArchivedCssPropertyName::GridRowStart
        | ArchivedCssPropertyName::GridRowEnd => None,

        // ── Gap ──────────────────────────────────────────────────
        ArchivedCssPropertyName::ColumnGap => ir_to_gap(value).map(PropertyDeclaration::ColumnGap),
        ArchivedCssPropertyName::RowGap => ir_to_gap(value).map(PropertyDeclaration::RowGap),

        // ── Visual ───────────────────────────────────────────────
        // `opacity` uses `specified::Opacity(Number)` whose inner field is
        // module-private in Stylo — cannot construct from outside the crate.
        ArchivedCssPropertyName::Opacity => None,
        ArchivedCssPropertyName::OverflowX => convert_overflow_x(value),
        ArchivedCssPropertyName::OverflowY => convert_overflow_y(value),
        ArchivedCssPropertyName::Overflow => {
            // `overflow` is a shorthand — the parser should expand it to
            // overflow-x/overflow-y before reaching IR. Skip here.
            None
        }
        ArchivedCssPropertyName::Visibility => convert_visibility(value),
        ArchivedCssPropertyName::ObjectFit => convert_object_fit(value),
        ArchivedCssPropertyName::ObjectPosition => None, // Needs two-component position

        // ── Color (not yet supported — needs color parsing) ──
        ArchivedCssPropertyName::Color | ArchivedCssPropertyName::BackgroundColor => None,

        // ── Typography (not yet supported — complex value types) ──
        ArchivedCssPropertyName::FontSize
        | ArchivedCssPropertyName::FontWeight
        | ArchivedCssPropertyName::FontFamily
        | ArchivedCssPropertyName::FontStyle
        | ArchivedCssPropertyName::LineHeight
        | ArchivedCssPropertyName::TextAlign
        | ArchivedCssPropertyName::TextDecoration
        | ArchivedCssPropertyName::TextTransform
        | ArchivedCssPropertyName::LetterSpacing
        | ArchivedCssPropertyName::WordSpacing
        | ArchivedCssPropertyName::WhiteSpace
        | ArchivedCssPropertyName::VerticalAlign => None,

        // ── Aspect ratio (not yet supported — needs ratio parsing) ──
        ArchivedCssPropertyName::AspectRatio => None,

        // ── Catch-all ────────────────────────────────────────────
        ArchivedCssPropertyName::Other(name_str) => convert_by_string(name_str.as_str(), value),
        ArchivedCssPropertyName::Custom(_) => None,
    }
}

// ─── Keyword-based property converters ───────────────────────────────

/// Converts a `display` keyword to a Stylo `PropertyDeclaration::Display`.
fn convert_display(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    let display = match ir_keyword(value)? {
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

/// Converts a `position` keyword.
fn convert_position(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::PositionProperty;

    let pos = match ir_keyword(value)? {
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
fn convert_float(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::Float;

    let f = match ir_keyword(value)? {
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
fn convert_clear(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::box_::Clear;

    let c = match ir_keyword(value)? {
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

/// Converts a `box-sizing` keyword.
fn convert_box_sizing(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::computed_values::box_sizing::T as BoxSizing;

    let bs = match ir_keyword(value)? {
        "content-box" => BoxSizing::ContentBox,
        "border-box" => BoxSizing::BorderBox,
        _ => return None,
    };
    Some(PropertyDeclaration::BoxSizing(bs))
}

/// Converts a `visibility` keyword.
fn convert_visibility(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::computed_values::visibility::T as Visibility;

    let v = match ir_keyword(value)? {
        "visible" => Visibility::Visible,
        "hidden" => Visibility::Hidden,
        "collapse" => Visibility::Collapse,
        _ => return None,
    };
    Some(PropertyDeclaration::Visibility(v))
}

/// Converts an `overflow-x` keyword.
fn convert_overflow_x(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    ir_to_overflow(value).map(PropertyDeclaration::OverflowX)
}

/// Converts an `overflow-y` keyword.
fn convert_overflow_y(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    ir_to_overflow(value).map(PropertyDeclaration::OverflowY)
}

/// Shared helper for overflow-x / overflow-y.
fn ir_to_overflow(
    value: &ArchivedCssPropertyIR,
) -> Option<::style::values::specified::box_::Overflow> {
    use ::style::values::specified::box_::Overflow;

    let o = match ir_keyword(value)? {
        "visible" => Overflow::Visible,
        "hidden" => Overflow::Hidden,
        "scroll" => Overflow::Scroll,
        "auto" => Overflow::Auto,
        "clip" => Overflow::Clip,
        _ => return None,
    };
    Some(o)
}

/// Converts a `flex-direction` keyword.
fn convert_flex_direction(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::computed_values::flex_direction::T as FlexDirection;

    let fd = match ir_keyword(value)? {
        "row" => FlexDirection::Row,
        "row-reverse" => FlexDirection::RowReverse,
        "column" => FlexDirection::Column,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => return None,
    };
    Some(PropertyDeclaration::FlexDirection(fd))
}

/// Converts a `flex-wrap` keyword.
fn convert_flex_wrap(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::computed_values::flex_wrap::T as FlexWrap;

    let fw = match ir_keyword(value)? {
        "nowrap" => FlexWrap::Nowrap,
        "wrap" => FlexWrap::Wrap,
        "wrap-reverse" => FlexWrap::WrapReverse,
        _ => return None,
    };
    Some(PropertyDeclaration::FlexWrap(fw))
}

/// Converts an `object-fit` keyword.
fn convert_object_fit(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::computed_values::object_fit::T as ObjectFit;

    let of = match ir_keyword(value)? {
        "fill" => ObjectFit::Fill,
        "contain" => ObjectFit::Contain,
        "cover" => ObjectFit::Cover,
        "none" => ObjectFit::None,
        "scale-down" => ObjectFit::ScaleDown,
        _ => return None,
    };
    Some(PropertyDeclaration::ObjectFit(of))
}

// ─── Numeric property converters ─────────────────────────────────────

/// Converts a `flex-grow` value (non-negative unitless number).
///
/// Rejects negative values so the fallback string parser can correctly
/// reject them per the CSS spec, rather than wrapping in `NonNegativeNumber`.
fn convert_flex_grow(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;

    let v = ir_unitless(value)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexGrow(NonNegativeNumber::new(v)))
}

/// Converts a `flex-shrink` value (non-negative unitless number).
///
/// Rejects negative values so the fallback string parser can correctly
/// reject them per the CSS spec, rather than wrapping in `NonNegativeNumber`.
fn convert_flex_shrink(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::NonNegativeNumber;

    let v = ir_unitless(value)?;
    if v < 0.0 {
        return None;
    }
    Some(PropertyDeclaration::FlexShrink(NonNegativeNumber::new(v)))
}

/// Converts a `flex-basis` value (`auto`, `content`, or size).
///
/// The `content` keyword is special-cased; `auto` and length-percentage
/// values are handled by `ir_to_size` which already supports both.
fn convert_flex_basis(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::generics::flex::GenericFlexBasis;

    if ir_keyword(value) == Some("content") {
        return Some(PropertyDeclaration::FlexBasis(Box::new(
            GenericFlexBasis::Content,
        )));
    }

    ir_to_size(value).map(|s| PropertyDeclaration::FlexBasis(Box::new(GenericFlexBasis::Size(s))))
}

/// Converts an `order` value (integer).
///
/// Rejects non-integer floats (e.g. `1.5`) since CSS `<integer>` values
/// must not have a fractional part — a string parser would reject them.
fn convert_order(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::specified::Integer;

    let v = ir_unitless(value)?;
    if v.fract() != 0.0 {
        return None;
    }
    Some(PropertyDeclaration::Order(Integer::new(v as i32)))
}

/// Converts a `z-index` value (`auto` or integer).
fn convert_z_index(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    use ::style::values::generics::position::ZIndex;
    use ::style::values::specified::Integer;

    match value {
        ArchivedCssPropertyIR::Keyword(ref kw) if kw.as_str() == "auto" => {
            Some(PropertyDeclaration::ZIndex(ZIndex::Auto))
        }
        ArchivedCssPropertyIR::Unit(val, ArchivedCssUnit::Unitless) => {
            let v: f32 = (*val).into();
            // CSS `<integer>` must not have a fractional part.
            if v.fract() != 0.0 {
                return None;
            }
            Some(PropertyDeclaration::ZIndex(ZIndex::Integer(Integer::new(
                v as i32,
            ))))
        }
        _ => None,
    }
}

// ─── Fallback ────────────────────────────────────────────────────────

/// Fallback: attempt string-based property parsing via Stylo's `PropertyId`.
///
/// Currently returns `None` since the engine's cssparser dependency was removed.
/// As more typed converters are added above, fewer properties reach this path.
fn convert_by_string(_name: &str, _value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    // Unparsed/fallback properties are currently not supported.
    // Add typed match arms in `convert_declaration` to support more properties.
    None
}
