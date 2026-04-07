//! `TElement` implementation for `&PawsElement`.

use style::context::SharedStyleContext;
use style::data::{ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::dom::{LayoutIterator, TElement};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{AttrValue, Lang, PseudoElement};
use style::servo_arc::Arc;
use style::shared_lock::Locked;
use stylo_dom::ElementState;

use selectors::matching::{ElementSelectorFlags, VisitedHandlingMode};
use selectors::sink::Push;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::values::AtomIdent;
use style::LocalName;

use app_units::Au;
use euclid::default::Size2D;

use crate::dom::PawsElement;

use super::ChildrenIterator;

impl<'a, S: Default + Send + 'static> TElement for &'a PawsElement<S> {
    type ConcreteNode = &'a PawsElement<S>;
    type TraversalChildrenIterator = ChildrenIterator<'a, S>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn traversal_children(&self) -> LayoutIterator<ChildrenIterator<'a, S>> {
        LayoutIterator(ChildrenIterator {
            node: *self,
            index: 0,
        })
    }

    fn is_html_element(&self) -> bool {
        self.is_element()
    }

    fn is_mathml_element(&self) -> bool {
        false
    }
    fn is_svg_element(&self) -> bool {
        false
    }

    fn style_attribute(
        &self,
    ) -> Option<style::servo_arc::ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.style_attribute.as_ref().map(|a| a.borrow_arc())
    }

    fn state(&self) -> ElementState {
        self.element_state
    }

    fn has_dirty_descendants(&self) -> bool {
        PawsElement::<S>::has_dirty_descendants(self)
    }

    unsafe fn set_dirty_descendants(&self) {
        PawsElement::<S>::set_dirty_descendants(self);
    }

    unsafe fn unset_dirty_descendants(&self) {
        PawsElement::<S>::unset_dirty_descendants(self);
    }

    fn store_children_to_process(&self, _c: isize) {}
    fn did_process_child(&self) -> isize {
        0
    }

    fn may_have_animations(&self) -> bool {
        false
    }
    fn has_animations(&self, _: &SharedStyleContext) -> bool {
        false
    }
    fn has_css_animations(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }
    fn has_css_transitions(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }

    fn shadow_root(&self) -> Option<Self::ConcreteNode> {
        self.shadow_root_id.map(|id| self.with(id))
    }

    fn containing_shadow(&self) -> Option<Self::ConcreteNode> {
        use crate::dom::NodeType;
        let mut current = self.parent;
        while let Some(id) = current {
            let node = self.with(id);
            if node.node_type == NodeType::ShadowRoot {
                return Some(node);
            }
            if node.node_type == NodeType::Document {
                return None;
            }
            current = node.parent;
        }
        None
    }

    fn lang_attr(&self) -> Option<AttrValue> {
        None
    }
    fn match_element_lang(&self, _override: Option<Option<AttrValue>>, _value: &Lang) -> bool {
        false
    }

    fn is_html_document_body_element(&self) -> bool {
        false
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _visited_handling: VisitedHandlingMode,
        _hints: &mut V,
    ) where
        V: Push<ApplicableDeclarationBlock>,
    {
    }

    fn local_name(
        &self,
    ) -> &<style::selector_parser::SelectorImpl as selectors::SelectorImpl>::BorrowedLocalName {
        &self.name.as_ref().unwrap().local
    }

    fn namespace(
        &self,
    ) -> &<style::selector_parser::SelectorImpl as selectors::SelectorImpl>::BorrowedNamespaceUrl
    {
        &self.name.as_ref().unwrap().ns
    }

    fn query_container_size(
        &self,
        _display: &style::values::computed::Display,
    ) -> Size2D<Option<Au>> {
        Size2D::new(None, None)
    }

    fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        let current = self.selector_flags.borrow();
        current.contains(flags)
    }

    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::empty()
    }

    fn implemented_pseudo_element(&self) -> Option<PseudoElement> {
        None
    }

    fn animation_rule(
        &self,
        _: &SharedStyleContext<'_>,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn transition_rule(
        &self,
        _: &SharedStyleContext<'_>,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn has_part_attr(&self) -> bool {
        false
    }
    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&stylo_atoms::Atom> {
        self.id_attr.as_ref()
    }

    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        for c in &self.classes {
            callback(AtomIdent::cast(c));
        }
    }

    fn each_custom_state<F>(&self, _callback: F)
    where
        F: FnMut(&AtomIdent),
    {
    }

    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&LocalName),
    {
        for name in self.attrs.keys() {
            let local_name = LocalName::from(name.as_ref());
            callback(&local_name);
        }
    }

    fn has_snapshot(&self) -> bool {
        false
    }
    fn handled_snapshot(&self) -> bool {
        true
    }
    unsafe fn set_handled_snapshot(&self) {}
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        // SAFETY: Caller guarantees exclusive access to this element's data.
        // UnsafeCell provides the interior mutability needed here.
        let slot = &mut *self.stylo_element_data.get();
        slot.get_or_insert_with(ElementDataWrapper::default)
            .borrow_mut()
    }
    unsafe fn clear_data(&self) {
        // SAFETY: Caller guarantees exclusive access.
        *self.stylo_element_data.get() = None;
        *self.selector_flags.borrow_mut() = ElementSelectorFlags::empty();
    }
    fn has_data(&self) -> bool {
        // SAFETY: We only read the Option discriminant; no concurrent mutation
        // can occur while we hold a shared reference to the element.
        unsafe { (*self.stylo_element_data.get()).is_some() }
    }
    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        // SAFETY: Read-only access; ElementDataWrapper handles interior borrow tracking.
        unsafe {
            (*self.stylo_element_data.get())
                .as_ref()
                .map(|w| w.borrow())
        }
    }
    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        // SAFETY: ElementDataWrapper uses AtomicRefCell internally for borrow tracking.
        unsafe {
            (*self.stylo_element_data.get())
                .as_ref()
                .map(|w| w.borrow_mut())
        }
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::dom::Document;
    use markup5ever::QualName;
    use url::Url;

    #[test]
    fn test_element_get_computed_values() {
        // Create an element through document
        let guard = style::shared_lock::SharedRwLock::new();
        let mut doc: Document = Document::new(guard, Url::parse("http://test.com").unwrap());

        let id = doc.create_element(QualName::new(None, "".into(), "p".into()));
        let elem = doc.get_node(id).unwrap();

        // Should initially be None
        assert!(elem.get_computed_values().is_none());

        // After resolving styles, it should be Some
        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        doc.append_child(doc.root, id).unwrap(); // Must be in the tree to get styled!
        doc.resolve_style(&style_ctx);

        let elem = doc.get_node(id).unwrap();
        assert!(elem.get_computed_values().is_some());
    }
}
