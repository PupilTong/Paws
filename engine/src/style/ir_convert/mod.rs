//! Converts pre-parsed CSS IR (from `paws-style-ir`) into Stylo types.
//!
//! This module bridges the zero-copy [`ArchivedStyleSheetIR`] and Stylo's
//! [`CssRule`] tree.  Property conversions are dispatched by enum
//! discriminant, eliminating runtime string comparisons.
//!
//! # Sub-modules
//!
//! | Module         | Contents |
//! |----------------|----------|
//! | [`helpers`]    | Shared IR → Stylo value primitives (`ir_to_lp`, `ir_to_size`, …) |
//! | [`keyword`]    | Keyword-only property converters (`display`, `position`, …) |
//! | [`numeric`]    | Numeric property converters (`flex-grow`, `z-index`, …) |

mod helpers;
mod keyword;
mod numeric;

use ::style::properties::{Importance, PropertyDeclaration, PropertyDeclarationBlock};
use ::style::servo_arc::Arc;
use ::style::shared_lock::SharedRwLock;
use ::style::stylesheets::{CssRule, CssRules, StyleRule, UrlExtraData};
use paws_style_ir::{ArchivedCssComponentValue, ArchivedCssPropertyName};

use helpers::{
    ir_to_border_style, ir_to_border_width, ir_to_gap, ir_to_inset, ir_to_margin, ir_to_max_size,
    ir_to_nn_lp, ir_to_size,
};
use keyword::{
    convert_box_sizing, convert_clear, convert_display, convert_flex_direction, convert_flex_wrap,
    convert_float, convert_object_fit, convert_overflow_x, convert_overflow_y, convert_position,
    convert_visibility,
};
use numeric::{
    convert_flex_basis, convert_flex_grow, convert_flex_shrink, convert_order, convert_z_index,
};

// ─── Public API ──────────────────────────────────────────────────────

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

// ─── Rule conversion ─────────────────────────────────────────────────

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
        let importance = if decl.important {
            Importance::Important
        } else {
            Importance::Normal
        };
        if let Some(prop_decl) = convert_declaration(&decl.name, &decl.value) {
            block.push(prop_decl, importance);
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

// ─── Property dispatch ───────────────────────────────────────────────

/// Converts a single IR property declaration to a Stylo `PropertyDeclaration`.
///
/// Known property names are dispatched by enum variant (integer comparison).
/// `Other(s)` falls back to string-based parsing via [`convert_by_string`].
fn convert_declaration(
    name: &ArchivedCssPropertyName,
    value: &[ArchivedCssComponentValue],
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
            // overflow-x/overflow-y before reaching IR.
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
        ArchivedCssPropertyName::Other(_) | ArchivedCssPropertyName::Custom(_) => None,
    }
}
