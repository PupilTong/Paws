use paws_style_ir::{CssRuleIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

#[test]
fn test_selectors_basic() {
    let ir = parse(css!(
        r#"
        * { color: red; }
        div { color: red; }
        .class { color: red; }
        #id { color: red; }
        [attr] { color: red; }
        [attr=val] { color: red; }
        [attr~=val] { color: red; }
    "#
    ));
    assert_eq!(ir.rules.len(), 7);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.selectors, "*");
    } else {
        panic!("Expected StyleRuleIR");
    }
}

#[test]
fn test_selectors_combinators() {
    let ir = parse(css!(
        r#"
        div > p { color: red; }
        div + p { color: red; }
        div ~ p { color: red; }
        div || p { color: red; }
        div p { color: red; }
        div, p { color: red; }
    "#
    ));
    assert_eq!(ir.rules.len(), 6);
    if let CssRuleIR::Style(_s) = &ir.rules[5] {
        // NOTE: cssparser returns valid selectors, stringified. Spaces might vary slightly
        // but we just check the macro parsing doesn't crash here.
    }
}

#[test]
fn test_selectors_pseudo() {
    let ir = parse(css!(
        r#"
        :hover { color: red; }
        :first-child { color: red; }
        :nth-child(2n+1) { color: red; }
        ::before { color: red; }
        ::after { color: red; }
        :not(.class) { color: red; }
        :is(div, p) { color: red; }
        :where(div) { color: red; }
        :has(> img) { color: red; }
    "#
    ));
    assert_eq!(ir.rules.len(), 9);
}
