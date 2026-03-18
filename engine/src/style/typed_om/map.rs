//! `StylePropertyMapReadOnly` — live handle to computed CSS properties.

use style::properties::{property_counts, LonghandId, PropertyId};
use stylo_traits::CssStringWriter;

use crate::dom::Document;
use crate::style::StyleContext;

use super::types::CSSStyleValue;

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
    /// Let props be the value of this's [[declarations]] internal slot.
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

        let mut entries = Vec::with_capacity(property_counts::LONGHANDS);
        for i in 0..property_counts::LONGHANDS {
            let id = longhand_id_from_index(i);
            let name = id.name();
            let value = extract_property(&cv, id);
            entries.push((name.to_owned(), value));
        }

        entries.sort_unstable_by(|a, b| {
            a.0.starts_with('-')
                .cmp(&b.0.starts_with('-'))
                .then_with(|| a.0.cmp(&b.0))
        });

        entries
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Returns the `LonghandId` for index `i`.
///
/// # Panics
///
/// Debug-asserts that `i` is in `0..property_counts::LONGHANDS`.
#[inline]
fn longhand_id_from_index(i: usize) -> LonghandId {
    debug_assert!(i < property_counts::LONGHANDS);
    // SAFETY: LonghandId is repr(u16) with variants 0..LONGHANDS-1.
    // The debug_assert above guards the bound; in release builds the
    // caller (a bounded loop) guarantees the invariant.
    unsafe { std::mem::transmute(i as u16) }
}

/// Extracts a single property's computed value from [`ComputedValues`].
///
/// Uses `computed_typed_value()` as the primary path (zero string allocation).
/// Falls back to `computed_or_resolved_value()` → CSS string for properties
/// whose computed types don't yet implement `ToTyped`.
pub(crate) fn extract_property(
    cv: &style::properties::ComputedValues,
    id: LonghandId,
) -> CSSStyleValue {
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
