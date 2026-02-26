use atomic_refcell::AtomicRefCell;
use bitflags::bitflags;
use markup5ever::QualName;
use selectors::matching::ElementSelectorFlags;
use slab::Slab;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use style::data::ElementData as StyloElementData;
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use stylo_atoms::Atom;
use stylo_dom::ElementState;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    pub struct NodeFlags: u32 {
        const IS_IN_DOCUMENT = 0b00000100;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Document,
    Element,
    Text,
    Comment,
    ShadowRoot,
}

pub struct PawsElement {
    /// Unsafe pointer to the slab containing this node.
    pub tree: *mut Slab<PawsElement>,

    /// The ID of this node in the slab.
    pub id: usize,

    /// The ID of the parent node.
    pub parent: Option<usize>,

    /// The IDs of the child nodes.
    pub children: Vec<usize>,

    /// Node flags.
    pub flags: NodeFlags,
    pub node_type: NodeType,

    // Element data
    pub name: Option<QualName>,
    pub id_attr: Option<Atom>,
    pub attrs: HashMap<Atom, String>,
    pub classes: HashSet<Atom>,
    pub style_attribute: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    pub shadow_root_id: Option<usize>,

    // Text data
    pub text_content: Option<String>,

    /// Stylo integration data.
    pub stylo_element_data: AtomicRefCell<Option<StyloElementData>>,

    /// Cached computed styles from the latest layout/style resolution pass.
    pub computed_values: Option<Arc<style::properties::ComputedValues>>,

    /// Selector flags for invalidation
    pub selector_flags: AtomicRefCell<ElementSelectorFlags>,

    pub guard: SharedRwLock,

    /// Element state (hover, focus, etc.).
    pub element_state: ElementState,

    /// Dirty descendants flag for Stylo.
    pub dirty_descendants: AtomicBool,
}

impl PawsElement {
    pub fn new(
        tree: *mut Slab<PawsElement>,
        id: usize,
        guard: SharedRwLock,
        node_type: NodeType,
    ) -> Self {
        Self {
            tree,
            id,
            parent: None,
            children: Vec::new(),
            flags: NodeFlags::default(),
            node_type,

            name: None,
            id_attr: None,
            attrs: HashMap::new(),
            classes: HashSet::new(),
            style_attribute: None,
            shadow_root_id: None,
            text_content: None,

            stylo_element_data: Default::default(),
            computed_values: None,
            selector_flags: AtomicRefCell::new(ElementSelectorFlags::empty()),
            guard,
            element_state: ElementState::empty(),
            dirty_descendants: AtomicBool::new(true),
        }
    }

    pub fn tree(&self) -> &Slab<PawsElement> {
        unsafe { &*self.tree }
    }

    pub fn with(&self, id: usize) -> &PawsElement {
        self.tree().get(id).expect("Node not found in slab")
    }

    pub fn is_element(&self) -> bool {
        self.node_type == NodeType::Element
    }

    pub fn is_text_node(&self) -> bool {
        self.node_type == NodeType::Text
    }
    pub fn get_computed_style_by_key(
        &self,
        state: &crate::style::StyleContext,
        key: &str,
    ) -> Option<String> {
        let parser_context = crate::style::build_parser_context(&state.url_data);
        let property_id = crate::style::PropertyId::parse(key, &parser_context).ok()?;
        let longhand = property_id.longhand_id()?;

        let computed = self.computed_values.as_ref()?;
        crate::style::serialize_computed_value(computed, longhand)
    }

    pub fn set_dirty_descendants(&self) {
        self.dirty_descendants.store(true, Ordering::Relaxed);
    }

    pub fn unset_dirty_descendants(&self) {
        self.dirty_descendants.store(false, Ordering::Relaxed);
    }

    pub fn has_dirty_descendants(&self) -> bool {
        self.dirty_descendants.load(Ordering::Relaxed)
    }

    pub fn mark_ancestors_dirty(&self) {
        let mut current_id = self.parent;
        while let Some(parent_id) = current_id {
            let parent = self.with(parent_id);
            if parent.dirty_descendants.swap(true, Ordering::Relaxed) {
                break;
            }
            current_id = parent.parent;
        }
    }

    pub fn set_attribute(&mut self, name: &str, value: &str) {
        let atom_name = Atom::from(name);
        self.attrs.insert(atom_name.clone(), value.to_string());

        if name == "id" {
            self.id_attr = Some(Atom::from(value));
        }

        if name == "class" {
            self.classes.clear();
            for class in value.split_whitespace() {
                self.classes.insert(Atom::from(class));
            }
        }
    }

    pub fn has_class(&self, name: &Atom) -> bool {
        self.classes.contains(name)
    }
}

// Implement equality and hash based on ID (reference identity)
impl PartialEq for PawsElement {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for PawsElement {}

impl std::hash::Hash for PawsElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Debug for PawsElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PawsElement")
            .field("id", &self.id)
            .field("type", &self.node_type)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .finish()
    }
}
