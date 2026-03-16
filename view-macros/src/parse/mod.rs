use paws_style_ir::{CssPropertyName, CssRuleIR, CssToken, CssUnit, PropertyDeclarationIR};

pub mod at_rule;
pub mod decl_only;
pub mod nested_body;
mod resolve;
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

/// Recursively parses cssparser tokens into a list of [`CssToken`].
///
/// Maps each cssparser token to the corresponding IR variant. Function tokens
/// are parsed recursively via `parse_nested_block`.
fn parse_tokens<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> Vec<CssToken> {
    let mut tokens = Vec::new();
    loop {
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            cssparser::Token::Ident(ref ident) => {
                tokens.push(CssToken::Ident(ident.as_ref().to_string()));
            }
            cssparser::Token::Dimension {
                value, ref unit, ..
            } => {
                if let Some(typed_unit) = CssUnit::parse(unit.as_ref()) {
                    tokens.push(CssToken::Number(value, typed_unit));
                } else {
                    // Unknown unit: store as unparsed dimension text
                    tokens.push(CssToken::Unparsed(format!("{}{}", value, unit)));
                }
            }
            cssparser::Token::Percentage { unit_value, .. } => {
                tokens.push(CssToken::Number(unit_value * 100.0, CssUnit::Percent));
            }
            cssparser::Token::Number { value, .. } => {
                tokens.push(CssToken::Number(value, CssUnit::Unitless));
            }
            cssparser::Token::QuotedString(ref s) => {
                tokens.push(CssToken::QuotedString(s.as_ref().to_string()));
            }
            cssparser::Token::Hash(ref s) | cssparser::Token::IDHash(ref s) => {
                tokens.push(CssToken::Hash(s.as_ref().to_string()));
            }
            cssparser::Token::Comma => {
                tokens.push(CssToken::Comma);
            }
            cssparser::Token::Delim(c) => {
                tokens.push(CssToken::Delimiter(c));
            }
            cssparser::Token::Function(ref name) => {
                let fn_name = name.as_ref().to_string();
                let args = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_tokens(nested))
                    })
                    .unwrap_or_default();
                tokens.push(CssToken::Function(fn_name, args));
            }
            // Skip tokens that don't map to component values (whitespace is implicit)
            _ => {}
        }
    }
    tokens
}

/// Strips a trailing `! important` from the token list.
///
/// Returns `true` if `!important` was found and removed.
fn strip_important(tokens: &mut Vec<CssToken>) -> bool {
    let len = tokens.len();
    if len >= 2 {
        let is_important = matches!(&tokens[len - 1], CssToken::Ident(s) if s == "important")
            && matches!(&tokens[len - 2], CssToken::Delimiter('!'));
        if is_important {
            tokens.truncate(len - 2);
            return true;
        }
    }
    false
}

/// Creates a `PropertyDeclarationIR` from a name and parser input.
///
/// Parses all tokens into structured component values, extracts `!important`,
/// then resolves into a typed [`PropertyValueIR`] when possible.
pub fn parse_declaration<'i, 't>(
    name: cssparser::CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
) -> PropertyDeclarationIR {
    let name_str = name.as_ref();
    let mut tokens = parse_tokens(input);
    let important = strip_important(&mut tokens);
    let prop_name = CssPropertyName::parse(name_str);
    let value = resolve::resolve_typed_value(&prop_name, tokens);
    PropertyDeclarationIR {
        name: prop_name,
        value,
        important,
    }
}
