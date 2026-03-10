use paws_style_ir::StyleSheetIR;
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

fn reconstruct_rules_test(rules: &[paws_style_ir::CssRuleIR], css: &mut String) {
    for rule in rules {
        match rule {
            paws_style_ir::CssRuleIR::Style(s) => {
                css.push_str(&s.selectors);
                css.push('{');
                for decl in s.declarations.iter() {
                    css.push_str(&decl.name);
                    css.push(':');
                    match &decl.value {
                        paws_style_ir::CssPropertyIR::Unparsed(val) => css.push_str(val),
                        paws_style_ir::CssPropertyIR::Keyword(val) => css.push_str(val),
                        paws_style_ir::CssPropertyIR::Unit(val, unit) => {
                            css.push_str(&val.to_string());
                            css.push_str(unit);
                        }
                        paws_style_ir::CssPropertyIR::Sum(_) => {}
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
                            css.push_str(&decl.name);
                            css.push(':');
                            match &decl.value {
                                paws_style_ir::CssPropertyIR::Unparsed(val) => css.push_str(val),
                                paws_style_ir::CssPropertyIR::Keyword(val) => css.push_str(val),
                                paws_style_ir::CssPropertyIR::Unit(val, unit) => {
                                    css.push_str(&val.to_string());
                                    css.push_str(unit);
                                }
                                paws_style_ir::CssPropertyIR::Sum(_) => {}
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
