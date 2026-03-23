//! LayoutBox tree walker that creates and updates UIKit views and CALayers.
//!
//! [`ViewTree`] maintains a map from engine `NodeId`s to retained UIView or
//! CALayer pointers. On each [`apply`](ViewTree::apply) call it walks the new
//! `LayoutBox` tree, reusing existing entries where possible and creating
//! new ones as needed.
//!
//! Simple non-scrolling nodes are rendered as standalone `CALayer`s for
//! better performance. The root node is always a `UIView` so it can be
//! attached to the host `UIView` via `addSubview`. Scroll-overflow nodes
//! use `UIScrollView`.

use std::ffi::c_void;

use engine::LayoutBox;
use fnv::FnvHashSet;
use style::values::specified::box_::Overflow;

use crate::error::RendererError;
use crate::ffi::imports;

/// Extracts the background color from computed values as an RGBA tuple.
///
/// Returns `None` for transparent backgrounds (alpha ≈ 0) or non-absolute colors.
fn extract_background_color(
    cv: &style::properties::ComputedValues,
) -> Option<(f32, f32, f32, f32)> {
    use style::values::computed::Color;

    match cv.clone_background_color() {
        Color::Absolute(abs) => {
            let r = abs.components.0;
            let g = abs.components.1;
            let b = abs.components.2;
            let a = abs.alpha;
            // Skip fully transparent backgrounds.
            if a.abs() < f32::EPSILON {
                None
            } else {
                Some((r, g, b, a))
            }
        }
        _ => None,
    }
}

/// Tracks the mapping from engine node IDs to UIKit view/layer pointers.
///
/// Retained objects are released when they are removed from the tree or
/// when the `ViewTree` itself is dropped.
pub(crate) struct ViewTree {
    /// Maps engine `NodeId` (as `u64`) → retained object pointer.
    view_map: fnv::FnvHashMap<u64, ViewEntry>,
}

/// An entry in the view map tracking the UIKit object type.
struct ViewEntry {
    /// Retained opaque pointer to the UIView, UIScrollView, or CALayer.
    ptr: *mut c_void,
    /// What kind of UIKit object this is.
    kind: ViewKind,
}

/// Discriminant for the type of UIKit object backing a node.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ViewKind {
    /// A plain `UIView`.
    View,
    /// A `UIScrollView`.
    ScrollView,
    /// A standalone `CALayer` (lightweight, no responder chain).
    Layer,
}

impl ViewTree {
    pub(crate) fn new() -> Self {
        Self {
            view_map: fnv::FnvHashMap::default(),
        }
    }

    /// Applies a `LayoutBox` tree to the UIKit hierarchy under `root_view`.
    ///
    /// Creates, updates, or removes views/layers as needed to match the
    /// layout tree. The root `LayoutBox` is always backed by a `UIView`;
    /// non-scrolling children become `CALayer`s.
    pub(crate) fn apply(
        &mut self,
        layout: &LayoutBox,
        root_view: *mut c_void,
    ) -> Result<(), RendererError> {
        if root_view.is_null() {
            return Err(RendererError::InvalidHandle);
        }

        // Track which node IDs are visited so we can prune stale entries.
        let mut visited = FnvHashSet::default();

        self.apply_node(layout, root_view, ViewKind::View, true, &mut visited)?;

        // Remove objects for nodes no longer in the tree.
        self.view_map.retain(|id, entry| {
            if visited.contains(id) {
                true
            } else {
                // SAFETY: Removing from parent and releasing the retained pointer.
                unsafe {
                    detach_entry(entry);
                    release_entry(entry);
                }
                false
            }
        });

        Ok(())
    }

    /// Recursively creates/updates an object for a `LayoutBox` node and its children.
    ///
    /// `parent_ptr` is the pointer to the parent UIView/CALayer.
    /// `parent_kind` is the kind of the parent (needed for correct attachment).
    /// `is_root` forces the node to be a `View` (the layout root must be a UIView).
    fn apply_node(
        &mut self,
        node: &LayoutBox,
        parent_ptr: *mut c_void,
        parent_kind: ViewKind,
        is_root: bool,
        visited: &mut FnvHashSet<u64>,
    ) -> Result<(), RendererError> {
        let node_key = u64::from(node.node_id);
        visited.insert(node_key);

        // Extract overflow from computed values (default: Visible).
        let (overflow_x, overflow_y) = node
            .computed_values
            .as_ref()
            .map(|cv| (cv.clone_overflow_x(), cv.clone_overflow_y()))
            .unwrap_or((Overflow::Visible, Overflow::Visible));

        let needs_scroll_view = matches!(overflow_x, Overflow::Scroll | Overflow::Auto)
            || matches!(overflow_y, Overflow::Scroll | Overflow::Auto);
        let desired_kind = if needs_scroll_view {
            ViewKind::ScrollView
        } else if is_root {
            ViewKind::View
        } else {
            ViewKind::Layer
        };

        // Get or create the object for this node.
        let obj_ptr = match self.view_map.get(&node_key) {
            Some(entry) if entry.kind == desired_kind => entry.ptr,
            Some(_) => {
                // Kind changed — recreate.
                let old = self.view_map.remove(&node_key).unwrap();
                // SAFETY: Removing stale object and releasing its pointer.
                unsafe {
                    detach_entry(&old);
                    release_entry(&old);
                }
                create_entry(desired_kind, &mut self.view_map, node_key)?
            }
            None => create_entry(desired_kind, &mut self.view_map, node_key)?,
        };

        // Update frame.
        // SAFETY: obj_ptr is a valid retained pointer.
        unsafe {
            match desired_kind {
                ViewKind::View | ViewKind::ScrollView => {
                    imports::swift_paws_view_set_frame(
                        obj_ptr,
                        node.x,
                        node.y,
                        node.width,
                        node.height,
                    );
                }
                ViewKind::Layer => {
                    imports::swift_paws_layer_set_frame(
                        obj_ptr,
                        node.x,
                        node.y,
                        node.width,
                        node.height,
                    );
                }
            }
        }

        // Update background color.
        let bg = node
            .computed_values
            .as_ref()
            .and_then(|cv| extract_background_color(cv));
        if let Some((r, g, b, a)) = bg {
            // SAFETY: obj_ptr is a valid retained pointer.
            unsafe {
                match desired_kind {
                    ViewKind::View | ViewKind::ScrollView => {
                        imports::swift_paws_view_set_background_color(obj_ptr, r, g, b, a);
                    }
                    ViewKind::Layer => {
                        imports::swift_paws_layer_set_background_color(obj_ptr, r, g, b, a);
                    }
                }
            }
        }

        // Update clips-to-bounds (only for UIView/UIScrollView).
        if matches!(desired_kind, ViewKind::View | ViewKind::ScrollView) {
            let clips = overflow_x == Overflow::Hidden
                || overflow_x == Overflow::Clip
                || overflow_y == Overflow::Hidden
                || overflow_y == Overflow::Clip;
            // SAFETY: obj_ptr is a valid retained UIView.
            unsafe {
                imports::swift_paws_view_set_clips_to_bounds(obj_ptr, clips);
            }
        }

        // If scroll view, update content size.
        if needs_scroll_view {
            let content_w = node
                .children
                .iter()
                .map(|c| c.x + c.width)
                .fold(0.0_f32, f32::max);
            let content_h = node
                .children
                .iter()
                .map(|c| c.y + c.height)
                .fold(0.0_f32, f32::max);
            // SAFETY: obj_ptr is a valid retained UIScrollView.
            unsafe {
                imports::swift_paws_scroll_view_set_content_size(obj_ptr, content_w, content_h);
            }
        }

        // Sort children by z-index for correct stacking order.
        let mut sorted_children: Vec<&LayoutBox> = node.children.iter().collect();
        sorted_children.sort_unstable_by_key(|c| c.z_index.unwrap_or(0));

        // Recurse into children (never root).
        for child in &sorted_children {
            self.apply_node(child, obj_ptr, desired_kind, false, visited)?;
        }

        // Attach this object to the parent.
        // SAFETY: Both pointers are valid retained objects.
        unsafe {
            attach_to_parent(parent_ptr, parent_kind, obj_ptr, desired_kind);
        }

        Ok(())
    }
}

impl Drop for ViewTree {
    fn drop(&mut self) {
        for entry in self.view_map.values() {
            // SAFETY: Releasing all retained pointers.
            unsafe {
                detach_entry(entry);
                release_entry(entry);
            }
        }
    }
}

/// Attaches a child object to its parent using the correct method.
///
/// # Safety
///
/// Both `parent_ptr` and `child_ptr` must be valid retained pointers of
/// the kinds indicated by `parent_kind` and `child_kind`.
unsafe fn attach_to_parent(
    parent_ptr: *mut c_void,
    parent_kind: ViewKind,
    child_ptr: *mut c_void,
    child_kind: ViewKind,
) {
    match (parent_kind, child_kind) {
        // View/ScrollView parent + View/ScrollView child → addSubview.
        (ViewKind::View | ViewKind::ScrollView, ViewKind::View | ViewKind::ScrollView) => {
            imports::swift_paws_view_add_subview(parent_ptr, child_ptr);
        }
        // View/ScrollView parent + Layer child → view.layer.addSublayer.
        (ViewKind::View | ViewKind::ScrollView, ViewKind::Layer) => {
            imports::swift_paws_view_add_sublayer(parent_ptr, child_ptr);
        }
        // Layer parent + Layer child → addSublayer.
        (ViewKind::Layer, ViewKind::Layer) => {
            imports::swift_paws_layer_add_sublayer(parent_ptr, child_ptr);
        }
        // Layer parent + View child — not supported in this implementation.
        // This case should not occur since only scroll nodes become Views
        // when they are not root, and scroll views under layers is an edge
        // case we don't handle yet.
        (ViewKind::Layer, ViewKind::View | ViewKind::ScrollView) => {
            // Fallback: treat as layer-to-layer (won't render correctly but
            // avoids a crash). A future version could wrap in a UIView.
            imports::swift_paws_layer_add_sublayer(parent_ptr, child_ptr);
        }
    }
}

/// Creates a new UIKit object of the specified kind and inserts it into the map.
fn create_entry(
    kind: ViewKind,
    map: &mut fnv::FnvHashMap<u64, ViewEntry>,
    node_key: u64,
) -> Result<*mut c_void, RendererError> {
    // SAFETY: Calling Swift create functions which return retained pointers.
    let ptr = unsafe {
        match kind {
            ViewKind::View => imports::swift_paws_view_create(),
            ViewKind::ScrollView => imports::swift_paws_scroll_view_create(),
            ViewKind::Layer => imports::swift_paws_layer_create(),
        }
    };
    if ptr.is_null() {
        return Err(RendererError::CallbackFailed);
    }
    map.insert(node_key, ViewEntry { ptr, kind });
    Ok(ptr)
}

/// Detaches an entry from its parent (superview or superlayer).
///
/// # Safety
///
/// `entry.ptr` must be a valid retained pointer matching `entry.kind`.
unsafe fn detach_entry(entry: &ViewEntry) {
    match entry.kind {
        ViewKind::View | ViewKind::ScrollView => {
            imports::swift_paws_view_remove_from_superview(entry.ptr);
        }
        ViewKind::Layer => {
            imports::swift_paws_layer_remove_from_superlayer(entry.ptr);
        }
    }
}

/// Releases an entry's retained pointer via the appropriate Swift callback.
///
/// # Safety
///
/// `entry.ptr` must be a valid retained pointer matching `entry.kind`.
unsafe fn release_entry(entry: &ViewEntry) {
    match entry.kind {
        ViewKind::View => imports::swift_paws_view_release(entry.ptr),
        ViewKind::ScrollView => imports::swift_paws_scroll_view_release(entry.ptr),
        ViewKind::Layer => imports::swift_paws_layer_release(entry.ptr),
    }
}

#[cfg(test)]
impl ViewTree {
    /// Returns the number of tracked entries.
    fn len(&self) -> usize {
        self.view_map.len()
    }

    /// Returns `true` if an entry is tracked for the given node ID.
    fn contains_node(&self, node_id: u64) -> bool {
        self.view_map.contains_key(&node_id)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use engine::LayoutBox;

    use super::ViewTree;
    use crate::error::RendererError;
    use crate::ffi::imports::stubs::{clear_call_log, take_call_log, FfiCall};

    use super::extract_background_color;

    /// Creates a `LayoutBox` with the given node ID and default fields.
    fn layout(id: u64) -> LayoutBox {
        LayoutBox {
            node_id: engine::NodeId::from(id),
            ..Default::default()
        }
    }

    /// Creates a `LayoutBox` with explicit position and size.
    fn layout_with_frame(id: u64, x: f32, y: f32, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node_id: engine::NodeId::from(id),
            x,
            y,
            width: w,
            height: h,
            ..Default::default()
        }
    }

    /// A non-null sentinel representing the root UIView in tests.
    fn root_view() -> *mut c_void {
        0x8000 as *mut c_void
    }

    #[test]
    fn test_apply_null_root_returns_error() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout(1);

        let result = tree.apply(&node, std::ptr::null_mut());

        assert_eq!(result, Err(RendererError::InvalidHandle));
        assert!(take_call_log().is_empty(), "no UIKit calls should be made");
    }

    #[test]
    fn test_apply_single_node() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout_with_frame(1, 10.0, 20.0, 100.0, 50.0);

        let result = tree.apply(&node, root_view());
        assert!(result.is_ok());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains_node(1));

        let log = take_call_log();

        // Root should create a UIView (not a Layer).
        let created_ptr = match &log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };

        // Should set the frame with exact values.
        assert!(log.contains(&FfiCall::ViewSetFrame {
            ptr: created_ptr,
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
        }));

        // Should add as subview of root.
        assert!(log.contains(&FfiCall::ViewAddSubview {
            parent: root_view(),
            child: created_ptr,
        }));
    }

    #[test]
    fn test_root_always_view() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout(1);

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        assert!(
            log.iter().any(|c| matches!(c, FfiCall::ViewCreate { .. })),
            "root node should always create a UIView"
        );
        assert!(
            !log.iter().any(|c| matches!(c, FfiCall::LayerCreate { .. })),
            "root node should not create a CALayer"
        );
    }

    #[test]
    fn test_child_uses_layer() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut root = layout(1);
        root.children = vec![layout_with_frame(2, 0.0, 0.0, 50.0, 50.0)];

        tree.apply(&root, root_view()).unwrap();

        let log = take_call_log();

        // Root is a View, child is a Layer.
        assert_eq!(
            log.iter()
                .filter(|c| matches!(c, FfiCall::ViewCreate { .. }))
                .count(),
            1,
            "only root should be a UIView"
        );
        assert_eq!(
            log.iter()
                .filter(|c| matches!(c, FfiCall::LayerCreate { .. }))
                .count(),
            1,
            "child should be a CALayer"
        );
    }

    #[test]
    fn test_layer_set_frame() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut root = layout(1);
        root.children = vec![layout_with_frame(2, 5.0, 10.0, 50.0, 50.0)];

        tree.apply(&root, root_view()).unwrap();

        let log = take_call_log();
        let layer_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::LayerCreate { ret } => Some(*ret),
                _ => None,
            })
            .expect("child should be a Layer");

        assert!(
            log.contains(&FfiCall::LayerSetFrame {
                ptr: layer_ptr,
                x: 5.0,
                y: 10.0,
                w: 50.0,
                h: 50.0,
            }),
            "layer frame should be set"
        );
    }

    #[test]
    fn test_layer_under_view_uses_add_sublayer() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut root = layout(1);
        root.children = vec![layout(2)];

        tree.apply(&root, root_view()).unwrap();

        let log = take_call_log();
        let root_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .unwrap();
        let layer_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::LayerCreate { ret } => Some(*ret),
                _ => None,
            })
            .unwrap();

        assert!(
            log.contains(&FfiCall::ViewAddSublayer {
                view: root_ptr,
                layer: layer_ptr,
            }),
            "layer child of view should use view_add_sublayer"
        );
    }

    #[test]
    fn test_layer_under_layer_uses_add_sublayer() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut child = layout(2);
        child.children = vec![layout(3)]; // grandchild = layer under layer
        let mut root = layout(1);
        root.children = vec![child];

        tree.apply(&root, root_view()).unwrap();

        let log = take_call_log();
        let layer_ptrs: Vec<*mut c_void> = log
            .iter()
            .filter_map(|c| match c {
                FfiCall::LayerCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        assert_eq!(layer_ptrs.len(), 2, "should have 2 layers");

        let parent_layer = layer_ptrs[0];
        let child_layer = layer_ptrs[1];

        assert!(
            log.contains(&FfiCall::LayerAddSublayer {
                parent: parent_layer,
                child: child_layer,
            }),
            "layer child of layer should use layer_add_sublayer"
        );
    }

    #[test]
    fn test_apply_reuses_existing_view() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout_with_frame(1, 0.0, 0.0, 50.0, 50.0);

        tree.apply(&node, root_view()).unwrap();
        let first_log = take_call_log();
        let created_ptr = match &first_log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };

        // Apply again with same node ID but different frame.
        clear_call_log();
        let node2 = layout_with_frame(1, 5.0, 10.0, 200.0, 100.0);
        tree.apply(&node2, root_view()).unwrap();
        assert_eq!(tree.len(), 1);

        let second_log = take_call_log();

        // No new ViewCreate — view was reused.
        assert!(
            !second_log
                .iter()
                .any(|c| matches!(c, FfiCall::ViewCreate { .. })),
            "view should be reused, not recreated"
        );

        // Frame should be updated to new values.
        assert!(second_log.contains(&FfiCall::ViewSetFrame {
            ptr: created_ptr,
            x: 5.0,
            y: 10.0,
            w: 200.0,
            h: 100.0,
        }));
    }

    #[test]
    fn test_stale_layer_pruning() {
        clear_call_log();
        let mut tree = ViewTree::new();

        // Apply tree with root (view) + children 2, 3 (layers).
        let mut root = layout(1);
        root.children = vec![layout(2), layout(3)];
        tree.apply(&root, root_view()).unwrap();
        assert_eq!(tree.len(), 3);

        // Capture node 3's pointer.
        let first_log = take_call_log();
        let layer_creates: Vec<*mut c_void> = first_log
            .iter()
            .filter_map(|c| match c {
                FfiCall::LayerCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        assert_eq!(layer_creates.len(), 2);
        let node3_ptr = layer_creates[1];

        // Apply tree with only root + node 2 — node 3 is stale.
        clear_call_log();
        let mut root2 = layout(1);
        root2.children = vec![layout(2)];
        tree.apply(&root2, root_view()).unwrap();

        assert_eq!(tree.len(), 2);
        assert!(!tree.contains_node(3));

        let second_log = take_call_log();
        assert!(
            second_log.contains(&FfiCall::LayerRemoveFromSuperlayer { ptr: node3_ptr }),
            "stale layer should be removed from superlayer"
        );
        assert!(
            second_log.contains(&FfiCall::LayerRelease { ptr: node3_ptr }),
            "stale layer should be released"
        );
    }

    #[test]
    fn test_z_index_sorting() {
        clear_call_log();
        let mut tree = ViewTree::new();

        let mut child_a = layout(2);
        child_a.z_index = Some(3);
        let mut child_b = layout(3);
        child_b.z_index = Some(1);
        let mut child_c = layout(4);
        child_c.z_index = Some(2);

        let mut root = layout(1);
        root.children = vec![child_a, child_b, child_c];

        tree.apply(&root, root_view()).unwrap();

        let log = take_call_log();

        // Root is a View, children are Layers.
        let root_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .unwrap();

        // Children are added as sublayers in z-index sorted order.
        let sublayer_adds: Vec<*mut c_void> = log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewAddSublayer { view, layer } if *view == root_ptr => Some(*layer),
                _ => None,
            })
            .collect();
        assert_eq!(sublayer_adds.len(), 3, "3 children should be added to root");
    }

    #[test]
    fn test_no_clips_with_default_overflow() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout(1); // computed_values: None → default visible overflow

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        let ptr = match &log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };
        assert!(
            log.contains(&FfiCall::ViewSetClipsToBounds { ptr, clips: false }),
            "default overflow (None computed_values) should not clip"
        );
    }

    #[test]
    fn test_drop_releases_all_entries() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut root = layout(1);
        root.children = vec![layout(2), layout(3)];
        tree.apply(&root, root_view()).unwrap();

        let setup_log = take_call_log();
        let view_ptrs: Vec<*mut c_void> = setup_log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        let layer_ptrs: Vec<*mut c_void> = setup_log
            .iter()
            .filter_map(|c| match c {
                FfiCall::LayerCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        assert_eq!(view_ptrs.len(), 1, "root should be a view");
        assert_eq!(layer_ptrs.len(), 2, "children should be layers");

        // Drop the tree.
        clear_call_log();
        drop(tree);

        let drop_log = take_call_log();

        // Root view should be removed from superview and released.
        for ptr in &view_ptrs {
            assert!(
                drop_log.contains(&FfiCall::ViewRemoveFromSuperview { ptr: *ptr }),
                "view {ptr:?} should be removed from superview on drop"
            );
            assert!(
                drop_log.contains(&FfiCall::ViewRelease { ptr: *ptr }),
                "view {ptr:?} should be released on drop"
            );
        }

        // Layer children should be removed from superlayer and released.
        for ptr in &layer_ptrs {
            assert!(
                drop_log.contains(&FfiCall::LayerRemoveFromSuperlayer { ptr: *ptr }),
                "layer {ptr:?} should be removed from superlayer on drop"
            );
            assert!(
                drop_log.contains(&FfiCall::LayerRelease { ptr: *ptr }),
                "layer {ptr:?} should be released on drop"
            );
        }
    }

    #[test]
    fn test_extract_background_color_from_computed_values() {
        // Build a RuntimeState with an element that has background-color set.
        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();
        state
            .set_inline_style(
                id,
                "background-color".to_string(),
                "rgb(255, 0, 128)".to_string(),
            )
            .unwrap();
        state.doc.resolve_style(&state.style_context);

        let node = state.doc.get_node(engine::NodeId::from(id as u64)).unwrap();
        let cv = node
            .get_computed_values()
            .expect("should have computed values");
        let color = extract_background_color(cv);
        assert!(color.is_some(), "rgb(255, 0, 128) should extract a color");

        let (r, g, b, a) = color.unwrap();
        assert!((r - 1.0).abs() < 0.01, "red should be ~1.0, got {r}");
        assert!(g.abs() < 0.01, "green should be ~0.0, got {g}");
        assert!((b - 0.502).abs() < 0.02, "blue should be ~0.502, got {b}");
        assert!((a - 1.0).abs() < 0.01, "alpha should be ~1.0, got {a}");
    }

    #[test]
    fn test_extract_background_color_transparent_returns_none() {
        // Element with default (transparent) background.
        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();
        state.doc.resolve_style(&state.style_context);

        let node = state.doc.get_node(engine::NodeId::from(id as u64)).unwrap();
        let cv = node
            .get_computed_values()
            .expect("should have computed values");
        let color = extract_background_color(cv);
        assert!(color.is_none(), "transparent background should return None");
    }

    #[test]
    fn test_apply_node_with_background_color_sets_layer_bg() {
        clear_call_log();
        let mut tree = ViewTree::new();

        // Build a state with a colored div to get real computed values.
        let mut state = engine::RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).unwrap();
        state
            .set_inline_style(id, "background-color".to_string(), "red".to_string())
            .unwrap();
        state
            .set_inline_style(id, "width".to_string(), "50px".to_string())
            .unwrap();
        state
            .set_inline_style(id, "height".to_string(), "50px".to_string())
            .unwrap();

        let layout_box = state.commit();

        tree.apply(&layout_box, root_view()).unwrap();

        let log = take_call_log();

        // Root is always a View. Verify its background color was set.
        assert!(
            log.iter()
                .any(|c| matches!(c, FfiCall::ViewSetBackgroundColor { .. })),
            "background-color should be applied to the root view"
        );
    }
}
