use std::sync::OnceLock;

use app_units::Au;
use selectors::matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode};
use selectors::sink::Push;
use selectors::OpaqueElement;
// Use style crate directly
use style::dom::{
    AttributeProvider, LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode,
    TShadowRoot,
};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{AttrValue, Lang, PseudoElement, SelectorImpl};
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use style::values::AtomIdent;
use stylo_atoms::Atom;
use stylo_dom::ElementState;

use crate::dom::{Node, NodeData};
use crate::runtime::RuntimeState;
use std::cell::RefCell;

thread_local! {
    /// Thread-local storage to allow Stylo traits (on Copy handles) to access the RuntimeState.
    pub static CONTEXT: RefCell<Option<&'static RuntimeState>> = const { RefCell::new(None) };
}

/// A lightweight handle to a node (Element or Text) in the DOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeHandle(pub usize);

impl NodeHandle {
    pub fn with_node<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Node) -> R,
        R: Default,
    {
        CONTEXT.with(|c| {
            if let Some(state) = *c.borrow() {
                if let Some(node) = state.doc.get_node(self.0) {
                    return f(node);
                }
            }
            Default::default()
        })
    }
}

impl NodeInfo for NodeHandle {
    fn is_element(&self) -> bool {
        self.with_node(|n| n.data.is_element())
    }

    fn is_text_node(&self) -> bool {
        self.with_node(|n| matches!(n.data, NodeData::Text(_)))
    }
}

impl TNode for NodeHandle {
    type ConcreteElement = NodeHandle;
    type ConcreteDocument = DocumentHandle;
    type ConcreteShadowRoot = ShadowRootHandle;

    fn parent_node(&self) -> Option<Self> {
        self.with_node(|n| n.parent.map(NodeHandle))
    }

    fn first_child(&self) -> Option<Self> {
        self.with_node(|n| n.children.first().map(|&id| NodeHandle(id)))
    }

    fn last_child(&self) -> Option<Self> {
        self.with_node(|n| n.children.last().map(|&id| NodeHandle(id)))
    }

    fn prev_sibling(&self) -> Option<Self> {
        let parent = self.parent_node()?;
        parent.with_node(|n| {
            let idx = n.children.iter().position(|&id| id == self.0)?;
            if idx > 0 {
                Some(NodeHandle(n.children[idx - 1]))
            } else {
                None
            }
        })
    }

    fn next_sibling(&self) -> Option<Self> {
        let parent = self.parent_node()?;
        parent.with_node(|n| {
            let idx = n.children.iter().position(|&id| id == self.0)?;
            if idx + 1 < n.children.len() {
                Some(NodeHandle(n.children[idx + 1]))
            } else {
                None
            }
        })
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        DocumentHandle
    }

    fn is_in_document(&self) -> bool {
        true
    }

    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.parent_node().and_then(|n| n.as_element())
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.0)
    }

    fn debug_id(self) -> usize {
        self.0
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        if self.is_element() {
            Some(*self)
        } else {
            None
        }
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        None
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        None
    }
}

impl AttributeProvider for NodeHandle {
    fn get_attr(&self, name: &style::LocalName) -> Option<String> {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                // Convert style::LocalName to Atom for lookup
                let atom = Atom::from(name.as_ref());
                e.attrs.get(&atom).cloned()
            } else {
                None
            }
        })
    }
}

impl TElement for NodeHandle {
    type ConcreteNode = NodeHandle;
    type TraversalChildrenIterator = std::vec::IntoIter<NodeHandle>;

    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }

    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        let children = self.with_node(|n| {
            n.children
                .iter()
                .map(|&id| NodeHandle(id))
                .collect::<Vec<_>>()
        });
        LayoutIterator(children.into_iter())
    }

    fn is_html_element(&self) -> bool {
        true
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
        // let ptr = CONTEXT.with(|c| {
        //     let state_ref = *c.borrow();
        //     let state = state_ref.expect("Context check failed");
        //     let node = state.doc.nodes.get(self.0).expect("Node not found");
        //     node as *const Node
        // });
        // let node = unsafe { &*ptr };

        // TODO: Fix lifetime issue for style attribute access via ArcBorrow
        // if let Some(e) = node.data.as_element() {
        //      e.style_attribute.as_ref().map(|arc| {
        //           unsafe {
        //               let borrow = style::servo_arc::ArcBorrow::<Locked<PropertyDeclarationBlock>>::from_ref(arc);
        //               std::mem::transmute(borrow)
        //           }
        //      })
        // } else {
        None
        // }
    }

    fn animation_rule(
        &self,
        _: &style::context::SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn transition_rule(
        &self,
        _: &style::context::SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn state(&self) -> ElementState {
        // TODO: Implement state in ElementData
        // For now return empty
        ElementState::empty()
    }

    fn has_part_attr(&self) -> bool {
        false
    }

    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&stylo_atoms::Atom> {
        let ptr = CONTEXT.with(|c| {
            let state = c.borrow().expect("Context check failed");
            let node = state.doc.nodes.get(self.0).expect("Node not found");
            node as *const Node
        });
        let node = unsafe { &*ptr };
        let res = node.data.as_element().and_then(|e| e.id.as_ref());
        unsafe { std::mem::transmute(res) }
    }

    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                for class in &e.classes {
                    callback(AtomIdent::cast(class));
                }
            }
        })
    }

    fn each_custom_state<F>(&self, _callback: F)
    where
        F: FnMut(&AtomIdent),
    {
    }
    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&style::LocalName),
    {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                for name in e.attrs.keys() {
                    let local_name = style::LocalName::from(name.as_ref());
                    callback(&local_name);
                }
            }
        })
    }

    fn has_dirty_descendants(&self) -> bool {
        false
    }
    fn has_snapshot(&self) -> bool {
        false
    }
    fn handled_snapshot(&self) -> bool {
        false
    }
    unsafe fn set_handled_snapshot(&self) {}
    unsafe fn set_dirty_descendants(&self) {}
    unsafe fn unset_dirty_descendants(&self) {}
    fn store_children_to_process(&self, _: isize) {}
    fn did_process_child(&self) -> isize {
        0
    }
    unsafe fn ensure_data(&self) -> atomic_refcell::AtomicRefMut<'_, style::data::ElementData> {
        let ptr = CONTEXT.with(|c| {
            let state = c.borrow().expect("Context check failed");
            let node = state.doc.nodes.get(self.0).expect("Node not found");
            node as *const Node
        });
        let node = unsafe { &*ptr };

        let mut borrow = node.stylo_element_data.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(style::data::ElementData::default());
        }

        atomic_refcell::AtomicRefMut::map(borrow, |opt| opt.as_mut().unwrap())
    }
    unsafe fn clear_data(&self) {}
    fn has_data(&self) -> bool {
        false
    }
    fn borrow_data(&self) -> Option<atomic_refcell::AtomicRef<'_, style::data::ElementData>> {
        let ptr = CONTEXT.with(|c| {
            let state = c.borrow().expect("Context check failed");
            let node = state.doc.nodes.get(self.0).expect("Node not found");
            node as *const Node
        });
        let node = unsafe { &*ptr };
        let borrow = node.stylo_element_data.borrow();
        if borrow.is_some() {
            Some(atomic_refcell::AtomicRef::map(borrow, |o| {
                o.as_ref().unwrap()
            }))
        } else {
            None
        }
    }
    fn mutate_data(&self) -> Option<atomic_refcell::AtomicRefMut<'_, style::data::ElementData>> {
        None
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
    fn may_have_animations(&self) -> bool {
        false
    }
    fn has_animations(&self, _: &style::context::SharedStyleContext) -> bool {
        false
    }
    fn has_css_animations(
        &self,
        _: &style::context::SharedStyleContext,
        _: Option<PseudoElement>,
    ) -> bool {
        false
    }
    fn has_css_transitions(
        &self,
        _: &style::context::SharedStyleContext,
        _: Option<PseudoElement>,
    ) -> bool {
        false
    }
    fn shadow_root(&self) -> Option<ShadowRootHandle> {
        None
    }
    fn containing_shadow(&self) -> Option<ShadowRootHandle> {
        None
    }
    fn lang_attr(&self) -> Option<AttrValue> {
        None
    }
    fn match_element_lang(&self, _: Option<Option<AttrValue>>, _: &Lang) -> bool {
        false
    }
    fn is_html_document_body_element(&self) -> bool {
        false
    }
    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _: VisitedHandlingMode,
        _: &mut V,
    ) where
        V: Push<style::applicable_declarations::ApplicableDeclarationBlock>,
    {
    }

    fn local_name(&self) -> &<SelectorImpl as selectors::parser::SelectorImpl>::BorrowedLocalName {
        let ptr = CONTEXT.with(|c| {
            let state_ref = *c.borrow();
            let state = state_ref.expect("Context check failed");
            let node = state.doc.get_node(self.0).expect("Node not found");
            node as *const Node
        });
        let node = unsafe { &*ptr };
        let res = match &node.data {
            NodeData::Element(e) => &e.name.local,
            _ => panic!("local_name called on non-element"),
        };
        unsafe { std::mem::transmute(res) }
    }

    fn namespace(
        &self,
    ) -> &<SelectorImpl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl {
        static NS: OnceLock<style::Namespace> = OnceLock::new();
        let ns = NS.get_or_init(|| style::Namespace::from(""));
        unsafe { std::mem::transmute(ns) }
    }

    fn query_container_size(
        &self,
        _: &style::values::computed::Display,
    ) -> euclid::default::Size2D<Option<Au>> {
        euclid::default::Size2D::new(None, None)
    }

    fn has_selector_flags(&self, _: ElementSelectorFlags) -> bool {
        false
    }
    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::empty()
    }
}

impl selectors::Element for NodeHandle {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self)
    }

    fn parent_element(&self) -> Option<Self> {
        self.parent_node().and_then(|n| n.as_element())
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut cursor = self.prev_sibling();
        while let Some(node) = cursor {
            if node.is_element() {
                return Some(node);
            }
            cursor = node.prev_sibling();
        }
        None
    }
    fn next_sibling_element(&self) -> Option<Self> {
        let mut cursor = self.next_sibling();
        while let Some(node) = cursor {
            if node.is_element() {
                return Some(node);
            }
            cursor = node.next_sibling();
        }
        None
    }
    fn first_element_child(&self) -> Option<Self> {
        let mut cursor = self.first_child();
        while let Some(node) = cursor {
            if node.is_element() {
                return Some(node);
            }
            cursor = node.next_sibling();
        }
        None
    }
    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(
        &self,
        name: &<Self::Impl as selectors::parser::SelectorImpl>::BorrowedLocalName,
    ) -> bool {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                e.name.local == *name
            } else {
                false
            }
        })
    }
    fn has_namespace(
        &self,
        _: &<Self::Impl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        false
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.with_node(|n1| {
            other.with_node(|n2| match (&n1.data, &n2.data) {
                (NodeData::Element(e1), NodeData::Element(e2)) => e1.name == e2.name,
                _ => false,
            })
        })
    }
    fn attr_matches(
        &self,
        _: &selectors::attr::NamespaceConstraint<
            &<Self::Impl as selectors::parser::SelectorImpl>::NamespaceUrl,
        >,
        other_name: &<Self::Impl as selectors::parser::SelectorImpl>::LocalName,
        operation: &selectors::attr::AttrSelectorOperation<
            &<Self::Impl as selectors::parser::SelectorImpl>::AttrValue,
        >,
    ) -> bool {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                let atom_name = Atom::from(other_name.as_ref());
                if let Some(val) = e.attrs.get(&atom_name) {
                    operation.eval_str(val)
                } else {
                    false
                }
            } else {
                false
            }
        })
    }
    fn match_non_ts_pseudo_class(
        &self,
        _: &<Self::Impl as selectors::parser::SelectorImpl>::NonTSPseudoClass,
        _: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }
    fn match_pseudo_element(
        &self,
        _: &<Self::Impl as selectors::parser::SelectorImpl>::PseudoElement,
        _: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }
    fn apply_selector_flags(&self, _: ElementSelectorFlags) {}
    fn is_link(&self) -> bool {
        false
    }
    fn is_html_slot_element(&self) -> bool {
        false
    }
    fn has_id(
        &self,
        id: &<Self::Impl as selectors::parser::SelectorImpl>::Identifier,
        _: selectors::attr::CaseSensitivity,
    ) -> bool {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                // Check ID
                if let Some(val) = &e.id {
                    return id.0 == *val;
                }
            }
            false
        })
    }
    fn has_class(
        &self,
        name: &<Self::Impl as selectors::parser::SelectorImpl>::Identifier,
        _: selectors::attr::CaseSensitivity,
    ) -> bool {
        self.with_node(|n| {
            if let Some(e) = n.data.as_element() {
                e.classes.contains(&name.0)
            } else {
                false
            }
        })
    }
    fn has_custom_state(
        &self,
        _: &<Self::Impl as selectors::parser::SelectorImpl>::Identifier,
    ) -> bool {
        false
    }
    fn imported_part(
        &self,
        _: &<Self::Impl as selectors::parser::SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as selectors::parser::SelectorImpl>::Identifier> {
        None
    }
    fn is_part(&self, _: &<Self::Impl as selectors::parser::SelectorImpl>::Identifier) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        self.with_node(|n| n.children.is_empty())
    }
    fn is_root(&self) -> bool {
        self.with_node(|n| n.parent.is_none())
    }
    fn add_element_unique_hashes(&self, _: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }
}

/// Dummy handle for Document (required by TNode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocumentHandle;

impl TDocument for DocumentHandle {
    type ConcreteNode = NodeHandle;

    fn as_node(&self) -> Self::ConcreteNode {
        NodeHandle(usize::MAX) // Placeholder
    }

    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> selectors::matching::QuirksMode {
        selectors::matching::QuirksMode::NoQuirks
    }
    fn shared_lock(&self) -> &SharedRwLock {
        CONTEXT.with(|c| {
            let state = c.borrow().expect("Context check failed");
            unsafe { std::mem::transmute(&state.doc.guard) }
        })
    }
}

/// Dummy handle for ShadowRoot (required by TNode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShadowRootHandle;

impl TShadowRoot for ShadowRootHandle {
    type ConcreteNode = NodeHandle;
    fn as_node(&self) -> Self::ConcreteNode {
        NodeHandle(usize::MAX)
    }
    fn host(&self) -> NodeHandle {
        NodeHandle(0)
    }
    fn style_data<'a>(&self) -> Option<&'a style::stylist::CascadeData> {
        None
    }
}
