use crate::dom::element::{NodeFlags, NodeType, PawsElement, ShadowRootMode};
use crate::layout::text::TextLayoutContext;
use crate::runtime::RenderState;
use markup5ever::QualName;
use slab::Slab;
use style::shared_lock::SharedRwLock;

/// Errors that can occur during DOM tree operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DomError {
    /// The specified parent node ID does not exist in the tree.
    InvalidParent,
    /// The specified child node ID does not exist in the tree.
    InvalidChild,
    /// Appending would create a cycle in the tree (child is ancestor of parent).
    CycleDetected,
    /// The child node already has a different parent. Detach it first.
    ChildAlreadyHasParent,
    /// The element already has a shadow root attached.
    ShadowRootAlreadyExists,
}

impl std::fmt::Display for DomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomError::InvalidParent => write!(f, "invalid parent id"),
            DomError::InvalidChild => write!(f, "invalid child id"),
            DomError::CycleDetected => write!(f, "append would create a cycle"),
            DomError::ChildAlreadyHasParent => write!(f, "child already has a parent"),
            DomError::ShadowRootAlreadyExists => write!(f, "element already has a shadow root"),
        }
    }
}

impl std::error::Error for DomError {}

/// The document tree, backed by a [`Slab`] arena for cache-friendly access.
///
/// Owns all DOM nodes and manages tree mutations (append, detach, remove)
/// with cycle detection and IS_IN_DOCUMENT flag propagation.
///
/// Taffy's layout traits (`TraversePartialTree`, `LayoutPartialTree`, etc.)
/// are implemented on this type in the `layout::block` module (the "fat tree"
/// pattern inspired by Blitz).
///
/// The type parameter `S` is the per-node render state for the
/// [`EngineRenderer`](crate::EngineRenderer) backend. Tests use `()`.
pub struct Document<S: RenderState = ()> {
    /// A slab-backed tree of nodes
    pub(crate) nodes: Box<Slab<PawsElement<S>>>,

    /// Stylo shared lock
    pub(crate) guard: SharedRwLock,

    /// Root node ID
    pub(crate) root: taffy::NodeId,

    /// Document stylesheets
    pub(crate) stylesheets: Vec<crate::style::CSSStyleSheet>,

    /// Document URL
    #[allow(dead_code)]
    pub(crate) url: url::Url,

    /// Parley-backed text layout context for measuring text leaf nodes.
    ///
    /// Created eagerly in [`Document::new`] and reused across layout passes.
    pub(crate) text_cx: TextLayoutContext,

    /// Render states captured from nodes removed since last commit.
    ///
    /// When a node is removed from the slab, its `render_state` is saved here
    /// so the [`EngineRenderer`](crate::EngineRenderer) can emit release ops.
    /// Cleared after each commit.
    pub(crate) removed_render_states: Vec<(taffy::NodeId, S)>,
}

impl<S: RenderState> Document<S> {
    pub(crate) fn new(guard: SharedRwLock, url: url::Url) -> Self {
        let mut nodes = Box::new(Slab::new());
        let slab_ptr = nodes.as_mut() as *mut Slab<PawsElement<S>>;

        // Create root node (Document)
        let root_entry = nodes.vacant_entry();
        let root_index = root_entry.key();
        let root_id = taffy::NodeId::from(root_index as u64);

        root_entry.insert(PawsElement::new(
            slab_ptr,
            root_id,
            guard.clone(),
            NodeType::Document,
        ));

        // Set IS_IN_DOCUMENT flag for root
        nodes[root_index].flags.insert(NodeFlags::IS_IN_DOCUMENT);

        Document {
            nodes,
            guard,
            root: root_id,
            stylesheets: Vec::new(),
            url,
            text_cx: TextLayoutContext::new(),
            removed_render_states: Vec::new(),
        }
    }

    /// Returns render states captured from nodes removed since last commit.
    ///
    /// The renderer should process these to emit release/detach ops,
    /// then `RuntimeState::commit()` clears them.
    pub fn removed_render_states(&self) -> &[(taffy::NodeId, S)] {
        &self.removed_render_states
    }

    /// Returns the first element child of the document root.
    ///
    /// This is the "root element" used for layout — the document node
    /// itself is not a styled element.
    pub fn root_element_id(&self) -> Option<taffy::NodeId> {
        self.get_node(self.root).and_then(|root| {
            root.children
                .iter()
                .copied()
                .find(|&id| self.get_node(id).is_some_and(|n| n.is_element()))
        })
    }

    pub fn get_node(&self, id: taffy::NodeId) -> Option<&PawsElement<S>> {
        self.nodes.get(u64::from(id) as usize)
    }

    pub fn get_node_mut(&mut self, id: taffy::NodeId) -> Option<&mut PawsElement<S>> {
        self.nodes.get_mut(u64::from(id) as usize)
    }

    pub(crate) fn mark_root_dirty(&self) {
        if let Some(root) = self.get_node(self.root) {
            root.set_dirty_descendants();
        }
    }

    /// Panicking accessor for layout passes. Use `get_node` for fallible access.
    #[inline]
    pub(crate) fn node(&self, id: taffy::NodeId) -> &PawsElement<S> {
        self.get_node(id).expect("valid node id during layout")
    }

    /// Panicking mutable accessor for layout passes. Use `get_node_mut` for fallible access.
    #[inline]
    pub(crate) fn node_mut(&mut self, id: taffy::NodeId) -> &mut PawsElement<S> {
        self.get_node_mut(id).expect("valid node id during layout")
    }

    pub(crate) fn create_node(&mut self, node_type: NodeType) -> taffy::NodeId {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<PawsElement<S>>;
        let guard = self.guard.clone();

        let entry = self.nodes.vacant_entry();
        let index = entry.key();
        let id = taffy::NodeId::from(index as u64);
        entry.insert(PawsElement::new(slab_ptr, id, guard, node_type));

        id
    }

    pub(crate) fn create_element(&mut self, name: QualName) -> taffy::NodeId {
        let id = self.create_node(NodeType::Element);
        let el = self.nodes.get_mut(u64::from(id) as usize).unwrap();
        el.name = Some(name);
        id
    }

    pub(crate) fn create_text_node(&mut self, content: String) -> taffy::NodeId {
        let id = self.create_node(NodeType::Text);
        let el = self.nodes.get_mut(u64::from(id) as usize).unwrap();
        el.text_content = Some(content);
        id
    }

    /// Creates a shadow root for the given host element.
    ///
    /// The shadow root node is stored in the slab but is **not** added to the
    /// host's `children` vec. It is only reachable via `host.shadow_root_id`.
    /// The shadow root's `parent` field points to the host for
    /// [`TShadowRoot::host()`] traversal.
    pub(crate) fn create_shadow_root(
        &mut self,
        host_id: taffy::NodeId,
        mode: ShadowRootMode,
    ) -> Result<taffy::NodeId, DomError> {
        // Validate host exists and is an element
        let host = self.get_node(host_id).ok_or(DomError::InvalidParent)?;
        if host.node_type != NodeType::Element {
            return Err(DomError::InvalidParent);
        }
        if host.shadow_root_id.is_some() {
            return Err(DomError::ShadowRootAlreadyExists);
        }
        let host_in_doc = host.flags.contains(NodeFlags::IS_IN_DOCUMENT);

        // Create the shadow root node
        let shadow_root_id = self.create_node(NodeType::ShadowRoot);
        let shadow_root = self
            .nodes
            .get_mut(u64::from(shadow_root_id) as usize)
            .expect("shadow root just created");
        shadow_root.parent = Some(host_id);
        shadow_root.shadow_mode = Some(mode);
        if host_in_doc {
            shadow_root.flags.insert(NodeFlags::IS_IN_DOCUMENT);
        }

        // Link host → shadow root
        let host_mut = self
            .nodes
            .get_mut(u64::from(host_id) as usize)
            .expect("host validated above");
        host_mut.shadow_root_id = Some(shadow_root_id);
        host_mut.mark_dirty_and_ancestors();

        Ok(shadow_root_id)
    }

    pub(crate) fn append_child(
        &mut self,
        parent_id: taffy::NodeId,
        child_id: taffy::NodeId,
    ) -> Result<(), DomError> {
        // 1. Transactional Pre-Checks
        if !self.nodes.contains(u64::from(parent_id) as usize) {
            return Err(DomError::InvalidParent);
        }
        if !self.nodes.contains(u64::from(child_id) as usize) {
            return Err(DomError::InvalidChild);
        }

        if parent_id == child_id {
            return Err(DomError::CycleDetected);
        }

        // Cycle check: walk up from parent to see if child is an ancestor
        let mut ancestor = Some(parent_id);
        while let Some(curr) = ancestor {
            if curr == child_id {
                return Err(DomError::CycleDetected);
            }
            ancestor = self.get_node(curr).and_then(|n| n.parent);
        }

        // Check if child already has a parent
        let old_parent = self.get_node(child_id).unwrap().parent;
        let needs_detach = old_parent == Some(parent_id);
        if !needs_detach && old_parent.is_some() {
            return Err(DomError::ChildAlreadyHasParent);
        }

        // 2. Mutation — all validation is complete above
        if needs_detach {
            self.detach_node(child_id);
        }
        // Add to parent's children
        let mut parent_in_doc = false;
        if let Some(parent) = self.get_node_mut(parent_id) {
            parent.children.push(child_id);
            parent.mark_dirty_and_ancestors();
            parent_in_doc = parent.flags.contains(NodeFlags::IS_IN_DOCUMENT);
        }

        // Set child's parent
        if let Some(child) = self.get_node_mut(child_id) {
            child.parent = Some(parent_id);
        }

        // Propagate IS_IN_DOCUMENT flag to child and all its descendants
        if parent_in_doc {
            self.propagate_in_document_flag(child_id);
        }

        Ok(())
    }

    /// Recursively iterates over a node and its descendants in DFS order.
    /// Also traverses into any attached shadow roots.
    fn traverse_nodes_dfs_mut(
        &mut self,
        node_id: taffy::NodeId,
        mut f: impl FnMut(&mut PawsElement<S>),
    ) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get_node_mut(id) {
                stack.extend(node.children.iter().copied());
                if let Some(shadow_root_id) = node.shadow_root_id {
                    stack.push(shadow_root_id);
                }
                f(node);
            }
        }
    }

    #[allow(dead_code)]
    fn traverse_nodes_dfs(&self, node_id: taffy::NodeId, mut f: impl FnMut(&PawsElement<S>)) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get_node(id) {
                stack.extend(node.children.iter().copied());
                if let Some(shadow_root_id) = node.shadow_root_id {
                    stack.push(shadow_root_id);
                }
                f(node);
            }
        }
    }

    /// Recursively sets the IS_IN_DOCUMENT flag on a node and all its descendants.
    fn propagate_in_document_flag(&mut self, node_id: taffy::NodeId) {
        self.traverse_nodes_dfs_mut(node_id, |node| node.flags.insert(NodeFlags::IS_IN_DOCUMENT));
    }

    /// Recursively clears the IS_IN_DOCUMENT flag on a node and all its descendants.
    fn clear_in_document_flag(&mut self, node_id: taffy::NodeId) {
        self.traverse_nodes_dfs_mut(node_id, |node| node.flags.remove(NodeFlags::IS_IN_DOCUMENT));
    }

    pub(crate) fn detach_node(&mut self, node_id: taffy::NodeId) {
        let parent_id = self.get_node(node_id).and_then(|n| n.parent);
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.get_node_mut(parent_id) {
                if let Some(pos) = parent.children.iter().position(|&id| id == node_id) {
                    parent.children.remove(pos);
                    parent.mark_dirty_and_ancestors();
                }
            }
            if let Some(child) = self.get_node_mut(node_id) {
                child.parent = None;
            }
            self.clear_in_document_flag(node_id);
        }
    }

    /// Removes a child from its parent without deleting the child node.
    ///
    /// Validates that the child's parent matches `parent_id` before detaching.
    /// Per the W3C DOM spec, throws if the child is not a child of the parent.
    pub(crate) fn remove_child(
        &mut self,
        parent_id: taffy::NodeId,
        child_id: taffy::NodeId,
    ) -> Result<(), DomError> {
        if !self.nodes.contains(u64::from(parent_id) as usize) {
            return Err(DomError::InvalidParent);
        }
        if !self.nodes.contains(u64::from(child_id) as usize) {
            return Err(DomError::InvalidChild);
        }
        let actual_parent = self.get_node(child_id).unwrap().parent;
        if actual_parent != Some(parent_id) {
            return Err(DomError::InvalidChild);
        }
        self.detach_node(child_id);
        Ok(())
    }

    /// Replaces an old child with a new child under a given parent.
    ///
    /// The new child is inserted at the same position as the old child.
    /// The old child is detached (not deleted). Per the W3C DOM spec,
    /// this is `parentNode.replaceChild(newChild, oldChild)`.
    pub(crate) fn replace_child(
        &mut self,
        parent_id: taffy::NodeId,
        new_child_id: taffy::NodeId,
        old_child_id: taffy::NodeId,
    ) -> Result<(), DomError> {
        // Validate existence
        if !self.nodes.contains(u64::from(parent_id) as usize) {
            return Err(DomError::InvalidParent);
        }
        if !self.nodes.contains(u64::from(new_child_id) as usize) {
            return Err(DomError::InvalidChild);
        }
        if !self.nodes.contains(u64::from(old_child_id) as usize) {
            return Err(DomError::InvalidChild);
        }

        // Verify old_child is actually a child of parent
        let old_parent = self.get_node(old_child_id).unwrap().parent;
        if old_parent != Some(parent_id) {
            return Err(DomError::InvalidChild);
        }

        // Check new_child doesn't create a cycle
        if new_child_id == parent_id {
            return Err(DomError::CycleDetected);
        }
        let mut ancestor = Some(parent_id);
        while let Some(curr) = ancestor {
            if curr == new_child_id {
                return Err(DomError::CycleDetected);
            }
            ancestor = self.get_node(curr).and_then(|n| n.parent);
        }

        // If new_child already has a parent that isn't this parent, reject
        let new_parent = self.get_node(new_child_id).unwrap().parent;
        if new_parent.is_some() && new_parent != Some(parent_id) {
            return Err(DomError::ChildAlreadyHasParent);
        }

        // Find old_child's position in parent's children
        let pos = self
            .get_node(parent_id)
            .unwrap()
            .children
            .iter()
            .position(|&id| id == old_child_id)
            .unwrap();

        // Detach new_child if it has a parent
        if new_parent.is_some() {
            self.detach_node(new_child_id);
        }

        // Detach old_child
        if let Some(old) = self.get_node_mut(old_child_id) {
            old.parent = None;
        }
        self.clear_in_document_flag(old_child_id);

        // Insert new_child at the old position
        if let Some(parent) = self.get_node_mut(parent_id) {
            // Remove old_child from children (it's at `pos`)
            parent.children.remove(pos);
            parent.children.insert(pos, new_child_id);
            parent.mark_dirty_and_ancestors();
        }

        // Set new_child's parent
        if let Some(new_child) = self.get_node_mut(new_child_id) {
            new_child.parent = Some(parent_id);
        }

        // Propagate IS_IN_DOCUMENT flag
        let parent_in_doc = self
            .get_node(parent_id)
            .is_some_and(|n| n.flags.contains(NodeFlags::IS_IN_DOCUMENT));
        if parent_in_doc {
            self.propagate_in_document_flag(new_child_id);
        }

        Ok(())
    }

    /// Inserts a new child before a reference child in the parent's children list.
    ///
    /// Per the W3C DOM spec, `parentNode.insertBefore(newChild, refChild)`.
    /// The reference child must be a child of the parent. If the new child
    /// already belongs to the same parent, it is moved to the new position.
    pub(crate) fn insert_before(
        &mut self,
        parent_id: taffy::NodeId,
        new_child_id: taffy::NodeId,
        ref_child_id: taffy::NodeId,
    ) -> Result<(), DomError> {
        // Validate existence
        if !self.nodes.contains(u64::from(parent_id) as usize) {
            return Err(DomError::InvalidParent);
        }
        if !self.nodes.contains(u64::from(new_child_id) as usize) {
            return Err(DomError::InvalidChild);
        }
        if !self.nodes.contains(u64::from(ref_child_id) as usize) {
            return Err(DomError::InvalidChild);
        }

        // Verify ref_child is a child of parent
        let ref_parent = self.get_node(ref_child_id).unwrap().parent;
        if ref_parent != Some(parent_id) {
            return Err(DomError::InvalidChild);
        }

        // Inserting a node before itself is a no-op per the W3C DOM spec
        if new_child_id == ref_child_id {
            return Ok(());
        }

        // Cycle detection
        if new_child_id == parent_id {
            return Err(DomError::CycleDetected);
        }
        let mut ancestor = Some(parent_id);
        while let Some(curr) = ancestor {
            if curr == new_child_id {
                return Err(DomError::CycleDetected);
            }
            ancestor = self.get_node(curr).and_then(|n| n.parent);
        }

        // Check if new_child already has a parent
        let old_parent = self.get_node(new_child_id).unwrap().parent;
        let needs_detach = old_parent == Some(parent_id);
        if !needs_detach && old_parent.is_some() {
            return Err(DomError::ChildAlreadyHasParent);
        }

        // Detach new_child if it's already in this parent (re-insert at new position)
        if needs_detach {
            self.detach_node(new_child_id);
        }

        // Re-find ref_child's position after potential detach (index may have shifted)
        let pos = self
            .get_node(parent_id)
            .unwrap()
            .children
            .iter()
            .position(|&id| id == ref_child_id)
            .unwrap();

        // Insert new_child before ref_child
        let mut parent_in_doc = false;
        if let Some(parent) = self.get_node_mut(parent_id) {
            parent.children.insert(pos, new_child_id);
            parent.mark_dirty_and_ancestors();
            parent_in_doc = parent.flags.contains(NodeFlags::IS_IN_DOCUMENT);
        }

        // Set new_child's parent
        if let Some(child) = self.get_node_mut(new_child_id) {
            child.parent = Some(parent_id);
        }

        // Propagate IS_IN_DOCUMENT flag
        if parent_in_doc {
            self.propagate_in_document_flag(new_child_id);
        }

        Ok(())
    }

    /// Creates a copy of a DOM node.
    ///
    /// If `deep` is true, all descendants are cloned recursively and attached
    /// to the cloned node. The cloned node has no parent (is orphaned) and is
    /// not connected to the document.
    ///
    /// Copies: node type, tag name, attributes, classes, id, text content,
    /// and inline style (Arc-cloned).
    /// Does NOT copy: parent, computed styles, layout data, render state,
    /// stylo element data, or shadow root.
    pub(crate) fn clone_node(
        &mut self,
        node_id: taffy::NodeId,
        deep: bool,
    ) -> Result<taffy::NodeId, DomError> {
        if !self.nodes.contains(u64::from(node_id) as usize) {
            return Err(DomError::InvalidChild);
        }

        // Read source node data
        let source = self.get_node(node_id).unwrap();
        let node_type = source.node_type;
        let name = source.name.clone();
        let id_attr = source.id_attr.clone();
        let attrs = source.attrs.clone();
        let classes = source.classes.clone();
        let style_attribute = source.style_attribute.clone();
        let text_content = source.text_content.clone();
        let children: Vec<taffy::NodeId> = if deep {
            source.children.clone()
        } else {
            Vec::new()
        };

        // Create the new node
        let new_id = self.create_node(node_type);
        let new_node = self.get_node_mut(new_id).unwrap();
        new_node.name = name;
        new_node.id_attr = id_attr;
        new_node.attrs = attrs;
        new_node.classes = classes;
        new_node.style_attribute = style_attribute;
        new_node.text_content = text_content;

        // Deep clone: recursively clone children and append
        if deep {
            for child_id in children {
                let cloned_child_id = self.clone_node(child_id, true)?;
                // append_child won't fail here: no cycles, no existing parent
                self.append_child(new_id, cloned_child_id)
                    .expect("cloned child append cannot fail");
            }
        }

        Ok(new_id)
    }

    /// Returns the next sibling of the given node, or `None`.
    pub(crate) fn get_next_sibling(&self, node_id: taffy::NodeId) -> Option<taffy::NodeId> {
        let parent_id = self.get_node(node_id)?.parent?;
        let parent = self.get_node(parent_id)?;
        let pos = parent.children.iter().position(|&id| id == node_id)?;
        parent.children.get(pos + 1).copied()
    }

    /// Returns the previous sibling of the given node, or `None`.
    pub(crate) fn get_previous_sibling(&self, node_id: taffy::NodeId) -> Option<taffy::NodeId> {
        let parent_id = self.get_node(node_id)?.parent?;
        let parent = self.get_node(parent_id)?;
        let pos = parent.children.iter().position(|&id| id == node_id)?;
        if pos == 0 {
            None
        } else {
            parent.children.get(pos - 1).copied()
        }
    }

    pub(crate) fn remove_node(&mut self, id: taffy::NodeId) -> Result<(), DomError> {
        if !self.nodes.contains(u64::from(id) as usize) {
            return Err(DomError::InvalidChild);
        }
        self.detach_node(id);

        // Recursively collect all descendants (including `id` itself) and remove them.
        // Also traverses into shadow trees attached to any descendant.
        let mut to_remove = Vec::new();
        let mut stack = vec![id];
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.get_node(nid) {
                to_remove.push(nid);
                stack.extend(node.children.iter().copied());
                // Also traverse into shadow root if present
                if let Some(shadow_root_id) = node.shadow_root_id {
                    stack.push(shadow_root_id);
                }
            }
        }

        for node_id in to_remove {
            let index = u64::from(node_id) as usize;
            if self.nodes.contains(index) {
                let removed = self.nodes.remove(index);
                self.removed_render_states
                    .push((node_id, removed.render_state));
            }
        }
        Ok(())
    }

    /// Returns a live handle to the computed style map for the given element.
    ///
    /// The returned [`StylePropertyMapReadOnly`] lazily triggers style
    /// resolution when its read methods are called.
    /// Returns `None` if the element does not exist or is not an element node.
    pub fn computed_style_map(
        &self,
        element_id: taffy::NodeId,
    ) -> Option<crate::style::typed_om::StylePropertyMapReadOnly> {
        let node = self.get_node(element_id)?;
        if !node.is_element() {
            return None;
        }
        Some(crate::style::typed_om::StylePropertyMapReadOnly::new(
            element_id,
        ))
    }

    /// Ensures computed styles are up-to-date for the document tree.
    ///
    /// Checks the root's dirty-descendants flag and triggers a full style
    /// resolution pass if any node is dirty. Layout caches are cleared after
    /// a restyle because current invalidation is full-tree, not incremental.
    /// This is the lazy resolution entry point used by
    /// [`StylePropertyMapReadOnly`] read operations.
    pub(crate) fn ensure_styles_resolved(&mut self, style_context: &crate::style::StyleContext) {
        if let Some(root) = self.get_node(self.root) {
            if root.has_dirty_descendants() {
                self.resolve_style(style_context);
                self.clear_layout_caches();
            }
        }
    }

    fn clear_layout_caches(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.layout_cache.clear();
        }
    }

    /// Resolves CSS styles for all element nodes in the document tree.
    ///
    /// Uses BFS traversal from the root to ensure parents are styled before
    /// children, which is required for CSS inheritance to work correctly.
    pub fn resolve_style(&mut self, style_context: &crate::style::StyleContext) {
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(self.root);

        while let Some(id) = queue.pop_front() {
            if let Some(node) = self.get_node(id) {
                if node.is_element() {
                    // Resolve the style parent: if the DOM parent is a ShadowRoot,
                    // inherit from the host element (the ShadowRoot's parent) instead.
                    let parent_style = {
                        let mut style_parent_id = node.parent;
                        if let Some(pid) = node.parent {
                            if let Some(p) = self.get_node(pid) {
                                if p.node_type == NodeType::ShadowRoot {
                                    style_parent_id = p.parent; // host element
                                }
                            }
                        }
                        style_parent_id
                            .and_then(|pid| self.get_node(pid))
                            .and_then(|p| p.computed_values.as_ref())
                            .cloned()
                    };

                    let computed = crate::style::compute_style_for_node(
                        self,
                        style_context,
                        node,
                        parent_style.as_deref(),
                    );
                    // Re-borrow to enqueue children before mutable borrow
                    let children: Vec<taffy::NodeId> =
                        self.get_node(id).map_or(Vec::new(), |n| n.children.clone());
                    for &child_id in &children {
                        queue.push_back(child_id);
                    }

                    // Also enter the shadow tree if this element is a shadow host.
                    // Run slot assignment first so the flat tree is available for layout.
                    if let Some(shadow_root_id) = self.get_node(id).and_then(|n| n.shadow_root_id) {
                        self.assign_slots(id);
                        if let Some(shadow_root) = self.get_node(shadow_root_id) {
                            let shadow_children: Vec<taffy::NodeId> = shadow_root.children.clone();
                            for &child_id in &shadow_children {
                                queue.push_back(child_id);
                            }
                        }
                    }

                    // Determine if this element creates a stacking context.
                    // Re-borrow node since we may have mutated during assign_slots.
                    let node = self.get_node(id).unwrap();
                    let parent_display_inside = node
                        .parent
                        .and_then(|pid| self.get_node(pid))
                        .and_then(|p| p.computed_values.as_ref())
                        .map(|cv| cv.clone_display().inside())
                        .unwrap_or(style::values::specified::box_::DisplayInside::Flow);
                    let is_root = node.parent.is_none_or(|pid| {
                        self.get_node(pid)
                            .is_some_and(|p| p.node_type == super::element::NodeType::Document)
                    });
                    let is_flex_or_grid_item = matches!(
                        parent_display_inside,
                        style::values::specified::box_::DisplayInside::Flex
                            | style::values::specified::box_::DisplayInside::Grid
                    );
                    let is_stacking_context = crate::layout::stacking::creates_stacking_context(
                        &computed,
                        is_root,
                        is_flex_or_grid_item,
                    );

                    if let Some(mut_node) = self.get_node_mut(id) {
                        mut_node.taffy_style = Some(crate::style::to_taffy_style(&computed));
                        mut_node.computed_values = Some(computed);
                        mut_node.creates_stacking_context = is_stacking_context;
                        mut_node.unset_dirty_descendants();
                    }
                } else if node.is_text_node() {
                    // Text nodes inherit parent styles and get a default
                    // taffy::Style so layout can measure them as leaf nodes.
                    // If the DOM parent is a ShadowRoot, inherit from the host.
                    let parent_cv = {
                        let mut style_parent_id = node.parent;
                        if let Some(pid) = node.parent {
                            if let Some(p) = self.get_node(pid) {
                                if p.node_type == NodeType::ShadowRoot {
                                    style_parent_id = p.parent;
                                }
                            }
                        }
                        style_parent_id
                            .and_then(|pid| self.get_node(pid))
                            .and_then(|p| p.computed_values.as_ref())
                            .cloned()
                    };
                    if let Some(mut_node) = self.get_node_mut(id) {
                        mut_node.taffy_style = Some(taffy::Style::default());
                        mut_node.computed_values = parent_cv;
                        mut_node.unset_dirty_descendants();
                    }
                } else {
                    // Non-element, non-text nodes (Document, ShadowRoot):
                    // enqueue children for traversal.
                    let children: Vec<taffy::NodeId> =
                        self.get_node(id).map_or(Vec::new(), |n| n.children.clone());
                    for &child_id in &children {
                        queue.push_back(child_id);
                    }
                    // Clear dirty flag on non-element nodes too
                    if let Some(node) = self.get_node(id) {
                        node.unset_dirty_descendants();
                    }
                }
            }
        }
    }

    /// Returns the child list used by layout and rendering.
    ///
    /// Shadow hosts expose their flat tree children: shadow root children with
    /// `<slot>` elements replaced by assigned light DOM or fallback content.
    pub(crate) fn flat_tree_child_ids(&self, parent_id: taffy::NodeId) -> Vec<taffy::NodeId> {
        let node = self.node(parent_id);
        if let Some(shadow_root_id) = node.shadow_root_id {
            self.flatten_shadow_children(shadow_root_id)
        } else {
            node.children.clone()
        }
    }

    /// Builds the flat tree children for a shadow root, replacing `<slot>`
    /// elements with their assigned light DOM children (or the slot's own
    /// children as fallback content).
    pub(crate) fn flatten_shadow_children(
        &self,
        shadow_root_id: taffy::NodeId,
    ) -> Vec<taffy::NodeId> {
        let shadow_root = self.node(shadow_root_id);
        let mut result = Vec::with_capacity(shadow_root.children.len());
        for &child_id in &shadow_root.children {
            let child = self.node(child_id);
            if child.is_slot_element() {
                if child.assigned_nodes.is_empty() {
                    result.extend_from_slice(&child.children);
                } else {
                    result.extend_from_slice(&child.assigned_nodes);
                }
            } else {
                result.push(child_id);
            }
        }
        result
    }

    /// Assigns light DOM children of a shadow host to `<slot>` elements
    /// in its shadow tree (named slot assignment algorithm per WHATWG DOM spec).
    pub(crate) fn assign_slots(&mut self, host_id: taffy::NodeId) {
        let shadow_root_id = match self.get_node(host_id).and_then(|n| n.shadow_root_id) {
            Some(id) => id,
            None => return,
        };

        // Collect all <slot> elements in the shadow tree (DFS, tree order).
        let mut slots = Vec::new();
        let mut stack: Vec<taffy::NodeId> = self
            .get_node(shadow_root_id)
            .map_or(Vec::new(), |shadow_root| {
                shadow_root.children.iter().copied().rev().collect()
            });
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.get_node(nid) {
                if node.is_slot_element() {
                    slots.push(nid);
                }
                // Push children in reverse so we visit in tree order
                for &child_id in node.children.iter().rev() {
                    stack.push(child_id);
                }
            }
        }

        // Clear previous slot assignments.
        for &slot_id in &slots {
            if let Some(slot) = self.get_node_mut(slot_id) {
                slot.assigned_nodes.clear();
            }
        }
        let host_children: Vec<taffy::NodeId> = self
            .get_node(host_id)
            .map_or(Vec::new(), |h| h.children.clone());
        for &child_id in &host_children {
            if let Some(child) = self.get_node_mut(child_id) {
                child.assigned_slot_id = None;
            }
        }

        // Build a snapshot of slot names for matching.
        let slot_names: Vec<(taffy::NodeId, String)> = slots
            .iter()
            .map(|&sid| {
                let name = self
                    .get_node(sid)
                    .and_then(|s| s.get_attribute("name"))
                    .unwrap_or("")
                    .to_string();
                (sid, name)
            })
            .collect();

        // Assign each host child to the first matching slot (named assignment).
        for &child_id in &host_children {
            let child_slot_attr = self
                .get_node(child_id)
                .and_then(|c| c.get_attribute("slot"))
                .unwrap_or("")
                .to_string();

            // Find the first slot whose name matches the child's slot attribute.
            if let Some(&(slot_id, _)) =
                slot_names.iter().find(|(_, name)| *name == child_slot_attr)
            {
                if let Some(slot) = self.get_node_mut(slot_id) {
                    slot.assigned_nodes.push(child_id);
                }
                if let Some(child) = self.get_node_mut(child_id) {
                    child.assigned_slot_id = Some(slot_id);
                }
            }
            // If no matching slot, the child is unslotted (not rendered).
        }
    }
}
