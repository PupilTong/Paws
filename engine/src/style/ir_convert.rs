//! Converts pre-parsed CSS IR (from `paws-style-ir`) into Stylo types.
//!
//! This module bridges the zero-copy `ArchivedStyleSheetIR` and Stylo's
//! `CssRule` tree. Known properties are dispatched by enum discriminant,
//! eliminating runtime string comparisons.

use ::style::properties::{Importance, PropertyDeclaration, PropertyDeclarationBlock};
use ::style::servo_arc::Arc;
use ::style::shared_lock::SharedRwLock;
use ::style::stylesheets::{CssRule, CssRules, StyleRule, UrlExtraData};
use ::style::values::specified::Display;
use paws_style_ir::{ArchivedCssPropertyIR, ArchivedCssPropertyName, ArchivedCssUnit};

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

/// Converts a single IR property declaration to a Stylo `PropertyDeclaration`.
///
/// Known property names are dispatched by enum variant (integer comparison).
/// `Other(s)` falls back to string-based `PropertyId::parse_unchecked`.
fn convert_declaration(
    name: &ArchivedCssPropertyName,
    value: &ArchivedCssPropertyIR,
) -> Option<PropertyDeclaration> {
    match name {
        ArchivedCssPropertyName::Display => convert_display(value),
        ArchivedCssPropertyName::Width => convert_width(value),
        // Known properties without typed converters yet — fall through to string path
        ArchivedCssPropertyName::Other(name_str) => convert_by_string(name_str.as_str(), value),
        ArchivedCssPropertyName::Custom(_) => {
            // Custom properties not yet supported
            None
        }
        // All other known enum variants: fall through to string-based path
        _ => convert_by_string(name.as_str(), value),
    }
}

/// Converts a `display` keyword to a Stylo `PropertyDeclaration::Display`.
fn convert_display(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    if let ArchivedCssPropertyIR::Keyword(ref val) = value {
        let display = match val.as_str() {
            "block" => Display::Block,
            "inline" => Display::Inline,
            "inline-block" => Display::InlineBlock,
            "none" => Display::None,
            "flex" => Display::Flex,
            "grid" => Display::Grid,
            _ => return None,
        };
        Some(PropertyDeclaration::Display(display))
    } else {
        None
    }
}

/// Converts a `width` value (px or %) to a Stylo `PropertyDeclaration::Width`.
fn convert_width(value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    if let ArchivedCssPropertyIR::Unit(val, ref unit) = value {
        use ::style::values::computed::Percentage;
        use ::style::values::generics::NonNegative;
        use ::style::values::specified::length::{LengthPercentage, NoCalcLength};
        use ::style::values::specified::Size;

        let lp = match unit {
            ArchivedCssUnit::Px => LengthPercentage::Length(NoCalcLength::from_px((*val).into())),
            ArchivedCssUnit::Percent => LengthPercentage::Percentage(Percentage(*val / 100.0)),
            _ => return None,
        };
        Some(PropertyDeclaration::Width(Size::LengthPercentage(
            NonNegative(lp),
        )))
    } else {
        None
    }
}

/// Fallback: attempt string-based property parsing via Stylo's `PropertyId`.
///
/// Currently returns `None` since the engine's cssparser dependency was removed.
/// As more typed converters are added above, fewer properties reach this path.
fn convert_by_string(_name: &str, _value: &ArchivedCssPropertyIR) -> Option<PropertyDeclaration> {
    // Unparsed/fallback properties are currently not supported.
    // Add typed match arms in `convert_declaration` to support more properties.
    None
}
