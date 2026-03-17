use paws_style_ir::{CssToken, PropertyValueIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

fn serialize_values(values: &[CssToken], css: &mut String) {
    for (i, val) in values.iter().enumerate() {
        if i > 0
            && !matches!(
                val,
                CssToken::Comma | CssToken::Delim(_) | CssToken::CloseParen
            )
        {
            let prev = &values[i - 1];
            if !matches!(
                prev,
                CssToken::Comma | CssToken::Delim(_) | CssToken::Function(_) | CssToken::OpenParen
            ) {
                // Omit space — reconstruct is lossy but sufficient for roundtrip checks
            }
        }
        match val {
            CssToken::Ident(s) => css.push_str(s),
            CssToken::Function(name) => {
                css.push_str(name);
                css.push('(');
            }
            CssToken::AtKeyword(s) => {
                css.push('@');
                css.push_str(s);
            }
            CssToken::Hash(s, _) => {
                css.push('#');
                css.push_str(s);
            }
            CssToken::String(s) => {
                css.push('"');
                css.push_str(s);
                css.push('"');
            }
            CssToken::BadString => css.push_str("/* bad-string */"),
            CssToken::Url(s) => {
                css.push_str("url(");
                css.push_str(s);
                css.push(')');
            }
            CssToken::BadUrl => css.push_str("/* bad-url */"),
            CssToken::Delim(c) => css.push(*c),
            CssToken::Number(v) => css.push_str(&v.to_string()),
            CssToken::Percentage(v) => {
                css.push_str(&v.to_string());
                css.push('%');
            }
            CssToken::Dimension(v, unit) => {
                css.push_str(&v.to_string());
                css.push_str(unit.as_str());
            }
            CssToken::UnicodeRange(start, end) => {
                css.push_str(&format!("U+{:X}-{:X}", start, end));
            }
            CssToken::Whitespace => css.push(' '),
            CssToken::CDO => css.push_str("<!--"),
            CssToken::CDC => css.push_str("-->"),
            CssToken::Colon => css.push(':'),
            CssToken::Semicolon => css.push(';'),
            CssToken::Comma => css.push(','),
            CssToken::OpenSquare => css.push('['),
            CssToken::CloseSquare => css.push(']'),
            CssToken::OpenParen => css.push('('),
            CssToken::CloseParen => css.push(')'),
            CssToken::OpenCurly => css.push('{'),
            CssToken::CloseCurly => css.push('}'),
        }
    }
}

fn serialize_property_value(value: &PropertyValueIR, css: &mut String) {
    match value {
        PropertyValueIR::Raw(tokens) => serialize_values(tokens, css),
        PropertyValueIR::CssWide(kw) => css.push_str(kw.as_str()),
        // For typed values, serialize a best-effort CSS representation
        _ => css.push_str("/* typed */"),
    }
}

fn reconstruct_rules_test(rules: &[paws_style_ir::CssRuleIR], css: &mut String) {
    for rule in rules {
        match rule {
            paws_style_ir::CssRuleIR::Style(s) => {
                css.push_str(&s.selectors);
                css.push('{');
                for decl in s.declarations.iter() {
                    css.push_str(decl.name.as_str());
                    css.push(':');
                    serialize_property_value(&decl.value, css);
                    if decl.important {
                        css.push_str("!important");
                    }
                    css.push(';');
                }
                reconstruct_rules_test(&s.rules, css);
                css.push('}');
            }
            paws_style_ir::CssRuleIR::AtRule(a) => {
                css.push('@');
                css.push_str(&a.name);
                if !a.prelude.is_empty() {
                    css.push(' ');
                    css.push_str(&a.prelude);
                }
                match &a.block {
                    Some(paws_style_ir::AtRuleBlockIR::Rules(r)) => {
                        css.push('{');
                        reconstruct_rules_test(r, css);
                        css.push('}');
                    }
                    Some(paws_style_ir::AtRuleBlockIR::Declarations(d)) => {
                        css.push('{');
                        for decl in d.iter() {
                            css.push_str(decl.name.as_str());
                            css.push(':');
                            serialize_property_value(&decl.value, css);
                            if decl.important {
                                css.push_str("!important");
                            }
                            css.push(';');
                        }
                        css.push('}');
                    }
                    None => {
                        css.push(';');
                    }
                }
            }
        }
    }
}

#[test]
fn test_roundtrip_valid_css() {
    let bytes = css!(
        r#"
        .card {
            background: white;
            padding: 1rem;
            @media (min-width: 768px) {
                padding: 2rem;
                & .title { font-size: 2rem; }
            }
        }
        @keyframes fade {
            from { opacity: 0; }
            to { opacity: 1; }
        }
        "#
    );
    let ir = parse(bytes);
    let mut reconstructed = String::new();
    reconstruct_rules_test(&ir.rules, &mut reconstructed);
    assert!(reconstructed.contains(".card{background:white;padding:1rem;"));
    assert!(reconstructed.contains("@media (min-width: 768px){"));
    assert!(reconstructed.contains("& .title{font-size:2rem;}"));
    assert!(reconstructed.contains("@keyframes fade{"));
}
