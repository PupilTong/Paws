use crate::dom::element::{NodeFlags, NodeType, PawsElement};
use markup5ever::QualName;
use slab::Slab;
use style::shared_lock::SharedRwLock;

/// Errors that can occur during DOM tree operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomError {
    /// The specified parent node ID does not exist in the tree.
    InvalidParent,
    /// The specified child node ID does not exist in the tree.
    InvalidChild,
    /// Appending would create a cycle in the tree (child is ancestor of parent).
    CycleDetected,
    /// The child node already has a different parent. Detach it first.
    ChildAlreadyHasParent,
}

impl std::fmt::Display for DomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomError::InvalidParent => write!(f, "invalid parent id"),
            DomError::InvalidChild => write!(f, "invalid child id"),
            DomError::CycleDetected => write!(f, "append would create a cycle"),
            DomError::ChildAlreadyHasParent => write!(f, "child already has a parent"),
        }
    }
}

impl std::error::Error for DomError {}

/// The document tree, backed by a [`Slab`] arena for cache-friendly access.
///
/// Owns all DOM nodes and manages tree mutations (append, detach, remove)
/// with cycle detection and IS_IN_DOCUMENT flag propagation.
pub struct Document {
    /// A slab-backed tree of nodes
    pub nodes: Box<Slab<PawsElement>>,

    /// Stylo shared lock
    pub guard: SharedRwLock,

    /// Root node ID
    pub root: usize,

    /// Document stylesheets
    pub stylesheets: Vec<crate::style::CSSStyleSheet>,

    /// Document URL
    pub url: url::Url,
}

impl Document {
    pub fn new(guard: SharedRwLock, url: url::Url) -> Self {
        let mut nodes = Box::new(Slab::new());
        let slab_ptr = nodes.as_mut() as *mut Slab<PawsElement>;

        // Create root node (Document)
        let root_entry = nodes.vacant_entry();
        let root_id = root_entry.key();

        root_entry.insert(PawsElement::new(
            slab_ptr,
            root_id,
            guard.clone(),
            NodeType::Document,
        ));

        // Set IS_IN_DOCUMENT flag for root
        nodes[root_id].flags.insert(NodeFlags::IS_IN_DOCUMENT);

        Document {
            nodes,
            guard,
            root: root_id,
            stylesheets: Vec::new(),
            url,
        }
    }

    pub fn tree(&self) -> &Slab<PawsElement> {
        &self.nodes
    }

    pub fn get_node(&self, id: usize) -> Option<&PawsElement> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut PawsElement> {
        self.nodes.get_mut(id)
    }

    pub fn create_node(&mut self, node_type: NodeType) -> usize {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<PawsElement>;
        let guard = self.guard.clone();

        let entry = self.nodes.vacant_entry();
        let id = entry.key();
        entry.insert(PawsElement::new(slab_ptr, id, guard, node_type));

        id
    }

    pub fn create_element(&mut self, name: QualName) -> usize {
        let id = self.create_node(NodeType::Element);
        let el = self.nodes.get_mut(id).unwrap();
        el.name = Some(name);
        id
    }

    pub fn create_text_node(&mut self, content: String) -> usize {
        let id = self.create_node(NodeType::Text);
        let el = self.nodes.get_mut(id).unwrap();
        el.text_content = Some(content);
        id
    }

    pub fn append_child(&mut self, parent_id: usize, child_id: usize) -> Result<(), DomError> {
        // 1. Transactional Pre-Checks
        if !self.nodes.contains(parent_id) {
            return Err(DomError::InvalidParent);
        }
        if !self.nodes.contains(child_id) {
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
            ancestor = self.nodes.get(curr).and_then(|n| n.parent);
        }

        // Check if child already has a parent
        let old_parent = self.nodes[child_id].parent;
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
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children.push(child_id);
            parent.set_dirty_descendants();
            parent_in_doc = parent.flags.contains(NodeFlags::IS_IN_DOCUMENT);
        }

        // Set child's parent
        if let Some(child) = self.nodes.get_mut(child_id) {
            child.parent = Some(parent_id);
        }

        // Propagate IS_IN_DOCUMENT flag to child and all its descendants
        if parent_in_doc {
            self.propagate_in_document_flag(child_id);
        }

        Ok(())
    }

    /// Recursively sets the IS_IN_DOCUMENT flag on a node and all its descendants.
    /// Uses iterative (stack-based) traversal to avoid stack overflow on deep trees.
    fn propagate_in_document_flag(&mut self, node_id: usize) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.get_mut(id) {
                node.flags.insert(NodeFlags::IS_IN_DOCUMENT);
                stack.extend(node.children.iter().copied());
            }
        }
    }

    /// Recursively clears the IS_IN_DOCUMENT flag on a node and all its descendants.
    /// Uses iterative (stack-based) traversal to avoid stack overflow on deep trees.
    fn clear_in_document_flag(&mut self, node_id: usize) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.get_mut(id) {
                node.flags.remove(NodeFlags::IS_IN_DOCUMENT);
                stack.extend(node.children.iter().copied());
            }
        }
    }

    pub fn detach_node(&mut self, node_id: usize) {
        let parent_id = self.nodes.get(node_id).and_then(|n| n.parent);
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                if let Some(pos) = parent.children.iter().position(|&id| id == node_id) {
                    parent.children.remove(pos);
                    parent.set_dirty_descendants();
                }
            }
            if let Some(child) = self.nodes.get_mut(node_id) {
                child.parent = None;
            }
            self.clear_in_document_flag(node_id);
        }
    }

    pub fn remove_node(&mut self, id: usize) -> Result<(), DomError> {
        if !self.nodes.contains(id) {
            return Err(DomError::InvalidChild);
        }
        self.detach_node(id);

        // Recursively collect all descendants (including `id` itself) and remove them.
        // Uses iterative DFS to avoid stack overflow on deep trees.
        let mut to_remove = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            to_remove.push(current);
            if let Some(node) = self.nodes.get(current) {
                stack.extend(node.children.iter().copied());
            }
        }
        for node_id in to_remove {
            if self.nodes.contains(node_id) {
                self.nodes.remove(node_id);
            }
        }
        Ok(())
    }

    /// Resolves CSS styles for all element nodes in the document tree.
    ///
    /// Uses BFS traversal from the root to ensure parents are styled before
    /// children, which is required for CSS inheritance to work correctly.
    pub fn resolve_style(&mut self, style_context: &crate::style::StyleContext) {
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(self.root);

        while let Some(id) = queue.pop_front() {
            if let Some(node) = self.nodes.get(id) {
                if node.is_element() {
                    let parent_style = node
                        .parent
                        .and_then(|pid| self.nodes.get(pid))
                        .and_then(|p| p.computed_values.as_ref())
                        .cloned();
                    let computed = crate::style::compute_style_for_node(
                        self,
                        style_context,
                        node,
                        parent_style.as_deref(),
                    );
                    // Re-borrow to enqueue children before mutable borrow
                    let children: Vec<usize> = self
                        .nodes
                        .get(id)
                        .map_or(Vec::new(), |n| n.children.clone());
                    for &child_id in &children {
                        queue.push_back(child_id);
                    }
                    if let Some(mut_node) = self.nodes.get_mut(id) {
                        mut_node.computed_values = Some(computed);
                    }
                } else {
                    // Non-element nodes: still enqueue children for traversal
                    let children: Vec<usize> = self
                        .nodes
                        .get(id)
                        .map_or(Vec::new(), |n| n.children.clone());
                    for &child_id in &children {
                        queue.push_back(child_id);
                    }
                }
            }
        }
    }
}
