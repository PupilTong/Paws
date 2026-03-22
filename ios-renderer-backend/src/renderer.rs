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

        let needs_scroll_view =
            node.overflow_x == Overflow::Scroll || node.overflow_y == Overflow::Scroll;
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
