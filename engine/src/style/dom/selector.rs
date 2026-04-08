//! `selectors::Element` and `AttributeProvider` implementations for `&PawsElement`.

use crate::runtime::RenderState;
use style::dom::{AttributeProvider, NodeInfo, TDocument, TElement, TNode};
use style::selector_parser::SelectorImpl;
use style::values::AtomString;
use style::{CaseSensitivityExt, LocalName, Namespace};
use stylo_atoms::Atom;

use selectors::attr::AttrSelectorOperation;
use selectors::matching::ElementSelectorFlags;

use crate::dom::{NodeType, PawsElement};

impl<S: RenderState> selectors::Element for &PawsElement<S> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> selectors::OpaqueElement {
        selectors::OpaqueElement::new(*self)
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
        use crate::dom::NodeType;
        // Walk up ancestors to find the containing shadow root, then return its host.
        let mut current = self.parent;
        while let Some(id) = current {
            let node = self.with(id);
            if node.node_type == NodeType::ShadowRoot {
                // Shadow root's parent is the host element.
                return node.parent.map(|host_id| self.with(host_id));
            }
            if node.node_type == NodeType::Document {
                return None;
            }
            current = node.parent;
        }
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn pseudo_element_originating_element(&self) -> Option<Self> {
        None
    }

    fn assigned_slot(&self) -> Option<Self> {
        self.assigned_slot_id.map(|id| self.with(id))
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
        self.is_slot_element()
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

impl<S: RenderState> AttributeProvider for &PawsElement<S> {
    fn get_attr(&self, name: &LocalName, _namespace: &Namespace) -> Option<String> {
        let key = Atom::from(name.0.as_ref());
        self.attrs.get(&key).cloned()
    }
}
