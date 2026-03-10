use paws_style_ir::{CssRuleIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

#[test]
fn test_properties_basic() {
    let ir = parse(css!(
        r#"
        div {
            color: red;
            display: block;
            width: 100%;
            height: 100vh;
            margin: 0;
            padding: 10px 20px;
            font-size: 16px;
            background: center / cover no-repeat url("img.png");
            border: 1px solid black;
            --custom-prop: 10px;
            color: red !important;
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.declarations.len(), 11);
        assert_eq!(s.declarations[0].name, "color");
        if let paws_style_ir::CssPropertyIR::Keyword(val) = &s.declarations[0].value {
            assert_eq!(val, "red");
        } else {
            panic!("Expected Keyword value");
        }
        assert_eq!(s.declarations[10].name, "color");
        if let paws_style_ir::CssPropertyIR::Unparsed(val) = &s.declarations[10].value {
            assert!(val.contains("!important") || val.contains("! important"));
        } else {
            panic!("Expected Unparsed value for declaration 10");
        }
    } else {
        panic!("Expected StyleRuleIR");
    }
}

#[test]
fn test_properties_functions() {
    let ir = parse(css!(
        r#"
        div {
            width: calc(100% - 20px);
            margin: var(--custom, 10px);
            height: min(100vh, 500px);
            max-width: max(100%, 200px);
            font-size: clamp(1rem, 2vw, 2rem);
            color: rgb(255, 0, 0);
            background: linear-gradient(to right, red, blue);
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.declarations.len(), 7);
    }
}
