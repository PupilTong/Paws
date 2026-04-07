use atomic_refcell::AtomicRefCell;
use bitflags::bitflags;
use markup5ever::QualName;
use selectors::matching::ElementSelectorFlags;
use slab::Slab;
use std::cell::UnsafeCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use style::data::ElementDataWrapper;
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use stylo_atoms::Atom;
use stylo_dom::ElementState;

bitflags! {
    /// Bitflags tracking node state within the DOM tree.
    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct NodeFlags: u32 {
        const IS_IN_DOCUMENT = 0b00000100;
    }
}

/// The type of a DOM node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeType {
    Document,
    Element,
    Text,
    #[allow(dead_code)]
    Comment,
    ShadowRoot,
}

/// A node in the Paws DOM tree, stored in a slab arena.
///
/// Integrates with Stylo for CSS style computation via
/// [`ElementDataWrapper`]-managed element data and selector flags.
///
/// The type parameter `S` is the per-node render state for the
/// [`EngineRenderer`](crate::EngineRenderer) backend. Each platform
/// defines its own `S` (e.g. `IosNodeState`). Tests use `()`.
pub struct PawsElement<S: Default + Send + 'static = ()> {
    /// Raw pointer to the slab containing this node.
    /// Only accessed via the safe `tree()` accessor or within the `engine` crate.
    pub(crate) tree: *mut Slab<PawsElement<S>>,

    /// The ID of this node in the slab.
    pub id: taffy::NodeId,

    /// The ID of the parent node.
    pub parent: Option<taffy::NodeId>,

    /// The IDs of the child nodes.
    pub children: Vec<taffy::NodeId>,

    /// Node flags.
    pub(crate) flags: NodeFlags,
    pub(crate) node_type: NodeType,

    // Element data
    pub(crate) name: Option<QualName>,
    pub(crate) id_attr: Option<Atom>,
    pub(crate) attrs: HashMap<Atom, String>,
    pub(crate) classes: HashSet<Atom>,
    pub(crate) style_attribute: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    pub(crate) shadow_root_id: Option<taffy::NodeId>,

    // Text data
    pub(crate) text_content: Option<String>,

    /// Stylo integration data.
    ///
    /// Wrapped in `UnsafeCell` because Stylo's `TElement` trait methods
    /// (`ensure_data`, `clear_data`) require mutation through `&self`.
    pub(crate) stylo_element_data: UnsafeCell<Option<ElementDataWrapper>>,

    /// Cached computed styles from the latest layout/style resolution pass.
    pub(crate) computed_values: Option<Arc<style::properties::ComputedValues>>,

    /// Selector flags for invalidation
    pub(crate) selector_flags: AtomicRefCell<ElementSelectorFlags>,

    pub(crate) guard: SharedRwLock,

    /// Element state (hover, focus, etc.).
    pub(crate) element_state: ElementState,

    /// Dirty descendants flag for Stylo.
    pub(crate) dirty_descendants: AtomicBool,

    // ── Layout data (persists across passes for CSS Containment) ──
    /// Cached Taffy style, recomputed when `computed_values` change.
    pub(crate) taffy_style: Option<taffy::Style>,
    /// Taffy layout cache (persists across passes for incremental re-layout).
    pub(crate) layout_cache: taffy::Cache,
    /// Unrounded layout from the current pass.
    pub(crate) unrounded_layout: taffy::tree::Layout,
    /// Pixel-snapped final layout.
    pub(crate) final_layout: taffy::tree::Layout,

    /// Whether this node establishes a CSS stacking context.
    ///
    /// Set during [`Document::resolve_style`] based on the node's computed
    /// style values. Used by renderers to determine paint order.
    pub creates_stacking_context: bool,

    /// Per-node render state for the [`EngineRenderer`](crate::EngineRenderer) backend.
    ///
    /// Stored directly on each node so there is truly one tree — no separate
    /// HashMap or snapshot store. For `()` (tests/headless) this is zero-sized.
    pub(crate) render_state: S,

    /// Event listeners registered on this node via `addEventListener`.
    ///
    /// Stored as a `Vec` to preserve registration order (required by the
    /// W3C spec for listener firing order). Most nodes have 0–5 listeners,
    /// making linear search for deduplication negligible.
    pub event_listeners: Vec<crate::events::EventListenerEntry>,
}

impl<S: Default + Send + 'static> PawsElement<S> {
    pub(crate) fn new(
        tree: *mut Slab<PawsElement<S>>,
        id: taffy::NodeId,
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

            stylo_element_data: UnsafeCell::new(None),
            computed_values: None,
            selector_flags: AtomicRefCell::new(ElementSelectorFlags::empty()),
            guard,
            element_state: ElementState::empty(),
            dirty_descendants: AtomicBool::new(true),

            taffy_style: None,
            layout_cache: taffy::Cache::new(),
            unrounded_layout: taffy::tree::Layout::with_order(0),
            final_layout: taffy::tree::Layout::with_order(0),

            creates_stacking_context: false,
            render_state: S::default(),
            event_listeners: Vec::new(),
        }
    }

    pub(crate) fn tree(&self) -> &Slab<PawsElement<S>> {
        // SAFETY: The `tree` pointer is set during construction by Document, which owns
        // the Box<Slab<PawsElement>> this pointer references. The Box ensures the slab
        // is heap-allocated and never moved. We only produce a shared reference here,
        // matching the shared access pattern (no mutable aliasing).
        unsafe { &*self.tree }
    }

    pub(crate) fn with(&self, id: taffy::NodeId) -> &PawsElement<S> {
        self.tree()
            .get(u64::from(id) as usize)
            .expect("Node not found in slab")
    }

    /// Returns the cached computed style values from the last style resolution.
    ///
    /// Available after [`Document::resolve_style`] has been called.
    pub fn get_computed_values(&self) -> Option<&Arc<style::properties::ComputedValues>> {
        self.computed_values.as_ref()
    }

    /// Returns the final pixel-snapped layout (position + size).
    ///
    /// Available after [`crate::layout::compute_layout_in_place`] has been called.
    pub fn layout(&self) -> &taffy::tree::Layout {
        &self.final_layout
    }

    /// Returns the z-index if explicitly set, or `None` for `auto`.
    pub fn z_index(&self) -> Option<i32> {
        self.computed_values.as_ref().and_then(|cv| {
            use style::values::generics::position::ZIndex;
            match cv.clone_z_index() {
                ZIndex::Integer(n) => Some(n),
                ZIndex::Auto => None,
            }
        })
    }

    /// Returns a reference to the per-node render state for the backend.
    pub fn render_state(&self) -> &S {
        &self.render_state
    }

    /// Replaces the per-node render state.
    pub fn set_render_state(&mut self, state: S) {
        self.render_state = state;
    }

    /// Returns the text content of this node, if any.
    pub fn text(&self) -> Option<&str> {
        self.text_content.as_deref()
    }

    /// Returns whether this node has a computed Taffy style (i.e., is a styled element).
    pub fn has_style(&self) -> bool {
        self.taffy_style.is_some()
    }

    pub fn is_element(&self) -> bool {
        self.node_type == NodeType::Element
    }

    pub fn is_text_node(&self) -> bool {
        self.node_type == NodeType::Text
    }

    pub(crate) fn set_dirty_descendants(&self) {
        self.dirty_descendants.store(true, Ordering::Relaxed);
    }

    pub(crate) fn unset_dirty_descendants(&self) {
        self.dirty_descendants.store(false, Ordering::Relaxed);
    }

    pub(crate) fn has_dirty_descendants(&self) -> bool {
        self.dirty_descendants.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_ancestors_dirty(&self) {
        let mut current_id = self.parent;
        while let Some(parent_id) = current_id {
            let parent = self.with(parent_id);
            if parent.dirty_descendants.swap(true, Ordering::Relaxed) {
                break;
            }
            current_id = parent.parent;
        }
    }

    pub(crate) fn set_attribute(&mut self, name: &str, value: &str) {
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

    #[allow(dead_code)]
    pub(crate) fn has_class(&self, name: &Atom) -> bool {
        self.classes.contains(name)
    }

    /// Returns `true` if the element has the named attribute.
    pub(crate) fn has_attribute(&self, name: &str) -> bool {
        self.attrs.contains_key(&Atom::from(name))
    }

    /// Returns the value of the named attribute, or `None` if absent.
    pub(crate) fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attrs.get(&Atom::from(name)).map(|s| s.as_str())
    }

    /// Removes the named attribute from the element.
    ///
    /// Handles special attributes (`id`, `class`) by clearing the
    /// corresponding cached fields.
    pub(crate) fn remove_attribute(&mut self, name: &str) {
        let atom_name = Atom::from(name);
        self.attrs.remove(&atom_name);

        if name == "id" {
            self.id_attr = None;
        }

        if name == "class" {
            self.classes.clear();
        }
    }
}

// Implement equality and hash based on ID (reference identity)
impl<S: Default + Send + 'static> PartialEq for PawsElement<S> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<S: Default + Send + 'static> Eq for PawsElement<S> {}

impl<S: Default + Send + 'static> std::hash::Hash for PawsElement<S> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<S: Default + Send + 'static> std::fmt::Debug for PawsElement<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PawsElement")
            .field("id", &self.id)
            .field("type", &self.node_type)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .finish()
    }
}
