use atomic_refcell::AtomicRefCell;
use bitflags::bitflags;
use slab::Slab;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use style::data::ElementData as StyloElementData;
use style::dom::TNode; // For Stylo integration later
use style::shared_lock::SharedRwLock;

use crate::dom::element::ElementData;
use crate::dom::text::TextNodeData;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    pub struct NodeFlags: u32 {
        const IS_IN_DOCUMENT = 0b00000100;
    }
}

pub struct Node {
    pub tree: *mut Slab<Node>, // unsafe pointer to the slab
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,

    // Basic Layout/Paint tree structure helpers (simplified for now)
    pub layout_parent: Cell<Option<usize>>,
    pub layout_children: RefCell<Option<Vec<usize>>>,

    pub flags: NodeFlags,
    pub data: NodeData,

    // Stylo integration
    pub stylo_element_data: AtomicRefCell<Option<StyloElementData>>,
    pub guard: SharedRwLock,
}

impl Node {
    pub fn new(tree: *mut Slab<Node>, id: usize, guard: SharedRwLock, data: NodeData) -> Self {
        Self {
            tree,
            id,
            parent: None,
            children: Vec::new(),
            layout_parent: Cell::new(None),
            layout_children: RefCell::new(None),
            flags: NodeFlags::default(),
            data,
            stylo_element_data: Default::default(),
            guard,
        }
    }

    pub fn tree(&self) -> &Slab<Node> {
        unsafe { &*self.tree }
    }
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    Element(ElementData),
    Text(TextNodeData),
    Comment,
}

impl NodeData {
    pub fn is_element(&self) -> bool {
        matches!(self, NodeData::Element(_))
    }

    pub fn as_element(&self) -> Option<&ElementData> {
        match self {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_element_mut(&mut self) -> Option<&mut ElementData> {
        match self {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }
}
