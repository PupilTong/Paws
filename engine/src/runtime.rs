use markup5ever::{LocalName, QualName};

use crate::dom::{Document, DomError};
use crate::style::StyleContext;

/// Error codes returned from host functions to WASM guests.
///
/// Uses `repr(i32)` for direct FFI compatibility with WASM's i32 return type.
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
    /// Returns the numeric error code for FFI.
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// Returns a human-readable description of this error code.
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

/// Detailed error information stored in [`RuntimeState::last_error`].
#[derive(Debug, Clone)]
pub struct HostError {
    pub code: i32,
    pub(crate) message: String,
}

/// Top-level state container for the WASM host runtime.
///
/// Owns the [`Document`], [`StyleContext`], and stylesheet cache.
/// All WASM-facing host functions operate through this struct.
pub struct RuntimeState {
    pub doc: Document,
    pub last_error: Option<HostError>,
    pub style_context: StyleContext,
    pub(crate) stylesheet_cache: crate::style::StylesheetCache,
}

impl RuntimeState {
    /// Creates a new runtime state with the given document URL.
    pub fn new(url_str: String) -> Self {
        let url = url::Url::parse(&url_str).expect("Valid Document URL");
        let context = StyleContext::new(url.clone());
        let lock = context.lock.clone();
        let doc = Document::new(lock.clone(), url);
        // Document and StyleContext share the same SharedRwLock (cloned from StyleContext)
        // to ensure consistent locking across style and DOM operations.

        let stylesheet_cache = crate::style::StylesheetCache::new(lock.clone());

        Self {
            doc,
            last_error: None,
            style_context: context,
            stylesheet_cache,
        }
    }
    /// Creates a new HTML element with the given tag name. Returns the node ID.
    pub fn create_element(&mut self, tag: String) -> u32 {
        let name = QualName::new(None, markup5ever::ns!(html), LocalName::from(tag));
        self.doc.create_element(name) as u32
    }

    /// Creates a new text node with the given content. Returns the node ID.
    pub fn create_text_node(&mut self, data: String) -> u32 {
        self.doc.create_text_node(data) as u32
    }

    /// Removes an element and all its descendants from the DOM tree.
    pub fn destroy_element(&mut self, id: u32) -> Result<(), HostErrorCode> {
        self.doc.remove_node(id as usize).map_err(|e| match e {
            DomError::InvalidParent => HostErrorCode::InvalidParent,
            DomError::InvalidChild => HostErrorCode::InvalidChild,
            DomError::CycleDetected => HostErrorCode::CycleDetected,
            DomError::ChildAlreadyHasParent => HostErrorCode::ChildAlreadyHasParent,
        })
    }

    /// Sets a single inline style property on an element.
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

        if node.is_element() {
            crate::style::update_inline_style(&self.style_context, node, &name, &value);

            // Mark ancestors dirty so lazy style resolution picks up the change.
            node.mark_ancestors_dirty();

            Ok(())
        } else {
            Err(HostErrorCode::InvalidChild)
        }
    }

    /// Parses and adds a CSS stylesheet to the document.
    pub fn add_stylesheet(&mut self, css: String) {
        // 1. Get or parse stylesheet from cache
        let sheet_arc = self.stylesheet_cache.get_or_parse(&css);
        let sheet = crate::style::CSSStyleSheet::new(sheet_arc);

        // 2. Add to Document (so it knows what sheets it has)
        self.doc.stylesheets.push(sheet);

        // 3. Add to StyleContext (so it applies to styling)
        // Note: We need to pass the *latest* sheet added.
        // In the future, we might rebuild the whole cascade from doc.stylesheets.
        // For now, we append to stylist.
        let added_sheet = self.doc.stylesheets.last().unwrap();
        self.style_context.add_stylesheet(added_sheet);
    }

    /// Adds a pre-parsed stylesheet from rkyv-encoded IR bytes.
    pub fn add_parsed_stylesheet(&mut self, bytes: &[u8]) {
        use paws_style_ir::StyleSheetIR;
        use rkyv::rancor::Error;

        let archived = match rkyv::from_bytes::<StyleSheetIR, Error>(bytes) {
            Ok(sheet) => sheet,
            Err(e) => {
                self.set_error(
                    HostErrorCode::MemoryError,
                    format!("rkyv decode error: {:?}", e),
                );
                return;
            }
        };

        // Note: Stylo `StylesheetContents` handles its own Arc/RwLock allocation internally
        // and its AST nodes (`StyleRule`, `CssRules`) do not expose simple constructors.
        // Bypassing Stylo's string parser entirely would require forking Stylo or unsafe transmutations.
        // For now, we reconstruct a minified valid CSS string from the validated IR, guaranteeing
        // 0-error runtime parsing and skipping format-lexing overhead, while hitting the StyleCache!
        let mut minified_css = String::new();
        for rule in archived.rules.iter() {
            minified_css.push_str(&rule.selectors);
            minified_css.push('{');
            for decl in rule.declarations.iter() {
                minified_css.push_str(&decl.name);
                minified_css.push(':');
                minified_css.push_str(&decl.value);
                minified_css.push(';');
            }
            minified_css.push('}');
        }

        self.add_stylesheet(minified_css);
    }

    /// Sets a DOM attribute on an element (e.g. `id`, `class`).
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
        if node.is_element() {
            node.set_attribute(&name, &value);
            Ok(())
        } else {
            Err(HostErrorCode::InvalidChild)
        }
    }

    /// Clears the last stored error.
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    /// Stores an error and returns its numeric code.
    pub fn set_error(&mut self, code: HostErrorCode, message: impl Into<String>) -> i32 {
        self.last_error = Some(HostError {
            code: code.as_i32(),
            message: message.into(),
        });
        code.as_i32()
    }

    /// Returns a computed style map handle for an element.
    ///
    /// The handle lazily triggers style resolution when its read methods
    /// are called. See [`StylePropertyMapReadOnly`] for the available API.
    pub fn computed_style_map(
        &self,
        id: u32,
    ) -> Result<crate::style::typed_om::StylePropertyMapReadOnly, HostErrorCode> {
        self.doc
            .computed_style_map(id as usize)
            .ok_or(HostErrorCode::InvalidChild)
    }

    /// Appends a child node to a parent node in the DOM tree.
    pub fn append_element(&mut self, parent: u32, child: u32) -> Result<(), HostErrorCode> {
        self.doc
            .append_child(parent as usize, child as usize)
            .map_err(|e| match e {
                DomError::InvalidParent => HostErrorCode::InvalidParent,
                DomError::InvalidChild => HostErrorCode::InvalidChild,
                DomError::CycleDetected => HostErrorCode::CycleDetected,
                DomError::ChildAlreadyHasParent => HostErrorCode::ChildAlreadyHasParent,
            })
    }

    /// Batch-appends multiple children to a parent with transactional pre-validation.
    pub fn append_elements(&mut self, parent: u32, children: &[u32]) -> Result<(), HostErrorCode> {
        // Pre-validate: check for duplicates
        let mut seen = fnv::FnvHashSet::default();
        for &child in children {
            if !seen.insert(child) {
                return Err(HostErrorCode::InvalidChild);
            }
        }

        // Pre-validate: check each child
        for &child in children {
            if self.doc.get_node(child as usize).is_none() {
                return Err(HostErrorCode::InvalidChild);
            }
            let old_parent = self.doc.get_node(child as usize).unwrap().parent;
            if old_parent.is_some() && old_parent != Some(parent as usize) {
                return Err(HostErrorCode::ChildAlreadyHasParent);
            }
            if parent == child {
                return Err(HostErrorCode::CycleDetected);
            }
        }
        for &child in children {
            self.append_element(parent, child)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_create_element() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        let node = state.doc.get_node(id as usize).unwrap();
        assert!(node.is_element());
        assert_eq!(node.name.as_ref().unwrap().local.as_ref(), "div");

        // Attach to document root so resolve_style traverses it
        state.append_element(0, id).unwrap();

        // Verify style application via Typed OM
        let map = state.computed_style_map(id).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .expect("computed color");

        // Default color is typically black/initial
        match color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => assert_eq!(s, "rgb(0, 0, 0)"),
            crate::style::typed_om::CSSStyleValue::Keyword(kw) => assert_eq!(kw.value, "initial"),
            _ => {} // Other types are possible depending on Stylo defaults
        }
    }

    #[test]
    fn test_create_text_node() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_text_node("hello".to_string());
        let node = state.doc.get_node(id as usize).unwrap();
        assert!(node.is_text_node());
        assert_eq!(node.text_content.as_deref().unwrap(), "hello");
    }

    #[test]
    fn test_destroy_element() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        assert!(state.destroy_element(id).is_ok());
        // Check if node is removed (simplified check, might still be allocated but detached/removed in real impl)
        // Document::remove_node removes from slab if we implemented it that way.
        // "if self.nodes.contains(id) { self.nodes.remove(id); }"
        // So yes.
        assert!(state.doc.get_node(id as usize).is_none());
        assert_eq!(state.destroy_element(id), Err(HostErrorCode::InvalidChild));
        assert_eq!(state.destroy_element(999), Err(HostErrorCode::InvalidChild));
    }

    #[test]
    fn test_set_inline_style_errors() {
        let mut state = RuntimeState::new("https://example.com".to_string());
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
        let mut state = RuntimeState::new("https://example.com".to_string());
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
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let _text = state.create_text_node("text".to_string());
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

        // 3. Child Already Has Parent
        let parent2 = state.create_element("section".to_string());
        assert_eq!(
            state.append_element(parent2, child),
            Err(HostErrorCode::ChildAlreadyHasParent)
        );
        let c_node = state.doc.get_node(child as usize).unwrap();
        assert_eq!(c_node.parent, Some(parent as usize));
    }

    #[test]
    fn test_recursive_in_document_flag() {
        use crate::dom::NodeFlags;

        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let grandchild = state.create_element("em".to_string());

        // Build subtree: parent -> child -> grandchild
        state.append_element(parent, child).unwrap();
        state.append_element(child, grandchild).unwrap();

        // Attach parent to document root (id 0)
        state.append_element(0, parent).unwrap();

        // All three should have IS_IN_DOCUMENT
        assert!(
            state
                .doc
                .get_node(parent as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "parent should be in document"
        );
        assert!(
            state
                .doc
                .get_node(child as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "child should be in document"
        );
        assert!(
            state
                .doc
                .get_node(grandchild as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "grandchild should be in document"
        );

        // Detach parent from root
        state.doc.detach_node(parent as usize);

        // All three should no longer have IS_IN_DOCUMENT
        assert!(
            !state
                .doc
                .get_node(parent as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "parent should not be in document after detach"
        );
        assert!(
            !state
                .doc
                .get_node(child as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "child should not be in document after detach"
        );
        assert!(
            !state
                .doc
                .get_node(grandchild as usize)
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "grandchild should not be in document after detach"
        );
    }

    #[test]
    fn test_remove_node_recursive_cleanup() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let grandchild = state.create_element("em".to_string());

        state.append_element(parent, child).unwrap();
        state.append_element(child, grandchild).unwrap();

        // Remove parent — child and grandchild should also be freed
        state.destroy_element(parent).unwrap();

        assert!(state.doc.get_node(parent as usize).is_none());
        assert!(state.doc.get_node(child as usize).is_none());
        assert!(state.doc.get_node(grandchild as usize).is_none());
    }

    #[test]
    fn test_style_inheritance() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());

        state.append_element(0, parent).unwrap();
        state.append_element(parent, child).unwrap();

        // Set color on parent
        state
            .set_inline_style(parent, "color".to_string(), "red".to_string())
            .unwrap();

        // Child should inherit color from parent
        let map = state.computed_style_map(child).unwrap();
        let child_color = map
            .get("color", &mut state.doc, &state.style_context)
            .expect("child should have computed color");

        match child_color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => assert_eq!(s, "rgb(255, 0, 0)"),
            _ => panic!("Expected unparsed rgb color, got {:?}", child_color),
        }
    }

    #[test]
    fn test_append_elements_rejects_duplicates() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());

        assert_eq!(
            state.append_elements(parent, &[child, child]),
            Err(HostErrorCode::InvalidChild)
        );
        // Parent should have no children since the operation was rejected
        let p = state.doc.get_node(parent as usize).unwrap();
        assert!(p.children.is_empty());
    }
}
