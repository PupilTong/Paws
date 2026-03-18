//! Tests for the CSS Typed OM implementation.

use super::map::{extract_property, StylePropertyMapReadOnly};
use super::types::{CSSKeywordValue, CSSStyleValue, CSSUnitValue};
use crate::runtime::RuntimeState;

use style::properties::LonghandId;
use stylo_traits::{MathSum, NumericValue, TypedValue, UnitValue};

fn make_runtime() -> RuntimeState {
    RuntimeState::new("https://example.com".to_string())
}

#[test]
fn test_from_typed_value_keyword() {
    let tv = TypedValue::Keyword("block".to_string());
    let result: CSSStyleValue = tv.into();
    assert_eq!(
        result,
        CSSStyleValue::Keyword(CSSKeywordValue {
            value: "block".to_string()
        })
    );
}

#[test]
fn test_from_typed_value_unit() {
    let tv = TypedValue::Numeric(NumericValue::Unit(UnitValue {
        value: 16.0,
        unit: "px".to_string(),
    }));
    let result: CSSStyleValue = tv.into();
    assert_eq!(
        result,
        CSSStyleValue::Unit(CSSUnitValue {
            value: 16.0,
            unit: "px".to_string()
        })
    );
}

#[test]
fn test_from_typed_value_sum() {
    use thin_vec::thin_vec;
    let tv = TypedValue::Numeric(NumericValue::Sum(MathSum {
        values: thin_vec![
            NumericValue::Unit(UnitValue {
                value: 10.0,
                unit: "px".to_string()
            }),
            NumericValue::Unit(UnitValue {
                value: 2.0,
                unit: "em".to_string()
            }),
        ],
    }));
    let result: CSSStyleValue = tv.into();
    assert_eq!(
        result,
        CSSStyleValue::Sum(vec![
            CSSUnitValue {
                value: 10.0,
                unit: "px".to_string()
            },
            CSSUnitValue {
                value: 2.0,
                unit: "em".to_string()
            },
        ])
    );
}

#[test]
fn test_resolve_longhand_valid() {
    // Test via the public `has` method which calls resolve_longhand internally
    let map = StylePropertyMapReadOnly::new(0);
    assert!(map.has("display"));
    assert!(map.has("width"));
    assert!(map.has("color"));
    assert!(map.has("font-size"));
}

#[test]
fn test_resolve_longhand_invalid() {
    let map = StylePropertyMapReadOnly::new(0);
    // Shorthand properties
    assert!(!map.has("margin"));
    assert!(!map.has("padding"));
    // Unknown properties
    assert!(!map.has("not-a-property"));
    assert!(!map.has(""));
}

#[test]
fn test_computed_style_map_display() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();
    state
        .set_inline_style(el, "display".to_string(), "flex".to_string())
        .unwrap();

    let map = state.computed_style_map(el).unwrap();
    let value = map.get("display", &mut state.doc, &state.style_context);

    // Display goes through either TypedValue::Keyword or Unparsed fallback
    // depending on Stylo's ToTyped coverage.
    match &value {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "flex"),
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "flex"),
        other => panic!("Expected flex keyword or unparsed, got: {other:?}"),
    }
}

#[test]
fn test_computed_style_map_width_px() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();
    state
        .set_inline_style(el, "width".to_string(), "100px".to_string())
        .unwrap();

    let map = state.computed_style_map(el).unwrap();
    let value = map.get("width", &mut state.doc, &state.style_context);

    // Width may come through as Unit or Unparsed depending on ToTyped coverage
    match &value {
        Some(CSSStyleValue::Unit(u)) => {
            assert_eq!(u.value, 100.0);
            assert_eq!(u.unit, "px");
        }
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "100px"),
        other => panic!("Expected 100px unit or unparsed, got: {other:?}"),
    }
}

#[test]
fn test_computed_style_map_width_percent() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();
    state
        .set_inline_style(el, "width".to_string(), "100%".to_string())
        .unwrap();

    let map = state.computed_style_map(el).unwrap();
    let value = map.get("width", &mut state.doc, &state.style_context);

    // Computed value should preserve percentage (not resolve to px).
    match &value {
        Some(CSSStyleValue::Unit(u)) => {
            assert_eq!(u.value, 100.0);
            assert_eq!(u.unit, "percent");
        }
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "100%"),
        other => panic!("Expected 100% unit or unparsed, got: {other:?}"),
    }
}

#[test]
fn test_behavior_parsed_stylesheet_typed_properties() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();
    state
        .set_attribute(el, "class".to_string(), "test-box".to_string())
        .unwrap();

    // Construct a typed StyleSheetIR that represents:
    // .test-box { display: flex; width: 50%; }
    use paws_style_ir::values::*;
    use paws_style_ir::{
        CssPropertyName, CssRuleIR, PropertyDeclarationIR, PropertyValueIR, StyleRuleIR,
        StyleSheetIR,
    };
    let rules = vec![CssRuleIR::Style(StyleRuleIR {
        selectors: ".test-box".to_string(),
        declarations: vec![
            PropertyDeclarationIR {
                name: CssPropertyName::Display,
                value: PropertyValueIR::Display(DisplayIR::Flex),
                important: false,
            },
            PropertyDeclarationIR {
                name: CssPropertyName::Width,
                value: PropertyValueIR::Size(SizeIR::LengthPercentage(
                    NonNegativeLPIR::Percentage(50.0),
                )),
                important: false,
            },
        ],
        rules: vec![],
    })];
    let stylesheet = StyleSheetIR { rules };
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&stylesheet).unwrap();

    // Apply via zero-copy engine path (this tests our direct AST mapping)
    state.add_parsed_stylesheet(&bytes);

    // Fetch the computed styles
    let map = state.computed_style_map(el).unwrap();

    let display = map.get("display", &mut state.doc, &state.style_context);
    match &display {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "flex"),
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "flex"),
        _ => panic!("Expected Display property to be Flex, got: {:?}", display),
    };

    let width = map.get("width", &mut state.doc, &state.style_context);
    match &width {
        Some(CSSStyleValue::Unit(u)) => {
            assert_eq!(u.value, 50.0);
            assert_eq!(u.value, 50.0);
        }
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "50%"),
        _ => panic!("Expected Width property to be 50%, got: {:?}", width),
    }
}

#[test]
fn test_behavior_parsed_stylesheet_multiple_rules() {
    let mut state = make_runtime();
    let el1 = state.create_element("div".to_string());
    state.append_element(0, el1).unwrap();
    state
        .set_attribute(el1, "class".to_string(), "box-1".to_string())
        .unwrap();

    let el2 = state.create_element("div".to_string());
    state.append_element(0, el2).unwrap();
    state
        .set_attribute(el2, "class".to_string(), "box-2".to_string())
        .unwrap();

    use paws_style_ir::values::*;
    use paws_style_ir::{
        CssPropertyName, CssRuleIR, CssUnit, PropertyDeclarationIR, PropertyValueIR, StyleRuleIR,
        StyleSheetIR,
    };
    let rules = vec![
        CssRuleIR::Style(StyleRuleIR {
            selectors: ".box-1".to_string(),
            declarations: vec![
                PropertyDeclarationIR {
                    name: CssPropertyName::Display,
                    value: PropertyValueIR::Display(DisplayIR::Block),
                    important: false,
                },
                PropertyDeclarationIR {
                    name: CssPropertyName::Width,
                    value: PropertyValueIR::Size(SizeIR::LengthPercentage(
                        NonNegativeLPIR::Length(100.0, CssUnit::Px),
                    )),
                    important: false,
                },
            ],
            rules: vec![],
        }),
        CssRuleIR::Style(StyleRuleIR {
            selectors: ".box-2".to_string(),
            declarations: vec![
                PropertyDeclarationIR {
                    name: CssPropertyName::Display,
                    value: PropertyValueIR::Display(DisplayIR::Inline),
                    important: false,
                },
                PropertyDeclarationIR {
                    name: CssPropertyName::Width,
                    value: PropertyValueIR::Size(SizeIR::LengthPercentage(
                        NonNegativeLPIR::Percentage(25.0),
                    )),
                    important: false,
                },
            ],
            rules: vec![],
        }),
    ];
    let stylesheet = StyleSheetIR { rules };
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&stylesheet).unwrap();

    state.add_parsed_stylesheet(&bytes);

    // Check el1
    let map1 = state.computed_style_map(el1).unwrap();
    let display1 = map1.get("display", &mut state.doc, &state.style_context);
    match display1 {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "block"),
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "block"),
        _ => panic!("Expected block for box-1"),
    }
    let width1 = map1.get("width", &mut state.doc, &state.style_context);
    match &width1 {
        Some(CSSStyleValue::Unit(u)) => {
            assert_eq!(u.value, 100.0);
        }
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "100px"),
        _ => panic!("Expected 100px for box-1"),
    }

    // Check el2
    let map2 = state.computed_style_map(el2).unwrap();
    let display2 = map2.get("display", &mut state.doc, &state.style_context);
    match display2 {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "inline"),
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "inline"),
        _ => panic!("Expected inline for box-2"),
    }
    let width2 = map2.get("width", &mut state.doc, &state.style_context);
    match &width2 {
        Some(CSSStyleValue::Unit(u)) => {
            assert_eq!(u.value, 25.0);
        }
        Some(CSSStyleValue::Unparsed(val)) => assert_eq!(val, "25%"),
        _ => panic!("Expected 25% for box-2"),
    }
}

#[test]
fn test_computed_style_map_has() {
    let map = StylePropertyMapReadOnly::new(0);
    assert!(map.has("display"));
    assert!(map.has("width"));
    assert!(map.has("color"));
    assert!(!map.has("margin")); // shorthand
    assert!(!map.has("not-a-property"));
}

#[test]
fn test_computed_style_map_size() {
    let map = StylePropertyMapReadOnly::new(0);
    assert!(map.size() > 0);
    assert_eq!(map.size(), style::properties::property_counts::LONGHANDS);
}

#[test]
fn test_computed_style_map_to_vec_sorted() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();

    let map = state.computed_style_map(el).unwrap();
    let entries = map.to_vec(&mut state.doc, &state.style_context);

    assert!(!entries.is_empty());

    // Verify standard properties come before vendor-prefixed ones
    let first_vendor_idx = entries.iter().position(|(name, _)| name.starts_with('-'));
    if let Some(vendor_idx) = first_vendor_idx {
        // All entries before vendor_idx should be non-prefixed
        for (name, _) in &entries[..vendor_idx] {
            assert!(
                !name.starts_with('-'),
                "Standard property {name} found after vendor prefix"
            );
        }
    }

    // Verify alphabetical ordering within standard properties
    let standard_names: Vec<&str> = entries
        .iter()
        .filter(|(name, _)| !name.starts_with('-'))
        .map(|(name, _)| name.as_str())
        .collect();
    for window in standard_names.windows(2) {
        assert!(
            window[0] <= window[1],
            "Properties out of order: {} > {}",
            window[0],
            window[1]
        );
    }
}

#[test]
fn test_direct_resolve_with_inline_style() {
    // Verifies that resolve_style picks up inline styles correctly.
    // Note: uses "block" instead of "grid" because Stylo in Servo mode
    // gates grid behind `layout.grid.enabled` which defaults to false.
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();
    state
        .set_inline_style(el, "display".to_string(), "block".to_string())
        .unwrap();

    // Verify inline style was set
    let has_style = state
        .doc
        .get_node(el as usize)
        .unwrap()
        .style_attribute
        .is_some();
    assert!(
        has_style,
        "Element should have style_attribute after set_inline_style"
    );

    // Direct resolve
    state.doc.resolve_style(&state.style_context);

    let cv = state
        .doc
        .get_node(el as usize)
        .unwrap()
        .computed_values
        .as_ref()
        .expect("Element should have computed_values after resolve");

    let display_value = extract_property(cv, LonghandId::Display);
    match &display_value {
        CSSStyleValue::Keyword(kw) => assert_eq!(kw.value, "block"),
        CSSStyleValue::Unparsed(s) => assert_eq!(s, "block"),
        other => panic!("Expected block, got: {other:?}"),
    }
}

#[test]
fn test_lazy_resolution_sees_updates() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();

    // Set inline style, then get via map (ensure_styles_resolved triggers resolve)
    state
        .set_inline_style(el, "display".to_string(), "block".to_string())
        .unwrap();

    let map = state.computed_style_map(el).unwrap();
    let value = map.get("display", &mut state.doc, &state.style_context);

    match &value {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "block"),
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "block"),
        other => panic!("Expected block keyword or unparsed, got: {other:?}"),
    }
}

#[test]
fn test_live_handle_sees_later_style_changes() {
    let mut state = make_runtime();
    let el = state.create_element("div".to_string());
    state.append_element(0, el).unwrap();

    // Create handle first
    let map = state.computed_style_map(el).unwrap();

    // First read triggers resolve (display should be initial value)
    let initial = map.get("display", &mut state.doc, &state.style_context);
    match &initial {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "inline"),
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "inline"),
        other => panic!("Expected initial display value, got: {other:?}"),
    }

    // Now change the style (set_inline_style marks ancestors dirty)
    state
        .set_inline_style(el, "display".to_string(), "block".to_string())
        .unwrap();

    // Live handle should see the updated value after lazy re-resolution
    let updated = map.get("display", &mut state.doc, &state.style_context);
    match &updated {
        Some(CSSStyleValue::Keyword(kw)) => assert_eq!(kw.value, "block"),
        Some(CSSStyleValue::Unparsed(s)) => assert_eq!(s, "block"),
        other => panic!("Expected block after update, got: {other:?}"),
    }
}

#[test]
fn test_invalid_element_returns_none() {
    let state = make_runtime();
    // Non-existent element
    assert!(state.computed_style_map(999).is_err());
}

#[test]
fn test_display_formatting() {
    let kw = CSSStyleValue::Keyword(CSSKeywordValue {
        value: "flex".to_string(),
    });
    assert_eq!(kw.to_string(), "flex");

    let unit = CSSStyleValue::Unit(CSSUnitValue {
        value: 16.0,
        unit: "px".to_string(),
    });
    assert_eq!(unit.to_string(), "16px");

    let pct = CSSStyleValue::Unit(CSSUnitValue {
        value: 50.0,
        unit: "percent".to_string(),
    });
    assert_eq!(pct.to_string(), "50%");

    let num = CSSStyleValue::Unit(CSSUnitValue {
        value: 1.5,
        unit: "number".to_string(),
    });
    assert_eq!(num.to_string(), "1.5");
}
