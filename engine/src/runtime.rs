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
#[allow(dead_code)]
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
        use paws_style_ir::ArchivedStyleSheetIR;
        use rkyv::rancor::Error;

        let archived = match rkyv::access::<ArchivedStyleSheetIR, Error>(bytes) {
            Ok(sheet) => sheet,
            Err(e) => {
                self.set_error(
                    HostErrorCode::MemoryError,
                    format!("rkyv decode error: {:?}", e),
                );
                return;
            }
        };

        use ::style::parser::ParserContext;
        use ::style::servo_arc::Arc;
        use ::style::stylesheets::{CssRules, Origin, StylesheetContents};
        use ::stylo_traits::ParsingMode;

        let lock = self.style_context.lock.clone();
        let url_data = self.style_context.url_data.clone();
        let quirks_mode = ::style::context::QuirksMode::NoQuirks;

        let context = ParserContext::new(
            Origin::Author,
            &url_data,
            Some(::style::stylesheets::CssRuleType::Style),
            ParsingMode::DEFAULT,
            quirks_mode,
            Default::default(),
            None,
            None,
        );

        let stylo_rules = crate::style::ir_convert::construct_stylo_rules(
            &archived.rules,
            &lock,
            &url_data,
            &context,
        );

        let rules_lock = lock.wrap(CssRules(stylo_rules));
        let css_rules = unsafe {
            ::style::servo_arc::Arc::new_static(|layout| std::alloc::alloc(layout), rules_lock)
        };
        let contents = StylesheetContents::from_shared_data(
            css_rules,
            Origin::Author,
            url_data.clone(),
            quirks_mode,
        );
        let stylesheet = Arc::new(::style::stylesheets::Stylesheet {
            contents: lock.wrap(contents),
            shared_lock: lock.clone(),
            media: Arc::new(lock.wrap(::style::media_queries::MediaList::empty())),
            disabled: std::sync::atomic::AtomicBool::new(false),
        });

        let sheet = crate::style::CSSStyleSheet::new(stylesheet);
        self.doc.stylesheets.push(sheet);
        let added_sheet = self.doc.stylesheets.last().unwrap();
        self.style_context.add_stylesheet(added_sheet);
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

    // ─── IR → Stylo pipeline integration tests ─────────────────────

    /// Helper: create a div in the document, add a pre-compiled stylesheet,
    /// resolve styles, and return the computed value of `prop`.
    fn ir_pipeline_get(css_bytes: &[u8], prop: &str) -> crate::style::typed_om::CSSStyleValue {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state.add_parsed_stylesheet(css_bytes);
        let map = state.computed_style_map(div).unwrap();
        map.get(prop, &mut state.doc, &state.style_context)
            .unwrap_or_else(|| panic!("no computed value for '{prop}'"))
    }

    /// Helper: check a computed value contains the expected keyword.
    /// Accepts both Keyword(...) and Unparsed("...") since Stylo may
    /// serialize some properties as strings rather than typed values.
    fn assert_keyword(val: &crate::style::typed_om::CSSStyleValue, expected: &str) {
        match val {
            crate::style::typed_om::CSSStyleValue::Keyword(kw) => {
                assert_eq!(kw.value, expected, "Expected keyword '{expected}'");
            }
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, expected, "Expected unparsed keyword '{expected}'");
            }
            other => panic!("Expected keyword '{expected}', got: {other:?}"),
        }
    }

    /// Helper: check a computed value contains the expected numeric value.
    /// Accepts both Unit(...) and Unparsed("...") since Stylo may
    /// serialize some properties as strings.
    fn assert_computed_contains(
        val: &crate::style::typed_om::CSSStyleValue,
        expected_substring: &str,
    ) {
        match val {
            crate::style::typed_om::CSSStyleValue::Unit(u) => {
                let serialized = format!("{}{}", u.value, u.unit);
                assert!(
                    serialized.contains(expected_substring),
                    "Expected '{expected_substring}' in '{serialized}'"
                );
            }
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert!(
                    s.contains(expected_substring),
                    "Expected '{expected_substring}' in '{s}'"
                );
            }
            crate::style::typed_om::CSSStyleValue::Keyword(kw) => {
                assert!(
                    kw.value.contains(expected_substring),
                    "Expected '{expected_substring}' in '{}'",
                    kw.value
                );
            }
            other => panic!("Expected value containing '{expected_substring}', got: {other:?}"),
        }
    }

    // ── Display ──────────────────────────────────────────────────

    #[test]
    fn test_ir_display_block() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: block; }"#), "display");
        assert_keyword(&val, "block");
    }

    #[test]
    fn test_ir_display_flex() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: flex; }"#), "display");
        assert_keyword(&val, "flex");
    }

    #[test]
    fn test_ir_display_grid() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: grid; }"#), "display");
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_ir_display_none() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: none; }"#), "display");
        assert_keyword(&val, "none");
    }

    #[test]
    fn test_ir_display_inline_block() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: inline-block; }"#),
            "display",
        );
        assert_keyword(&val, "inline-block");
    }

    #[test]
    fn test_ir_display_inline_flex() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: inline-flex; }"#),
            "display",
        );
        assert_keyword(&val, "inline-flex");
    }

    #[test]
    fn test_ir_display_table() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: table; }"#), "display");
        assert_keyword(&val, "table");
    }

    #[test]
    fn test_ir_display_contents() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: contents; }"#),
            "display",
        );
        assert_keyword(&val, "contents");
    }

    // ── Position ─────────────────────────────────────────────────

    #[test]
    fn test_ir_position_relative() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: relative; }"#),
            "position",
        );
        assert_keyword(&val, "relative");
    }

    #[test]
    fn test_ir_position_absolute() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: absolute; }"#),
            "position",
        );
        assert_keyword(&val, "absolute");
    }

    #[test]
    fn test_ir_position_fixed() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { position: fixed; }"#), "position");
        assert_keyword(&val, "fixed");
    }

    #[test]
    fn test_ir_position_sticky() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: sticky; }"#),
            "position",
        );
        assert_keyword(&val, "sticky");
    }

    // ── Box-sizing ───────────────────────────────────────────────

    #[test]
    fn test_ir_box_sizing_border_box() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { box-sizing: border-box; }"#),
            "box-sizing",
        );
        assert_keyword(&val, "border-box");
    }

    #[test]
    fn test_ir_box_sizing_content_box() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { box-sizing: content-box; }"#),
            "box-sizing",
        );
        assert_keyword(&val, "content-box");
    }

    // ── Visibility ───────────────────────────────────────────────

    #[test]
    fn test_ir_visibility_hidden() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { visibility: hidden; }"#),
            "visibility",
        );
        assert_keyword(&val, "hidden");
    }

    #[test]
    fn test_ir_visibility_collapse() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { visibility: collapse; }"#),
            "visibility",
        );
        assert_keyword(&val, "collapse");
    }

    // ── Overflow ─────────────────────────────────────────────────

    #[test]
    fn test_ir_overflow_hidden() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { overflow-x: hidden; }"#),
            "overflow-x",
        );
        assert_keyword(&val, "hidden");
    }

    #[test]
    fn test_ir_overflow_scroll() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { overflow-y: scroll; }"#),
            "overflow-y",
        );
        assert_keyword(&val, "scroll");
    }

    #[test]
    fn test_ir_overflow_auto() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { overflow-x: auto; }"#),
            "overflow-x",
        );
        assert_keyword(&val, "auto");
    }

    // ── Float & Clear ────────────────────────────────────────────

    #[test]
    fn test_ir_float_left() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { float: left; }"#), "float");
        assert_keyword(&val, "left");
    }

    #[test]
    fn test_ir_float_right() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { float: right; }"#), "float");
        assert_keyword(&val, "right");
    }

    #[test]
    fn test_ir_clear_both() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { clear: both; }"#), "clear");
        assert_keyword(&val, "both");
    }

    // ── Object-fit ───────────────────────────────────────────────

    #[test]
    fn test_ir_object_fit_cover() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { object-fit: cover; }"#),
            "object-fit",
        );
        assert_keyword(&val, "cover");
    }

    #[test]
    fn test_ir_object_fit_contain() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { object-fit: contain; }"#),
            "object-fit",
        );
        assert_keyword(&val, "contain");
    }

    // ── Sizing ───────────────────────────────────────────────────

    #[test]
    fn test_ir_width_px() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { width: 200px; }"#), "width");
        assert_computed_contains(&val, "200");
    }

    #[test]
    fn test_ir_height_percent() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { height: 50%; }"#), "height");
        // Stylo may compute percentages or leave as %; just check value present
        assert_computed_contains(&val, "50");
    }

    #[test]
    fn test_ir_width_auto() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { width: auto; }"#), "width");
        assert_keyword(&val, "auto");
    }

    #[test]
    fn test_ir_max_width_none() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { max-width: none; }"#),
            "max-width",
        );
        assert_keyword(&val, "none");
    }

    #[test]
    fn test_ir_min_height_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { min-height: 100px; }"#),
            "min-height",
        );
        assert_computed_contains(&val, "100");
    }

    // ── Margin ───────────────────────────────────────────────────

    #[test]
    fn test_ir_margin_top_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { margin-top: 10px; }"#),
            "margin-top",
        );
        assert_computed_contains(&val, "10");
    }

    #[test]
    fn test_ir_margin_left_auto() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { margin-left: auto; }"#),
            "margin-left",
        );
        assert_keyword(&val, "auto");
    }

    #[test]
    fn test_ir_margin_bottom_percent() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { margin-bottom: 25%; }"#),
            "margin-bottom",
        );
        assert_computed_contains(&val, "25");
    }

    // ── Padding ──────────────────────────────────────────────────

    #[test]
    fn test_ir_padding_top_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-top: 15px; }"#),
            "padding-top",
        );
        assert_computed_contains(&val, "15");
    }

    #[test]
    fn test_ir_padding_right_em() {
        // em is computed to px by Stylo; 2em = 2*16 = 32px (default font)
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-right: 2em; }"#),
            "padding-right",
        );
        assert_computed_contains(&val, "32");
    }

    // ── Inset (top/right/bottom/left) ────────────────────────────

    #[test]
    fn test_ir_top_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: absolute; top: 10px; }"#),
            "top",
        );
        assert_computed_contains(&val, "10");
    }

    #[test]
    fn test_ir_left_auto() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { left: auto; }"#), "left");
        assert_keyword(&val, "auto");
    }

    // ── Flexbox ──────────────────────────────────────────────────

    #[test]
    fn test_ir_flex_direction_column() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-direction: column; }"#),
            "flex-direction",
        );
        assert_keyword(&val, "column");
    }

    #[test]
    fn test_ir_flex_direction_row_reverse() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-direction: row-reverse; }"#),
            "flex-direction",
        );
        assert_keyword(&val, "row-reverse");
    }

    #[test]
    fn test_ir_flex_wrap() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-wrap: wrap; }"#),
            "flex-wrap",
        );
        assert_keyword(&val, "wrap");
    }

    #[test]
    fn test_ir_flex_grow() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { flex-grow: 2; }"#), "flex-grow");
        assert_computed_contains(&val, "2");
    }

    #[test]
    fn test_ir_flex_shrink() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-shrink: 0; }"#),
            "flex-shrink",
        );
        assert_computed_contains(&val, "0");
    }

    #[test]
    fn test_ir_flex_basis_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-basis: 200px; }"#),
            "flex-basis",
        );
        assert_computed_contains(&val, "200");
    }

    #[test]
    fn test_ir_flex_basis_auto() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-basis: auto; }"#),
            "flex-basis",
        );
        assert_keyword(&val, "auto");
    }

    #[test]
    fn test_ir_order() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { order: 3; }"#), "order");
        assert_computed_contains(&val, "3");
    }

    // ── Z-index ──────────────────────────────────────────────────

    #[test]
    fn test_ir_z_index_number() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { z-index: 10; }"#), "z-index");
        assert_computed_contains(&val, "10");
    }

    #[test]
    fn test_ir_z_index_auto() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { z-index: auto; }"#), "z-index");
        assert_keyword(&val, "auto");
    }

    // ── Border style ─────────────────────────────────────────────

    #[test]
    fn test_ir_border_top_style_solid() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-top-style: solid; }"#),
            "border-top-style",
        );
        assert_keyword(&val, "solid");
    }

    #[test]
    fn test_ir_border_bottom_style_dashed() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-bottom-style: dashed; }"#),
            "border-bottom-style",
        );
        assert_keyword(&val, "dashed");
    }

    #[test]
    fn test_ir_border_left_style_dotted() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-left-style: dotted; }"#),
            "border-left-style",
        );
        assert_keyword(&val, "dotted");
    }

    // ── Gap ──────────────────────────────────────────────────────

    #[test]
    fn test_ir_column_gap_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: flex; column-gap: 20px; }"#),
            "column-gap",
        );
        assert_computed_contains(&val, "20");
    }

    #[test]
    fn test_ir_row_gap_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: flex; row-gap: 10px; }"#),
            "row-gap",
        );
        assert_computed_contains(&val, "10");
    }

    // ── Viewport / relative units (computed to px by Stylo) ──────

    #[test]
    fn test_ir_width_vh() {
        // viewport units are computed to px; without a real viewport Stylo
        // resolves them to 0px, but the pipeline still exercises the code path
        let val = ir_pipeline_get(view_macros::css!(r#"div { width: 100vh; }"#), "width");
        assert_computed_contains(&val, "px");
    }

    #[test]
    fn test_ir_height_vw() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { height: 50vw; }"#), "height");
        assert_computed_contains(&val, "px");
    }

    #[test]
    fn test_ir_padding_rem() {
        // 1.5rem = 1.5 * 16 = 24px (default font size)
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-left: 1.5rem; }"#),
            "padding-left",
        );
        assert_computed_contains(&val, "24");
    }

    // ── Combined real-world stylesheet ───────────────────────────

    #[test]
    fn test_ir_realistic_flexbox_layout() {
        let css_bytes = view_macros::css!(
            r#"
            div {
                display: flex;
                flex-direction: column;
                flex-wrap: nowrap;
                width: 300px;
                height: 200px;
                padding-top: 10px;
                padding-right: 20px;
                padding-bottom: 10px;
                padding-left: 20px;
                margin-top: 0;
                margin-bottom: 16px;
                box-sizing: border-box;
                overflow-x: hidden;
                overflow-y: auto;
                position: relative;
                z-index: 1;
            }
        "#
        );
        let mut state = RuntimeState::new("https://example.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state.add_parsed_stylesheet(css_bytes);
        let map = state.computed_style_map(div).unwrap();

        let mut get = |prop: &str| {
            map.get(prop, &mut state.doc, &state.style_context)
                .unwrap_or_else(|| panic!("no computed value for '{prop}'"))
        };

        assert_keyword(&get("display"), "flex");
        assert_keyword(&get("flex-direction"), "column");
        assert_keyword(&get("flex-wrap"), "nowrap");
        assert_computed_contains(&get("width"), "300");
        assert_computed_contains(&get("height"), "200");
        assert_computed_contains(&get("padding-top"), "10");
        assert_computed_contains(&get("padding-right"), "20");
        assert_keyword(&get("box-sizing"), "border-box");
        assert_keyword(&get("overflow-x"), "hidden");
        assert_keyword(&get("overflow-y"), "auto");
        assert_keyword(&get("position"), "relative");
        assert_computed_contains(&get("z-index"), "1");
    }

    // ── Border width (exercises Raw fallback in ir_convert) ─────

    #[test]
    fn test_ir_border_top_width_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-top-width: 2px; border-top-style: solid; }"#),
            "border-top-width",
        );
        // Border width is computed to px; verify a numeric px result
        assert_computed_contains(&val, "px");
    }

    #[test]
    fn test_ir_border_bottom_width_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-bottom-width: 5px; border-bottom-style: solid; }"#),
            "border-bottom-width",
        );
        assert_computed_contains(&val, "px");
    }

    // ── Additional display variants ─────────────────────────────

    #[test]
    fn test_ir_display_inline() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { display: inline; }"#), "display");
        assert_keyword(&val, "inline");
    }

    #[test]
    fn test_ir_display_inline_grid() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: inline-grid; }"#),
            "display",
        );
        assert_keyword(&val, "inline-grid");
    }

    #[test]
    fn test_ir_display_table_row() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: table-row; }"#),
            "display",
        );
        assert_keyword(&val, "table-row");
    }

    #[test]
    fn test_ir_display_table_cell() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: table-cell; }"#),
            "display",
        );
        assert_keyword(&val, "table-cell");
    }

    // ── Additional position variants ────────────────────────────

    #[test]
    fn test_ir_position_static() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: static; }"#),
            "position",
        );
        assert_keyword(&val, "static");
    }

    // ── Float none ──────────────────────────────────────────────

    #[test]
    fn test_ir_float_none() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { float: none; }"#), "float");
        assert_keyword(&val, "none");
    }

    // ── Clear variants ──────────────────────────────────────────

    #[test]
    fn test_ir_clear_left() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { clear: left; }"#), "clear");
        assert_keyword(&val, "left");
    }

    #[test]
    fn test_ir_clear_right() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { clear: right; }"#), "clear");
        assert_keyword(&val, "right");
    }

    #[test]
    fn test_ir_clear_none() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { clear: none; }"#), "clear");
        assert_keyword(&val, "none");
    }

    // ── Visibility visible ──────────────────────────────────────

    #[test]
    fn test_ir_visibility_visible() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { visibility: visible; }"#),
            "visibility",
        );
        assert_keyword(&val, "visible");
    }

    // ── Overflow variants ───────────────────────────────────────

    #[test]
    fn test_ir_overflow_visible() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { overflow-x: visible; }"#),
            "overflow-x",
        );
        assert_keyword(&val, "visible");
    }

    #[test]
    fn test_ir_overflow_clip() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { overflow-y: clip; }"#),
            "overflow-y",
        );
        assert_keyword(&val, "clip");
    }

    // ── Object-fit variants ─────────────────────────────────────

    #[test]
    fn test_ir_object_fit_fill() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { object-fit: fill; }"#),
            "object-fit",
        );
        assert_keyword(&val, "fill");
    }

    #[test]
    fn test_ir_object_fit_none() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { object-fit: none; }"#),
            "object-fit",
        );
        assert_keyword(&val, "none");
    }

    #[test]
    fn test_ir_object_fit_scale_down() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { object-fit: scale-down; }"#),
            "object-fit",
        );
        assert_keyword(&val, "scale-down");
    }

    // ── Flex direction variants ─────────────────────────────────

    #[test]
    fn test_ir_flex_direction_row() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-direction: row; }"#),
            "flex-direction",
        );
        assert_keyword(&val, "row");
    }

    #[test]
    fn test_ir_flex_direction_column_reverse() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-direction: column-reverse; }"#),
            "flex-direction",
        );
        assert_keyword(&val, "column-reverse");
    }

    // ── Flex wrap variants ──────────────────────────────────────

    #[test]
    fn test_ir_flex_wrap_nowrap() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-wrap: nowrap; }"#),
            "flex-wrap",
        );
        assert_keyword(&val, "nowrap");
    }

    #[test]
    fn test_ir_flex_wrap_wrap_reverse() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { flex-wrap: wrap-reverse; }"#),
            "flex-wrap",
        );
        assert_keyword(&val, "wrap-reverse");
    }

    // ── Border style variants ───────────────────────────────────

    #[test]
    fn test_ir_border_style_none() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-top-style: none; }"#),
            "border-top-style",
        );
        assert_keyword(&val, "none");
    }

    #[test]
    fn test_ir_border_style_double() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-right-style: double; }"#),
            "border-right-style",
        );
        assert_keyword(&val, "double");
    }

    // ── Gap normal ──────────────────────────────────────────────

    #[test]
    fn test_ir_row_gap_normal() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: flex; row-gap: normal; }"#),
            "row-gap",
        );
        assert_keyword(&val, "normal");
    }

    // ── Sizing with max-height ──────────────────────────────────

    #[test]
    fn test_ir_max_height_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { max-height: 500px; }"#),
            "max-height",
        );
        assert_computed_contains(&val, "500");
    }

    // ── Margin negative ─────────────────────────────────────────

    #[test]
    fn test_ir_margin_right_negative() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { margin-right: -8px; }"#),
            "margin-right",
        );
        assert_computed_contains(&val, "-8");
    }

    // ── Inset bottom/right ──────────────────────────────────────

    #[test]
    fn test_ir_bottom_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: absolute; bottom: 5px; }"#),
            "bottom",
        );
        assert_computed_contains(&val, "5");
    }

    #[test]
    fn test_ir_right_percent() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { position: absolute; right: 10%; }"#),
            "right",
        );
        assert_computed_contains(&val, "10");
    }

    // ── Multi-rule stylesheet ───────────────────────────────────

    #[test]
    fn test_ir_multi_rule_specificity() {
        // Later rule with same specificity should win
        let css_bytes = view_macros::css!(
            r#"
            div { display: block; }
            div { display: flex; }
        "#
        );
        let val = ir_pipeline_get(css_bytes, "display");
        assert_keyword(&val, "flex");
    }

    // ── Min-width with percentage ───────────────────────────────

    #[test]
    fn test_ir_min_width_percent() {
        let val = ir_pipeline_get(view_macros::css!(r#"div { min-width: 50%; }"#), "min-width");
        assert_computed_contains(&val, "50");
    }

    // ── Padding bottom/left ─────────────────────────────────────

    #[test]
    fn test_ir_padding_bottom_px() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-bottom: 8px; }"#),
            "padding-bottom",
        );
        assert_computed_contains(&val, "8");
    }

    #[test]
    fn test_ir_padding_left_percent() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-left: 5%; }"#),
            "padding-left",
        );
        assert_computed_contains(&val, "5");
    }
}
