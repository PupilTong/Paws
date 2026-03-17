//! CSS Typed OM implementation.
//!
//! Provides typed access to computed CSS property values following the
//! [CSS Typed OM spec](https://drafts.css-houdini.org/css-typed-om/).
//!
//! The primary entry point is [`StylePropertyMapReadOnly`], a live handle
//! returned by `Document::computed_style_map()`. Read operations lazily
//! trigger style resolution when the DOM tree is dirty.

use std::fmt;

use style::properties::{property_counts, LonghandId, PropertyId};
use stylo_traits::{CssStringWriter, NumericValue, TypedValue};

use crate::dom::Document;
use crate::style::StyleContext;

// ─── CSS Value Types ─────────────────────────────────────────────────

/// A single CSS value in the Typed OM.
///
/// Maps directly from Stylo's [`TypedValue`]. See the CSS Typed OM spec:
/// <https://drafts.css-houdini.org/css-typed-om/#cssstylevalue>
#[derive(Debug, Clone, PartialEq)]
pub enum CSSStyleValue {
    /// A keyword value (e.g. `block`, `none`, `auto`).
    /// Corresponds to `CSSKeywordValue` in the spec.
    Keyword(CSSKeywordValue),
    /// A single numeric value with a unit (e.g. `16px`, `50%`).
    /// Corresponds to `CSSUnitValue` in the spec.
    Unit(CSSUnitValue),
    /// A sum of numeric values (e.g. `calc(10px + 2em)`).
    /// Corresponds to `CSSMathSum` in the spec.
    Sum(Vec<CSSUnitValue>),
    /// Fallback for values that Stylo cannot yet reify into typed form.
    /// Serialized as a CSS string.
    Unparsed(String),
}

/// A CSS keyword value (e.g. `block`, `none`, `auto`, `inherit`).
///
/// Corresponds to `CSSKeywordValue` in the Typed OM spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CSSKeywordValue {
    /// The keyword string (lowercase, e.g. `"flex"`, `"auto"`).
    pub value: String,
}

/// A CSS numeric value with a unit (e.g. `16px`, `50%`, `2em`).
///
/// Corresponds to `CSSUnitValue` in the Typed OM spec.
/// The `unit` field uses Stylo's canonical unit strings: `"px"`, `"em"`,
/// `"percent"`, `"number"`, `"deg"`, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct CSSUnitValue {
    /// The numeric component of the value.
    pub value: f32,
    /// The unit string (e.g. `"px"`, `"percent"`, `"number"`).
    pub unit: String,
}

// ─── StylePropertyMapReadOnly ────────────────────────────────────────

/// Read-only live handle to an element's computed CSS property values.
///
/// Corresponds to `StylePropertyMapReadOnly` in the CSS Typed OM spec.
/// Holds an `element_id` — read operations lazily resolve styles when
/// the DOM tree has dirty descendants.
///
/// Created via [`Document::computed_style_map()`].
#[derive(Debug, Clone, Copy)]
pub struct StylePropertyMapReadOnly {
    element_id: usize,
}

// TODO: support custom properties( CSS variable )
impl StylePropertyMapReadOnly {
    /// Creates a new live handle for the given element.
    pub(crate) fn new(element_id: usize) -> Self {
        Self { element_id }
    }

    /// Returns the element ID this handle refers to.
    pub fn element_id(&self) -> usize {
        self.element_id
    }

    /// Returns the number of CSS longhand properties.
    ///
    /// This is a constant — every longhand property is always present
    /// in a computed style map.
    pub fn size(&self) -> usize {
        property_counts::LONGHANDS
    }

    /// Returns the computed value for a single CSS property.
    ///
    /// Triggers style resolution if the tree is dirty.
    /// Returns `None` if the property name is invalid or the element
    /// no longer exists.
    pub fn get(
        &self,
        property: &str,
        doc: &mut Document,
        ctx: &StyleContext,
    ) -> Option<CSSStyleValue> {
        let longhand_id = resolve_longhand(property)?;
        doc.ensure_styles_resolved(ctx);
        let cv = doc.get_node(self.element_id)?.computed_values.as_ref()?;
        Some(extract_property(cv, longhand_id))
    }

    /// Returns all computed values for a CSS property.
    ///
    /// For most properties this returns a single-element Vec.
    /// Triggers style resolution if the tree is dirty.
    /// TODO: support the real get_all, test it by using multiple background-image
    /// See https://developer.mozilla.org/en-US/docs/Web/API/StylePropertyMapReadOnly/getAll
    pub fn get_all(
        &self,
        property: &str,
        doc: &mut Document,
        ctx: &StyleContext,
    ) -> Vec<CSSStyleValue> {
        match self.get(property, doc, ctx) {
            Some(v) => vec![v],
            None => Vec::new(),
        }
    }

    /// Returns whether the given property is present in the map.
    ///
    /// Returns `true` for any valid CSS longhand property name.
    /// TODO: This implementation is not correct
    /// This is supposed to follow the spec: (https://drafts.css-houdini.org/css-typed-om/#dom-stylepropertymapreadonly-has)
    /// The has(property) method, when called on a StylePropertyMapReadOnly this, must perform the following steps:
    /// If property is not a custom property name string, set property to property ASCII lowercased.
    ///     If property is not a valid CSS property, throw a TypeError.
    /// Let props be the value of this’s [[declarations]] internal slot.
    /// If props[property] exists, return true. Otherwise, return false.
    ///
    /// here is a bad case(pseudo code):
    /// <div style="--foo: 1;"></div>
    /// div.computed_style_map().has("--foo") should return true
    pub fn has(&self, property: &str) -> bool {
        resolve_longhand(property).is_some()
    }

    /// Materializes the entire computed style map as a sorted list of
    /// `(property_name, CSSStyleValue)` pairs.
    ///
    /// Properties are sorted alphabetically: standard properties first,
    /// then vendor-prefixed (e.g. `-webkit-*`).
    ///
    /// Triggers style resolution if the tree is dirty.
    pub fn to_vec(&self, doc: &mut Document, ctx: &StyleContext) -> Vec<(String, CSSStyleValue)> {
        doc.ensure_styles_resolved(ctx);
        let cv = match doc
            .get_node(self.element_id)
            .and_then(|n| n.computed_values.as_ref())
        {
            Some(cv) => cv.clone(),
            None => return Vec::new(),
        };

        let mut standard = Vec::new();
        let mut vendor = Vec::new();

        for i in 0..property_counts::LONGHANDS {
            // SAFETY: LonghandId is repr(u16) with variants 0..LONGHANDS-1.
            // We iterate within bounds, so the transmute is valid.
            let id: LonghandId = unsafe { std::mem::transmute(i as u16) };
            let name = id.name();
            let value = extract_property(&cv, id);

            if name.starts_with('-') {
                vendor.push((name.to_owned(), value));
            } else {
                standard.push((name.to_owned(), value));
            }
        }

        standard.sort_by(|a, b| a.0.cmp(&b.0));
        vendor.sort_by(|a, b| a.0.cmp(&b.0));
        standard.extend(vendor);
        standard
    }
}

// ─── Value Conversion ────────────────────────────────────────────────

impl From<TypedValue> for CSSStyleValue {
    fn from(tv: TypedValue) -> Self {
        match tv {
            TypedValue::Keyword(s) => CSSStyleValue::Keyword(CSSKeywordValue { value: s }),
            TypedValue::Numeric(NumericValue::Unit(uv)) => CSSStyleValue::Unit(CSSUnitValue {
                value: uv.value,
                unit: uv.unit.clone(),
            }),
            TypedValue::Numeric(NumericValue::Sum(sum)) => {
                let units: Vec<CSSUnitValue> = sum
                    .values
                    .iter()
                    .filter_map(|v| match v {
                        NumericValue::Unit(uv) => Some(CSSUnitValue {
                            value: uv.value,
                            unit: uv.unit.clone(),
                        }),
                        // Nested sums are flattened — this shouldn't occur in practice
                        // for computed values, but we skip them defensively.
                        NumericValue::Sum(..) => None,
                    })
                    .collect();
                CSSStyleValue::Sum(units)
            }
        }
    }
}

/// Extracts a single property's computed value from [`ComputedValues`].
///
/// Uses `computed_typed_value()` as the primary path (zero string allocation).
/// Falls back to `computed_or_resolved_value()` → CSS string for properties
/// whose computed types don't yet implement `ToTyped`.
fn extract_property(cv: &style::properties::ComputedValues, id: LonghandId) -> CSSStyleValue {
    // Primary: direct typed extraction
    if let Some(tv) = cv.computed_typed_value(id) {
        return tv.into();
    }

    // Fallback: serialize to CSS string
    let mut css_string = CssStringWriter::new();
    let _ = cv.computed_or_resolved_value(id, None, &mut css_string);
    CSSStyleValue::Unparsed(css_string)
}

/// Resolves a CSS property name string to a [`LonghandId`].
///
/// Returns `None` for unknown, shorthand, or custom properties.
fn resolve_longhand(name: &str) -> Option<LonghandId> {
    let prop_id = PropertyId::parse_enabled_for_all_content(name).ok()?;
    match prop_id {
        PropertyId::NonCustom(non_custom) => non_custom.as_longhand(),
        PropertyId::Custom(_) => None,
    }
}

impl fmt::Display for CSSStyleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CSSStyleValue::Keyword(kw) => write!(f, "{}", kw.value),
            CSSStyleValue::Unit(u) => {
                if u.unit == "number" {
                    write!(f, "{}", u.value)
                } else if u.unit == "percent" {
                    write!(f, "{}%", u.value)
                } else {
                    write!(f, "{}{}", u.value, u.unit)
                }
            }
            CSSStyleValue::Sum(units) => {
                for (i, u) in units.iter().enumerate() {
                    if i > 0 {
                        write!(f, " + ")?;
                    }
                    if u.unit == "number" {
                        write!(f, "{}", u.value)?;
                    } else if u.unit == "percent" {
                        write!(f, "{}%", u.value)?;
                    } else {
                        write!(f, "{}{}", u.value, u.unit)?;
                    }
                }
                Ok(())
            }
            CSSStyleValue::Unparsed(s) => write!(f, "{}", s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeState;
    use stylo_traits::{MathSum, UnitValue};

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
        assert!(resolve_longhand("display").is_some());
        assert!(resolve_longhand("width").is_some());
        assert!(resolve_longhand("color").is_some());
        assert!(resolve_longhand("font-size").is_some());
    }

    #[test]
    fn test_resolve_longhand_invalid() {
        // Shorthand properties
        assert!(resolve_longhand("margin").is_none());
        assert!(resolve_longhand("padding").is_none());
        // Unknown properties
        assert!(resolve_longhand("not-a-property").is_none());
        assert!(resolve_longhand("").is_none());
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
        use paws_style_ir::{
            CssComponentValue, CssPropertyName, CssRuleIR, CssUnit, PropertyDeclarationIR,
            StyleRuleIR, StyleSheetIR,
        };
        let rules = vec![CssRuleIR::Style(StyleRuleIR {
            selectors: ".test-box".to_string(),
            declarations: vec![
                PropertyDeclarationIR {
                    name: CssPropertyName::Display,
                    value: vec![CssComponentValue::Ident("flex".to_string())],
                    important: false,
                },
                PropertyDeclarationIR {
                    name: CssPropertyName::Width,
                    value: vec![CssComponentValue::Number(50.0, CssUnit::Percent)],
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

        use paws_style_ir::{
            CssComponentValue, CssPropertyName, CssRuleIR, CssUnit, PropertyDeclarationIR,
            StyleRuleIR, StyleSheetIR,
        };
        let rules = vec![
            CssRuleIR::Style(StyleRuleIR {
                selectors: ".box-1".to_string(),
                declarations: vec![
                    PropertyDeclarationIR {
                        name: CssPropertyName::Display,
                        value: vec![CssComponentValue::Ident("block".to_string())],
                        important: false,
                    },
                    PropertyDeclarationIR {
                        name: CssPropertyName::Width,
                        value: vec![CssComponentValue::Number(100.0, CssUnit::Px)],
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
                        value: vec![CssComponentValue::Ident("inline".to_string())],
                        important: false,
                    },
                    PropertyDeclarationIR {
                        name: CssPropertyName::Width,
                        value: vec![CssComponentValue::Number(25.0, CssUnit::Percent)],
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
        assert_eq!(map.size(), property_counts::LONGHANDS);
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
}
