use slab::Slab;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use style::shared_lock::SharedRwLock;

use crate::dom::element::ElementData; // Assuming we need this
use crate::dom::node::{Node, NodeData, NodeFlags};
use crate::dom::text::TextNodeData;
use markup5ever::QualName;

pub struct Document {
    /// ID of the document
    pub id: usize,

    /// A slab-backed tree of nodes
    pub nodes: Slab<Node>,

    /// Stylo shared lock
    pub guard: SharedRwLock,

    /// Root node ID
    pub root: usize,
}

impl Document {
    pub fn new() -> Self {
        Self::new_with_lock(SharedRwLock::new())
    }

    pub fn new_with_lock(guard: SharedRwLock) -> Self {
        static ID_GENERATOR: AtomicUsize = AtomicUsize::new(1);
        let id = ID_GENERATOR.fetch_add(1, Ordering::SeqCst);

        let mut nodes = Slab::new();

        // entry 0 is reserved or root? blitz uses 0 as root usually if inserted first.
        let slab_ptr = &mut nodes as *mut Slab<Node>;

        // Create root node (Document)
        let root_entry = nodes.vacant_entry();
        let root_id = root_entry.key();

        root_entry.insert(Node::new(
            slab_ptr,
            root_id,
            guard.clone(),
            NodeData::Document,
        ));

        // Set IS_IN_DOCUMENT flag for root
        nodes[root_id].flags.insert(NodeFlags::IS_IN_DOCUMENT);

        Document {
            id,
            nodes,
            guard,
            root: root_id, // Should be 0
        }
    }

    pub fn tree(&self) -> &Slab<Node> {
        &self.nodes
    }

    pub fn get_node(&self, id: usize) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    pub fn create_node(&mut self, data: NodeData) -> usize {
        let slab_ptr = &mut self.nodes as *mut Slab<Node>;
        let guard = self.guard.clone();

        let entry = self.nodes.vacant_entry();
        let id = entry.key();
        entry.insert(Node::new(slab_ptr, id, guard, data));

        id
    }

    pub fn create_element(&mut self, name: QualName, attrs: HashMap<style::Atom, String>) -> usize {
        let data = NodeData::Element(ElementData::new(name, attrs));
        self.create_node(data)
    }

    pub fn create_text_node(&mut self, content: String) -> usize {
        let data = NodeData::Text(TextNodeData::new(content));
        self.create_node(data)
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
        if let Some(old_parent_id) = old_parent {
            if old_parent_id == parent_id {
                // Already child of this parent. Move to end?
                // For now, just return Ok or Error. Standard DOM appends to end.
                // We will remove from old position first.
                self.detach_node(child_id);
            } else {
                return Err("Child already has a parent");
            }
        }

        // 2. Mutation
        // Add to parent's children
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children.push(child_id);
        }

        // Set child's parent
        if let Some(child) = self.nodes.get_mut(child_id) {
            child.parent = Some(parent_id);
        }

        Ok(())
    }

    pub fn detach_node(&mut self, node_id: usize) {
        let parent_id = self.nodes.get(node_id).and_then(|n| n.parent);
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                if let Some(pos) = parent.children.iter().position(|&id| id == node_id) {
                    parent.children.remove(pos);
                }
            }
            if let Some(child) = self.nodes.get_mut(node_id) {
                child.parent = None;
            }
        }
    }

    pub fn remove_node(&mut self, id: usize) -> Result<(), &'static str> {
        if !self.nodes.contains(id) {
            return Err("Invalid child id");
        }
        self.detach_node(id);
        // We do NOT remove from slab here, as per 'remove' semantics (detach).
        // Destruction is separate or handled by dropping.
        Ok(())
    }

    pub fn replace_child(
        &mut self,
        parent_id: usize,
        new_child: usize,
        old_child: usize,
    ) -> Result<(), &'static str> {
        // TODO: Implement replace logic
        self.detach_node(old_child);
        self.append_child(parent_id, new_child) // Simplified: put at end? No, replace means put at same position.
                                                // Fixme: Implement insert_at or similar.
    }
}
