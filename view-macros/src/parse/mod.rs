use paws_style_ir::{
    CssPropertyName, CssRuleIR, CssToken, CssUnit, HashType, PropertyDeclarationIR,
};

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

/// Recursively parses cssparser tokens into a flat list of [`CssToken`].
///
/// Strictly follows CSS Syntax Level 3 token types.  Block/function contents
/// are flattened into the token stream with matching open/close bracket tokens.
fn parse_tokens<'i, 't>(input: &mut cssparser::Parser<'i, 't>) -> Vec<CssToken> {
    let mut tokens = Vec::new();
    loop {
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            // <ident-token>
            cssparser::Token::Ident(ref ident) => {
                tokens.push(CssToken::Ident(ident.as_ref().to_string()));
            }
            // <dimension-token>
            cssparser::Token::Dimension {
                value, ref unit, ..
            } => {
                if let Some(typed_unit) = CssUnit::parse(unit.as_ref()) {
                    tokens.push(CssToken::Dimension(value, typed_unit));
                } else {
                    // Unknown unit: store value + unit as separate tokens
                    tokens.push(CssToken::Ident(format!("{}{}", value, unit)));
                }
            }
            // <percentage-token>
            cssparser::Token::Percentage { unit_value, .. } => {
                tokens.push(CssToken::Percentage(unit_value * 100.0));
            }
            // <number-token>
            cssparser::Token::Number { value, .. } => {
                tokens.push(CssToken::Number(value));
            }
            // <string-token>
            cssparser::Token::QuotedString(ref s) => {
                tokens.push(CssToken::String(s.as_ref().to_string()));
            }
            // <hash-token> (unrestricted)
            cssparser::Token::Hash(ref s) => {
                tokens.push(CssToken::Hash(
                    s.as_ref().to_string(),
                    HashType::Unrestricted,
                ));
            }
            // <hash-token> (id)
            cssparser::Token::IDHash(ref s) => {
                tokens.push(CssToken::Hash(s.as_ref().to_string(), HashType::Id));
            }
            // <comma-token>
            cssparser::Token::Comma => {
                tokens.push(CssToken::Comma);
            }
            // <colon-token>
            cssparser::Token::Colon => {
                tokens.push(CssToken::Colon);
            }
            // <semicolon-token>
            cssparser::Token::Semicolon => {
                tokens.push(CssToken::Semicolon);
            }
            // <delim-token>
            cssparser::Token::Delim(c) => {
                tokens.push(CssToken::Delim(c));
            }
            // <function-token> — emit name, then flat arguments, then CloseParen
            cssparser::Token::Function(ref name) => {
                let fn_name = name.as_ref().to_string();
                tokens.push(CssToken::Function(fn_name));
                let args = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_tokens(nested))
                    })
                    .unwrap_or_default();
                tokens.extend(args);
                tokens.push(CssToken::CloseParen);
            }
            // <at-keyword-token>
            cssparser::Token::AtKeyword(ref name) => {
                tokens.push(CssToken::AtKeyword(name.as_ref().to_string()));
            }
            // <bad-string-token>
            cssparser::Token::BadString(_) => {
                tokens.push(CssToken::BadString);
            }
            // <url-token>
            cssparser::Token::UnquotedUrl(ref url) => {
                tokens.push(CssToken::Url(url.as_ref().to_string()));
            }
            // <bad-url-token>
            cssparser::Token::BadUrl(_) => {
                tokens.push(CssToken::BadUrl);
            }
            // <whitespace-token>
            cssparser::Token::WhiteSpace(_) => {
                tokens.push(CssToken::Whitespace);
            }
            // <CDO-token>
            cssparser::Token::CDO => {
                tokens.push(CssToken::CDO);
            }
            // <CDC-token>
            cssparser::Token::CDC => {
                tokens.push(CssToken::CDC);
            }
            // Block tokens — flatten contents with matching brackets
            cssparser::Token::ParenthesisBlock => {
                tokens.push(CssToken::OpenParen);
                let inner = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_tokens(nested))
                    })
                    .unwrap_or_default();
                tokens.extend(inner);
                tokens.push(CssToken::CloseParen);
            }
            cssparser::Token::SquareBracketBlock => {
                tokens.push(CssToken::OpenSquare);
                let inner = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_tokens(nested))
                    })
                    .unwrap_or_default();
                tokens.extend(inner);
                tokens.push(CssToken::CloseSquare);
            }
            cssparser::Token::CurlyBracketBlock => {
                tokens.push(CssToken::OpenCurly);
                let inner = input
                    .parse_nested_block(|nested| {
                        Ok::<_, cssparser::ParseError<'_, ()>>(parse_tokens(nested))
                    })
                    .unwrap_or_default();
                tokens.extend(inner);
                tokens.push(CssToken::CloseCurly);
            }
            // Close tokens emitted by nested parsing — should not appear at top level
            cssparser::Token::CloseParenthesis => {
                tokens.push(CssToken::CloseParen);
            }
            cssparser::Token::CloseSquareBracket => {
                tokens.push(CssToken::CloseSquare);
            }
            cssparser::Token::CloseCurlyBracket => {
                tokens.push(CssToken::CloseCurly);
            }
            // Remaining tokens (includes match variants like ~= |= etc.)
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
            && matches!(&tokens[len - 2], CssToken::Delim('!'));
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
