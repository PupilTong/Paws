use paws_style_ir::{CssRuleIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

#[test]
fn test_empty_stylesheet() {
    let ir = parse(css!(""));
    assert_eq!(ir.rules.len(), 0);
}

#[test]
fn test_comments_and_whitespace() {
    let ir = parse(css!(
        r#"
        /* Top level comment */
        div {
            /* Inside rule comment */
            color: /* inline comment */ red;
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.selectors, "div");
        match &s.declarations[0].value {
            paws_style_ir::PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [paws_style_ir::CssToken::Ident(val)] => {
                    assert_eq!(val.as_str(), "red");
                }
                other => panic!("Expected Raw Ident token, got: {other:?}"),
            },
            other => panic!("Expected Raw value for color, got: {other:?}"),
        }
    }
}

#[test]
fn test_unicode_and_escapes() {
    let ir = parse(css!(
        r#"
        .\31 234 {
            content: "hello \u{1234}";
        }
        "#
    ));
    assert_eq!(ir.rules.len(), 1);
}
