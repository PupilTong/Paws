//! `NodeInfo` and `TNode` implementations for `&PawsElement`.

use style::dom::{NodeInfo, OpaqueNode, TNode};

use crate::dom::{NodeFlags, NodeType, PawsElement};

impl NodeInfo for &PawsElement {
    fn is_element(&self) -> bool {
        self.node_type == NodeType::Element
    }

    fn is_text_node(&self) -> bool {
        self.node_type == NodeType::Text
    }
}

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
