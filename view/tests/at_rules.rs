use paws_style_ir::{AtRuleBlockIR, CssPropertyName, CssRuleIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

#[test]
fn test_at_rule_media() {
    let ir = parse(css!(
        r#"
        @media screen and (min-width: 900px) {
            article {
                padding: 1rem 3rem;
            }
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::AtRule(a) = &ir.rules[0] {
        assert_eq!(a.name, "media");
        assert!(!a.prelude.is_empty());
        match &a.block {
            Some(AtRuleBlockIR::Rules(r)) => {
                assert_eq!(r.len(), 1);
            }
            _ => panic!("Expected rules in @media"),
        }
    } else {
        panic!("Expected AtRule");
    }
}

#[test]
fn test_at_rule_keyframes() {
    let ir = parse(css!(
        r#"
        @keyframes slide {
            from { transform: translateX(0); }
            to { transform: translateX(100%); }
            50% { opacity: 0; }
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::AtRule(a) = &ir.rules[0] {
        assert_eq!(a.name, "keyframes");
        assert_eq!(a.prelude, "slide");
        match &a.block {
            Some(AtRuleBlockIR::Rules(r)) => {
                assert_eq!(r.len(), 3);
            }
            _ => panic!("Expected rules in @keyframes"),
        }
    }
}

#[test]
fn test_at_rule_font_face() {
    let ir = parse(css!(
        r#"
        @font-face {
            font-family: "MyFont";
            src: url("myfont.woff2") format("woff2");
            font-weight: 400;
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::AtRule(a) = &ir.rules[0] {
        assert_eq!(a.name, "font-face");
        assert!(a.prelude.is_empty());
        match &a.block {
            Some(AtRuleBlockIR::Declarations(d)) => {
                assert_eq!(d.len(), 3);
                assert_eq!(d[0].name, CssPropertyName::FontFamily);
                assert_eq!(d[1].name, CssPropertyName::Other("src".to_string()));
                assert_eq!(d[2].name, CssPropertyName::FontWeight);
            }
            _ => panic!("Expected declarations in @font-face"),
        }
    }
}

#[test]
fn test_at_rule_others() {
    let ir = parse(css!(
        r#"
        @supports (display: grid) {
            div { display: grid; }
        }
        @layer base {
            div { color: red; }
        }
        @layer framework;
        @import url("style.css");
        @container (min-width: 700px) {
            .card { color: red; }
        }
        @property --my-color {
            syntax: "<color>";
            inherits: false;
            initial-value: #c0ffee;
        }
        @scope (.card) {
            p { color: blue; }
        }
        @namespace svg url(http://www.w3.org/2000/svg);
        @page {
            margin: 1cm;
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 9);
}
