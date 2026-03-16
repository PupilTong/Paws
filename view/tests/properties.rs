use paws_style_ir::values::*;
use paws_style_ir::{
    CssPropertyName, CssRuleIR, CssToken, CssUnit, CssWideKeyword, PropertyValueIR, StyleSheetIR,
};
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

        // color: red → Raw(Ident("red"))
        assert_eq!(s.declarations[0].name, CssPropertyName::Color);
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Ident("red".to_string())])
        );
        assert!(!s.declarations[0].important);

        // display: block → Display(DisplayIR::Block)
        assert_eq!(s.declarations[1].name, CssPropertyName::Display);
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Display(DisplayIR::Block)
        );

        // width: 100% → Size(SizeIR::LengthPercentage(NonNegativeLPIR::Percentage(100.0)))
        assert_eq!(s.declarations[2].name, CssPropertyName::Width);
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Percentage(100.0)))
        );

        // height: 100vh → Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(100.0, Vh)))
        assert_eq!(s.declarations[3].name, CssPropertyName::Height);
        assert_eq!(
            s.declarations[3].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                100.0,
                CssUnit::Vh
            )))
        );

        // font-size: 16px → Raw (font-size is not typed yet)
        assert_eq!(s.declarations[6].name, CssPropertyName::FontSize);
        assert_eq!(
            s.declarations[6].value,
            PropertyValueIR::Raw(vec![CssToken::Number(16.0, CssUnit::Px)])
        );

        // --custom-prop: 10px → Raw
        assert_eq!(
            s.declarations[9].name,
            CssPropertyName::Custom("--custom-prop".to_string())
        );

        // color: red !important → Raw(Ident("red")) + important=true
        assert_eq!(s.declarations[10].name, CssPropertyName::Color);
        assert!(s.declarations[10].important);
        assert_eq!(
            s.declarations[10].value,
            PropertyValueIR::Raw(vec![CssToken::Ident("red".to_string())])
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

        // width: calc(100% - 20px) → Raw(Function("calc", [...]))
        assert_eq!(s.declarations[0].name, CssPropertyName::Width);
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [CssToken::Function(name, args)] => {
                    assert_eq!(name, "calc");
                    // Should contain: Number(100, Percent), Delimiter('-'), Number(20, Px)
                    assert!(args.len() >= 3, "calc args: {args:?}");
                }
                other => panic!("Expected Function for calc, got: {other:?}"),
            },
            other => panic!("Expected Raw value for width calc, got: {other:?}"),
        }

        // color: rgb(255, 0, 0) → Raw(Function("rgb", [...]))
        assert_eq!(s.declarations[5].name, CssPropertyName::Color);
        match &s.declarations[5].value {
            PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [CssToken::Function(name, args)] => {
                    assert_eq!(name, "rgb");
                    // Should contain numbers and commas
                    assert!(args.len() >= 5, "rgb args: {args:?}");
                }
                other => panic!("Expected Function for rgb, got: {other:?}"),
            },
            other => panic!("Expected Raw value for color rgb, got: {other:?}"),
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
            PropertyValueIR::Raw(vec![CssToken::Hash("ff0000".to_string())])
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
            PropertyValueIR::Raw(vec![CssToken::Hash("abc".to_string())])
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
            PropertyValueIR::Raw(vec![CssToken::QuotedString("Helvetica Neue".to_string())])
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
            PropertyValueIR::Raw(vec![
                CssToken::Ident("Arial".to_string()),
                CssToken::Comma,
                CssToken::Ident("sans-serif".to_string()),
            ])
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
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [CssToken::Function(name, args)] => {
                    assert_eq!(name, "calc");
                    // Verify delimiters are present
                    assert!(
                        args.contains(&CssToken::Delimiter('-')),
                        "Missing '-' delimiter in: {args:?}"
                    );
                    assert!(
                        args.contains(&CssToken::Delimiter('+')),
                        "Missing '+' delimiter in: {args:?}"
                    );
                }
                other => panic!("Expected Function, got: {other:?}"),
            },
            other => panic!("Expected Raw value, got: {other:?}"),
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
            PropertyValueIR::Display(DisplayIR::Block)
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
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                100.0,
                CssUnit::Px
            )))
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
            PropertyValueIR::CssWide(CssWideKeyword::Inherit)
        );
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::CssWide(CssWideKeyword::Initial)
        );
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::CssWide(CssWideKeyword::Unset)
        );
        assert_eq!(
            s.declarations[3].value,
            PropertyValueIR::CssWide(CssWideKeyword::Revert)
        );
        assert_eq!(
            s.declarations[4].value,
            PropertyValueIR::CssWide(CssWideKeyword::RevertLayer)
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
        // width: 10em → Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(10.0, Em)))
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                10.0,
                CssUnit::Em
            )))
        );
        // height: 2rem → Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(2.0, Rem)))
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                2.0,
                CssUnit::Rem
            )))
        );
        // top: 50vh → Inset(InsetIR::LengthPercentage(LengthPercentageIR::Length(50.0, Vh)))
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::Inset(InsetIR::LengthPercentage(LengthPercentageIR::Length(
                50.0,
                CssUnit::Vh
            )))
        );
        // left: 50vw → Inset(InsetIR::LengthPercentage(LengthPercentageIR::Length(50.0, Vw)))
        assert_eq!(
            s.declarations[3].value,
            PropertyValueIR::Inset(InsetIR::LengthPercentage(LengthPercentageIR::Length(
                50.0,
                CssUnit::Vw
            )))
        );
        // font-size: 90deg → Raw (font-size is not typed)
        assert_eq!(
            s.declarations[4].value,
            PropertyValueIR::Raw(vec![CssToken::Number(90.0, CssUnit::Deg)])
        );
        // margin: 1s → Raw (shorthand margin → Other → Raw)
        assert_eq!(
            s.declarations[5].value,
            PropertyValueIR::Raw(vec![CssToken::Number(1.0, CssUnit::S)])
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
            PropertyValueIR::FlexGrow(NonNegativeNumberIR(2.0))
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
            PropertyValueIR::Raw(vec![
                CssToken::Number(10.0, CssUnit::Px),
                CssToken::Number(20.0, CssUnit::Px),
            ])
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
            PropertyValueIR::Raw(vec![CssToken::Hash("ff0000".to_string())])
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
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [CssToken::Function(name, args)] => {
                    assert_eq!(name, "min");
                    assert_eq!(
                        args,
                        &vec![
                            CssToken::Number(100.0, CssUnit::Percent),
                            CssToken::Comma,
                            CssToken::Number(500.0, CssUnit::Px),
                        ]
                    );
                }
                other => panic!("Expected Function, got: {other:?}"),
            },
            other => panic!("Expected Raw value, got: {other:?}"),
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
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => match &tokens[..] {
                [CssToken::Function(name, args)] => {
                    assert_eq!(name, "var");
                    // var(--primary, red) → args should contain ident, comma, ident
                    assert!(args.len() >= 3, "var args: {args:?}");
                    assert!(
                        matches!(&args[0], CssToken::Ident(s) if s == "--primary"),
                        "Expected --primary ident, got: {:?}",
                        args[0]
                    );
                }
                other => panic!("Expected Function for var, got: {other:?}"),
            },
            other => panic!("Expected Raw value for color var, got: {other:?}"),
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
            PropertyValueIR::Raw(vec![
                CssToken::Number(10.0, CssUnit::Px),
                CssToken::Number(20.0, CssUnit::Px),
            ])
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
            PropertyValueIR::Raw(vec![CssToken::Number(0.0, CssUnit::Unitless)])
        );
    }
}
