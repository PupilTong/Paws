use paws_style_ir::{CssRuleIR, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

#[test]
fn test_nesting_basic() {
    let ir = parse(css!(
        r#"
        table.colortable {
          & td {
            text-align: center;
            &.c { text-transform: uppercase }
            &:first-child, &:first-child + td { border: 1px solid black }
          }
          & th {
            text-align: center;
            background: black;
            color: white;
          }
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.selectors, "table.colortable");
        assert_eq!(s.rules.len(), 2);

        if let CssRuleIR::Style(nested1) = &s.rules[0] {
            // macros output string without strictly normalizing whitespace in prelude
            // so we loosely check or just match length
            assert_eq!(nested1.declarations.len(), 1);
            assert_eq!(nested1.rules.len(), 2);
        } else {
            panic!("Expected nested style rule");
        }
    } else {
        panic!("Expected StyleRuleIR");
    }
}

#[test]
fn test_nesting_with_at_rules() {
    let ir = parse(css!(
        r#"
        .foo {
            color: red;
            @media (min-width: 480px) {
                & { color: blue; }
            }
        }
    "#
    ));
    assert_eq!(ir.rules.len(), 1);
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(s.declarations.len(), 1);
        assert_eq!(s.rules.len(), 1);
        if let CssRuleIR::AtRule(a) = &s.rules[0] {
            assert_eq!(a.name, "media");
            assert_eq!(a.prelude, "(min-width: 480px)");
        } else {
            panic!("Expected AtRule");
        }
    }
}
