use paws_style_ir::{
    CssComponentValue, CssPropertyName, CssRuleIR, CssUnit, CssWideKeyword, PropertyDeclarationIR,
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

/// Recursively parses cssparser tokens into a list of `CssComponentValue`.
///
/// Maps each cssparser token to the corresponding IR variant. Function tokens
/// are parsed recursively via `parse_nested_block`.
fn parse_component_values<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> Vec<CssComponentValue> {
    let mut values = Vec::new();
    loop {
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            cssparser::Token::Ident(ref ident) => {
                let s = ident.as_ref();
                if let Some(wide) = CssWideKeyword::parse(s) {
                    values.push(CssComponentValue::CssWide(wide));
                } else {
                    values.push(CssComponentValue::Ident(s.to_string()));
                }
            }
            cssparser::Token::Dimension {
                value, ref unit, ..
            } => {
                if let Some(typed_unit) = CssUnit::parse(unit.as_ref()) {
                    values.push(CssComponentValue::Number(value, typed_unit));
                } else {
                    // Unknown unit: store as unparsed dimension text
                    values.push(CssComponentValue::Unparsed(format!("{}{}", value, unit)));
                }
            }
            cssparser::Token::Percentage { unit_value, .. } => {
                values.push(CssComponentValue::Number(
                    unit_value * 100.0,
                    CssUnit::Percent,
                ));
            }
            cssparser::Token::Number { value, .. } => {
                values.push(CssComponentValue::Number(value, CssUnit::Unitless));
            }
            cssparser::Token::QuotedString(ref s) => {
                values.push(CssComponentValue::QuotedString(s.as_ref().to_string()));
            }
            cssparser::Token::Hash(ref s) | cssparser::Token::IDHash(ref s) => {
                values.push(CssComponentValue::Hash(s.as_ref().to_string()));
            }
            cssparser::Token::Comma => {
                values.push(CssComponentValue::Comma);
            }
            cssparser::Token::Delim(c) => {
                values.push(CssComponentValue::Delimiter(c));
            }
            cssparser::Token::Function(ref name) => {
                let fn_name = name.as_ref().to_string();
                let args = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_component_values(nested))
                    })
                    .unwrap_or_default();
                values.push(CssComponentValue::Function(fn_name, args));
            }
            // Skip tokens that don't map to component values (whitespace is implicit)
            _ => {}
        }
    }
    values
}

/// Strips a trailing `! important` from the component value list.
///
/// Returns `true` if `!important` was found and removed.
fn strip_important(values: &mut Vec<CssComponentValue>) -> bool {
    let len = values.len();
    if len >= 2 {
        let is_important = matches!(&values[len - 1], CssComponentValue::Ident(s) if s == "important")
            && matches!(&values[len - 2], CssComponentValue::Delimiter('!'));
        if is_important {
            values.truncate(len - 2);
            return true;
        }
    }
    false
}

/// Creates a `PropertyDeclarationIR` from a name and parser input.
///
/// Parses all tokens into structured component values and extracts
/// `!important` into a separate flag.
pub fn parse_declaration<'i, 't>(
    name: cssparser::CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
) -> PropertyDeclarationIR {
    let name_str = name.as_ref();
    let mut values = parse_component_values(input);
    let important = strip_important(&mut values);
    PropertyDeclarationIR {
        name: CssPropertyName::parse(name_str),
        value: values,
        important,
    }
}
