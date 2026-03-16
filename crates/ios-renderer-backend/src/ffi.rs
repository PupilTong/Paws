use crate::pipeline::RendererPipeline;
use crate::scroll::ScrollRegistry;
use crate::types::*;

pub struct RendererInstance {
    pipeline: RendererPipeline,
    layout_root: Option<LayoutNode>,
    scroll_registry: ScrollRegistry,
    pool_capacity: u32,
}

#[no_mangle]
pub extern "C" fn rb_create(layer_pool_capacity: u32) -> u64 {
    crate::cull::preallocate_pools(layer_pool_capacity as usize * 4);
    crate::layerize::preallocate_pools(layer_pool_capacity as usize * 4);
    crate::flatten::preallocate_pools(layer_pool_capacity as usize * 4);

    let instance = Box::new(RendererInstance {
        pipeline: RendererPipeline::new(layer_pool_capacity as usize),
        layout_root: None,
        scroll_registry: ScrollRegistry::new(),
        pool_capacity: layer_pool_capacity,
    });
    Box::into_raw(instance) as u64
}

#[no_mangle]
pub extern "C" fn rb_destroy(handle: u64) {
    if handle != 0 {
        let _ = unsafe { Box::from_raw(handle as *mut RendererInstance) };
    }
}

#[no_mangle]
pub extern "C" fn rb_render_frame(
    handle: u64,
    _timestamp_ns: u64,
    out_cmds: *mut LayerCmd,
    out_count: *mut u32,
) {
    if handle == 0 || out_cmds.is_null() || out_count.is_null() {
        return;
    }
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    // SAFETY: caller guarantees out_cmds points to an array of size pool_capacity
    let cmds_slice =
        unsafe { std::slice::from_raw_parts_mut(out_cmds, instance.pool_capacity as usize) };

    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 10000.0,
        height: 10000.0,
    };

    instance.pipeline.render_frame(
        instance.layout_root.as_ref(),
        viewport,
        &instance.scroll_registry,
        1.5,
        cmds_slice,
        unsafe { &mut *out_count },
    );
}

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
    let instance = unsafe { &*(handle as *const RendererInstance) };
    instance
        .scroll_registry
        .update_offset(scroll_id, offset_x, offset_y);
}

#[no_mangle]
pub extern "C" fn rb_submit_layout(handle: u64, root: *const LayoutNode, _node_count: u32) {
    if handle == 0 {
        return;
    }
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    if !root.is_null() {
        instance.layout_root = Some(unsafe { (*root).clone() });
    } else {
        instance.layout_root = None;
    }
}

#[no_mangle]
pub extern "C" fn rb_set_pool_capacity(handle: u64, capacity: u32) {
    if handle == 0 {
        return;
    }
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };
    instance.pool_capacity = capacity;
}
