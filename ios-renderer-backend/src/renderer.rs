//! LayoutBox tree walker that creates and updates UIKit views.
//!
//! [`ViewTree`] maintains a map from engine `NodeId`s to retained UIView
//! pointers. On each [`apply`](ViewTree::apply) call it walks the new
//! `LayoutBox` tree, reusing existing views where possible and creating
//! new ones as needed.

use std::ffi::c_void;

use engine::{LayoutBox, Overflow};
use fnv::FnvHashSet;

use crate::error::RendererError;
use crate::ffi::imports;

/// Tracks the mapping from engine node IDs to UIKit view pointers.
///
/// Retained views are released when they are removed from the tree or
/// when the `ViewTree` itself is dropped.
pub(crate) struct ViewTree {
    /// Maps engine `NodeId` (as `u64`) → retained `UIView*` (or subclass).
    view_map: fnv::FnvHashMap<u64, ViewEntry>,
}

/// An entry in the view map tracking the UIKit object type.
struct ViewEntry {
    /// Retained opaque pointer to the UIView (or subclass).
    ptr: *mut c_void,
    /// What kind of UIKit object this is.
    kind: ViewKind,
}

/// Discriminant for the type of UIKit view backing a node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewKind {
    View,
    ScrollView,
}

impl ViewTree {
    pub(crate) fn new() -> Self {
        Self {
            view_map: fnv::FnvHashMap::default(),
        }
    }

    /// Applies a `LayoutBox` tree to the UIKit view hierarchy under `root_view`.
    ///
    /// Creates, updates, or removes views as needed to match the layout tree.
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

        self.apply_node(layout, root_view, &mut visited)?;

        // Remove views for nodes no longer in the tree.
        self.view_map.retain(|id, entry| {
            if visited.contains(id) {
                true
            } else {
                // SAFETY: Removing from superview and releasing the retained pointer.
                unsafe {
                    imports::swift_paws_view_remove_from_superview(entry.ptr);
                    release_entry(entry);
                }
                false
            }
        });

        Ok(())
    }

    /// Recursively creates/updates a view for a `LayoutBox` node and its children.
    fn apply_node(
        &mut self,
        node: &LayoutBox,
        parent_view: *mut c_void,
        visited: &mut FnvHashSet<u64>,
    ) -> Result<(), RendererError> {
        let node_key = u64::from(node.node_id);
        visited.insert(node_key);

        let needs_scroll_view = matches!(node.overflow_x, Overflow::Scroll | Overflow::Auto)
            || matches!(node.overflow_y, Overflow::Scroll | Overflow::Auto);
        let desired_kind = if needs_scroll_view {
            ViewKind::ScrollView
        } else {
            ViewKind::View
        };

        // Get or create the view for this node.
        let view_ptr = match self.view_map.get(&node_key) {
            Some(entry) if entry.kind == desired_kind => entry.ptr,
            Some(_) => {
                // Kind changed (e.g. view became scroll view) — recreate.
                let old = self.view_map.remove(&node_key).unwrap();
                // SAFETY: Removing stale view and releasing its pointer.
                unsafe {
                    imports::swift_paws_view_remove_from_superview(old.ptr);
                    release_entry(&old);
                }
                create_view(desired_kind, &mut self.view_map, node_key)?
            }
            None => create_view(desired_kind, &mut self.view_map, node_key)?,
        };

        // Update frame.
        // SAFETY: view_ptr is a valid retained UIView.
        unsafe {
            imports::swift_paws_view_set_frame(view_ptr, node.x, node.y, node.width, node.height);
        }

        // Update clips-to-bounds based on overflow.
        let clips = node.overflow_x == Overflow::Hidden
            || node.overflow_x == Overflow::Clip
            || node.overflow_y == Overflow::Hidden
            || node.overflow_y == Overflow::Clip;
        // SAFETY: view_ptr is a valid retained UIView.
        unsafe {
            imports::swift_paws_view_set_clips_to_bounds(view_ptr, clips);
        }

        // If scroll view, update content size.
        if needs_scroll_view {
            // Content size is the bounding box of all children.
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
            // SAFETY: view_ptr is a valid retained UIScrollView.
            unsafe {
                imports::swift_paws_scroll_view_set_content_size(view_ptr, content_w, content_h);
            }
        }

        // Sort children by z-index for correct stacking order.
        let mut sorted_children: Vec<&LayoutBox> = node.children.iter().collect();
        sorted_children.sort_unstable_by_key(|c| c.z_index.unwrap_or(0));

        // Recurse into children.
        for child in &sorted_children {
            self.apply_node(child, view_ptr, visited)?;
        }

        // Ensure this view is attached to the parent.
        // SAFETY: Both pointers are valid retained UIViews.
        unsafe {
            imports::swift_paws_view_add_subview(parent_view, view_ptr);
        }

        Ok(())
    }
}

impl Drop for ViewTree {
    fn drop(&mut self) {
        for entry in self.view_map.values() {
            // SAFETY: Releasing all retained UIView pointers.
            unsafe {
                imports::swift_paws_view_remove_from_superview(entry.ptr);
                release_entry(entry);
            }
        }
    }
}

/// Creates a new UIKit view of the specified kind and inserts it into the map.
fn create_view(
    kind: ViewKind,
    map: &mut fnv::FnvHashMap<u64, ViewEntry>,
    node_key: u64,
) -> Result<*mut c_void, RendererError> {
    // SAFETY: Calling Swift create functions which return retained pointers.
    let ptr = unsafe {
        match kind {
            ViewKind::View => imports::swift_paws_view_create(),
            ViewKind::ScrollView => imports::swift_paws_scroll_view_create(),
        }
    };
    if ptr.is_null() {
        return Err(RendererError::CallbackFailed);
    }
    map.insert(node_key, ViewEntry { ptr, kind });
    Ok(ptr)
}

/// Releases a view entry's retained pointer via the appropriate Swift callback.
///
/// # Safety
///
/// `entry.ptr` must be a valid retained pointer matching `entry.kind`.
unsafe fn release_entry(entry: &ViewEntry) {
    match entry.kind {
        ViewKind::View => imports::swift_paws_view_release(entry.ptr),
        ViewKind::ScrollView => imports::swift_paws_scroll_view_release(entry.ptr),
    }
}

#[cfg(test)]
impl ViewTree {
    /// Returns the number of tracked views.
    fn len(&self) -> usize {
        self.view_map.len()
    }

    /// Returns `true` if a view is tracked for the given node ID.
    fn contains_node(&self, node_id: u64) -> bool {
        self.view_map.contains_key(&node_id)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use engine::{LayoutBox, Overflow};

    use super::ViewTree;
    use crate::error::RendererError;
    use crate::ffi::imports::stubs::{clear_call_log, take_call_log, FfiCall};

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

        // Should create a UIView.
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

        // Should set clipsToBounds to false (overflow is Visible).
        assert!(log.contains(&FfiCall::ViewSetClipsToBounds {
            ptr: created_ptr,
            clips: false,
        }));

        // Should add as subview of root.
        assert!(log.contains(&FfiCall::ViewAddSubview {
            parent: root_view(),
            child: created_ptr,
        }));
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
    fn test_scroll_view_for_overflow_scroll() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut node = layout(1);
        node.overflow_x = Overflow::Scroll;
        node.children = vec![layout_with_frame(2, 0.0, 0.0, 50.0, 50.0)];

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();

        // Root node should be a ScrollView, not a plain View.
        assert!(
            log.iter()
                .any(|c| matches!(c, FfiCall::ScrollViewCreate { .. })),
            "overflow:scroll should create a UIScrollView"
        );

        // Content size should be set.
        let scroll_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::ScrollViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .unwrap();
        assert!(log.contains(&FfiCall::ScrollViewSetContentSize {
            ptr: scroll_ptr,
            w: 50.0,
            h: 50.0,
        }));
    }

    #[test]
    fn test_scroll_view_content_size_calculation() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut node = layout(1);
        node.overflow_x = Overflow::Scroll;
        node.children = vec![
            layout_with_frame(2, 0.0, 0.0, 50.0, 50.0),
            layout_with_frame(3, 60.0, 0.0, 40.0, 100.0),
        ];

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        let scroll_ptr = log
            .iter()
            .find_map(|c| match c {
                FfiCall::ScrollViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .unwrap();

        // Content size = bounding box: max(50, 60+40)=100 x max(50, 100)=100.
        assert!(log.contains(&FfiCall::ScrollViewSetContentSize {
            ptr: scroll_ptr,
            w: 100.0,
            h: 100.0,
        }));
    }

    #[test]
    fn test_kind_change_recreates_view() {
        clear_call_log();
        let mut tree = ViewTree::new();

        // First: plain View.
        let node = layout(1);
        tree.apply(&node, root_view()).unwrap();
        let first_log = take_call_log();
        let old_ptr = match &first_log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };

        // Second: change to ScrollView.
        clear_call_log();
        let mut node2 = layout(1);
        node2.overflow_x = Overflow::Scroll;
        tree.apply(&node2, root_view()).unwrap();

        let second_log = take_call_log();

        // Old view should be removed and released.
        assert!(
            second_log.contains(&FfiCall::ViewRemoveFromSuperview { ptr: old_ptr }),
            "old view should be removed from superview"
        );
        assert!(
            second_log.contains(&FfiCall::ViewRelease { ptr: old_ptr }),
            "old view should be released"
        );

        // New scroll view should be created.
        assert!(
            second_log
                .iter()
                .any(|c| matches!(c, FfiCall::ScrollViewCreate { .. })),
            "new scroll view should be created"
        );
    }

    #[test]
    fn test_stale_view_pruning() {
        clear_call_log();
        let mut tree = ViewTree::new();

        // Apply tree with nodes 1, 2, 3.
        let mut root = layout(1);
        root.children = vec![layout(2), layout(3)];
        tree.apply(&root, root_view()).unwrap();
        assert_eq!(tree.len(), 3);

        // Capture node 3's pointer.
        let first_log = take_call_log();
        let view_creates: Vec<*mut c_void> = first_log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        // Views are created in DFS order: node 1, then node 2 (sorted child), then node 3.
        assert_eq!(view_creates.len(), 3);
        let node3_ptr = view_creates[2];

        // Apply tree with only nodes 1, 2 — node 3 is stale.
        clear_call_log();
        let mut root2 = layout(1);
        root2.children = vec![layout(2)];
        tree.apply(&root2, root_view()).unwrap();

        assert_eq!(tree.len(), 2);
        assert!(!tree.contains_node(3));

        let second_log = take_call_log();
        assert!(
            second_log.contains(&FfiCall::ViewRemoveFromSuperview { ptr: node3_ptr }),
            "stale view should be removed from superview"
        );
        assert!(
            second_log.contains(&FfiCall::ViewRelease { ptr: node3_ptr }),
            "stale view should be released"
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

        // Collect ViewCreate calls to identify child pointers.
        // The root view is created first, then children are created in
        // z-index order: node 3 (z=1), node 4 (z=2), node 2 (z=3).
        let creates: Vec<*mut c_void> = log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        assert_eq!(creates.len(), 4); // root + 3 children

        let root_ptr = creates[0];

        // Children are added as subviews in z-index sorted order.
        // After root is created, children are processed in z-index order.
        // Each child is added to root_ptr via ViewAddSubview.
        let subview_adds: Vec<(*mut c_void, *mut c_void)> = log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewAddSubview { parent, child } if *parent == root_ptr => {
                    Some((*parent, *child))
                }
                _ => None,
            })
            .collect();
        assert_eq!(subview_adds.len(), 3, "3 children should be added to root");
    }

    #[test]
    fn test_clips_to_bounds_hidden() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut node = layout(1);
        node.overflow_x = Overflow::Hidden;

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        let ptr = match &log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };
        assert!(log.contains(&FfiCall::ViewSetClipsToBounds { ptr, clips: true }));
    }

    #[test]
    fn test_clips_to_bounds_clip() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut node = layout(1);
        node.overflow_y = Overflow::Clip;

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        let ptr = match &log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };
        assert!(log.contains(&FfiCall::ViewSetClipsToBounds { ptr, clips: true }));
    }

    #[test]
    fn test_clips_to_bounds_visible() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let node = layout(1); // default overflow is Visible

        tree.apply(&node, root_view()).unwrap();

        let log = take_call_log();
        let ptr = match &log[0] {
            FfiCall::ViewCreate { ret } => *ret,
            other => panic!("expected ViewCreate, got {other:?}"),
        };
        assert!(log.contains(&FfiCall::ViewSetClipsToBounds { ptr, clips: false }));
    }

    #[test]
    fn test_drop_releases_all_views() {
        clear_call_log();
        let mut tree = ViewTree::new();
        let mut root = layout(1);
        root.children = vec![layout(2), layout(3)];
        tree.apply(&root, root_view()).unwrap();

        let setup_log = take_call_log();
        let ptrs: Vec<*mut c_void> = setup_log
            .iter()
            .filter_map(|c| match c {
                FfiCall::ViewCreate { ret } => Some(*ret),
                _ => None,
            })
            .collect();
        assert_eq!(ptrs.len(), 3);

        // Drop the tree.
        clear_call_log();
        drop(tree);

        let drop_log = take_call_log();

        // Every view should be removed from superview and released.
        for ptr in &ptrs {
            assert!(
                drop_log.contains(&FfiCall::ViewRemoveFromSuperview { ptr: *ptr }),
                "view {ptr:?} should be removed from superview on drop"
            );
            assert!(
                drop_log.contains(&FfiCall::ViewRelease { ptr: *ptr }),
                "view {ptr:?} should be released on drop"
            );
        }
    }
}
