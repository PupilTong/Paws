use crate::dom::element::{NodeFlags, NodeType, PawsElement};
use crate::layout::text::TextMeasurer;
use markup5ever::QualName;
use slab::Slab;
use style::shared_lock::SharedRwLock;
use style::values::computed::length::CSSPixelLength;
use style::values::computed::length_percentage::CalcLengthPercentage;
use style::values::specified::font::FONT_MEDIUM_PX;
use taffy::compute_block_layout;
use taffy::prelude::*;
use taffy::tree::{Layout, LayoutInput, LayoutOutput};
use taffy::{
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_hidden_layout,
    compute_leaf_layout, CacheTree,
};

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
    pub(crate) nodes: Box<Slab<PawsElement>>,

    /// Stylo shared lock
    pub(crate) guard: SharedRwLock,

    /// Root node ID
    pub(crate) root: taffy::NodeId,

    /// Document stylesheets
    pub(crate) stylesheets: Vec<crate::style::CSSStyleSheet>,

    /// Document URL
    #[allow(dead_code)]
    pub(crate) url: url::Url,

    /// Type-erased pointer to the text measurer, set only during a layout pass.
    ///
    /// `None` outside of `compute_layout`. Stored as `(data_ptr, vtable_ptr)`
    /// to avoid lifetime issues with fat pointers to trait objects. The pointer
    /// is valid for the duration of the `compute_layout` call because the
    /// caller holds the `&dyn TextMeasurer` borrow that outlives the layout pass.
    pub(crate) text_measurer: Option<(*const (), *const ())>,
}

impl Document {
    pub(crate) fn new(guard: SharedRwLock, url: url::Url) -> Self {
        let mut nodes = Box::new(Slab::new());
        let slab_ptr = nodes.as_mut() as *mut Slab<PawsElement>;

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
            text_measurer: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn tree(&self) -> &Slab<PawsElement> {
        &self.nodes
    }

    pub fn get_node(&self, id: taffy::NodeId) -> Option<&PawsElement> {
        self.nodes.get(u64::from(id) as usize)
    }

    pub(crate) fn get_node_mut(&mut self, id: taffy::NodeId) -> Option<&mut PawsElement> {
        self.nodes.get_mut(u64::from(id) as usize)
    }

    /// Panicking accessor for layout passes. Use `get_node` for fallible access.
    #[inline]
    fn node(&self, id: taffy::NodeId) -> &PawsElement {
        self.get_node(id).expect("valid node id during layout")
    }

    /// Panicking mutable accessor for layout passes. Use `get_node_mut` for fallible access.
    #[inline]
    fn node_mut(&mut self, id: taffy::NodeId) -> &mut PawsElement {
        self.get_node_mut(id).expect("valid node id during layout")
    }

    /// Returns the text measurer set for the current layout pass.
    ///
    /// # Panics
    /// Panics if called outside of a layout pass.
    fn text_measurer(&self) -> &dyn TextMeasurer {
        let (data, vtable) = self
            .text_measurer
            .expect("text_measurer accessed outside layout pass");
        // SAFETY: The `(data, vtable)` pair was created by `compute_layout`
        // from a valid `&dyn TextMeasurer` via `std::mem::transmute`. The
        // original reference's lifetime spans the entire layout pass, and
        // this field is cleared to `None` before `compute_layout` returns.
        unsafe {
            let fat_ptr: *const dyn TextMeasurer = std::mem::transmute((data, vtable));
            &*fat_ptr
        }
    }

    pub(crate) fn create_node(&mut self, node_type: NodeType) -> taffy::NodeId {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<PawsElement>;
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
            parent.set_dirty_descendants();
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
    fn traverse_nodes_dfs_mut(
        &mut self,
        node_id: taffy::NodeId,
        mut f: impl FnMut(&mut PawsElement),
    ) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get_node_mut(id) {
                stack.extend(node.children.iter().copied());
                f(node);
            }
        }
    }

    fn traverse_nodes_dfs(&self, node_id: taffy::NodeId, mut f: impl FnMut(&PawsElement)) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get_node(id) {
                stack.extend(node.children.iter().copied());
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
                    parent.set_dirty_descendants();
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
            parent.set_dirty_descendants();
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
        let mut to_remove = Vec::new();
        self.traverse_nodes_dfs(id, |node| to_remove.push(node.id));

        for node_id in to_remove {
            let index = u64::from(node_id) as usize;
            if self.nodes.contains(index) {
                self.nodes.remove(index);
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
    /// resolution pass if any node is dirty. This is the lazy resolution
    /// entry point used by [`StylePropertyMapReadOnly`] read operations.
    pub(crate) fn ensure_styles_resolved(&mut self, style_context: &crate::style::StyleContext) {
        if let Some(root) = self.get_node(self.root) {
            if root.has_dirty_descendants() {
                self.resolve_style(style_context);
            }
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
                    let parent_style = node
                        .parent
                        .and_then(|pid| self.get_node(pid))
                        .and_then(|p| p.computed_values.as_ref())
                        .cloned();
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
                    if let Some(mut_node) = self.get_node_mut(id) {
                        mut_node.taffy_style = Some(crate::style::to_taffy_style(&computed));
                        mut_node.computed_values = Some(computed);
                        mut_node.unset_dirty_descendants();
                    }
                } else {
                    // Non-element nodes: still enqueue children for traversal
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
}

// ─── ChildIter ───────────────────────────────────────────────────────

/// Zero-allocation iterator over a node's children.
///
/// Wraps a slice iterator directly — no Vec allocation per traversal call.
/// Zero-allocation iterator over a node's children for Taffy layout traversal.
pub struct ChildIter<'a>(std::slice::Iter<'a, taffy::NodeId>);

impl Iterator for ChildIter<'_> {
    type Item = taffy::NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

// ─── TraversePartialTree ─────────────────────────────────────────────

impl taffy::TraversePartialTree for Document {
    type ChildIter<'a> = ChildIter<'a>;

    #[inline]
    fn child_ids(&self, parent_node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.node(parent_node_id).children.iter())
    }

    #[inline]
    fn child_count(&self, parent_node_id: taffy::NodeId) -> usize {
        self.node(parent_node_id).children.len()
    }

    #[inline]
    fn get_child_id(&self, parent_node_id: taffy::NodeId, child_index: usize) -> taffy::NodeId {
        self.node(parent_node_id).children[child_index]
    }
}

// ─── TraverseTree (marker) ───────────────────────────────────────────

impl taffy::TraverseTree for Document {}

// ─── LayoutPartialTree ───────────────────────────────────────────────

impl taffy::LayoutPartialTree for Document {
    type CoreContainerStyle<'a> = &'a taffy::Style;

    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: taffy::NodeId) -> Self::CoreContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("node must have taffy_style for layout")
    }

    fn resolve_calc_value(&self, val: *const (), basis: f32) -> f32 {
        // SAFETY: `val` was created by `CompactLength::calc(ptr)` in
        // `style::convert::length::length_percentage()`, where `ptr` is a
        // `*const CalcLengthPercentage`. The pointee remains live because the
        // `ComputedValues` (which owns it via `Arc`) are borrowed through the
        // `Document` reference held during the layout pass.
        let calc = unsafe { &*(val as *const CalcLengthPercentage) };
        calc.resolve(CSSPixelLength::new(basis)).px()
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &Layout) {
        self.node_mut(node_id).unrounded_layout = *layout;
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
    ) -> LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |doc, node_id, inputs| {
            let node = doc.node(node_id);
            let style = node
                .taffy_style
                .as_ref()
                .expect("node must have taffy_style");
            let display = style.display;
            let is_text = node.node_type == NodeType::Text;
            let has_children = doc.child_count(node_id) > 0;

            if display == Display::None {
                return compute_hidden_layout(doc, node_id);
            }

            if is_text {
                return compute_text_leaf(doc, node_id, inputs);
            }

            if !has_children {
                return compute_leaf_layout(
                    inputs,
                    style,
                    |val, basis| doc.resolve_calc_value(val, basis),
                    |_known_dimensions, _available_space| Size::ZERO,
                );
            }

            match display {
                Display::Flex => compute_flexbox_layout(doc, node_id, inputs),
                Display::Grid => compute_grid_layout(doc, node_id, inputs),
                _ => compute_block_layout(doc, node_id, inputs),
            }
        })
    }
}

/// Computes layout for a text leaf node using the text measurer.
fn compute_text_leaf(
    doc: &mut Document,
    node_id: taffy::NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let node = doc.node(node_id);
    let font_size = node
        .computed_values
        .as_ref()
        .map(|cv| {
            let fs = cv.clone_font_size().computed_size().px();
            if fs > 0.0 {
                fs
            } else {
                FONT_MEDIUM_PX
            }
        })
        .unwrap_or(FONT_MEDIUM_PX);
    let text = node.text_content.as_deref().unwrap_or("");

    let (width, height) = doc.text_measurer().measure_text(text, font_size, None);

    let style = doc
        .node(node_id)
        .taffy_style
        .as_ref()
        .expect("text node must have taffy_style");

    compute_leaf_layout(
        inputs,
        style,
        |val, basis| doc.resolve_calc_value(val, basis),
        |_known_dimensions, _available_space| Size { width, height },
    )
}

// ─── CacheTree ───────────────────────────────────────────────────────

impl CacheTree for Document {
    fn cache_get(
        &self,
        node_id: taffy::NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
    ) -> Option<LayoutOutput> {
        self.node(node_id)
            .layout_cache
            .get(known_dimensions, available_space, run_mode)
    }

    fn cache_store(
        &mut self,
        node_id: taffy::NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
        layout_output: LayoutOutput,
    ) {
        self.node_mut(node_id).layout_cache.store(
            known_dimensions,
            available_space,
            run_mode,
            layout_output,
        );
    }

    fn cache_clear(&mut self, node_id: taffy::NodeId) {
        self.node_mut(node_id).layout_cache.clear();
    }
}

// ─── LayoutFlexboxContainer ──────────────────────────────────────────

impl taffy::LayoutFlexboxContainer for Document {
    type FlexboxContainerStyle<'a> = &'a taffy::Style;

    type FlexboxItemStyle<'a> = &'a taffy::Style;

    fn get_flexbox_container_style(
        &self,
        node_id: taffy::NodeId,
    ) -> Self::FlexboxContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("flexbox container must have taffy_style")
    }

    fn get_flexbox_child_style(&self, child_node_id: taffy::NodeId) -> Self::FlexboxItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("flexbox child must have taffy_style")
    }
}

// ─── LayoutGridContainer ─────────────────────────────────────────────

impl taffy::LayoutGridContainer for Document {
    type GridContainerStyle<'a> = &'a taffy::Style;

    type GridItemStyle<'a> = &'a taffy::Style;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("grid container must have taffy_style")
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("grid child must have taffy_style")
    }
}

// ─── LayoutBlockContainer ────────────────────────────────────────────

impl taffy::LayoutBlockContainer for Document {
    type BlockContainerStyle<'a> = &'a taffy::Style;

    type BlockItemStyle<'a> = &'a taffy::Style;

    fn get_block_container_style(&self, node_id: taffy::NodeId) -> Self::BlockContainerStyle<'_> {
        self.node(node_id)
            .taffy_style
            .as_ref()
            .expect("block container must have taffy_style")
    }

    fn get_block_child_style(&self, child_node_id: taffy::NodeId) -> Self::BlockItemStyle<'_> {
        self.node(child_node_id)
            .taffy_style
            .as_ref()
            .expect("block child must have taffy_style")
    }
}

// ─── RoundTree ───────────────────────────────────────────────────────

impl taffy::RoundTree for Document {
    fn get_unrounded_layout(&self, node_id: taffy::NodeId) -> Layout {
        self.node(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: taffy::NodeId, layout: &Layout) {
        self.node_mut(node_id).final_layout = *layout;
    }
}
