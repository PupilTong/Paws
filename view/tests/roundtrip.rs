use paws_style_ir::{CssComponentValue, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

fn serialize_values(values: &[CssComponentValue], css: &mut String) {
    for (i, val) in values.iter().enumerate() {
        if i > 0
            && !matches!(
                val,
                CssComponentValue::Comma | CssComponentValue::Delimiter(_)
            )
        {
            // Don't add space before comma/delimiter
            let prev = &values[i - 1];
            if !matches!(
                prev,
                CssComponentValue::Comma | CssComponentValue::Delimiter(_)
            ) {
                // Omit space — reconstruct is lossy but sufficient for roundtrip checks
            }
        }
        match val {
            CssComponentValue::CssWide(kw) => css.push_str(kw.as_str()),
            CssComponentValue::Ident(s) => css.push_str(s),
            CssComponentValue::Number(v, unit) => {
                css.push_str(&v.to_string());
                css.push_str(unit.as_str());
            }
            CssComponentValue::QuotedString(s) => {
                css.push('"');
                css.push_str(s);
                css.push('"');
            }
            CssComponentValue::Hash(s) => {
                css.push('#');
                css.push_str(s);
            }
            CssComponentValue::Delimiter(c) => css.push(*c),
            CssComponentValue::Comma => css.push(','),
            CssComponentValue::Function(name, args) => {
                css.push_str(name);
                css.push('(');
                serialize_values(args, css);
                css.push(')');
            }
            CssComponentValue::Unparsed(s) => css.push_str(s),
        }
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
                    serialize_values(&decl.value, css);
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
                            serialize_values(&decl.value, css);
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
