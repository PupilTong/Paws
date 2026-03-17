use style::context::SharedStyleContext;
use style::data::{ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::dom::AttributeProvider;
use style::dom::{LayoutIterator, OpaqueNode};
use style::dom::{NodeInfo, TDocument, TElement, TNode, TShadowRoot};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{PseudoElement, SelectorImpl};
use style::servo_arc::{Arc, ArcBorrow};
use style::shared_lock::{Locked, SharedRwLock};
use stylo_dom::ElementState;

use crate::dom::{NodeFlags, NodeType, PawsElement};

use app_units::Au;
use euclid::default::Size2D;
use selectors::matching::{ElementSelectorFlags, VisitedHandlingMode};
use selectors::sink::Push;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::selector_parser::{AttrValue, Lang};

use selectors::attr::AttrSelectorOperation;
use style::values::{AtomIdent, AtomString};
use style::{CaseSensitivityExt, LocalName, Namespace};
use stylo_atoms::Atom;

// Custom iterator to avoid style::dom::LayoutIterator issues
pub struct ChildrenIterator<'a> {
    node: &'a PawsElement,
    index: usize,
}

impl<'a> Iterator for ChildrenIterator<'a> {
    type Item = &'a PawsElement;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.node.children.len() {
            let child_id = self.node.children[self.index];
            self.index += 1;
            Some(self.node.with(child_id))
        } else {
            None
        }
    }
}

// Implement NodeInfo for &PawsElement
impl NodeInfo for &PawsElement {
    fn is_element(&self) -> bool {
        self.node_type == NodeType::Element
    }

    fn is_text_node(&self) -> bool {
        self.node_type == NodeType::Text
    }
}

// Implement TNode for &PawsElement
impl<'a> TNode for &'a PawsElement {
    type ConcreteElement = &'a PawsElement;
    type ConcreteDocument = &'a PawsElement;
    type ConcreteShadowRoot = &'a PawsElement;

    fn parent_node(&self) -> Option<Self> {
        self.parent.map(|id| self.with(id))
    }

    fn first_child(&self) -> Option<Self> {
        self.children.first().map(|id| self.with(*id))
    }

    fn last_child(&self) -> Option<Self> {
        self.children.last().map(|id| self.with(*id))
    }

    fn prev_sibling(&self) -> Option<Self> {
        let parent = self.parent_node()?;
        let idx = parent.children.iter().position(|id| *id == self.id)?;
        if idx > 0 {
            Some(parent.with(parent.children[idx - 1]))
        } else {
            None
        }
    }

    fn next_sibling(&self) -> Option<Self> {
        let parent = self.parent_node()?;
        let idx = parent.children.iter().position(|id| *id == self.id)?;
        if idx + 1 < parent.children.len() {
            Some(parent.with(parent.children[idx + 1]))
        } else {
            None
        }
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        self.with(0) // Assume root is always doc and id 0
    }

    fn is_in_document(&self) -> bool {
        self.flags.contains(NodeFlags::IS_IN_DOCUMENT)
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        if self.is_element() {
            Some(self)
        } else {
            None
        }
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        if self.node_type == NodeType::Document {
            Some(self)
        } else {
            None
        }
    }

    fn as_shadow_root(&self) -> Option<&'a PawsElement> {
        if self.node_type == NodeType::ShadowRoot {
            Some(self)
        } else {
            None
        }
    }

    fn opaque(&self) -> OpaqueNode {
        let ptr: *const PawsElement = *self;
        // SAFETY: OpaqueNode is a newtype around usize, used as an opaque identity token
        // by Stylo. We transmute a valid pointer-as-usize into OpaqueNode. The value is
        // only used for identity comparison, never dereferenced back to a pointer by Stylo.
        unsafe { std::mem::transmute(ptr as usize) }
    }

    fn debug_id(self) -> usize {
        self.id
    }

    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.parent_element()
    }
}

// Ensure ElementData has methods required
impl<'a> TElement for &'a PawsElement {
    type ConcreteNode = &'a PawsElement;
    type TraversalChildrenIterator = ChildrenIterator<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn traversal_children(&self) -> LayoutIterator<ChildrenIterator<'a>> {
        LayoutIterator(ChildrenIterator {
            node: self,
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

    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.style_attribute.as_ref().map(|a| a.borrow_arc())
    }

    fn state(&self) -> ElementState {
        self.element_state
    }

    fn has_dirty_descendants(&self) -> bool {
        PawsElement::has_dirty_descendants(self)
    }

    unsafe fn set_dirty_descendants(&self) {
        PawsElement::set_dirty_descendants(self);
    }

    unsafe fn unset_dirty_descendants(&self) {
        PawsElement::unset_dirty_descendants(self);
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

    fn id(&self) -> Option<&Atom> {
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

// Stub TDocument
impl<'a> TDocument for &'a PawsElement {
    type ConcreteNode = &'a PawsElement;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> style::context::QuirksMode {
        style::context::QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        // SAFETY: TDocument requires returning a reference with the lifetime of the document.
        // Since `self` is `&'a PawsElement` and `guard` is a field of PawsElement, the
        // reference is valid for 'a. The transmute extends the borrow lifetime to match
        // the trait's required lifetime, which is sound because the PawsElement (and its
        // guard) lives at least as long as the reference.
        unsafe { std::mem::transmute(&self.guard) }
    }
}

impl<'a> TShadowRoot for &'a PawsElement {
    type ConcreteNode = &'a PawsElement;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }
    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        // Technically host requires a separate host tracking, simplified to `self.parent`
        self.parent.map(|id| self.with(id)).unwrap()
    }
    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        None
    }
}

// Implement selectors::Element for &PawsElement
impl selectors::Element for &PawsElement {
    type Impl = SelectorImpl;

    fn opaque(&self) -> selectors::OpaqueElement {
        let ptr: *const PawsElement = *self;
        // SAFETY: OpaqueElement is a newtype around usize used by the selectors crate as
        // an opaque identity token. We transmute a valid pointer-as-usize. The value is
        // only used for identity comparison within selector matching, never dereferenced.
        unsafe { std::mem::transmute(ptr as usize) }
    }

    fn parent_element(&self) -> Option<Self> {
        self.parent_node().and_then(|n| n.as_element())
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        self.parent_node()
            .map(|n| n.node_type == NodeType::ShadowRoot)
            .unwrap_or(false)
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn pseudo_element_originating_element(&self) -> Option<Self> {
        None
    }

    fn assigned_slot(&self) -> Option<Self> {
        None
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let mut curr = self.prev_sibling();
        while let Some(n) = curr {
            if n.is_element() {
                return Some(n);
            }
            curr = n.prev_sibling();
        }
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let mut curr = self.next_sibling();
        while let Some(n) = curr {
            if n.is_element() {
                return Some(n);
            }
            curr = n.next_sibling();
        }
        None
    }

    fn first_element_child(&self) -> Option<Self> {
        let mut curr = self.first_child();
        while let Some(n) = curr {
            if n.is_element() {
                return Some(n);
            }
            curr = n.next_sibling();
        }
        None
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.is_element() && self.owner_doc().is_html_document()
    }

    fn has_local_name(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::BorrowedLocalName,
    ) -> bool {
        self.name
            .as_ref()
            .map(|n| n.local == *name)
            .unwrap_or(false)
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as selectors::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        self.name.as_ref().map(|n| n.ns == *ns).unwrap_or(false)
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.name == other.name
    }

    fn attr_matches(
        &self,
        _ns: &selectors::attr::NamespaceConstraint<&Namespace>,
        local_name: &LocalName,
        msg: &AttrSelectorOperation<&AtomString>,
    ) -> bool {
        match self.get_attr(local_name, &Namespace::default()) {
            Some(val) => msg.eval_str(&val),
            None => false,
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        _pc: &<Self::Impl as selectors::SelectorImpl>::NonTSPseudoClass,
        _context: &mut selectors::context::MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn match_pseudo_element(
        &self,
        _pe: &<Self::Impl as selectors::SelectorImpl>::PseudoElement,
        _context: &mut selectors::context::MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn is_link(&self) -> bool {
        false
    }
    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn is_part(&self, _name: &<Self::Impl as selectors::SelectorImpl>::Identifier) -> bool {
        false
    }
    fn imported_part(
        &self,
        _name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as selectors::SelectorImpl>::Identifier> {
        None
    }
    fn is_empty(&self) -> bool {
        !self
            .children
            .iter()
            .any(|&id| self.with(id).is_element() || self.with(id).is_text_node())
    }
    fn is_root(&self) -> bool {
        match self.parent_node() {
            Some(p) => p.node_type == NodeType::Document,
            None => false,
        }
    }

    fn has_id(
        &self,
        id: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        self.id()
            .map(|my_id| case_sensitivity.eq_atom(my_id, id))
            .unwrap_or(false)
    }

    fn has_class(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        for c in &self.classes {
            if case_sensitivity.eq_atom(c, name) {
                return true;
            }
        }
        false
    }

    fn has_custom_state(
        &self,
        _name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> bool {
        false
    }

    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }

    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        let mut current = self.selector_flags.borrow_mut();
        current.insert(flags);
    }
}

impl AttributeProvider for &PawsElement {
    fn get_attr(&self, name: &LocalName, _namespace: &Namespace) -> Option<String> {
        let key = Atom::from(name.0.as_ref());
        self.attrs.get(&key).cloned()
    }
}
