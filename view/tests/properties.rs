use paws_style_ir::{CssComponentValue, CssPropertyName, CssRuleIR, CssUnit, StyleSheetIR};
use view_macros::css;

fn parse(bytes: &[u8]) -> StyleSheetIR {
    rkyv::from_bytes::<StyleSheetIR, rkyv::rancor::Error>(bytes).unwrap()
}

// ─── Basic property parsing ─────────────────────────────────────────

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

        // color: red → Ident("red")
        assert_eq!(s.declarations[0].name, CssPropertyName::Color);
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Ident("red".to_string())]
        );
        assert!(!s.declarations[0].important);

        // display: block
        assert_eq!(s.declarations[1].name, CssPropertyName::Display);
        assert_eq!(
            s.declarations[1].value,
            vec![CssComponentValue::Ident("block".to_string())]
        );

        // width: 100%
        assert_eq!(s.declarations[2].name, CssPropertyName::Width);
        assert_eq!(
            s.declarations[2].value,
            vec![CssComponentValue::Number(100.0, CssUnit::Percent)]
        );

        // height: 100vh
        assert_eq!(s.declarations[3].name, CssPropertyName::Height);
        assert_eq!(
            s.declarations[3].value,
            vec![CssComponentValue::Number(100.0, CssUnit::Vh)]
        );

        // font-size: 16px
        assert_eq!(s.declarations[6].name, CssPropertyName::FontSize);
        assert_eq!(
            s.declarations[6].value,
            vec![CssComponentValue::Number(16.0, CssUnit::Px)]
        );

        // --custom-prop: 10px
        assert_eq!(
            s.declarations[9].name,
            CssPropertyName::Custom("--custom-prop".to_string())
        );

        // color: red !important → important flag set, value is just Ident("red")
        assert_eq!(s.declarations[10].name, CssPropertyName::Color);
        assert!(s.declarations[10].important);
        assert_eq!(
            s.declarations[10].value,
            vec![CssComponentValue::Ident("red".to_string())]
        );
    } else {
        panic!("Expected StyleRuleIR");
    }
}

// ─── Function values ────────────────────────────────────────────────

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

        // width: calc(100% - 20px) → Function("calc", [...])
        assert_eq!(s.declarations[0].name, CssPropertyName::Width);
        match &s.declarations[0].value[..] {
            [CssComponentValue::Function(name, args)] => {
                assert_eq!(name, "calc");
                // Should contain: Number(100, Percent), Delimiter('-'), Number(20, Px)
                assert!(args.len() >= 3, "calc args: {args:?}");
            }
            other => panic!("Expected Function for calc, got: {other:?}"),
        }

        // color: rgb(255, 0, 0) → Function("rgb", [Number(255, Unitless), Comma, ...])
        assert_eq!(s.declarations[5].name, CssPropertyName::Color);
        match &s.declarations[5].value[..] {
            [CssComponentValue::Function(name, args)] => {
                assert_eq!(name, "rgb");
                // Should contain numbers and commas
                assert!(args.len() >= 5, "rgb args: {args:?}");
            }
            other => panic!("Expected Function for rgb, got: {other:?}"),
        }
    }
}

// ─── Component value AST structure ──────────────────────────────────

#[test]
fn test_hash_color() {
    let ir = parse(css!(
        r#"
        div { color: #ff0000; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Hash("ff0000".to_string())]
        );
    }
}

#[test]
fn test_short_hash_color() {
    let ir = parse(css!(
        r#"
        div { color: #abc; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Hash("abc".to_string())]
        );
    }
}

#[test]
fn test_quoted_string() {
    let ir = parse(css!(
        r#"
        div { font-family: "Helvetica Neue"; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::QuotedString(
                "Helvetica Neue".to_string()
            )]
        );
    }
}

#[test]
fn test_comma_separated_values() {
    let ir = parse(css!(
        r#"
        div { font-family: Arial, sans-serif; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![
                CssComponentValue::Ident("Arial".to_string()),
                CssComponentValue::Comma,
                CssComponentValue::Ident("sans-serif".to_string()),
            ]
        );
    }
}

#[test]
fn test_delimiter_tokens() {
    let ir = parse(css!(
        r#"
        div { width: calc(100% - 20px + 5em); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value[..] {
            [CssComponentValue::Function(name, args)] => {
                assert_eq!(name, "calc");
                // Verify delimiters are present
                assert!(
                    args.contains(&CssComponentValue::Delimiter('-')),
                    "Missing '-' delimiter in: {args:?}"
                );
                assert!(
                    args.contains(&CssComponentValue::Delimiter('+')),
                    "Missing '+' delimiter in: {args:?}"
                );
            }
            other => panic!("Expected Function, got: {other:?}"),
        }
    }
}

// ─── Importance tests ───────────────────────────────────────────────

#[test]
fn test_important_keyword() {
    let ir = parse(css!(
        r#"
        div { display: block !important; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert!(s.declarations[0].important);
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Ident("block".to_string())]
        );
    }
}

#[test]
fn test_important_numeric() {
    let ir = parse(css!(
        r#"
        div { width: 100px !important; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert!(s.declarations[0].important);
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Number(100.0, CssUnit::Px)]
        );
    }
}

#[test]
fn test_no_important() {
    let ir = parse(css!(
        r#"
        div { color: red; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert!(!s.declarations[0].important);
    }
}

// ─── CSS-wide keywords ─────────────────────────────────────────────

#[test]
fn test_css_wide_keywords() {
    use paws_style_ir::CssWideKeyword;
    let ir = parse(css!(
        r#"
        div {
            color: inherit;
            display: initial;
            width: unset;
            height: revert;
            margin: revert-layer;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::CssWide(CssWideKeyword::Inherit)]
        );
        assert_eq!(
            s.declarations[1].value,
            vec![CssComponentValue::CssWide(CssWideKeyword::Initial)]
        );
        assert_eq!(
            s.declarations[2].value,
            vec![CssComponentValue::CssWide(CssWideKeyword::Unset)]
        );
        assert_eq!(
            s.declarations[3].value,
            vec![CssComponentValue::CssWide(CssWideKeyword::Revert)]
        );
        assert_eq!(
            s.declarations[4].value,
            vec![CssComponentValue::CssWide(CssWideKeyword::RevertLayer)]
        );
    }
}

// ─── Unit types ─────────────────────────────────────────────────────

#[test]
fn test_unit_types() {
    let ir = parse(css!(
        r#"
        div {
            width: 10em;
            height: 2rem;
            top: 50vh;
            left: 50vw;
            font-size: 90deg;
            margin: 1s;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Number(10.0, CssUnit::Em)]
        );
        assert_eq!(
            s.declarations[1].value,
            vec![CssComponentValue::Number(2.0, CssUnit::Rem)]
        );
        assert_eq!(
            s.declarations[2].value,
            vec![CssComponentValue::Number(50.0, CssUnit::Vh)]
        );
        assert_eq!(
            s.declarations[3].value,
            vec![CssComponentValue::Number(50.0, CssUnit::Vw)]
        );
        assert_eq!(
            s.declarations[4].value,
            vec![CssComponentValue::Number(90.0, CssUnit::Deg)]
        );
        assert_eq!(
            s.declarations[5].value,
            vec![CssComponentValue::Number(1.0, CssUnit::S)]
        );
    }
}

#[test]
fn test_unitless_number() {
    let ir = parse(css!(
        r#"
        div { flex-grow: 2; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Number(2.0, CssUnit::Unitless)]
        );
    }
}

// ─── Custom properties ─────────────────────────────────────────────

#[test]
fn test_custom_property() {
    let ir = parse(css!(
        r#"
        div { --my-var: 10px 20px; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].name,
            CssPropertyName::Custom("--my-var".to_string())
        );
        assert_eq!(
            s.declarations[0].value,
            vec![
                CssComponentValue::Number(10.0, CssUnit::Px),
                CssComponentValue::Number(20.0, CssUnit::Px),
            ]
        );
    }
}

#[test]
fn test_custom_property_hash() {
    let ir = parse(css!(
        r#"
        div { --color: #ff0000; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].name,
            CssPropertyName::Custom("--color".to_string())
        );
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Hash("ff0000".to_string())]
        );
    }
}

// ─── Nested functions ───────────────────────────────────────────────

#[test]
fn test_nested_function() {
    let ir = parse(css!(
        r#"
        div { width: min(100%, 500px); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value[..] {
            [CssComponentValue::Function(name, args)] => {
                assert_eq!(name, "min");
                assert_eq!(
                    args,
                    &vec![
                        CssComponentValue::Number(100.0, CssUnit::Percent),
                        CssComponentValue::Comma,
                        CssComponentValue::Number(500.0, CssUnit::Px),
                    ]
                );
            }
            other => panic!("Expected Function, got: {other:?}"),
        }
    }
}

#[test]
fn test_var_with_fallback() {
    let ir = parse(css!(
        r#"
        div { color: var(--primary, red); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value[..] {
            [CssComponentValue::Function(name, args)] => {
                assert_eq!(name, "var");
                // var(--primary, red) → args should contain ident, comma, ident
                assert!(args.len() >= 3, "var args: {args:?}");
                assert!(
                    matches!(&args[0], CssComponentValue::Ident(s) if s == "--primary"),
                    "Expected --primary ident, got: {:?}",
                    args[0]
                );
            }
            other => panic!("Expected Function for var, got: {other:?}"),
        }
    }
}

// ─── Multi-value properties ─────────────────────────────────────────

#[test]
fn test_multi_value_margin() {
    let ir = parse(css!(
        r#"
        div { margin: 10px 20px; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        // margin is parsed as Other("margin") since it's a shorthand
        assert_eq!(
            s.declarations[0].value,
            vec![
                CssComponentValue::Number(10.0, CssUnit::Px),
                CssComponentValue::Number(20.0, CssUnit::Px),
            ]
        );
    }
}

#[test]
fn test_zero_unitless() {
    let ir = parse(css!(
        r#"
        div { margin: 0; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            vec![CssComponentValue::Number(0.0, CssUnit::Unitless)]
        );
    }
}
