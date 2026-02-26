use markup5ever::QualName;
use slab::Slab;
use std::collections::HashMap;
use style::shared_lock::SharedRwLock;

use crate::dom::element::{NodeFlags, NodeType, PawsElement};

pub struct Document {
    /// A slab-backed tree of nodes
    pub nodes: Box<Slab<PawsElement>>,

    /// Stylo shared lock
    pub guard: SharedRwLock,

    /// Root node ID
    pub root: usize,

    /// Document stylesheets
    pub stylesheets: Vec<crate::style::CSSStyleSheet>,
}

impl Document {
    pub fn new(guard: SharedRwLock) -> Self {
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

    pub fn create_element(&mut self, name: QualName, attrs: HashMap<style::Atom, String>) -> usize {
        let id = self.create_node(NodeType::Element);
        let el = self.nodes.get_mut(id).unwrap();
        el.name = Some(name);
        for (k, v) in attrs {
            el.set_attribute(k.as_ref(), &v);
        }
        id
    }

    pub fn create_text_node(&mut self, content: String) -> usize {
        let id = self.create_node(NodeType::Text);
        let el = self.nodes.get_mut(id).unwrap();
        el.text_content = Some(content);
        id
    }

    pub fn append_child(&mut self, parent_id: usize, child_id: usize) -> Result<(), &'static str> {
        // 1. Transactional Pre-Checks
        if !self.nodes.contains(parent_id) {
            return Err("Invalid parent id");
        }
        if !self.nodes.contains(child_id) {
            return Err("Invalid child id");
        }

        if parent_id == child_id {
            return Err("Cycle detected");
        }

        // Cycle check: walk up from parent to see if child is an ancestor
        let mut ancestor = Some(parent_id);
        while let Some(curr) = ancestor {
            if curr == child_id {
                return Err("Cycle detected");
            }
            ancestor = self.nodes.get(curr).and_then(|n| n.parent);
        }

        // Check if child already has a parent
        let old_parent = self.nodes[child_id].parent;
        if old_parent == Some(parent_id) {
            self.detach_node(child_id);
        } else if old_parent.is_some() {
            return Err("Child already has a parent");
        }

        // 2. Mutation
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
            // Propagate flags
            if parent_in_doc {
                child.flags.insert(NodeFlags::IS_IN_DOCUMENT);
                // todo recursive flags
            }
        }

        Ok(())
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
                child.flags.remove(NodeFlags::IS_IN_DOCUMENT);
            }
        }
    }

    pub fn remove_node(&mut self, id: usize) -> Result<(), &'static str> {
        if !self.nodes.contains(id) {
            return Err("Invalid child id");
        }
        self.detach_node(id);
        self.nodes.remove(id);
        Ok(())
    }

    pub fn resolve_style(&mut self, style_context: &crate::style::StyleContext) {
        // Collect IDs to avoid borrowing issues while iterating
        let ids: Vec<usize> = self.nodes.iter().map(|(id, _)| id).collect();
        for id in ids {
            if let Some(node) = self.nodes.get(id) {
                if node.is_element() {
                    let computed = crate::style::compute_style_for_node(self, style_context, node);
                    if let Some(mut_node) = self.nodes.get_mut(id) {
                        mut_node.computed_values = Some(computed);
                    }
                }
            }
        }
    }

    pub fn layout(
        &self,
        style_context: &crate::style::StyleContext,
    ) -> Option<crate::layout::LayoutBox> {
        crate::layout::compute_layout(self, style_context, self.root)
    }
}
