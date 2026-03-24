use paws_style_ir::values::*;
use paws_style_ir::{
    CssPropertyName, CssRuleIR, CssToken, CssUnit, CssWideKeyword, HashType, PropertyValueIR,
    StyleSheetIR,
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
            PropertyValueIR::Raw(vec![CssToken::Dimension(16.0, CssUnit::Px)])
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

        // width: calc(100% - 20px) → Raw([Function("calc"), ..., CloseParen])
        assert_eq!(s.declarations[0].name, CssPropertyName::Width);
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "calc"),
                    "Expected Function(calc), got: {:?}",
                    tokens[0]
                );
                assert!(
                    matches!(tokens.last(), Some(CssToken::CloseParen)),
                    "Expected CloseParen at end"
                );
                // Interior should contain percentage, delim, dimension tokens
                assert!(tokens.len() >= 5, "calc tokens: {tokens:?}");
            }
            other => panic!("Expected Raw value for width calc, got: {other:?}"),
        }

        // color: rgb(255, 0, 0) → Raw([Function("rgb"), ..., CloseParen])
        assert_eq!(s.declarations[5].name, CssPropertyName::Color);
        match &s.declarations[5].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "rgb"),
                    "Expected Function(rgb), got: {:?}",
                    tokens[0]
                );
                assert!(
                    matches!(tokens.last(), Some(CssToken::CloseParen)),
                    "Expected CloseParen at end"
                );
                // Should contain numbers and commas
                assert!(tokens.len() >= 7, "rgb tokens: {tokens:?}");
            }
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
            PropertyValueIR::Raw(vec![CssToken::Hash("ff0000".to_string(), HashType::Id)])
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
            PropertyValueIR::Raw(vec![CssToken::Hash("abc".to_string(), HashType::Id)])
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
            PropertyValueIR::Raw(vec![CssToken::String("Helvetica Neue".to_string())])
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
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "calc"),
                    "Expected Function(calc), got: {:?}",
                    tokens[0]
                );
                // Verify delimiters are present in the flat token list
                assert!(
                    tokens.contains(&CssToken::Delim('-')),
                    "Missing '-' delimiter in: {tokens:?}"
                );
                assert!(
                    tokens.contains(&CssToken::Delim('+')),
                    "Missing '+' delimiter in: {tokens:?}"
                );
            }
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
            PropertyValueIR::Raw(vec![CssToken::Dimension(90.0, CssUnit::Deg)])
        );
        // margin: 1s → Raw (shorthand margin → Other → Raw)
        assert_eq!(
            s.declarations[5].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(1.0, CssUnit::S)])
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
                CssToken::Dimension(10.0, CssUnit::Px),
                CssToken::Dimension(20.0, CssUnit::Px),
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
            PropertyValueIR::Raw(vec![CssToken::Hash("ff0000".to_string(), HashType::Id)])
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
            PropertyValueIR::Raw(tokens) => {
                assert_eq!(
                    tokens,
                    &vec![
                        CssToken::Function("min".to_string()),
                        CssToken::Percentage(100.0),
                        CssToken::Comma,
                        CssToken::Dimension(500.0, CssUnit::Px),
                        CssToken::CloseParen,
                    ]
                );
            }
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
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "var"),
                    "Expected Function(var), got: {:?}",
                    tokens[0]
                );
                // var(--primary, red) → flat tokens: Function, Ident, Comma, Ident, CloseParen
                assert!(tokens.len() >= 5, "var tokens: {tokens:?}");
                assert!(
                    matches!(&tokens[1], CssToken::Ident(s) if s == "--primary"),
                    "Expected --primary ident, got: {:?}",
                    tokens[1]
                );
                assert!(
                    matches!(tokens.last(), Some(CssToken::CloseParen)),
                    "Expected CloseParen at end"
                );
            }
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
                CssToken::Dimension(10.0, CssUnit::Px),
                CssToken::Dimension(20.0, CssUnit::Px),
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
            PropertyValueIR::Raw(vec![CssToken::Number(0.0)])
        );
    }
}

// ─── CSS3 token coverage ────────────────────────────────────────────

#[test]
fn test_url_token() {
    let ir = parse(css!(
        r#"
        div { background-image: url(foo.png); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert_eq!(tokens, &vec![CssToken::Url("foo.png".to_string())]);
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_url_token_quoted() {
    // url("...") with quotes is parsed as Function("url") + String + CloseParen
    let ir = parse(css!(
        r#"
        div { background-image: url("bar.png"); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // cssparser treats url("...") as an unquoted url token
                // (the quotes are consumed by the url() function parser)
                assert!(
                    matches!(&tokens[0], CssToken::Url(u) if u == "bar.png")
                        || matches!(&tokens[0], CssToken::Function(name) if name == "url"),
                    "Expected Url or Function(url), got: {:?}",
                    tokens[0]
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_whitespace_tokens_between_values() {
    // cssparser strips whitespace between top-level value tokens,
    // so multi-value shorthands don't contain Whitespace tokens.
    let ir = parse(css!(
        r#"
        div { margin: 10px 20px 30px 40px; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Should have exactly 4 dimension tokens, no whitespace
                assert_eq!(
                    tokens,
                    &vec![
                        CssToken::Dimension(10.0, CssUnit::Px),
                        CssToken::Dimension(20.0, CssUnit::Px),
                        CssToken::Dimension(30.0, CssUnit::Px),
                        CssToken::Dimension(40.0, CssUnit::Px),
                    ]
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_deeply_nested_functions() {
    let ir = parse(css!(
        r#"
        div { width: calc(min(100%, 200px) + 10px); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Flattened: Function("calc"), Function("min"), 100%, Comma, 200px,
                //            CloseParen, +, 10px, CloseParen
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "calc"),
                    "Expected outer calc, got: {:?}",
                    tokens[0]
                );
                // Find inner min function
                let has_min = tokens
                    .iter()
                    .any(|t| matches!(t, CssToken::Function(name) if name == "min"));
                assert!(has_min, "Expected nested min function: {tokens:?}");
                // Should have exactly 2 CloseParen (one for min, one for calc)
                let close_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::CloseParen))
                    .count();
                assert_eq!(close_count, 2, "Expected 2 CloseParen tokens: {tokens:?}");
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_negative_numbers() {
    let ir = parse(css!(
        r#"
        div {
            margin-left: -20px;
            z-index: -5;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        // margin-left: -20px → Margin(MarginIR::LengthPercentage(...))
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Margin(MarginIR::LengthPercentage(LengthPercentageIR::Length(
                -20.0,
                CssUnit::Px
            )))
        );

        // z-index: -5 → ZIndex(ZIndexIR::Integer(IntegerIR(-5)))
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::ZIndex(ZIndexIR::Integer(IntegerIR(-5)))
        );
    }
}

#[test]
fn test_fractional_numbers() {
    let ir = parse(css!(
        r#"
        div {
            flex-grow: 0.5;
            opacity: 0.75;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::FlexGrow(NonNegativeNumberIR(0.5))
        );
        // opacity is not typed, so Raw
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Number(0.75)])
        );
    }
}

#[test]
fn test_percentage_token() {
    let ir = parse(css!(
        r#"
        div {
            width: 50%;
            opacity: 100%;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        // width: 50% → typed Size
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Percentage(50.0)))
        );
        // opacity: 100% → Raw (not typed)
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Percentage(100.0)])
        );
    }
}

#[test]
fn test_multiple_functions_in_value() {
    let ir = parse(css!(
        r#"
        div { background: url(bg.png) no-repeat center; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Should have Url token, Ident tokens
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Url(u) if u == "bg.png")),
                    "Expected url token: {tokens:?}"
                );
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Ident(s) if s == "no-repeat")),
                    "Expected no-repeat ident: {tokens:?}"
                );
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Ident(s) if s == "center")),
                    "Expected center ident: {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_custom_property_with_brackets() {
    let ir = parse(css!(
        r#"
        div { --grid: [header] 1fr [content] 2fr [footer]; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Square brackets should be present
                assert!(
                    tokens.contains(&CssToken::OpenSquare),
                    "Expected OpenSquare: {tokens:?}"
                );
                assert!(
                    tokens.contains(&CssToken::CloseSquare),
                    "Expected CloseSquare: {tokens:?}"
                );
                // Should have fr units
                let fr_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::Dimension(_, CssUnit::Fr)))
                    .count();
                assert_eq!(fr_count, 2, "Expected 2 fr dimensions: {tokens:?}");
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_delim_slash_token() {
    let ir = parse(css!(
        r#"
        div { font: 16px/1.5 Arial; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    tokens.contains(&CssToken::Delim('/')),
                    "Expected '/' delimiter in font shorthand: {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_delim_star_token() {
    let ir = parse(css!(
        r#"
        div { width: calc(100% * 0.5); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    tokens.contains(&CssToken::Delim('*')),
                    "Expected '*' delimiter in calc: {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_hash_type_id() {
    // Hex colors that are valid identifiers get HashType::Id
    let ir = parse(css!(
        r#"
        div {
            color: #abcdef;
            background-color: #123;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Hash("abcdef".to_string(), HashType::Id)])
        );
        // #123 starts with a digit, so cssparser classifies it as Unrestricted hash
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Hash(
                "123".to_string(),
                HashType::Unrestricted
            )])
        );
    }
}

#[test]
fn test_dimension_various_units() {
    let ir = parse(css!(
        r#"
        div {
            font-size: 12pt;
            width: 2.5cm;
            height: 10mm;
            border-width: 1pc;
            padding: 1in;
            min-width: 50vmin;
            max-width: 50vmax;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        // font-size: 12pt → Raw(Dimension(12.0, Pt))
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(12.0, CssUnit::Pt)])
        );
        // width: 2.5cm → Size(...)
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                2.5,
                CssUnit::Cm
            )))
        );
        // height: 10mm
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                10.0,
                CssUnit::Mm
            )))
        );
        // border-width: 1pc → Raw
        assert_eq!(
            s.declarations[3].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(1.0, CssUnit::Pc)])
        );
        // padding: 1in → Raw (padding is a shorthand, maps to Other)
        assert_eq!(
            s.declarations[4].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(1.0, CssUnit::In)])
        );
        // min-width: 50vmin → Size
        assert_eq!(
            s.declarations[5].value,
            PropertyValueIR::Size(SizeIR::LengthPercentage(NonNegativeLPIR::Length(
                50.0,
                CssUnit::Vmin
            )))
        );
        // max-width: 50vmax → MaxSize
        assert_eq!(
            s.declarations[6].value,
            PropertyValueIR::MaxSize(MaxSizeIR::LengthPercentage(NonNegativeLPIR::Length(
                50.0,
                CssUnit::Vmax
            )))
        );
    }
}

#[test]
fn test_important_with_whitespace_variants() {
    // !important with extra spacing
    let ir = parse(css!(
        r#"
        div {
            color: red !important;
            display: flex ! important;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert!(s.declarations[0].important);
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Ident("red".to_string())])
        );
        // cssparser normalizes `! important` to `!important`
        assert!(
            s.declarations[1].important,
            "Expected important flag for `! important` variant"
        );
    }
}

#[test]
fn test_calc_exact_tokens() {
    // cssparser strips whitespace inside function blocks, so calc tokens
    // are just the meaningful tokens without whitespace.
    let ir = parse(css!(
        r#"
        div { width: calc(100% - 20px); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert_eq!(
                    tokens,
                    &vec![
                        CssToken::Function("calc".to_string()),
                        CssToken::Percentage(100.0),
                        CssToken::Delim('-'),
                        CssToken::Dimension(20.0, CssUnit::Px),
                        CssToken::CloseParen,
                    ]
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_bare_number_zero_as_length() {
    // margin-left: 0 (bare zero) should be typed as MarginIR
    let ir = parse(css!(
        r#"
        div { margin-left: 0; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Margin(MarginIR::LengthPercentage(LengthPercentageIR::Length(
                0.0,
                CssUnit::Px
            )))
        );
    }
}

#[test]
fn test_comma_in_function_args() {
    let ir = parse(css!(
        r#"
        div { color: rgba(255, 128, 0, 0.5); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    matches!(&tokens[0], CssToken::Function(name) if name == "rgba"),
                    "Expected Function(rgba): {tokens:?}"
                );
                // Count commas inside the function
                let comma_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::Comma))
                    .count();
                assert_eq!(comma_count, 3, "Expected 3 commas in rgba(): {tokens:?}");
                // Should have 4 number tokens
                let num_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::Number(_)))
                    .count();
                assert_eq!(
                    num_count, 4,
                    "Expected 4 number tokens in rgba(): {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_multiple_strings() {
    let ir = parse(css!(
        r#"
        div { font-family: "Segoe UI", "Roboto", sans-serif; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                let string_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::String(_)))
                    .count();
                assert_eq!(string_count, 2, "Expected 2 string tokens: {tokens:?}");
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Ident(s) if s == "sans-serif")),
                    "Expected sans-serif ident: {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_resolution_units() {
    let ir = parse(css!(
        r#"
        div {
            --dpi-val: 96dpi;
            --dpcm-val: 300dpcm;
            --dppx-val: 2dppx;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(96.0, CssUnit::Dpi)])
        );
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(300.0, CssUnit::Dpcm)])
        );
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(2.0, CssUnit::Dppx)])
        );
    }
}

#[test]
fn test_time_units() {
    let ir = parse(css!(
        r#"
        div {
            --dur: 500ms;
            --dur2: 1.5s;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(500.0, CssUnit::Ms)])
        );
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(1.5, CssUnit::S)])
        );
    }
}

#[test]
fn test_angle_units() {
    let ir = parse(css!(
        r#"
        div {
            --angle1: 90deg;
            --angle2: 1.5rad;
            --angle3: 100grad;
            --angle4: 0.25turn;
        }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(90.0, CssUnit::Deg)])
        );
        assert_eq!(
            s.declarations[1].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(1.5, CssUnit::Rad)])
        );
        assert_eq!(
            s.declarations[2].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(100.0, CssUnit::Grad)])
        );
        assert_eq!(
            s.declarations[3].value,
            PropertyValueIR::Raw(vec![CssToken::Dimension(0.25, CssUnit::Turn)])
        );
    }
}

#[test]
fn test_parenthesis_block_in_custom_property() {
    let ir = parse(css!(
        r#"
        div { --expr: (a + b); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                assert!(
                    tokens.contains(&CssToken::OpenParen),
                    "Expected OpenParen: {tokens:?}"
                );
                assert!(
                    tokens.contains(&CssToken::CloseParen),
                    "Expected CloseParen: {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_triple_nested_functions() {
    let ir = parse(css!(
        r#"
        div { width: calc(max(min(100%, 200px), 50px) + 10px); }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Three function tokens: calc, max, min
                let fn_names: Vec<&str> = tokens
                    .iter()
                    .filter_map(|t| match t {
                        CssToken::Function(name) => Some(name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(fn_names, vec!["calc", "max", "min"]);
                // Three CloseParen tokens
                let close_count = tokens
                    .iter()
                    .filter(|t| matches!(t, CssToken::CloseParen))
                    .count();
                assert_eq!(close_count, 3);
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}

#[test]
fn test_negative_percentage() {
    let ir = parse(css!(
        r#"
        div { margin-left: -50%; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        assert_eq!(
            s.declarations[0].value,
            PropertyValueIR::Margin(MarginIR::LengthPercentage(LengthPercentageIR::Percentage(
                -50.0
            )))
        );
    }
}

#[test]
fn test_flex_shorthand_tokens() {
    let ir = parse(css!(
        r#"
        div { flex: 1 0 auto; }
    "#
    ));
    if let CssRuleIR::Style(s) = &ir.rules[0] {
        // flex is a shorthand → falls through to Raw
        match &s.declarations[0].value {
            PropertyValueIR::Raw(tokens) => {
                // Should contain Number(1), Number(0), Ident("auto")
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Number(v) if *v == 1.0)),
                    "Expected Number(1.0): {tokens:?}"
                );
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Number(v) if *v == 0.0)),
                    "Expected Number(0.0): {tokens:?}"
                );
                assert!(
                    tokens
                        .iter()
                        .any(|t| matches!(t, CssToken::Ident(s) if s == "auto")),
                    "Expected Ident(auto): {tokens:?}"
                );
            }
            other => panic!("Expected Raw value, got: {other:?}"),
        }
    }
}
