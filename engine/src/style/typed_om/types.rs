//! CSS Typed OM value types and their conversions.

use std::fmt;

use stylo_traits::{NumericValue, TypedValue};

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

// ─── Display formatting ─────────────────────────────────────────────

impl fmt::Display for CSSUnitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit == "number" {
            write!(f, "{}", self.value)
        } else if self.unit == "percent" {
            write!(f, "{}%", self.value)
        } else {
            write!(f, "{}{}", self.value, self.unit)
        }
    }
}

impl fmt::Display for CSSStyleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CSSStyleValue::Keyword(kw) => write!(f, "{}", kw.value),
            CSSStyleValue::Unit(u) => write!(f, "{u}"),
            CSSStyleValue::Sum(units) => {
                for (i, u) in units.iter().enumerate() {
                    if i > 0 {
                        write!(f, " + ")?;
                    }
                    write!(f, "{u}")?;
                }
                Ok(())
            }
            CSSStyleValue::Unparsed(s) => write!(f, "{s}"),
        }
    }
}
