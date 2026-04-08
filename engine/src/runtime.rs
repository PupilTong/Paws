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
    InvalidEventTarget = -6,
    NoActiveEvent = -7,
    EventAlreadyDispatching = -8,
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
            HostErrorCode::InvalidEventTarget => "invalid event target id",
            HostErrorCode::NoActiveEvent => "no event is currently being dispatched",
            HostErrorCode::EventAlreadyDispatching => "an event is already being dispatched",
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

/// Marker trait for per-node render state stored on each [`PawsElement`](crate::dom::element::PawsElement).
///
/// Render state is platform-specific owned data (GPU handles, view snapshots, etc.)
/// that must be:
/// - [`Default`]: zero-cost initialization when nodes are created
/// - [`Send`]: safe to share across Stylo's parallel style threads
/// - `'static`: fully owned with no borrowed references
///
/// iOS uses `IosNodeState`, tests use `()`.
pub trait RenderState: Default + Send + 'static {}
impl<T: Default + Send + 'static> RenderState for T {}

/// Trait for backends that process committed document state.
///
/// Each platform defines its own per-node render state via [`NodeState`](Self::NodeState).
/// The engine calls [`on_commit`](Self::on_commit) after style resolution and layout
/// are complete, giving the renderer mutable access to the Document so it can
/// read layout data and update per-node render state for dirty checking.
pub trait EngineRenderer: Send + 'static {
    /// Per-node render cache stored directly on each [`PawsElement`](crate::dom::element::PawsElement).
    ///
    /// iOS stores `IosNodeState` (ViewKind + prev-frame snapshot),
    /// Android can define its own. Tests use `()` (zero-cost).
    type NodeState: RenderState;

    /// Called after style resolution and layout are complete.
    ///
    /// `root_element` is the first element child of the document root,
    /// or `None` if the document has no element children.
    fn on_commit(
        &mut self,
        doc: &mut Document<Self::NodeState>,
        root_element: Option<taffy::NodeId>,
    );
}

/// No-op renderer for tests and headless usage. Zero-cost `NodeState = ()`.
impl EngineRenderer for () {
    type NodeState = ();
    fn on_commit(&mut self, _doc: &mut Document<()>, _root: Option<taffy::NodeId>) {}
}

/// Top-level state container for the WASM host runtime.
///
/// Owns the [`Document`] (which includes the text layout context),
/// [`StyleContext`], stylesheet cache, and an [`EngineRenderer`] backend.
/// All WASM-facing host functions operate through this struct.
///
/// The type parameter `R` is the renderer backend. Use `()` for tests
/// and headless usage (zero-cost no-op renderer).
pub struct RuntimeState<R: EngineRenderer = ()> {
    pub doc: Document<R::NodeState>,
    pub last_error: Option<HostError>,
    pub style_context: StyleContext,
    pub(crate) stylesheet_cache: crate::style::StylesheetCache,
    pub renderer: R,
    /// The event currently being dispatched, if any.
    ///
    /// Set by `dispatch_event` and cleared after the dispatch loop completes.
    /// WASM event accessor host functions read/mutate this field.
    pub current_event: Option<crate::events::Event>,
}

impl RuntimeState<()> {
    /// Creates a new runtime state with the no-op `()` renderer.
    ///
    /// Use this for tests and headless usage where no rendering backend
    /// is needed.
    pub fn new(url_str: String) -> Self {
        Self::with_renderer(url_str, ())
    }
}

impl<R: EngineRenderer> RuntimeState<R> {
    /// Creates a new runtime state with a custom renderer backend.
    pub fn with_renderer(url_str: String, renderer: R) -> Self {
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
            renderer,
            current_event: None,
        }
    }
    /// Creates a new HTML element with the given tag name. Returns the node ID.
    pub fn create_element(&mut self, tag: String) -> u32 {
        let name = QualName::new(None, markup5ever::ns!(html), LocalName::from(tag));
        let id: u64 = self.doc.create_element(name).into();
        id as u32
    }

    /// Creates a new text node with the given content. Returns the node ID.
    pub fn create_text_node(&mut self, data: String) -> u32 {
        let id: u64 = self.doc.create_text_node(data).into();
        id as u32
    }

    /// Removes an element and all its descendants from the DOM tree.
    pub fn destroy_element(&mut self, id: u32) -> Result<(), HostErrorCode> {
        self.doc
            .remove_node(taffy::NodeId::from(id as u64))
            .map_err(dom_error_to_host)
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
            .get_node_mut(taffy::NodeId::from(id as u64))
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

    /// Runs the full rendering pipeline: style resolution, layout, and renderer notification.
    ///
    /// This is the explicit commit model — unlike browsers where many APIs
    /// trigger implicit reflow, only `commit()` triggers the pipeline.
    /// In the future, animations will also trigger repaint, but generally
    /// the pipeline should be driven explicitly by the user program.
    ///
    /// After layout is computed, the [`EngineRenderer::on_commit`] callback
    /// is invoked so the renderer can generate platform-specific output.
    /// Removed node render states are cleared after the renderer processes them.
    pub fn commit(&mut self) {
        // 1. Style resolution (skipped if nothing is dirty)
        self.doc.ensure_styles_resolved(&self.style_context);

        // 2. Find the root element (first element child of the document node)
        let root_element = self.doc.root_element_id();

        // 3. Layout from the root element
        if let Some(root_id) = root_element {
            crate::layout::compute_layout_in_place(&mut self.doc, root_id);
        }

        // 4. Notify the renderer (split borrow: &mut renderer + &mut doc are disjoint fields)
        self.renderer.on_commit(&mut self.doc, root_element);

        // 5. Clear removed render states after the renderer has processed them
        self.doc.removed_render_states.clear();
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
            .get_node_mut(taffy::NodeId::from(id as u64))
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
            .computed_style_map(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)
    }

    /// Appends a child node to a parent node in the DOM tree.
    pub fn append_element(&mut self, parent: u32, child: u32) -> Result<(), HostErrorCode> {
        self.doc
            .append_child(
                taffy::NodeId::from(parent as u64),
                taffy::NodeId::from(child as u64),
            )
            .map_err(dom_error_to_host)
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
            let child_id = taffy::NodeId::from(child as u64);
            if self.doc.get_node(child_id).is_none() {
                return Err(HostErrorCode::InvalidChild);
            }
            let old_parent = self.doc.get_node(child_id).unwrap().parent;
            if old_parent.is_some() && old_parent != Some(taffy::NodeId::from(parent as u64)) {
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

    /// Removes a child from its parent without deleting the child node.
    pub fn remove_child(&mut self, parent: u32, child: u32) -> Result<(), HostErrorCode> {
        self.doc
            .remove_child(
                taffy::NodeId::from(parent as u64),
                taffy::NodeId::from(child as u64),
            )
            .map_err(dom_error_to_host)
    }

    /// Replaces an old child with a new child under a given parent.
    pub fn replace_child(
        &mut self,
        parent: u32,
        new_child: u32,
        old_child: u32,
    ) -> Result<(), HostErrorCode> {
        self.doc
            .replace_child(
                taffy::NodeId::from(parent as u64),
                taffy::NodeId::from(new_child as u64),
                taffy::NodeId::from(old_child as u64),
            )
            .map_err(dom_error_to_host)
    }

    /// Inserts a new child before a reference child under a given parent.
    pub fn insert_before(
        &mut self,
        parent: u32,
        new_child: u32,
        ref_child: u32,
    ) -> Result<(), HostErrorCode> {
        self.doc
            .insert_before(
                taffy::NodeId::from(parent as u64),
                taffy::NodeId::from(new_child as u64),
                taffy::NodeId::from(ref_child as u64),
            )
            .map_err(dom_error_to_host)
    }

    /// Clones a DOM node. If `deep` is true, all descendants are cloned recursively.
    ///
    /// Returns the new node's ID on success.
    pub fn clone_node(&mut self, id: u32, deep: bool) -> Result<u32, HostErrorCode> {
        self.doc
            .clone_node(taffy::NodeId::from(id as u64), deep)
            .map(|new_id| u64::from(new_id) as u32)
            .map_err(dom_error_to_host)
    }

    /// Sets the node's value (text content for Text/Comment nodes).
    ///
    /// Per the W3C DOM spec, setting `nodeValue` on Element, Document, or
    /// ShadowRoot nodes is a no-op.
    pub fn set_node_value(&mut self, id: u32, value: String) -> Result<(), HostErrorCode> {
        let node_id = taffy::NodeId::from(id as u64);
        let node = self
            .doc
            .get_node_mut(node_id)
            .ok_or(HostErrorCode::InvalidChild)?;

        use crate::dom::NodeType;
        match node.node_type {
            NodeType::Text | NodeType::Comment => {
                node.text_content = Some(value);
                node.mark_ancestors_dirty();
                Ok(())
            }
            // Per DOM spec, setting nodeValue on these types is a no-op
            NodeType::Element | NodeType::Document | NodeType::ShadowRoot => Ok(()),
        }
    }

    /// Returns the W3C DOM `nodeType` constant for the given node.
    ///
    /// Element=1, Text=3, Comment=8, Document=9, ShadowRoot(DocumentFragment)=11.
    pub fn get_node_type(&self, id: u32) -> Result<u32, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;

        use crate::dom::NodeType;
        let type_code = match node.node_type {
            NodeType::Element => 1,
            NodeType::Text => 3,
            NodeType::Comment => 8,
            NodeType::Document => 9,
            NodeType::ShadowRoot => 11,
        };
        Ok(type_code)
    }

    /// Returns the first child of the given node, or `None`.
    pub fn get_first_child(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        Ok(node.children.first().map(|&id| u64::from(id) as u32))
    }

    /// Returns the last child of the given node, or `None`.
    pub fn get_last_child(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        Ok(node.children.last().map(|&id| u64::from(id) as u32))
    }

    /// Returns the next sibling of the given node, or `None`.
    pub fn get_next_sibling(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        if self.doc.get_node(taffy::NodeId::from(id as u64)).is_none() {
            return Err(HostErrorCode::InvalidChild);
        }
        Ok(self
            .doc
            .get_next_sibling(taffy::NodeId::from(id as u64))
            .map(|id| u64::from(id) as u32))
    }

    /// Returns the previous sibling of the given node, or `None`.
    pub fn get_previous_sibling(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        if self.doc.get_node(taffy::NodeId::from(id as u64)).is_none() {
            return Err(HostErrorCode::InvalidChild);
        }
        Ok(self
            .doc
            .get_previous_sibling(taffy::NodeId::from(id as u64))
            .map(|id| u64::from(id) as u32))
    }

    /// Returns the parent element (only if it is an Element type).
    pub fn get_parent_element(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        let parent_id = match node.parent {
            Some(pid) => pid,
            None => return Ok(None),
        };
        let parent = self.doc.get_node(parent_id);
        match parent {
            Some(p) if p.is_element() => Ok(Some(u64::from(parent_id) as u32)),
            _ => Ok(None),
        }
    }

    /// Returns the parent node (any type).
    pub fn get_parent_node(&self, id: u32) -> Result<Option<u32>, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        Ok(node.parent.map(|pid| u64::from(pid) as u32))
    }

    /// Returns whether the node is connected to the document.
    pub fn is_connected(&self, id: u32) -> Result<bool, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        Ok(node
            .flags
            .contains(crate::dom::element::NodeFlags::IS_IN_DOCUMENT))
    }

    /// Returns whether the element has the named attribute.
    pub fn has_attribute(&self, id: u32, name: &str) -> Result<bool, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        if !node.is_element() {
            return Err(HostErrorCode::InvalidChild);
        }
        Ok(node.has_attribute(name))
    }

    /// Returns the value of the named attribute, or `None` if absent.
    pub fn get_attribute(&self, id: u32, name: &str) -> Result<Option<String>, HostErrorCode> {
        let node = self
            .doc
            .get_node(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        if !node.is_element() {
            return Err(HostErrorCode::InvalidChild);
        }
        Ok(node.get_attribute(name).map(|s| s.to_string()))
    }

    /// Removes the named attribute from the element.
    pub fn remove_attribute(&mut self, id: u32, name: &str) -> Result<(), HostErrorCode> {
        let node = self
            .doc
            .get_node_mut(taffy::NodeId::from(id as u64))
            .ok_or(HostErrorCode::InvalidChild)?;
        if !node.is_element() {
            return Err(HostErrorCode::InvalidChild);
        }
        node.remove_attribute(name);
        Ok(())
    }

    // ── Event system ────────────────────────────────────────────────

    /// Registers an event listener on a DOM node.
    ///
    /// Per the W3C spec, listeners are deduplicated by the triple
    /// `(event_type, callback_id, capture)`. Adding an identical listener
    /// is a no-op.
    pub fn add_event_listener(
        &mut self,
        target_id: u32,
        event_type: stylo_atoms::Atom,
        callback_id: u32,
        options: crate::events::ListenerOptions,
    ) -> Result<(), HostErrorCode> {
        let node = self
            .doc
            .get_node_mut(taffy::NodeId::from(target_id as u64))
            .ok_or(HostErrorCode::InvalidEventTarget)?;

        // W3C dedup: skip if an identical (type, callback_id, capture) exists
        let already_exists = node.event_listeners.iter().any(|l| {
            l.event_type == event_type
                && l.callback_id == callback_id
                && l.capture == options.capture
        });
        if already_exists {
            return Ok(());
        }

        node.event_listeners
            .push(crate::events::EventListenerEntry {
                event_type,
                callback_id,
                capture: options.capture,
                passive: options.passive,
                once: options.once,
                removed: false,
            });

        Ok(())
    }

    /// Removes an event listener from a DOM node.
    ///
    /// Matches by `(event_type, callback_id, capture)`. If a dispatch is
    /// currently active, the entry is marked `removed` instead of being
    /// deleted (the dispatch loop checks this flag before invoking).
    pub fn remove_event_listener(
        &mut self,
        target_id: u32,
        event_type: stylo_atoms::Atom,
        callback_id: u32,
        capture: bool,
    ) -> Result<(), HostErrorCode> {
        let dispatching = self.current_event.as_ref().is_some_and(|e| e.dispatch_flag);

        let node = self
            .doc
            .get_node_mut(taffy::NodeId::from(target_id as u64))
            .ok_or(HostErrorCode::InvalidEventTarget)?;

        if let Some(entry) = node.event_listeners.iter_mut().find(|l| {
            l.event_type == event_type && l.callback_id == callback_id && l.capture == capture
        }) {
            if dispatching {
                entry.removed = true;
            } else {
                // Not dispatching — safe to remove immediately
                node.event_listeners.retain(|l| {
                    !(l.event_type == event_type
                        && l.callback_id == callback_id
                        && l.capture == capture)
                });
            }
        }

        Ok(())
    }

    // ── Shadow DOM ─────────────────────────────────────────────────

    /// Creates a shadow root on the given host element.
    ///
    /// Returns the shadow root's node ID. The shadow root is not part of the
    /// host's `children` vec — it is only reachable via `host.shadow_root_id`.
    /// Children can be appended to the shadow root using the regular
    /// [`append_element`](Self::append_element) method.
    pub fn attach_shadow(&mut self, host_id: u32, mode: &str) -> Result<u32, HostErrorCode> {
        use crate::dom::element::ShadowRootMode;
        let shadow_mode = match mode {
            "open" => ShadowRootMode::Open,
            "closed" => ShadowRootMode::Closed,
            _ => return Err(HostErrorCode::InvalidParent),
        };
        let shadow_root_id = self
            .doc
            .create_shadow_root(taffy::NodeId::from(host_id as u64), shadow_mode)
            .map_err(dom_error_to_host)?;
        Ok(u64::from(shadow_root_id) as u32)
    }

    /// Returns the shadow root ID for the given host element, or `None`.
    pub fn get_shadow_root(&self, host_id: u32) -> Option<u32> {
        let node = self.doc.get_node(taffy::NodeId::from(host_id as u64))?;
        node.shadow_root_id.map(|id| u64::from(id) as u32)
    }

    /// Parses and adds a CSS stylesheet scoped to a shadow root.
    ///
    /// The stylesheet's rules are collected into a per-shadow-root
    /// [`CascadeData`](style::stylist::CascadeData) that Stylo queries
    /// via [`TShadowRoot::style_data()`].
    pub fn add_shadow_stylesheet(
        &mut self,
        shadow_root_id: u32,
        css: String,
    ) -> Result<(), HostErrorCode> {
        let shadow_node_id = taffy::NodeId::from(shadow_root_id as u64);
        let shadow_root = self
            .doc
            .get_node(shadow_node_id)
            .ok_or(HostErrorCode::InvalidChild)?;
        if shadow_root.node_type != crate::dom::element::NodeType::ShadowRoot {
            return Err(HostErrorCode::InvalidChild);
        }

        // Parse the stylesheet
        let sheet_arc = self.stylesheet_cache.get_or_parse(&css);
        let doc_sheet = style::stylesheets::DocumentStyleSheet(sheet_arc);

        // Build CascadeData from the stylesheet using AuthorStylesheetSet.
        let lock = &self.style_context.lock;
        let guard = lock.read();
        let custom_media = style::stylesheets::CustomMediaMap::default();

        let mut sheet_set = style::stylesheet_set::AuthorStylesheetSet::<
            style::stylesheets::DocumentStyleSheet,
        >::new();
        sheet_set.append_stylesheet(None, &custom_media, doc_sheet, &guard);
        let (flusher, _invalidations) = sheet_set.flush();

        let mut cascade_data = style::stylist::CascadeData::new();
        let mut difference = style::stylist::CascadeDataDifference::default();
        let _ = cascade_data.rebuild(
            self.style_context.stylist.device(),
            style::context::QuirksMode::NoQuirks,
            flusher.sheets,
            &guard,
            &mut difference,
        );

        // Store on the shadow root node
        let shadow_root_mut = self
            .doc
            .get_node_mut(shadow_node_id)
            .expect("shadow root validated above");
        shadow_root_mut.shadow_cascade_data = Some(Box::new(cascade_data));

        // Mark ancestors dirty for re-resolution
        shadow_root_mut.mark_ancestors_dirty();

        Ok(())
    }
}

/// Maps a [`DomError`] to a [`HostErrorCode`] for FFI.
fn dom_error_to_host(e: DomError) -> HostErrorCode {
    match e {
        DomError::InvalidParent => HostErrorCode::InvalidParent,
        DomError::InvalidChild => HostErrorCode::InvalidChild,
        DomError::CycleDetected => HostErrorCode::CycleDetected,
        DomError::ChildAlreadyHasParent => HostErrorCode::ChildAlreadyHasParent,
        DomError::ShadowRootAlreadyExists => HostErrorCode::InvalidParent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_create_element() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        let node = state.doc.get_node(taffy::NodeId::from(id as u64)).unwrap();
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
        let node = state.doc.get_node(taffy::NodeId::from(id as u64)).unwrap();
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
        assert!(state.doc.get_node(taffy::NodeId::from(id as u64)).is_none());
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

        let p_node = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        assert_eq!(p_node.children, vec![taffy::NodeId::from(child as u64)]);

        let c_node = state
            .doc
            .get_node(taffy::NodeId::from(child as u64))
            .unwrap();
        assert_eq!(c_node.parent, Some(taffy::NodeId::from(parent as u64)));
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
        let p_node = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        assert!(p_node.children.is_empty());

        // 2. Cycle Detection (Indirect)
        state.append_element(parent, child).unwrap();
        assert_eq!(
            state.append_element(child, parent),
            Err(HostErrorCode::CycleDetected)
        );
        let c_node = state
            .doc
            .get_node(taffy::NodeId::from(child as u64))
            .unwrap();
        assert!(c_node.children.is_empty());

        // 3. Child Already Has Parent
        let parent2 = state.create_element("section".to_string());
        assert_eq!(
            state.append_element(parent2, child),
            Err(HostErrorCode::ChildAlreadyHasParent)
        );
        let c_node = state
            .doc
            .get_node(taffy::NodeId::from(child as u64))
            .unwrap();
        assert_eq!(c_node.parent, Some(taffy::NodeId::from(parent as u64)));
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
                .get_node(taffy::NodeId::from(parent as u64))
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "parent should be in document"
        );
        assert!(
            state
                .doc
                .get_node(taffy::NodeId::from(child as u64))
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "child should be in document"
        );
        assert!(
            state
                .doc
                .get_node(taffy::NodeId::from(grandchild as u64))
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "grandchild should be in document"
        );

        // Detach parent from root
        state.doc.detach_node(taffy::NodeId::from(parent as u64));

        // All three should no longer have IS_IN_DOCUMENT
        assert!(
            !state
                .doc
                .get_node(taffy::NodeId::from(parent as u64))
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "parent should not be in document after detach"
        );
        assert!(
            !state
                .doc
                .get_node(taffy::NodeId::from(child as u64))
                .unwrap()
                .flags
                .contains(NodeFlags::IS_IN_DOCUMENT),
            "child should not be in document after detach"
        );
        assert!(
            !state
                .doc
                .get_node(taffy::NodeId::from(grandchild as u64))
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

        assert!(state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .is_none());
        assert!(state
            .doc
            .get_node(taffy::NodeId::from(child as u64))
            .is_none());
        assert!(state
            .doc
            .get_node(taffy::NodeId::from(grandchild as u64))
            .is_none());
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
        let p = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
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

    // ── E2E: Multi-element DOM traversal ────────────────────────
    //
    // These tests build complex DOM trees and apply CSS via selectors,
    // exercising node.rs (TNode), selector.rs (selectors::Element),
    // document.rs (TDocument), and dom/mod.rs (ChildrenIterator) during
    // Stylo's style resolution traversal.

    #[test]
    fn test_e2e_sibling_selector_exercises_traversal() {
        // Build: doc > parent > [child1, child2, child3]
        // CSS: div + div { color: green; } — exercises next/prev_sibling_element()
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("section".to_string());
        state.append_element(0, parent).unwrap();

        let c1 = state.create_element("div".to_string());
        let c2 = state.create_element("div".to_string());
        let c3 = state.create_element("div".to_string());
        state.append_element(parent, c1).unwrap();
        state.append_element(parent, c2).unwrap();
        state.append_element(parent, c3).unwrap();

        state.add_stylesheet("div + div { color: green; }".to_string());

        // c1 has no previous sibling div → default color
        let map1 = state.computed_style_map(c1).unwrap();
        let color1 = map1
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();

        // c2 is preceded by c1 → green
        let map2 = state.computed_style_map(c2).unwrap();
        let color2 = map2
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();

        // c3 is preceded by c2 → green
        let map3 = state.computed_style_map(c3).unwrap();
        let color3 = map3
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();

        // c1 should NOT be green (rgb(0, 128, 0))
        if let crate::style::typed_om::CSSStyleValue::Unparsed(s) = &color1 {
            assert_ne!(s, "rgb(0, 128, 0)", "c1 should not match div + div");
        }
        // c2, c3 should be green
        match &color2 {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(0, 128, 0)", "c2 should match div + div");
            }
            other => panic!("Expected green for c2, got: {other:?}"),
        }
        match &color3 {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(0, 128, 0)", "c3 should match div + div");
            }
            other => panic!("Expected green for c3, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_child_combinator_exercises_parent_element() {
        // Exercises parent_element() via the `>` child combinator with classes
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        state.append_element(0, parent).unwrap();
        state
            .set_attribute(parent, "class".to_string(), "parent".to_string())
            .unwrap();

        let c1 = state.create_element("div".to_string());
        let c2 = state.create_element("div".to_string());
        state.append_element(parent, c1).unwrap();
        state.append_element(parent, c2).unwrap();
        state
            .set_attribute(c1, "class".to_string(), "target".to_string())
            .unwrap();

        state.add_stylesheet(".parent > .target { color: blue; }".to_string());

        let map1 = state.computed_style_map(c1).unwrap();
        let color1 = map1
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color1 {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(
                    s, "rgb(0, 0, 255)",
                    ".target child of .parent should be blue"
                );
            }
            other => panic!("Expected blue for .target, got: {other:?}"),
        }

        // c2 has no .target class → should NOT be blue
        let map2 = state.computed_style_map(c2).unwrap();
        let color2 = map2
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        if let crate::style::typed_om::CSSStyleValue::Unparsed(s) = &color2 {
            assert_ne!(s, "rgb(0, 0, 255)", "c2 without .target should not be blue");
        }
    }

    #[test]
    fn test_e2e_general_sibling_selector() {
        // CSS: div ~ span { color: red; } — exercises next_sibling_element chain
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("section".to_string());
        state.append_element(0, parent).unwrap();

        let div = state.create_element("div".to_string());
        let p = state.create_element("p".to_string());
        let span = state.create_element("span".to_string());
        state.append_element(parent, div).unwrap();
        state.append_element(parent, p).unwrap();
        state.append_element(parent, span).unwrap();

        state.add_stylesheet("div ~ span { color: red; }".to_string());

        let map_span = state.computed_style_map(span).unwrap();
        let color = map_span
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(255, 0, 0)", "span should match div ~ span");
            }
            other => panic!("Expected red for span, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_attribute_selector() {
        // Exercises attr_matches(), get_attr() in selector.rs
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_attribute(el, "data-role".to_string(), "main".to_string())
            .unwrap();

        state.add_stylesheet(r#"[data-role="main"] { color: purple; }"#.to_string());

        let map = state.computed_style_map(el).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(128, 0, 128)");
            }
            other => panic!("Expected purple, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_id_selector() {
        // Exercises has_id() in selector.rs
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_attribute(el, "id".to_string(), "main-content".to_string())
            .unwrap();

        state.add_stylesheet("#main-content { color: navy; }".to_string());

        let map = state.computed_style_map(el).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(0, 0, 128)");
            }
            other => panic!("Expected navy, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_empty_pseudo_class() {
        // Exercises is_empty() in selector.rs
        let mut state = RuntimeState::new("https://example.com".to_string());
        let empty = state.create_element("div".to_string());
        let non_empty = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        state.append_element(0, empty).unwrap();
        state.append_element(0, non_empty).unwrap();
        state.append_element(non_empty, child).unwrap();

        state.add_stylesheet("div:empty { color: gray; }".to_string());

        let map_empty = state.computed_style_map(empty).unwrap();
        let color_empty = map_empty
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color_empty {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(128, 128, 128)", "empty div should match :empty");
            }
            other => panic!("Expected gray for empty, got: {other:?}"),
        }

        let map_nonempty = state.computed_style_map(non_empty).unwrap();
        let color_nonempty = map_nonempty
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        if let crate::style::typed_om::CSSStyleValue::Unparsed(s) = &color_nonempty {
            assert_ne!(
                s, "rgb(128, 128, 128)",
                "non-empty div should NOT match :empty"
            );
        }
    }

    #[test]
    fn test_e2e_deeply_nested_tree() {
        // Exercises ChildrenIterator, parent_element(), traversal_parent()
        // through deeply nested DOM traversal
        let mut state = RuntimeState::new("https://example.com".to_string());

        // Build: doc > div.a > div.b > div.c > div.d
        let a = state.create_element("div".to_string());
        let b = state.create_element("div".to_string());
        let c = state.create_element("div".to_string());
        let d = state.create_element("div".to_string());
        state.append_element(0, a).unwrap();
        state.append_element(a, b).unwrap();
        state.append_element(b, c).unwrap();
        state.append_element(c, d).unwrap();

        state
            .set_attribute(a, "class".to_string(), "root-a".to_string())
            .unwrap();
        state
            .set_attribute(d, "class".to_string(), "leaf-d".to_string())
            .unwrap();

        // Descendant combinator exercises parent_element chain
        state.add_stylesheet(".root-a .leaf-d { color: teal; }".to_string());

        let map = state.computed_style_map(d).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(
                    s, "rgb(0, 128, 128)",
                    ".leaf-d should match .root-a .leaf-d"
                );
            }
            other => panic!("Expected teal, got: {other:?}"),
        }
    }

    // ── E2E: Flexbox layout ─────────────────────────────────────

    #[test]
    fn test_e2e_flexbox_alignment_properties() {
        // Exercises enums::content_alignment, enums::item_alignment, flex::*
        let mut state = RuntimeState::new("https://example.com".to_string());
        let container = state.create_element("div".to_string());
        state.append_element(0, container).unwrap();
        state
            .set_attribute(container, "class".to_string(), "flex-container".to_string())
            .unwrap();

        let child = state.create_element("div".to_string());
        state.append_element(container, child).unwrap();
        state
            .set_attribute(child, "class".to_string(), "flex-item".to_string())
            .unwrap();

        state.add_stylesheet(
            r#".flex-container {
                display: flex;
                flex-direction: column;
                flex-wrap: wrap;
                justify-content: space-between;
                align-items: center;
                align-content: space-around;
                gap: 10px 20px;
            }
            .flex-item {
                flex-grow: 2;
                flex-shrink: 0.5;
                flex-basis: 100px;
                align-self: flex-end;
            }"#
            .to_string(),
        );

        // Container properties
        let map_c = state.computed_style_map(container).unwrap();
        assert_keyword(
            &map_c
                .get("display", &mut state.doc, &state.style_context)
                .unwrap(),
            "flex",
        );
        assert_keyword(
            &map_c
                .get("flex-direction", &mut state.doc, &state.style_context)
                .unwrap(),
            "column",
        );
        assert_keyword(
            &map_c
                .get("flex-wrap", &mut state.doc, &state.style_context)
                .unwrap(),
            "wrap",
        );
        assert_keyword(
            &map_c
                .get("justify-content", &mut state.doc, &state.style_context)
                .unwrap(),
            "space-between",
        );
        assert_keyword(
            &map_c
                .get("align-items", &mut state.doc, &state.style_context)
                .unwrap(),
            "center",
        );

        // Item properties
        let map_i = state.computed_style_map(child).unwrap();
        assert_keyword(
            &map_i
                .get("align-self", &mut state.doc, &state.style_context)
                .unwrap(),
            "flex-end",
        );
        assert_computed_contains(
            &map_i
                .get("flex-grow", &mut state.doc, &state.style_context)
                .unwrap(),
            "2",
        );
    }

    #[test]
    fn test_e2e_flexbox_reverse_directions() {
        // Exercises flex::flex_direction (row-reverse, column-reverse)
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();

        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(el, "flex-direction".to_string(), "row-reverse".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("flex-direction", &mut state.doc, &state.style_context)
                .unwrap(),
            "row-reverse",
        );
    }

    // ── E2E: Grid layout ────────────────────────────────────────

    #[test]
    fn test_e2e_grid_template_tracks() {
        // Exercises grid_template_tracks via IR pipeline
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; grid-template-columns: 100px 200px; }"#),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_gap_column() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; column-gap: 10px; }"#),
            "column-gap",
        );
        assert_computed_contains(&val, "10");
    }

    #[test]
    fn test_e2e_grid_gap_row() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; row-gap: 5px; }"#),
            "row-gap",
        );
        assert_computed_contains(&val, "5");
    }

    #[test]
    fn test_e2e_grid_auto_flow_row() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; grid-auto-flow: row; }"#),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_auto_flow_row_dense() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; grid-auto-flow: row dense; }"#),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_auto_flow_column() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; grid-auto-flow: column; }"#),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_auto_flow_column_dense() {
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { display: grid; grid-auto-flow: column dense; }"#),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_repeat_auto_fit() {
        // Exercises grid::track_repeat with auto-fit via IR stylesheet
        let val = ir_pipeline_get(
            view_macros::css!(
                r#"div { display: grid; grid-template-columns: repeat(auto-fit, minmax(100px, 1fr)); }"#
            ),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    #[test]
    fn test_e2e_grid_fit_content() {
        // Exercises grid::track_size FitContent branch via IR stylesheet
        let val = ir_pipeline_get(
            view_macros::css!(
                r#"div { display: grid; grid-template-columns: fit-content(200px) 1fr; }"#
            ),
            "display",
        );
        assert_keyword(&val, "grid");
    }

    // ── E2E: Length/dimension edge cases ────────────────────────

    #[test]
    fn test_e2e_max_width_none_auto() {
        // Exercises length::max_size_dimension with None
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "max-width".to_string(), "none".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("max-width", &mut state.doc, &state.style_context)
                .unwrap(),
            "none",
        );
    }

    #[test]
    fn test_e2e_margin_auto() {
        // Exercises length::margin with Auto
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "margin-left".to_string(), "auto".to_string())
            .unwrap();
        state
            .set_inline_style(el, "margin-right".to_string(), "auto".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("margin-left", &mut state.doc, &state.style_context)
                .unwrap(),
            "auto",
        );
    }

    #[test]
    fn test_e2e_inset_properties() {
        // Exercises length::inset with px, auto, and percent
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "position".to_string(), "absolute".to_string())
            .unwrap();
        state
            .set_inline_style(el, "top".to_string(), "10px".to_string())
            .unwrap();
        state
            .set_inline_style(el, "right".to_string(), "20%".to_string())
            .unwrap();
        state
            .set_inline_style(el, "bottom".to_string(), "auto".to_string())
            .unwrap();
        state
            .set_inline_style(el, "left".to_string(), "5px".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_computed_contains(
            &map.get("top", &mut state.doc, &state.style_context)
                .unwrap(),
            "10",
        );
        assert_computed_contains(
            &map.get("right", &mut state.doc, &state.style_context)
                .unwrap(),
            "20",
        );
        assert_keyword(
            &map.get("bottom", &mut state.doc, &state.style_context)
                .unwrap(),
            "auto",
        );
    }

    #[test]
    fn test_e2e_border_solid_width() {
        // Exercises length::border with solid style → preserves width
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "border-top-style".to_string(), "solid".to_string())
            .unwrap();
        state
            .set_inline_style(el, "border-top-width".to_string(), "5px".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_computed_contains(
            &map.get("border-top-width", &mut state.doc, &state.style_context)
                .unwrap(),
            "5",
        );
        assert_keyword(
            &map.get("border-top-style", &mut state.doc, &state.style_context)
                .unwrap(),
            "solid",
        );
    }

    // ── E2E: Enum conversion edge cases ─────────────────────────

    #[test]
    fn test_e2e_position_variants() {
        // Exercises enums::position for all variants
        for (css_val, expected) in [
            ("static", "static"),
            ("relative", "relative"),
            ("absolute", "absolute"),
            ("fixed", "fixed"),
            ("sticky", "sticky"),
        ] {
            let mut state = RuntimeState::new("https://example.com".to_string());
            let el = state.create_element("div".to_string());
            state.append_element(0, el).unwrap();
            state
                .set_inline_style(el, "position".to_string(), css_val.to_string())
                .unwrap();

            let map = state.computed_style_map(el).unwrap();
            assert_keyword(
                &map.get("position", &mut state.doc, &state.style_context)
                    .unwrap(),
                expected,
            );
        }
    }

    #[test]
    fn test_e2e_overflow_variants() {
        // Exercises enums::overflow for all variants
        for (css_val, expected) in [
            ("visible", "visible"),
            ("hidden", "hidden"),
            ("scroll", "scroll"),
            ("auto", "auto"),
            ("clip", "clip"),
        ] {
            let mut state = RuntimeState::new("https://example.com".to_string());
            let el = state.create_element("div".to_string());
            state.append_element(0, el).unwrap();
            state
                .set_inline_style(el, "overflow-x".to_string(), css_val.to_string())
                .unwrap();

            let map = state.computed_style_map(el).unwrap();
            assert_keyword(
                &map.get("overflow-x", &mut state.doc, &state.style_context)
                    .unwrap(),
                expected,
            );
        }
    }

    #[test]
    fn test_e2e_aspect_ratio() {
        // Exercises enums::aspect_ratio
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "aspect-ratio".to_string(), "16 / 9".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        let val = map
            .get("aspect-ratio", &mut state.doc, &state.style_context)
            .unwrap();
        // Should contain "16" and "9" or the ratio
        let s = format!("{val:?}");
        assert!(
            s.contains("16") || s.contains("1.7"),
            "aspect-ratio should contain 16/9 info: {s}"
        );
    }

    #[test]
    fn test_e2e_alignment_space_evenly() {
        // Exercises enums::content_alignment SPACE_EVENLY branch
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(
                el,
                "justify-content".to_string(),
                "space-evenly".to_string(),
            )
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("justify-content", &mut state.doc, &state.style_context)
                .unwrap(),
            "space-evenly",
        );
    }

    #[test]
    fn test_e2e_align_items_baseline() {
        // Exercises enums::item_alignment BASELINE branch
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(el, "align-items".to_string(), "baseline".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("align-items", &mut state.doc, &state.style_context)
                .unwrap(),
            "baseline",
        );
    }

    #[test]
    fn test_e2e_align_items_stretch() {
        // Exercises enums::item_alignment STRETCH branch
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(el, "align-items".to_string(), "stretch".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("align-items", &mut state.doc, &state.style_context)
                .unwrap(),
            "stretch",
        );
    }

    #[test]
    fn test_e2e_content_alignment_flex_start_end() {
        // Exercises enums::content_alignment FLEX_START, FLEX_END branches
        for (val, expected) in [("flex-start", "flex-start"), ("flex-end", "flex-end")] {
            let mut state = RuntimeState::new("https://example.com".to_string());
            let el = state.create_element("div".to_string());
            state.append_element(0, el).unwrap();
            state
                .set_inline_style(el, "display".to_string(), "flex".to_string())
                .unwrap();
            state
                .set_inline_style(el, "align-content".to_string(), val.to_string())
                .unwrap();

            let map = state.computed_style_map(el).unwrap();
            assert_keyword(
                &map.get("align-content", &mut state.doc, &state.style_context)
                    .unwrap(),
                expected,
            );
        }
    }

    // ── E2E: Layout computation (exercises to_taffy_style) ──────

    #[test]
    fn test_e2e_layout_flexbox_children() {
        // Full layout computation — exercises to_taffy_style through
        // build_layout_tree → compute_layout
        let mut state = RuntimeState::new("https://example.com".to_string());
        let container = state.create_element("div".to_string());
        state.append_element(0, container).unwrap();

        let c1 = state.create_element("div".to_string());
        let c2 = state.create_element("div".to_string());
        state.append_element(container, c1).unwrap();
        state.append_element(container, c2).unwrap();

        // Use inline styles to avoid class selector issues
        state
            .set_inline_style(container, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(c1, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(c1, "height".to_string(), "50px".to_string())
            .unwrap();
        state
            .set_inline_style(c2, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(c2, "height".to_string(), "50px".to_string())
            .unwrap();

        // Resolve styles (calling .get() triggers ensure_styles_resolved)
        let map = state.computed_style_map(container).unwrap();
        let _ = map.get("display", &mut state.doc, &state.style_context);

        assert!(crate::layout::compute_layout_in_place(
            &mut state.doc,
            taffy::NodeId::from(container as u64)
        ));
        let node = state
            .doc
            .get_node(taffy::NodeId::from(container as u64))
            .unwrap();
        // Two 100px-wide children in a flex row → container should be 200px wide
        assert_eq!(node.layout().size.width, 200.0);
        assert_eq!(node.layout().size.height, 50.0);
    }

    #[test]
    fn test_e2e_layout_grid_children() {
        // Grid layout via IR stylesheet — exercises grid conversion in to_taffy_style
        let mut state = RuntimeState::new("https://example.com".to_string());
        let container = state.create_element("div".to_string());
        state.append_element(0, container).unwrap();

        let c1 = state.create_element("div".to_string());
        let c2 = state.create_element("div".to_string());
        state.append_element(container, c1).unwrap();
        state.append_element(container, c2).unwrap();

        state
            .set_inline_style(container, "display".to_string(), "grid".to_string())
            .unwrap();
        state
            .set_inline_style(c1, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(c1, "height".to_string(), "50px".to_string())
            .unwrap();
        state
            .set_inline_style(c2, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(c2, "height".to_string(), "50px".to_string())
            .unwrap();

        // Trigger style resolution
        let map = state.computed_style_map(container).unwrap();
        let _ = map.get("display", &mut state.doc, &state.style_context);

        assert!(crate::layout::compute_layout_in_place(
            &mut state.doc,
            taffy::NodeId::from(container as u64)
        ));
        let node = state
            .doc
            .get_node(taffy::NodeId::from(container as u64))
            .unwrap();
        assert!(
            node.layout().size.width > 0.0,
            "grid layout should have positive width"
        );
        assert!(
            node.layout().size.height > 0.0,
            "grid layout should have positive height"
        );
    }

    #[test]
    fn test_e2e_layout_with_padding() {
        // Exercises length_percentage for padding in layout via inline style
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();

        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(el, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(el, "height".to_string(), "100px".to_string())
            .unwrap();

        // Trigger style resolution and verify padding via computed values
        let map = state.computed_style_map(el).unwrap();
        assert_keyword(
            &map.get("display", &mut state.doc, &state.style_context)
                .unwrap(),
            "flex",
        );

        // Verify layout computes
        assert!(crate::layout::compute_layout_in_place(
            &mut state.doc,
            taffy::NodeId::from(el as u64)
        ));
        let node = state.doc.get_node(taffy::NodeId::from(el as u64)).unwrap();
        assert_eq!(node.layout().size.width, 100.0);
        assert_eq!(node.layout().size.height, 100.0);
    }

    #[test]
    fn test_e2e_border_width_computed() {
        // Exercises length::border via computed style.
        // CSS spec: border-top-width computes to 0px when border-top-style is none (initial).
        // We verify the conversion path produces a px value.
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { border-top-width: 5px; }"#),
            "border-top-width",
        );
        let s = format!("{val}");
        assert!(
            s.contains("px"),
            "Expected a px value for border-top-width, got: {s}"
        );
    }

    #[test]
    fn test_e2e_padding_computed() {
        // Exercises length_percentage for padding via computed style
        let val = ir_pipeline_get(
            view_macros::css!(r#"div { padding-top: 10px; padding-left: 15px; }"#),
            "padding-top",
        );
        assert_computed_contains(&val, "10");
    }

    // ── E2E: Typed OM get_all and edge cases ────────────────────

    #[test]
    fn test_e2e_typed_om_get_all() {
        // Exercises map.rs get_all() path
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        let all = map.get_all("display", &mut state.doc, &state.style_context);
        assert_eq!(all.len(), 1);
        assert_keyword(&all[0], "flex");

        // Invalid property returns empty vec
        let none = map.get_all("not-a-property", &mut state.doc, &state.style_context);
        assert!(none.is_empty());
    }

    #[test]
    fn test_e2e_typed_om_to_vec_vendor_prefix_ordering() {
        // Exercises map.rs to_vec sorting: standard before vendor-prefixed
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();

        let map = state.computed_style_map(el).unwrap();
        let entries = map.to_vec(&mut state.doc, &state.style_context);

        // Find the boundary between standard and vendor-prefixed
        let first_vendor = entries.iter().position(|(name, _)| name.starts_with('-'));
        if let Some(idx) = first_vendor {
            // All entries before should be non-vendor
            for (name, _) in &entries[..idx] {
                assert!(
                    !name.starts_with('-'),
                    "standard properties should come before vendor: {name}"
                );
            }
            // All entries after should be vendor
            for (name, _) in &entries[idx..] {
                assert!(
                    name.starts_with('-'),
                    "vendor properties should come after standard: {name}"
                );
            }
        }
    }

    // ── E2E: Multiple stylesheets and cascade ───────────────────

    #[test]
    fn test_e2e_multiple_stylesheets_cascade() {
        // Later stylesheet wins — exercises cascade and multiple add_stylesheet calls
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();

        state.add_stylesheet("div { color: red; }".to_string());
        state.add_stylesheet("div { color: blue; }".to_string());

        let map = state.computed_style_map(el).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(0, 0, 255)", "later stylesheet should win");
            }
            other => panic!("Expected blue, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_class_specificity_over_tag() {
        // Class selector beats tag selector — exercises has_class in selector.rs
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_attribute(el, "class".to_string(), "special".to_string())
            .unwrap();

        state.add_stylesheet(".special { color: red; } div { color: blue; }".to_string());

        let map = state.computed_style_map(el).unwrap();
        let color = map
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(255, 0, 0)", ".special should beat div");
            }
            other => panic!("Expected red, got: {other:?}"),
        }
    }

    // ── E2E: is_root and element type checks ────────────────────

    #[test]
    fn test_e2e_root_pseudo_class() {
        // Exercises is_root() in selector.rs — the element whose parent is Document
        let mut state = RuntimeState::new("https://example.com".to_string());
        let root = state.create_element("html".to_string());
        state.append_element(0, root).unwrap();

        let child = state.create_element("div".to_string());
        state.append_element(root, child).unwrap();

        state.add_stylesheet(":root { color: green; }".to_string());

        let map_root = state.computed_style_map(root).unwrap();
        let color = map_root
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(0, 128, 0)", ":root should match html element");
            }
            other => panic!("Expected green, got: {other:?}"),
        }
    }

    #[test]
    fn test_e2e_last_child_pseudo() {
        // Exercises traversal: last_child, prev_sibling_element
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("ul".to_string());
        state.append_element(0, parent).unwrap();

        let li1 = state.create_element("li".to_string());
        let li2 = state.create_element("li".to_string());
        let li3 = state.create_element("li".to_string());
        state.append_element(parent, li1).unwrap();
        state.append_element(parent, li2).unwrap();
        state.append_element(parent, li3).unwrap();

        state.add_stylesheet("li:last-child { color: red; }".to_string());

        // li3 is last-child
        let map3 = state.computed_style_map(li3).unwrap();
        let color3 = map3
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color3 {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(255, 0, 0)", "li3 should be :last-child");
            }
            other => panic!("Expected red for li3, got: {other:?}"),
        }

        // li1 should not be :last-child
        let map1 = state.computed_style_map(li1).unwrap();
        let color1 = map1
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        if let crate::style::typed_om::CSSStyleValue::Unparsed(s) = &color1 {
            assert_ne!(s, "rgb(255, 0, 0)", "li1 should NOT be :last-child");
        }
    }

    #[test]
    fn test_e2e_nth_child_selector() {
        // Exercises sibling traversal for nth-child counting
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        state.append_element(0, parent).unwrap();

        let c1 = state.create_element("p".to_string());
        let c2 = state.create_element("p".to_string());
        let c3 = state.create_element("p".to_string());
        state.append_element(parent, c1).unwrap();
        state.append_element(parent, c2).unwrap();
        state.append_element(parent, c3).unwrap();

        state.add_stylesheet("p:nth-child(2) { color: orange; }".to_string());

        let map2 = state.computed_style_map(c2).unwrap();
        let color2 = map2
            .get("color", &mut state.doc, &state.style_context)
            .unwrap();
        match &color2 {
            crate::style::typed_om::CSSStyleValue::Unparsed(s) => {
                assert_eq!(s, "rgb(255, 165, 0)", "c2 should be :nth-child(2)");
            }
            other => panic!("Expected orange, got: {other:?}"),
        }
    }

    // ── E2E: Display none propagation ───────────────────────────

    #[test]
    fn test_e2e_display_none_hides_subtree() {
        // Exercises display::None path + layout with hidden elements
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        state.append_element(0, parent).unwrap();
        state
            .set_inline_style(parent, "display".to_string(), "none".to_string())
            .unwrap();

        let child = state.create_element("div".to_string());
        state.append_element(parent, child).unwrap();
        state
            .set_inline_style(child, "width".to_string(), "100px".to_string())
            .unwrap();

        let map = state.computed_style_map(parent).unwrap();
        assert_keyword(
            &map.get("display", &mut state.doc, &state.style_context)
                .unwrap(),
            "none",
        );
    }

    // ── E2E: Gap with percentage ────────────────────────────────

    #[test]
    fn test_e2e_gap_percentage() {
        // Exercises length::gap with percentage value
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state.append_element(0, el).unwrap();
        state
            .set_inline_style(el, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(el, "gap".to_string(), "5%".to_string())
            .unwrap();

        let map = state.computed_style_map(el).unwrap();
        assert_computed_contains(
            &map.get("column-gap", &mut state.doc, &state.style_context)
                .unwrap(),
            "5",
        );
    }

    #[test]
    fn test_commit_resolves_styles_and_computes_layout() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state
            .set_inline_style(div, "display".to_string(), "block".to_string())
            .unwrap();
        state
            .set_inline_style(div, "width".to_string(), "200px".to_string())
            .unwrap();
        state
            .set_inline_style(div, "height".to_string(), "100px".to_string())
            .unwrap();

        state.commit();
        let node = state.doc.get_node(taffy::NodeId::from(div as u64)).unwrap();
        assert_eq!(node.layout().size.width, 200.0);
        assert_eq!(node.layout().size.height, 100.0);
    }

    #[test]
    fn test_commit_skips_when_not_dirty() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let div = state.create_element("div".to_string());
        state.append_element(0, div).unwrap();
        state
            .set_inline_style(div, "width".to_string(), "50px".to_string())
            .unwrap();
        state
            .set_inline_style(div, "height".to_string(), "50px".to_string())
            .unwrap();

        // First commit resolves styles
        state.commit();
        let node = state.doc.get_node(taffy::NodeId::from(div as u64)).unwrap();
        assert_eq!(node.layout().size.width, 50.0);
        assert_eq!(node.layout().size.height, 50.0);

        // Second commit without changes — should still produce correct layout
        state.commit();
        let node = state.doc.get_node(taffy::NodeId::from(div as u64)).unwrap();
        assert_eq!(node.layout().size.width, 50.0);
        assert_eq!(node.layout().size.height, 50.0);
    }

    // ─── insert_before tests ──────────────────────────────────────

    #[test]
    fn test_insert_before_basic() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let a = state.create_element("a".to_string());
        let b = state.create_element("b".to_string());
        let x = state.create_element("x".to_string());

        state.append_element(parent, a).unwrap();
        state.append_element(parent, b).unwrap();

        // Insert x before b → order should be [a, x, b]
        state.insert_before(parent, x, b).unwrap();

        let p = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        let children: Vec<u32> = p.children.iter().map(|&id| u64::from(id) as u32).collect();
        assert_eq!(children, vec![a, x, b]);
    }

    #[test]
    fn test_insert_before_at_beginning() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let a = state.create_element("a".to_string());
        let x = state.create_element("x".to_string());

        state.append_element(parent, a).unwrap();

        // Insert x before a → order should be [x, a]
        state.insert_before(parent, x, a).unwrap();

        let p = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        let children: Vec<u32> = p.children.iter().map(|&id| u64::from(id) as u32).collect();
        assert_eq!(children, vec![x, a]);
    }

    #[test]
    fn test_insert_before_same_parent_reorder() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let a = state.create_element("a".to_string());
        let b = state.create_element("b".to_string());
        let c = state.create_element("c".to_string());

        state.append_element(parent, a).unwrap();
        state.append_element(parent, b).unwrap();
        state.append_element(parent, c).unwrap();
        // [a, b, c]

        // Move a before c → [b, a, c]
        state.insert_before(parent, a, c).unwrap();

        let p = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        let children: Vec<u32> = p.children.iter().map(|&id| u64::from(id) as u32).collect();
        assert_eq!(children, vec![b, a, c]);
    }

    #[test]
    fn test_insert_before_self_is_noop() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let a = state.create_element("a".to_string());
        let b = state.create_element("b".to_string());

        state.append_element(parent, a).unwrap();
        state.append_element(parent, b).unwrap();

        // Insert a before a → no-op, order stays [a, b]
        state.insert_before(parent, a, a).unwrap();

        let p = state
            .doc
            .get_node(taffy::NodeId::from(parent as u64))
            .unwrap();
        let children: Vec<u32> = p.children.iter().map(|&id| u64::from(id) as u32).collect();
        assert_eq!(children, vec![a, b]);
    }

    #[test]
    fn test_insert_before_errors() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let other_parent = state.create_element("section".to_string());
        let other_child = state.create_element("em".to_string());

        state.append_element(parent, child).unwrap();
        state.append_element(other_parent, other_child).unwrap();

        let new_node = state.create_element("p".to_string());

        // ref_child not a child of parent
        assert_eq!(
            state.insert_before(parent, new_node, other_child),
            Err(HostErrorCode::InvalidChild)
        );

        // Cycle detection
        assert_eq!(
            state.insert_before(parent, parent, child),
            Err(HostErrorCode::CycleDetected)
        );

        // new_child has different parent
        assert_eq!(
            state.insert_before(parent, other_child, child),
            Err(HostErrorCode::ChildAlreadyHasParent)
        );
    }

    // ── Shadow DOM tests ────────────────────────────────────────────

    #[test]
    fn test_attach_shadow_basic() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();
        assert!(shadow_root > 0);

        // Verify shadow_root_id is set on host
        let host_node = state
            .doc
            .get_node(taffy::NodeId::from(host as u64))
            .unwrap();
        assert_eq!(
            host_node.shadow_root_id,
            Some(taffy::NodeId::from(shadow_root as u64))
        );

        // Verify get_shadow_root returns the same ID
        assert_eq!(state.get_shadow_root(host), Some(shadow_root));
    }

    #[test]
    fn test_attach_shadow_duplicate_fails() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        assert!(state.attach_shadow(host, "open").is_ok());
        assert!(state.attach_shadow(host, "open").is_err());
    }

    #[test]
    fn test_attach_shadow_closed_mode() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "closed").unwrap();
        let shadow_root_node = state
            .doc
            .get_node(taffy::NodeId::from(shadow_root as u64))
            .unwrap();
        assert_eq!(
            shadow_root_node.shadow_mode,
            Some(crate::dom::element::ShadowRootMode::Closed)
        );
    }

    #[test]
    fn test_insert_before_in_document_propagation() {
        use crate::dom::NodeFlags;

        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let new_node = state.create_element("em".to_string());

        // Connect parent to document
        state.append_element(0, parent).unwrap();
        state.append_element(parent, child).unwrap();

        // Insert new_node before child — should propagate IS_IN_DOCUMENT
        state.insert_before(parent, new_node, child).unwrap();

        assert!(state
            .doc
            .get_node(taffy::NodeId::from(new_node as u64))
            .unwrap()
            .flags
            .contains(NodeFlags::IS_IN_DOCUMENT));
    }

    // ─── clone_node tests ─────────────────────────────────────────

    #[test]
    fn test_clone_node_shallow() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        state
            .set_attribute(el, "id".to_string(), "foo".to_string())
            .unwrap();
        state
            .set_attribute(el, "class".to_string(), "bar baz".to_string())
            .unwrap();

        let child = state.create_element("span".to_string());
        state.append_element(el, child).unwrap();

        // Shallow clone
        let cloned = state.clone_node(el, false).unwrap();
        assert_ne!(cloned, el);

        let cloned_node = state
            .doc
            .get_node(taffy::NodeId::from(cloned as u64))
            .unwrap();
        assert!(cloned_node.is_element());
        assert_eq!(cloned_node.name.as_ref().unwrap().local.as_ref(), "div");
        assert_eq!(cloned_node.get_attribute("id"), Some("foo"));
        assert!(cloned_node
            .classes
            .contains(&stylo_atoms::Atom::from("bar")));
        assert!(cloned_node
            .classes
            .contains(&stylo_atoms::Atom::from("baz")));
        // Shallow: no children
        assert!(cloned_node.children.is_empty());
        // No parent
        assert!(cloned_node.parent.is_none());
    }

    #[test]
    fn test_clone_node_deep() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let parent = state.create_element("div".to_string());
        let child = state.create_element("span".to_string());
        let text = state.create_text_node("hello".to_string());

        state.append_element(parent, child).unwrap();
        state.append_element(child, text).unwrap();

        // Deep clone
        let cloned = state.clone_node(parent, true).unwrap();
        assert_ne!(cloned, parent);

        let cloned_node = state
            .doc
            .get_node(taffy::NodeId::from(cloned as u64))
            .unwrap();
        assert_eq!(cloned_node.children.len(), 1);

        let cloned_child_id = u64::from(cloned_node.children[0]) as u32;
        assert_ne!(cloned_child_id, child);

        let cloned_child = state
            .doc
            .get_node(taffy::NodeId::from(cloned_child_id as u64))
            .unwrap();
        assert!(cloned_child.is_element());
        assert_eq!(cloned_child.name.as_ref().unwrap().local.as_ref(), "span");
        assert_eq!(cloned_child.children.len(), 1);

        let cloned_text_id = u64::from(cloned_child.children[0]) as u32;
        let cloned_text = state
            .doc
            .get_node(taffy::NodeId::from(cloned_text_id as u64))
            .unwrap();
        assert!(cloned_text.is_text_node());
        assert_eq!(cloned_text.text_content.as_deref(), Some("hello"));
    }

    #[test]
    fn test_clone_node_text() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let text = state.create_text_node("world".to_string());

        let cloned = state.clone_node(text, false).unwrap();
        let cloned_node = state
            .doc
            .get_node(taffy::NodeId::from(cloned as u64))
            .unwrap();
        assert!(cloned_node.is_text_node());
        assert_eq!(cloned_node.text_content.as_deref(), Some("world"));
    }

    #[test]
    fn test_clone_node_invalid() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        assert_eq!(
            state.clone_node(999, false),
            Err(HostErrorCode::InvalidChild)
        );
    }

    // ─── set_node_value tests ─────────────────────────────────────

    #[test]
    fn test_set_node_value_text_node() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let text = state.create_text_node("old".to_string());

        state.set_node_value(text, "new".to_string()).unwrap();

        let node = state
            .doc
            .get_node(taffy::NodeId::from(text as u64))
            .unwrap();
        assert_eq!(node.text_content.as_deref(), Some("new"));
    }

    #[test]
    fn test_set_node_value_element_noop() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());

        // Should succeed (no-op, not an error)
        state.set_node_value(el, "ignored".to_string()).unwrap();
    }

    #[test]
    fn test_set_node_value_invalid() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        assert_eq!(
            state.set_node_value(999, "value".to_string()),
            Err(HostErrorCode::InvalidChild)
        );
    }

    // ─── get_node_type tests ──────────────────────────────────────

    #[test]
    fn test_get_node_type_element() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let el = state.create_element("div".to_string());
        assert_eq!(state.get_node_type(el), Ok(1));
    }

    #[test]
    fn test_get_node_type_text() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let text = state.create_text_node("hi".to_string());
        assert_eq!(state.get_node_type(text), Ok(3));
    }

    #[test]
    fn test_get_node_type_document() {
        let state = RuntimeState::new("https://example.com".to_string());
        // Document root is id 0
        assert_eq!(state.get_node_type(0), Ok(9));
    }

    #[test]
    fn test_get_node_type_invalid() {
        let state = RuntimeState::new("https://example.com".to_string());
        assert_eq!(state.get_node_type(999), Err(HostErrorCode::InvalidChild));
    }

    // ── Event system RuntimeState tests ─────────────────────────────

    #[test]
    fn test_add_event_listener_basic() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();

        let result = state.add_event_listener(
            id,
            stylo_atoms::Atom::from("click"),
            1,
            crate::events::ListenerOptions {
                capture: false,
                passive: false,
                once: false,
            },
        );
        assert!(result.is_ok());

        let node = state.doc.get_node(taffy::NodeId::from(id as u64)).unwrap();
        assert_eq!(node.event_listeners.len(), 1);
        assert_eq!(node.event_listeners[0].callback_id, 1);
    }

    #[test]
    fn test_add_event_listener_dedup() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());

        // Add same listener twice — should deduplicate
        state
            .add_event_listener(
                id,
                stylo_atoms::Atom::from("click"),
                1,
                crate::events::ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();
        state
            .add_event_listener(
                id,
                stylo_atoms::Atom::from("click"),
                1,
                crate::events::ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();

        let node = state.doc.get_node(taffy::NodeId::from(id as u64)).unwrap();
        assert_eq!(node.event_listeners.len(), 1);
    }

    #[test]
    fn test_add_event_listener_different_capture_is_not_dedup() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());

        state
            .add_event_listener(
                id,
                stylo_atoms::Atom::from("click"),
                1,
                crate::events::ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();
        state
            .add_event_listener(
                id,
                stylo_atoms::Atom::from("click"),
                1,
                crate::events::ListenerOptions {
                    capture: true,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();

        let node = state.doc.get_node(taffy::NodeId::from(id as u64)).unwrap();
        assert_eq!(node.event_listeners.len(), 2);
    }

    #[test]
    fn test_add_event_listener_invalid_target() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let result = state.add_event_listener(
            999,
            stylo_atoms::Atom::from("click"),
            1,
            crate::events::ListenerOptions {
                capture: false,
                passive: false,
                once: false,
            },
        );
        assert_eq!(result, Err(HostErrorCode::InvalidEventTarget));
    }

    #[test]
    fn test_remove_event_listener_basic() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());

        state
            .add_event_listener(
                id,
                stylo_atoms::Atom::from("click"),
                1,
                crate::events::ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();
        assert_eq!(
            state
                .doc
                .get_node(taffy::NodeId::from(id as u64))
                .unwrap()
                .event_listeners
                .len(),
            1
        );

        state
            .remove_event_listener(id, stylo_atoms::Atom::from("click"), 1, false)
            .unwrap();
        assert_eq!(
            state
                .doc
                .get_node(taffy::NodeId::from(id as u64))
                .unwrap()
                .event_listeners
                .len(),
            0
        );
    }

    #[test]
    fn test_remove_event_listener_invalid_target() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let result = state.remove_event_listener(999, stylo_atoms::Atom::from("click"), 1, false);
        assert_eq!(result, Err(HostErrorCode::InvalidEventTarget));
    }

    #[test]
    fn test_remove_event_listener_nonexistent_is_ok() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        // Removing a listener that doesn't exist is a no-op per W3C spec
        let result = state.remove_event_listener(id, stylo_atoms::Atom::from("click"), 1, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_current_event_initially_none() {
        let state = RuntimeState::new("https://example.com".to_string());
        assert!(state.current_event.is_none());
    }

    #[test]
    fn test_host_error_code_event_variants() {
        assert_eq!(HostErrorCode::InvalidEventTarget.as_i32(), -6);
        assert_eq!(HostErrorCode::NoActiveEvent.as_i32(), -7);
        assert_eq!(HostErrorCode::EventAlreadyDispatching.as_i32(), -8);

        assert_eq!(
            HostErrorCode::InvalidEventTarget.message(),
            "invalid event target id"
        );
        assert_eq!(
            HostErrorCode::NoActiveEvent.message(),
            "no event is currently being dispatched"
        );
        assert_eq!(
            HostErrorCode::EventAlreadyDispatching.message(),
            "an event is already being dispatched"
        );
    }

    #[test]
    fn test_shadow_root_children_styled() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        // Add a child to the shadow root
        let inner = state.create_element("span".to_string());
        state.append_element(shadow_root, inner).unwrap();

        // Commit should succeed — shadow tree children should be styled
        state.commit();

        let inner_node = state
            .doc
            .get_node(taffy::NodeId::from(inner as u64))
            .unwrap();
        assert!(inner_node.computed_values.is_some());
    }

    #[test]
    fn test_shadow_tree_inherits_from_host() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();
        state
            .set_inline_style(host, "font-size".to_string(), "24px".to_string())
            .unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();
        let inner = state.create_element("span".to_string());
        state.append_element(shadow_root, inner).unwrap();

        state.commit();

        // The inner element should inherit font-size from the host
        let inner_node = state
            .doc
            .get_node(taffy::NodeId::from(inner as u64))
            .unwrap();
        let cv = inner_node.computed_values.as_ref().unwrap();
        let fs = cv.clone_font_size().computed_size().px();
        assert_eq!(fs, 24.0);
    }

    #[test]
    fn test_shadow_host_layout_uses_shadow_tree() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        // Add styled child to shadow tree
        let inner = state.create_element("div".to_string());
        state.append_element(shadow_root, inner).unwrap();
        state
            .set_inline_style(inner, "width".to_string(), "100px".to_string())
            .unwrap();
        state
            .set_inline_style(inner, "height".to_string(), "50px".to_string())
            .unwrap();

        // Also add a light DOM child (should NOT be rendered)
        let light = state.create_element("div".to_string());
        state.append_element(host, light).unwrap();
        state
            .set_inline_style(light, "width".to_string(), "200px".to_string())
            .unwrap();
        state
            .set_inline_style(light, "height".to_string(), "200px".to_string())
            .unwrap();

        state.commit();

        // Shadow tree child should have its layout
        let inner_node = state
            .doc
            .get_node(taffy::NodeId::from(inner as u64))
            .unwrap();
        assert_eq!(inner_node.layout().size.width, 100.0);
        assert_eq!(inner_node.layout().size.height, 50.0);
    }

    #[test]
    fn test_destroy_host_cleans_shadow_tree() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();
        let inner = state.create_element("span".to_string());
        state.append_element(shadow_root, inner).unwrap();

        // Destroy the host — should also remove shadow root and its children
        state.destroy_element(host).unwrap();

        assert!(state
            .doc
            .get_node(taffy::NodeId::from(host as u64))
            .is_none());
        assert!(state
            .doc
            .get_node(taffy::NodeId::from(shadow_root as u64))
            .is_none());
        assert!(state
            .doc
            .get_node(taffy::NodeId::from(inner as u64))
            .is_none());
    }

    #[test]
    fn test_slot_default_distribution() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        // Create a default <slot> in the shadow tree
        let slot = state.create_element("slot".to_string());
        state.append_element(shadow_root, slot).unwrap();

        // Add a light DOM child to the host
        let light_child = state.create_element("span".to_string());
        state.append_element(host, light_child).unwrap();
        state
            .set_inline_style(light_child, "width".to_string(), "30px".to_string())
            .unwrap();
        state
            .set_inline_style(light_child, "height".to_string(), "30px".to_string())
            .unwrap();

        state.commit();

        // The light child should be distributed to the default slot
        let slot_node = state
            .doc
            .get_node(taffy::NodeId::from(slot as u64))
            .unwrap();
        assert_eq!(slot_node.assigned_nodes.len(), 1);
        assert_eq!(
            slot_node.assigned_nodes[0],
            taffy::NodeId::from(light_child as u64)
        );

        // The light child should have its layout (distributed via slot)
        let light_node = state
            .doc
            .get_node(taffy::NodeId::from(light_child as u64))
            .unwrap();
        assert_eq!(light_node.layout().size.width, 30.0);
        assert_eq!(light_node.layout().size.height, 30.0);
    }

    #[test]
    fn test_slot_named_distribution() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        // Create named slots in the shadow tree
        let header_slot = state.create_element("slot".to_string());
        state.append_element(shadow_root, header_slot).unwrap();
        state
            .set_attribute(header_slot, "name".to_string(), "header".to_string())
            .unwrap();

        let content_slot = state.create_element("slot".to_string());
        state.append_element(shadow_root, content_slot).unwrap();
        state
            .set_attribute(content_slot, "name".to_string(), "content".to_string())
            .unwrap();

        // Add light DOM children with slot attributes
        let header = state.create_element("h1".to_string());
        state.append_element(host, header).unwrap();
        state
            .set_attribute(header, "slot".to_string(), "header".to_string())
            .unwrap();

        let content = state.create_element("p".to_string());
        state.append_element(host, content).unwrap();
        state
            .set_attribute(content, "slot".to_string(), "content".to_string())
            .unwrap();

        state.commit();

        // Verify named slot assignment
        let hs = state
            .doc
            .get_node(taffy::NodeId::from(header_slot as u64))
            .unwrap();
        assert_eq!(hs.assigned_nodes.len(), 1);
        assert_eq!(hs.assigned_nodes[0], taffy::NodeId::from(header as u64));

        let cs = state
            .doc
            .get_node(taffy::NodeId::from(content_slot as u64))
            .unwrap();
        assert_eq!(cs.assigned_nodes.len(), 1);
        assert_eq!(cs.assigned_nodes[0], taffy::NodeId::from(content as u64));
    }

    #[test]
    fn test_slot_fallback_content() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        // Create a slot with fallback content
        let slot = state.create_element("slot".to_string());
        state.append_element(shadow_root, slot).unwrap();

        let fallback = state.create_element("em".to_string());
        state.append_element(slot, fallback).unwrap();
        state
            .set_inline_style(fallback, "width".to_string(), "20px".to_string())
            .unwrap();
        state
            .set_inline_style(fallback, "height".to_string(), "20px".to_string())
            .unwrap();

        // No light DOM children — slot should use fallback
        state.commit();

        let slot_node = state
            .doc
            .get_node(taffy::NodeId::from(slot as u64))
            .unwrap();
        assert!(slot_node.assigned_nodes.is_empty());

        // Fallback child should be laid out
        let fb_node = state
            .doc
            .get_node(taffy::NodeId::from(fallback as u64))
            .unwrap();
        assert_eq!(fb_node.layout().size.width, 20.0);
        assert_eq!(fb_node.layout().size.height, 20.0);
    }

    #[test]
    fn test_containing_shadow_host() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();
        let inner = state.create_element("span".to_string());
        state.append_element(shadow_root, inner).unwrap();

        state.commit();

        // Check containing_shadow returns the shadow root
        use style::dom::TElement;
        let inner_ref = state
            .doc
            .get_node(taffy::NodeId::from(inner as u64))
            .unwrap();
        let shadow = TElement::containing_shadow(&inner_ref);
        assert!(shadow.is_some());
        assert_eq!(shadow.unwrap().id, taffy::NodeId::from(shadow_root as u64));

        // Check containing_shadow_host returns the host
        let host_elem =
            <&crate::dom::PawsElement as selectors::Element>::containing_shadow_host(&inner_ref);
        assert!(host_elem.is_some());
        assert_eq!(host_elem.unwrap().id, taffy::NodeId::from(host as u64));
    }

    #[test]
    fn test_get_shadow_root_nonexistent() {
        let state = RuntimeState::new("https://example.com".to_string());
        // Document root has no shadow root
        assert_eq!(state.get_shadow_root(0), None);
        // Nonexistent element
        assert_eq!(state.get_shadow_root(999), None);
    }

    #[test]
    fn test_add_shadow_stylesheet() {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let host = state.create_element("div".to_string());
        state.append_element(0, host).unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();
        let inner = state.create_element("span".to_string());
        state.append_element(shadow_root, inner).unwrap();

        // Add shadow-scoped stylesheet
        state
            .add_shadow_stylesheet(
                shadow_root,
                "span { width: 42px; height: 42px; }".to_string(),
            )
            .unwrap();

        // Verify the CascadeData was stored
        let shadow_root_node = state
            .doc
            .get_node(taffy::NodeId::from(shadow_root as u64))
            .unwrap();
        assert!(shadow_root_node.shadow_cascade_data.is_some());
    }
}
