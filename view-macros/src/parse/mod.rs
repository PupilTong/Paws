use paws_style_ir::{
    CssPropertyIR, CssPropertyName, CssRuleIR, CssUnit, CssWideKeyword, PropertyDeclarationIR,
};

pub mod at_rule;
pub mod decl_only;
pub mod nested_body;
pub mod stylesheet;

pub enum BodyItem {
    Declaration(PropertyDeclarationIR),
    Rule(CssRuleIR),
}

pub struct AtRulePrelude {
    pub name: String,
    pub prelude: String,
}

pub fn collect_tokens_as_string<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> String {
    let position = input.position();
    while input.next().is_ok() {}
    input.slice_from(position).trim().to_string()
}

pub fn partition_body_items(items: Vec<BodyItem>) -> (Vec<PropertyDeclarationIR>, Vec<CssRuleIR>) {
    let mut decls = Vec::new();
    let mut rules = Vec::new();
    for item in items {
        match item {
            BodyItem::Declaration(d) => decls.push(d),
            BodyItem::Rule(r) => rules.push(r),
        }
    }
    (decls, rules)
}

/// Parses a CSS declaration value from cssparser tokens into typed IR.
///
/// Single-token values are converted to typed forms (CssWide, Keyword, Unit).
/// Multi-token or complex values fall back to Unparsed.
fn parse_declaration_value<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> CssPropertyIR {
    let state = input.state();
    let mut ir_value = None;
    let token = input.next().ok().cloned();
    if let Some(token) = token {
        if input.is_exhausted() {
            match token {
                cssparser::Token::Ident(ident) => {
                    let ident_str = ident.as_ref();
                    ir_value = if let Some(wide) = CssWideKeyword::parse(ident_str) {
                        Some(CssPropertyIR::CssWide(wide))
                    } else {
                        Some(CssPropertyIR::Keyword(ident_str.to_string()))
                    };
                }
                cssparser::Token::Dimension { value, unit, .. } => {
                    if let Some(typed_unit) = CssUnit::parse(unit.as_ref()) {
                        ir_value = Some(CssPropertyIR::Unit(value, typed_unit));
                    }
                    // Unknown unit → falls through to Unparsed
                }
                cssparser::Token::Percentage { unit_value, .. } => {
                    ir_value = Some(CssPropertyIR::Unit(unit_value * 100.0, CssUnit::Percent));
                }
                cssparser::Token::Number { value, .. } => {
                    ir_value = Some(CssPropertyIR::Unit(value, CssUnit::Unitless));
                }
                _ => {}
            }
        }
    }

    if let Some(ir) = ir_value {
        ir
    } else {
        input.reset(&state);
        CssPropertyIR::Unparsed(collect_tokens_as_string(input))
    }
}

/// Creates a `PropertyDeclarationIR` from a name and parser input.
pub fn parse_declaration<'i, 't>(
    name: cssparser::CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
) -> PropertyDeclarationIR {
    let name_str = name.as_ref();
    let value = parse_declaration_value(input);
    PropertyDeclarationIR {
        name: CssPropertyName::parse(name_str),
        value,
    }
}
