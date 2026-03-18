//! `extern "C"` FFI surface consumed by Swift via a C bridging header.
//!
//! All functions use an opaque `u64` handle that wraps a heap-allocated
//! [`RendererInstance`]. Swift obtains a handle from [`rb_create`] and
//! must call [`rb_destroy`] when done.
//!
//! # Threading contract
//!
//! | Function                  | Thread |
//! |---------------------------|--------|
//! | `rb_create`               | Any    |
//! | `rb_destroy`              | Main   |
//! | `rb_render_frame`         | Main   |
//! | `rb_update_scroll_offset` | Main   |
//! | `rb_submit_layout`        | Main   |
//! | `rb_set_pool_capacity`    | Main   |

use crate::pipeline::RendererPipeline;
use crate::scroll::ScrollRegistry;
use crate::types::*;

/// Opaque renderer state, heap-allocated behind an FFI handle.
struct RendererInstance {
    pipeline: RendererPipeline,
    layout_root: Option<LayoutNode>,
    scroll_registry: ScrollRegistry,
    pool_capacity: u32,
}

/// Create a new renderer instance and return an opaque handle.
///
/// `layer_pool_capacity` controls pre-allocation for the command buffer
/// and internal vec pools. A typical value is 1024.
///
/// The returned handle must be passed to [`rb_destroy`] when no longer
/// needed.
#[no_mangle]
pub extern "C" fn rb_create(layer_pool_capacity: u32) -> u64 {
    let cap = layer_pool_capacity as usize;
    crate::cull::preallocate_pools(cap * 4);
    crate::layerize::preallocate_pools(cap * 4);
    crate::flatten::preallocate_pools(cap * 4);

    let instance = Box::new(RendererInstance {
        pipeline: RendererPipeline::new(cap),
        layout_root: None,
        scroll_registry: ScrollRegistry::new(),
        pool_capacity: layer_pool_capacity,
    });
    // SAFETY: We Box::into_raw to transfer ownership to the caller.
    // The caller must call rb_destroy to reclaim this allocation.
    Box::into_raw(instance) as u64
}

/// Destroy a renderer instance previously created with [`rb_create`].
///
/// Passing `0` is a no-op.
#[no_mangle]
pub extern "C" fn rb_destroy(handle: u64) {
    if handle != 0 {
        // SAFETY: The handle was produced by Box::into_raw in rb_create.
        // The caller guarantees this is the only outstanding reference and
        // that no further calls will be made with this handle.
        let _ = unsafe { Box::from_raw(handle as *mut RendererInstance) };
    }
}

/// Execute one frame of the rendering pipeline.
///
/// Writes [`LayerCmd`] values into the caller-allocated buffer at
/// `out_cmds` (which must have room for at least `pool_capacity` entries)
/// and sets `*out_count` to the number of commands written.
///
/// Called from the `CAMetalDisplayLink` callback on the main thread in
/// Swift.
///
/// # Safety
///
/// - `handle` must be a valid, non-zero handle from [`rb_create`].
/// - `out_cmds` must point to an array of at least `pool_capacity`
///   [`LayerCmd`] values.
/// - `out_count` must point to a valid `u32`.
#[no_mangle]
pub unsafe extern "C" fn rb_render_frame(
    handle: u64,
    _timestamp_ns: u64,
    out_cmds: *mut LayerCmd,
    out_count: *mut u32,
) {
    if handle == 0 || out_cmds.is_null() || out_count.is_null() {
        return;
    }
    // SAFETY: Caller guarantees handle is valid (from rb_create, not yet
    // destroyed). We have exclusive access on the main thread.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    // SAFETY: Caller guarantees out_cmds points to an array of at least
    // pool_capacity LayerCmd values.
    let cmds_slice =
        unsafe { std::slice::from_raw_parts_mut(out_cmds, instance.pool_capacity as usize) };

    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 10000.0,
        height: 10000.0,
    };

    // SAFETY: Caller guarantees out_count points to a valid u32.
    instance.pipeline.render_frame(
        instance.layout_root.as_ref(),
        viewport,
        &instance.scroll_registry,
        1.5,
        cmds_slice,
        unsafe { &mut *out_count },
    );
}

/// Update the scroll offset for a scroll container.
///
/// Called from `UIScrollViewDelegate.scrollViewDidScroll` in Swift.
/// May be called concurrently with rayon pool reads (lock-free atomic
/// update).
#[no_mangle]
pub extern "C" fn rb_update_scroll_offset(
    handle: u64,
    scroll_id: u64,
    offset_x: f32,
    offset_y: f32,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid. update_offset uses
    // atomic operations and is safe to call from the main thread while
    // the rayon pool reads offsets concurrently.
    let instance = unsafe { &*(handle as *const RendererInstance) };
    instance
        .scroll_registry
        .update_offset(scroll_id, offset_x, offset_y);
}

/// Submit a new layout tree for rendering.
///
/// The tree rooted at `root` is deep-cloned (O(n) in node count).
/// Passing a null `root` clears the current layout.
///
/// # Safety
///
/// - `handle` must be a valid, non-zero handle from [`rb_create`].
/// - `root`, if non-null, must point to a valid [`LayoutNode`].
#[no_mangle]
pub unsafe extern "C" fn rb_submit_layout(handle: u64, root: *const LayoutNode, _node_count: u32) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid and we have exclusive
    // main-thread access.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    if root.is_null() {
        instance.layout_root = None;
    } else {
        // SAFETY: Caller guarantees root points to a valid LayoutNode.
        instance.layout_root = Some(unsafe { (*root).clone() });
    }
}

/// Change the command buffer capacity for future frames.
#[no_mangle]
pub extern "C" fn rb_set_pool_capacity(handle: u64, capacity: u32) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid and we have exclusive
    // main-thread access.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };
    instance.pool_capacity = capacity;
}
