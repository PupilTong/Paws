//! RAII DOM wrapper types over the standalone host function wrappers.
//!
//! These types own a slab id and destroy it on `Drop` (best-effort — errors
//! from [`destroy_element`] are silently ignored because the host may have
//! destroyed the node as part of a parent subtree teardown before this
//! wrapper's Drop runs).
//!
//! # Design
//!
//! * [`NodeOps`] is a trait with default method implementations. Every
//!   wrapper type impls `fn id(&self) -> i32` and inherits the rest.
//! * [`ElementOps`]`: `[`NodeOps`] adds element-only methods (attributes,
//!   inline styles, shadow DOM). [`Text`] does NOT implement [`ElementOps`],
//!   so `text.set_attribute(...)` fails at compile time.
//! * [`Drop`] is implemented on each concrete type, not via a blanket impl,
//!   so the ownership story for each type is obvious from reading the struct
//!   in isolation.
//! * Tree traversal returns raw `Option<i32>` — the returned id is borrowed
//!   from the parent's subtree and must NOT be wrapped (would cause a double
//!   `destroy_element` on Drop). Use [`unsafe Element::from_raw`] +
//!   [`NodeOps::into_raw`] to take a traversed id through a short typed window.
//! * [`NodeOps::into_raw`] / [`Element::from_raw`] etc. provide escape hatches
//!   for FFI edges and for "disarming" Drop when a parent is destroyed first.

use super::{
    add_event_listener, add_shadow_stylesheet, append_element, append_elements, attach_shadow,
    clone_node, create_element, create_element_ns, create_text_node, destroy_element,
    dispatch_event, get_attribute, get_first_child, get_last_child, get_namespace_uri,
    get_next_sibling, get_node_type, get_parent_element, get_parent_node, get_previous_sibling,
    get_shadow_root, has_attribute, insert_before, is_connected, remove_attribute, remove_child,
    remove_event_listener, replace_child, set_attribute, set_inline_style, set_node_value,
    EventListenerOptions,
};

// ---------------------------------------------------------------------------
// W3C node type constants (mirrors get_node_type() return values)
// ---------------------------------------------------------------------------

/// DOM `nodeType` for an element node.
pub const NODE_TYPE_ELEMENT: i32 = 1;
/// DOM `nodeType` for a text node.
pub const NODE_TYPE_TEXT: i32 = 3;
/// DOM `nodeType` for a comment node.
pub const NODE_TYPE_COMMENT: i32 = 8;
/// DOM `nodeType` for a document node.
pub const NODE_TYPE_DOCUMENT: i32 = 9;
/// DOM `nodeType` for a document fragment / shadow root.
pub const NODE_TYPE_DOCUMENT_FRAGMENT: i32 = 11;

// ---------------------------------------------------------------------------
// NodeOps — common methods shared by every wrapper type
// ---------------------------------------------------------------------------

/// Operations available on every DOM wrapper (element, text node, future
/// comment node, etc.).
///
/// This trait is the sharing mechanism that avoids duplicating ~20 method
/// signatures across [`Element`], [`Text`], [`PawsInputElement`], and friends.
/// Implementers only need to provide [`NodeOps::id`]; every other method has
/// a default body that routes through `id()`.
///
/// Tree-traversal methods return raw `Option<i32>` rather than a wrapped
/// type because the returned id is NOT owned — the parent still owns the
/// child, and wrapping it would cause a double `destroy_element` on Drop.
/// If you need to treat a traversed id as a typed wrapper for a short
/// window, use [`Element::from_raw`] + [`NodeOps::into_raw`] to disarm Drop
/// before the wrapper leaves scope.
pub trait NodeOps: Sized {
    /// Returns the raw host slab id this wrapper owns.
    ///
    /// This is the only method an implementer must provide; every other
    /// method on the trait has a default body that routes through `id()`.
    fn id(&self) -> i32;

    /// Consumes the wrapper and returns the raw id, **suppressing Drop**.
    ///
    /// Use this when ownership of the underlying DOM node is being handed
    /// to another system (e.g. a parent wrapper is about to destroy the
    /// whole subtree, and you don't want this child's Drop to attempt a
    /// second destroy).
    #[inline]
    fn into_raw(self) -> i32 {
        let id = self.id();
        core::mem::forget(self);
        id
    }

    // -- tree traversal (unowned results) -----------------------------------

    /// Returns the first child's raw id, or `None` if there is no first child.
    ///
    /// The returned id is owned by this node — do NOT wrap it in an owning
    /// type; use [`Element::from_raw`] + [`NodeOps::into_raw`] if you need
    /// method access for a short window.
    #[inline]
    fn first_child(&self) -> Option<i32> {
        get_first_child(self.id())
    }

    /// Returns the last child's raw id, or `None`.
    #[inline]
    fn last_child(&self) -> Option<i32> {
        get_last_child(self.id())
    }

    /// Returns the next sibling's raw id, or `None`.
    #[inline]
    fn next_sibling(&self) -> Option<i32> {
        get_next_sibling(self.id())
    }

    /// Returns the previous sibling's raw id, or `None`.
    #[inline]
    fn previous_sibling(&self) -> Option<i32> {
        get_previous_sibling(self.id())
    }

    /// Returns the parent element id, or `None`.
    #[inline]
    fn parent_element(&self) -> Option<i32> {
        get_parent_element(self.id())
    }

    /// Returns the parent node id (any node type), or `None`.
    #[inline]
    fn parent_node(&self) -> Option<i32> {
        get_parent_node(self.id())
    }

    /// Returns whether this node is currently attached to the document tree.
    #[inline]
    fn is_connected(&self) -> Result<bool, i32> {
        is_connected(self.id())
    }

    /// Returns this node's DOM `nodeType`, or `None` if the node no longer exists.
    #[inline]
    fn node_type(&self) -> Option<i32> {
        get_node_type(self.id())
    }

    // -- tree mutation (borrowed child, matches web-sys) --------------------

    /// Appends `child` as the last child of this node.
    ///
    /// Mirrors the web-sys signature — the child is borrowed, not consumed,
    /// so the caller continues to own the wrapper (the slab entry is aliased
    /// via this parent until the parent's Drop destroys the subtree).
    #[inline]
    fn append_child<N: NodeOps>(&self, child: &N) -> Result<(), i32> {
        append_element(self.id(), child.id())
    }

    /// Appends multiple children in a single host call.
    #[inline]
    fn append_children(&self, children: &[i32]) -> Result<(), i32> {
        append_elements(self.id(), children)
    }

    /// Removes `child` from this node without destroying it.
    #[inline]
    fn remove_child<N: NodeOps>(&self, child: &N) -> Result<(), i32> {
        remove_child(self.id(), child.id())
    }

    /// Replaces `old_child` with `new_child` under this node.
    #[inline]
    fn replace_child<New: NodeOps, Old: NodeOps>(
        &self,
        new_child: &New,
        old_child: &Old,
    ) -> Result<(), i32> {
        replace_child(self.id(), new_child.id(), old_child.id())
    }

    /// Inserts `new_child` before `ref_child` under this node.
    #[inline]
    fn insert_before<New: NodeOps, Ref: NodeOps>(
        &self,
        new_child: &New,
        ref_child: &Ref,
    ) -> Result<(), i32> {
        insert_before(self.id(), new_child.id(), ref_child.id())
    }

    /// Inserts `new_child` at the end of this node's children. This is a
    /// convenience form of [`NodeOps::insert_before`] with `ref_child = -1`
    /// (the sentinel the host understands as "append at end").
    #[inline]
    fn insert_at_end<N: NodeOps>(&self, new_child: &N) -> Result<(), i32> {
        insert_before(self.id(), new_child.id(), -1)
    }

    /// Sets this node's text value (valid on Text and Comment nodes; a no-op
    /// on elements per the DOM spec).
    #[inline]
    fn set_node_value(&self, value: &str) -> Result<(), i32> {
        set_node_value(self.id(), value)
    }

    // -- event listeners (inherited by all NodeOps impls) -------------------

    /// Registers an event listener on this node.
    #[inline]
    fn add_event_listener(
        &self,
        event_type: &str,
        callback_id: i32,
        options: EventListenerOptions,
    ) -> Result<(), i32> {
        add_event_listener(self.id(), event_type, callback_id, options)
    }

    /// Removes an event listener from this node.
    #[inline]
    fn remove_event_listener(
        &self,
        event_type: &str,
        callback_id: i32,
        capture: bool,
    ) -> Result<(), i32> {
        remove_event_listener(self.id(), event_type, callback_id, capture)
    }

    /// Dispatches an event on this node.
    #[inline]
    fn dispatch_event(
        &self,
        event_type: &str,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
    ) -> Result<bool, i32> {
        dispatch_event(self.id(), event_type, bubbles, cancelable, composed)
    }
}

// ---------------------------------------------------------------------------
// ElementOps — element-only methods (attributes, styles, shadow DOM)
// ---------------------------------------------------------------------------

/// Element-only operations (attributes, inline styles, shadow DOM).
///
/// This trait is implemented on every element-backed wrapper ([`Element`],
/// [`PawsInputElement`], [`PawsTextAreaElement`]) but NOT on [`Text`], so
/// calling `text.set_attribute(...)` is a compile error.
pub trait ElementOps: NodeOps {
    // -- attributes ---------------------------------------------------------

    /// Sets a DOM attribute on this element.
    #[inline]
    fn set_attribute(&self, name: &str, value: &str) -> Result<(), i32> {
        set_attribute(self.id(), name, value)
    }

    /// Returns whether this element has the named attribute.
    #[inline]
    fn has_attribute(&self, name: &str) -> Result<bool, i32> {
        has_attribute(self.id(), name)
    }

    /// Reads an attribute value. Returns `Ok(Some(value))` if the attribute
    /// is set, `Ok(None)` if it is not, and `Err(code)` on host error.
    #[inline]
    fn get_attribute(&self, name: &str) -> Result<Option<String>, i32> {
        get_attribute(self.id(), name)
    }

    /// Removes the named attribute from this element.
    #[inline]
    fn remove_attribute(&self, name: &str) -> Result<(), i32> {
        remove_attribute(self.id(), name)
    }

    // -- inline style -------------------------------------------------------

    /// Sets an inline CSS property.
    #[inline]
    fn set_inline_style(&self, name: &str, value: &str) -> Result<(), i32> {
        set_inline_style(self.id(), name, value)
    }

    // -- namespace & cloning ------------------------------------------------

    /// Returns the namespace URI of this element, or `Ok(None)` if unset.
    /// Returns `Err(code)` on host error.
    #[inline]
    fn get_namespace_uri(&self) -> Result<Option<String>, i32> {
        get_namespace_uri(self.id())
    }

    /// Clones this element. If `deep` is `true`, descendants are cloned
    /// recursively.
    ///
    /// Returns an owning [`Element`] wrapping the new id — the clone is a
    /// fresh subtree, owned by the caller.
    #[inline]
    fn clone_node(&self, deep: bool) -> Result<Element, i32> {
        let new_id = clone_node(self.id(), deep)?;
        // SAFETY: `clone_node` returns a fresh, caller-owned slab id on
        // success. Wrapping it in `Element` transfers ownership to the
        // returned value.
        Ok(unsafe { Element::from_raw(new_id) })
    }

    // -- shadow DOM ---------------------------------------------------------

    /// Attaches a shadow root to this element. Returns the shadow root id.
    ///
    /// The returned id is NOT wrapped because a shadow root has a different
    /// ownership model — it is owned by its host element, not by the caller.
    #[inline]
    fn attach_shadow(&self, mode: &str) -> Result<i32, i32> {
        attach_shadow(self.id(), mode)
    }

    /// Returns this element's shadow root id, or `None`.
    #[inline]
    fn shadow_root(&self) -> Option<i32> {
        get_shadow_root(self.id())
    }

    /// Adds a stylesheet scoped to a shadow root owned by this element.
    #[inline]
    fn add_shadow_stylesheet(&self, shadow_root_id: i32, css: &str) -> Result<(), i32> {
        add_shadow_stylesheet(shadow_root_id, css)
    }
}

// ---------------------------------------------------------------------------
// Element — base HTML / SVG / MathML element wrapper
// ---------------------------------------------------------------------------

/// RAII wrapper for a DOM element.
///
/// Owns a single slab id. [`Drop`] calls [`destroy_element`] best-effort —
/// any error is silently swallowed because the host may have already
/// destroyed the node as part of a parent subtree teardown.
///
/// Construct with [`Element::new`] (HTML), [`Element::new_ns`] (namespaced),
/// or [`Element::from_raw`] (FFI edges).
#[repr(transparent)]
#[derive(Debug)]
pub struct Element {
    id: i32,
}

impl Element {
    /// Creates a new element with the given tag name.
    #[inline]
    pub fn new(tag: &str) -> Result<Self, i32> {
        let id = create_element(tag)?;
        Ok(Self { id })
    }

    /// Creates a new element with a namespace URI (SVG, MathML, etc.).
    #[inline]
    pub fn new_ns(namespace: &str, tag: &str) -> Result<Self, i32> {
        let id = create_element_ns(namespace, tag)?;
        Ok(Self { id })
    }

    /// Reconstructs an owning wrapper from a raw slab id.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// 1. `id` refers to a valid element node in the host slab.
    /// 2. No other owning wrapper currently holds `id` — otherwise Drop
    ///    would fire twice, and in the worst case the second call would hit
    ///    a slab slot that has been reused for an unrelated node.
    /// 3. The caller has effectively transferred ownership of `id` to this
    ///    new wrapper.
    ///
    /// Pair with [`NodeOps::into_raw`] to hand off ownership between FFI
    /// boundaries without running Drop.
    #[inline]
    pub unsafe fn from_raw(id: i32) -> Self {
        Self { id }
    }
}

impl NodeOps for Element {
    #[inline]
    fn id(&self) -> i32 {
        self.id
    }
}

impl ElementOps for Element {}

impl Drop for Element {
    fn drop(&mut self) {
        // Best-effort — ignore errors (host may have destroyed the subtree
        // ahead of this wrapper if a parent was dropped first).
        let _ = destroy_element(self.id);
    }
}

// ---------------------------------------------------------------------------
// Text — text node wrapper
// ---------------------------------------------------------------------------

/// RAII wrapper for a DOM text node.
///
/// Owns a slab id and destroys it on Drop. Does NOT implement [`ElementOps`],
/// so attribute/style methods are unavailable at compile time — text nodes
/// don't have attributes.
#[repr(transparent)]
#[derive(Debug)]
pub struct Text {
    id: i32,
}

impl Text {
    /// Creates a new text node with the given content.
    #[inline]
    pub fn new(text: &str) -> Result<Self, i32> {
        let id = create_text_node(text)?;
        Ok(Self { id })
    }

    /// Reconstructs an owning wrapper from a raw slab id.
    ///
    /// # Safety
    ///
    /// Same invariants as [`Element::from_raw`], plus the id must refer to a
    /// text node (node type [`NODE_TYPE_TEXT`]).
    #[inline]
    pub unsafe fn from_raw(id: i32) -> Self {
        Self { id }
    }
}

impl NodeOps for Text {
    #[inline]
    fn id(&self) -> i32 {
        self.id
    }
}

// Intentionally NO `impl ElementOps for Text` — text nodes have no attributes.

impl Drop for Text {
    fn drop(&mut self) {
        let _ = destroy_element(self.id);
    }
}

// ---------------------------------------------------------------------------
// PawsInputElement — <input> element wrapper
// ---------------------------------------------------------------------------

/// RAII wrapper for an `<input>` element.
///
/// Inherits all [`NodeOps`] and [`ElementOps`] methods (append_child,
/// set_attribute, set_inline_style, ...) and adds input-specific convenience
/// methods ([`set_value`](Self::set_value), [`set_checked`](Self::set_checked)).
#[repr(transparent)]
#[derive(Debug)]
pub struct PawsInputElement {
    id: i32,
}

impl PawsInputElement {
    /// Creates a new `<input>` element.
    #[inline]
    pub fn new() -> Result<Self, i32> {
        let id = create_element("input")?;
        Ok(Self { id })
    }

    /// Reconstructs an owning wrapper from a raw slab id.
    ///
    /// # Safety
    ///
    /// See [`Element::from_raw`]. The id must refer to an `<input>` element.
    #[inline]
    pub unsafe fn from_raw(id: i32) -> Self {
        Self { id }
    }

    /// Sets the `value` attribute.
    ///
    /// Note: the host may treat this as an attribute or as a live `.value`
    /// property; today it routes through [`set_attribute`].
    #[inline]
    pub fn set_value(&self, value: &str) -> Result<(), i32> {
        set_attribute(self.id, "value", value)
    }

    /// Sets the `checked` attribute for `<input type="checkbox">` and
    /// `<input type="radio">`. Uses attribute-presence semantics: `true`
    /// sets `checked=""`, `false` removes the attribute.
    #[inline]
    pub fn set_checked(&self, checked: bool) -> Result<(), i32> {
        if checked {
            set_attribute(self.id, "checked", "")
        } else {
            remove_attribute(self.id, "checked")
        }
    }
}

impl NodeOps for PawsInputElement {
    #[inline]
    fn id(&self) -> i32 {
        self.id
    }
}

impl ElementOps for PawsInputElement {}

impl Drop for PawsInputElement {
    fn drop(&mut self) {
        let _ = destroy_element(self.id);
    }
}

// ---------------------------------------------------------------------------
// PawsTextAreaElement — <textarea> element wrapper
// ---------------------------------------------------------------------------

/// RAII wrapper for a `<textarea>` element.
///
/// Inherits all [`NodeOps`] and [`ElementOps`] methods and adds
/// textarea-specific convenience methods
/// ([`set_value`](Self::set_value),
/// [`set_default_value`](Self::set_default_value)).
#[repr(transparent)]
#[derive(Debug)]
pub struct PawsTextAreaElement {
    id: i32,
}

impl PawsTextAreaElement {
    /// Creates a new `<textarea>` element.
    #[inline]
    pub fn new() -> Result<Self, i32> {
        let id = create_element("textarea")?;
        Ok(Self { id })
    }

    /// Reconstructs an owning wrapper from a raw slab id.
    ///
    /// # Safety
    ///
    /// See [`Element::from_raw`]. The id must refer to a `<textarea>`.
    #[inline]
    pub unsafe fn from_raw(id: i32) -> Self {
        Self { id }
    }

    /// Sets the live value of the textarea (current content).
    #[inline]
    pub fn set_value(&self, value: &str) -> Result<(), i32> {
        set_attribute(self.id, "value", value)
    }

    /// Sets the default value — what the textarea resets to on form reset.
    #[inline]
    pub fn set_default_value(&self, value: &str) -> Result<(), i32> {
        set_attribute(self.id, "defaultValue", value)
    }
}

impl NodeOps for PawsTextAreaElement {
    #[inline]
    fn id(&self) -> i32 {
        self.id
    }
}

impl ElementOps for PawsTextAreaElement {}

impl Drop for PawsTextAreaElement {
    fn drop(&mut self) {
        let _ = destroy_element(self.id);
    }
}
