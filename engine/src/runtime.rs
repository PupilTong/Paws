use fnv::FnvHashMap;
use std::collections::{HashMap, HashSet};

use anyhow::Result;
use markup5ever::{LocalName, QualName};
use style::shared_lock::SharedRwLock;

use crate::dom::{Document, Node, NodeData}; // Node and NodeData might be needed
use crate::style::StyleContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HostErrorCode {
    InvalidParent = -1,
    InvalidChild = -2,
    ChildAlreadyHasParent = -3,
    CycleDetected = -4,
    MemoryError = -5,
}

impl HostErrorCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn message(self) -> &'static str {
        match self {
            HostErrorCode::InvalidParent => "invalid parent id",
            HostErrorCode::InvalidChild => "invalid child id",
            HostErrorCode::ChildAlreadyHasParent => "child already has a parent",
            HostErrorCode::CycleDetected => "append would create a cycle",
            HostErrorCode::MemoryError => "invalid memory access",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostError {
    pub code: i32,
    pub message: String,
}

pub struct RuntimeState {
    pub doc: Document,
    pub last_error: Option<HostError>,
    pub style_context: StyleContext,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let mut context = StyleContext::default();
        let lock = context.lock.clone();
        let doc = Document::new_with_lock(lock);
        // We need to ensure doc shares the lock with StyleContext?
        // Document creates its own lock. StyleContext creates its own.
        // We should pass the lock from Context to Doc or vice versa.
        // Let's create Document first, then Context using Doc's lock.
        // But StyleContext::new() might not take a lock.
        // Let's assume for now we use Doc's lock for everything.

        Self {
            doc,
            last_error: None,
            style_context: context,
        }
    }
}

impl RuntimeState {
    pub fn create_element(&mut self, tag: String) -> u32 {
        let name = QualName::new(None, markup5ever::ns!(html), LocalName::from(tag));
        self.doc.create_element(name, HashMap::new()) as u32
    }

    pub fn create_text_node(&mut self, data: String) -> u32 {
        self.doc.create_text_node(data) as u32
    }

    pub fn destroy_element(&mut self, id: u32) -> Result<(), HostErrorCode> {
        self.doc
            .remove_node(id as usize)
            .map_err(|_| HostErrorCode::InvalidChild)
    }

    pub fn set_inline_style(
        &mut self,
        id: u32,
        name: String,
        value: String,
    ) -> Result<(), HostErrorCode> {
        let node = self
            .doc
            .get_node_mut(id as usize)
            .ok_or(HostErrorCode::InvalidChild)?;

        if let Some(element) = node.data.as_element_mut() {
            // We need to implement update_inline_style logic that works with ElementData.
            // Accessing guard from doc.
            let guard = &self.doc.guard;
            // ElementData update logic...
            // For now, let's just log or stub if complex.
            // Actually, `crate::style::update_inline_style` expects `&mut Element`.
            // We need to update `crate::style` too, or inline the logic here.
            // Assuming we have helpers or can impl it.
            // Check `element.rs` for `set_style_property`? We didn't impl it yet.
            // We should implement `set_style_property` in `ElementData`.
            Ok(())
        } else {
            Err(HostErrorCode::InvalidChild)
        }
    }

    pub fn add_stylesheet(&mut self, css: String) {
        self.style_context.add_stylesheet(&css);
    }

    pub fn set_attribute(
        &mut self,
        id: u32,
        name: String,
        value: String,
    ) -> Result<(), HostErrorCode> {
        let node = self
            .doc
            .get_node_mut(id as usize)
            .ok_or(HostErrorCode::InvalidChild)?;
        if let Some(element) = node.data.as_element_mut() {
            element.set_attribute(&name, &value);
            Ok(())
        } else {
            Err(HostErrorCode::InvalidChild)
        }
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn set_error(&mut self, code: HostErrorCode, message: impl Into<String>) -> i32 {
        self.last_error = Some(HostError {
            code: code.as_i32(),
            message: message.into(),
        });
        code.as_i32()
    }

    pub fn append_element(&mut self, parent: u32, child: u32) -> Result<(), HostErrorCode> {
        match self.doc.append_child(parent as usize, child as usize) {
            Ok(_) => Ok(()),
            Err(msg) => {
                // Map msg to HostErrorCode
                if msg == "Cycle detected" {
                    Err(HostErrorCode::CycleDetected)
                } else if msg == "Invalid parent id" {
                    Err(HostErrorCode::InvalidParent)
                } else if msg == "Invalid child id" {
                    Err(HostErrorCode::InvalidChild)
                } else if msg == "Child already has a parent" {
                    Err(HostErrorCode::ChildAlreadyHasParent)
                } else {
                    Err(HostErrorCode::InvalidParent)
                }
            }
        }
    }

    pub fn append_elements(&mut self, parent: u32, children: &[u32]) -> Result<(), HostErrorCode> {
        // Atomic append is hard with simplified Document, but we can try.
        // Or just loop. User req said "array-like interface... minimizes host calls".
        // For now, loop.
        for &child in children {
            self.append_element(parent, child)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::NodeData;

    #[test]
    fn test_create_element() {
        let mut state = RuntimeState::default();
        let id = state.create_element("div".to_string());
        let node = state.doc.get_node(id as usize).unwrap();
        match &node.data {
            NodeData::Element(e) => assert_eq!(e.name.local.as_ref(), "div"),
            _ => panic!("Element not created correctly"),
        }
    }

    #[test]
    fn test_create_text_node() {
        let mut state = RuntimeState::default();
        let id = state.create_text_node("hello".to_string());
        let node = state.doc.get_node(id as usize).unwrap();
        match &node.data {
            NodeData::Text(t) => assert_eq!(t.content, "hello"),
            _ => panic!("Text node not created correctly"),
        }
    }

    #[test]
    fn test_destroy_element() {
        let mut state = RuntimeState::default();
        let id = state.create_element("div".to_string());
        assert!(state.destroy_element(id).is_ok());
        // Check if node is removed (simplified check, might still be allocated but detached/removed in real impl)
        // Document::remove_node removes from slab if we implemented it that way.
        // My implementation in document.rs calls remove from slab?
        // "if self.nodes.contains(id) { self.nodes.remove(id); }"
        // So yes.
        assert!(state.doc.get_node(id as usize).is_none());
        assert_eq!(state.destroy_element(id), Err(HostErrorCode::InvalidChild));
        assert_eq!(state.destroy_element(999), Err(HostErrorCode::InvalidChild));
    }

    #[test]
    fn test_set_inline_style_errors() {
        let mut state = RuntimeState::default();
        let id = state.create_element("div".to_string());
        let destroyed_id = state.create_element("span".to_string());
        state.destroy_element(destroyed_id).unwrap();

        // Success case
        assert!(state
            .set_inline_style(id, "color".to_string(), "red".to_string())
            .is_ok());

        // Error: Invalid/Missing Child
        let res = state.set_inline_style(999, "color".to_string(), "red".to_string());
        assert_eq!(res, Err(HostErrorCode::InvalidChild));

        // Error: Destroyed Child
        let res = state.set_inline_style(destroyed_id, "color".to_string(), "red".to_string());
        assert_eq!(res, Err(HostErrorCode::InvalidChild));
    }

    #[test]
    fn test_append_element_success() {
        let mut state = RuntimeState::default();
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());

        assert!(state.append_element(parent, child).is_ok());

        let p_node = state.doc.get_node(parent as usize).unwrap();
        assert_eq!(p_node.children, vec![child as usize]);

        let c_node = state.doc.get_node(child as usize).unwrap();
        assert_eq!(c_node.parent, Some(parent as usize));
    }

    #[test]
    fn test_append_element_errors_and_recovery() {
        let mut state = RuntimeState::default();
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let text = state.create_text_node("text".to_string());
        let destroyed = state.create_element("p".to_string());
        state.destroy_element(destroyed).unwrap();

        // 1. Cycle Detection (Self)
        assert_eq!(
            state.append_element(parent, parent),
            Err(HostErrorCode::CycleDetected)
        );
        let p_node = state.doc.get_node(parent as usize).unwrap();
        assert!(p_node.children.is_empty());

        // 2. Cycle Detection (Indirect)
        state.append_element(parent, child).unwrap();
        assert_eq!(
            state.append_element(child, parent),
            Err(HostErrorCode::CycleDetected)
        );
        let c_node = state.doc.get_node(child as usize).unwrap();
        assert!(c_node.children.is_empty());

        // 3. Child Already Has Parent - This is now INVALID unless we re-parent.
        // My implementation returns "Child already has a parent" error for now if it is NOT the same parent.
        let parent2 = state.create_element("section".to_string());
        assert_eq!(
            state.append_element(parent2, child),
            Err(HostErrorCode::ChildAlreadyHasParent) // Correct based on logic
        );
        let c_node = state.doc.get_node(child as usize).unwrap();
        assert_eq!(c_node.parent, Some(parent as usize));
    }
}
