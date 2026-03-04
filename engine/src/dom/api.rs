use crate::dom::PawsElement;

/// Modern DOM API for getting computed styles without re-parsing.
pub trait ModernDomApi<'a> {
    /// Returns a map-like structure for the computed style,
    /// rather than stringified CSS text as legacy `getComputedStyle` did.
    fn get_computed_style_map(
        &'a self,
        context: &'a crate::style::StyleContext,
    ) -> ComputedStyleMap<'a>;
}

pub struct ComputedStyleMap<'a> {
    node: &'a PawsElement,
    context: &'a crate::style::StyleContext,
}

impl<'a> ComputedStyleMap<'a> {
    pub fn new(node: &'a PawsElement, context: &'a crate::style::StyleContext) -> Self {
        Self { node, context }
    }

    /// Gets a specific property value directly from the Stylo computed values,
    /// returning it in a structured or typed format rather than a raw string.
    /// (For now it still returns `String` via `serialize_computed_value`, but represents the typed getter API).
    pub fn get(&self, property_name: &str) -> Option<String> {
        self.node
            .get_computed_style_by_key(self.context, property_name)
    }
}

impl<'a> ModernDomApi<'a> for PawsElement {
    fn get_computed_style_map(
        &'a self,
        context: &'a crate::style::StyleContext,
    ) -> ComputedStyleMap<'a> {
        ComputedStyleMap::new(self, context)
    }
}
