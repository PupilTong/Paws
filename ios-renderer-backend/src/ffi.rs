//! `extern "C"` FFI surface consumed by Swift via a C bridging header.
//!
//! All functions use an opaque `u64` handle that wraps a heap-allocated
//! [`RendererInstance`]. Swift obtains a handle from [`rb_create`] and
//! must call [`rb_destroy`] when done.
//!
//! # Threading contract
//!
//! | Function                    | Thread |
//! |-----------------------------|--------|
//! | `rb_create`                 | Any    |
//! | `rb_destroy`                | Main   |
//! | `rb_render_frame`           | Main   |
//! | `rb_update_scroll_offset`   | Main   |
//! | `rb_submit_layout`          | Main   |
//! | `rb_set_pool_capacity`      | Main   |
//! | `rb_set_render_callback`    | Main   |
//! | `rb_trigger_render`         | Main   |
//! | `rb_run_wasm_app`           | Main   |

use crate::pipeline::RendererPipeline;
use crate::scroll::ScrollRegistry;
use crate::types::*;

/// Callback type for push-model rendering.
///
/// Swift registers a callback via [`rb_set_render_callback`]. When the
/// renderer has new commands, it invokes this callback with the command
/// buffer, count, and the opaque user_data pointer.
pub type RenderCallback = Option<unsafe extern "C" fn(*const LayerCmd, u32, *mut std::ffi::c_void)>;

/// Opaque renderer state, heap-allocated behind an FFI handle.
struct RendererInstance {
    pipeline: RendererPipeline,
    layout_root: Option<LayoutNode>,
    scroll_registry: ScrollRegistry,
    pool_capacity: u32,
    /// Push-model callback and associated data.
    render_callback: RenderCallback,
    callback_user_data: *mut std::ffi::c_void,
    /// Owned command buffer for push-model rendering (avoids caller allocation).
    cmd_buffer: Vec<LayerCmd>,
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
        render_callback: None,
        callback_user_data: std::ptr::null_mut(),
        cmd_buffer: Vec::with_capacity(cap),
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

/// Submit a built-in demo layout tree: a scrollable list of colored rows.
///
/// Useful for verifying the Swift rendering pipeline without needing a
/// separate WASM module or Rust crate to construct layout trees.
///
/// Submit a demo layout that mirrors `demo.wat`:
///
/// - Root div (full viewport, transparent bg)
/// - Container div (full viewport, scroll, content height = 4 × 50)
/// - 4 rows (50×50pt) with colours from `demo.wat`:
///   - `rgb(255,100,100)` — red
///   - `rgb(100,255,100)` — green
///   - `rgb(100,100,255)` — blue
///   - `rgb(255,255,100)` — yellow
#[no_mangle]
pub extern "C" fn rb_submit_demo_layout(handle: u64, viewport_w: f32, viewport_h: f32) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid and we have exclusive
    // main-thread access.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    let row_size: f32 = 50.0;
    // Matches demo.wat: rgb(255,100,100), rgb(100,255,100), rgb(100,100,255), rgb(255,255,100)
    const C: f32 = 100.0 / 255.0; // ≈ 0.392
    let colors: [(f32, f32, f32); 4] = [
        (1.0, C, C),   // red
        (C, 1.0, C),   // green
        (C, C, 1.0),   // blue
        (1.0, 1.0, C), // yellow
    ];
    let row_count = colors.len();
    let total_content_h = row_count as f32 * row_size;

    let rows: Vec<LayoutNode> = colors
        .iter()
        .enumerate()
        .map(|(i, &(r, g, b))| LayoutNode {
            id: 100 + i as u64,
            frame: Rect {
                x: 0.0,
                y: i as f32 * row_size,
                width: row_size,
                height: row_size,
            },
            children: vec![],
            scroll: None,
            style: ComputedStyle {
                opacity: 1.0,
                transform: None,
                clip: None,
                background: Color { r, g, b, a: 1.0 },
                border_radius: 0.0,
                will_change: false,
            },
            generation: 1,
        })
        .collect();

    let scroll_container = LayoutNode {
        id: 2,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_w,
            height: viewport_h,
        },
        children: rows,
        scroll: Some(ScrollProps {
            content_size: Size {
                width: viewport_w,
                height: total_content_h,
            },
            overflow_x: Overflow::Hidden,
            overflow_y: Overflow::Scroll,
        }),
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation: 1,
    };

    let root = LayoutNode {
        id: 1,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: viewport_w,
            height: viewport_h,
        },
        children: vec![scroll_container],
        scroll: None,
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation: 1,
    };

    instance.layout_root = Some(root);
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

// ── Push-model API ──────────────────────────────────────────────────

/// Set the callback that will be invoked whenever Rust wants to push
/// a new set of layer commands to Swift.
///
/// Pass `None` (null function pointer) to clear the callback.
#[no_mangle]
pub extern "C" fn rb_set_render_callback(
    handle: u64,
    callback: RenderCallback,
    user_data: *mut std::ffi::c_void,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid and we have exclusive
    // main-thread access.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };
    instance.render_callback = callback;
    instance.callback_user_data = user_data;
}

/// Trigger a frame render and push the resulting commands via callback.
///
/// If `render_callback` is not set, this does nothing.
#[no_mangle]
pub extern "C" fn rb_trigger_render(handle: u64) {
    if handle == 0 {
        return;
    }
    // SAFETY: Caller guarantees handle is valid and we have exclusive
    // main-thread access.
    let instance = unsafe { &mut *(handle as *mut RendererInstance) };

    let callback = match instance.render_callback {
        Some(cb) => cb,
        None => return,
    };

    // Ensure the buffer has enough capacity.
    let cap = instance.pool_capacity as usize;
    if instance.cmd_buffer.len() < cap {
        instance
            .cmd_buffer
            .resize_with(cap, || LayerCmd::RemoveLayer { id: 0 });
    }

    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 10000.0,
        height: 10000.0,
    };

    let mut count: u32 = 0;
    instance.pipeline.render_frame(
        instance.layout_root.as_ref(),
        viewport,
        &instance.scroll_registry,
        1.5,
        &mut instance.cmd_buffer,
        &mut count,
    );

    if count > 0 {
        // SAFETY: callback is a valid function pointer set by the caller.
        // cmd_buffer is valid for `count` elements. user_data was provided
        // by the caller and is their responsibility.
        unsafe {
            callback(
                instance.cmd_buffer.as_ptr(),
                count,
                instance.callback_user_data,
            );
        }
    }
}

/// Runs a WASM application and submits the resulting layout to the
/// renderer pipeline.
///
/// The WASM module must export a `run` function that returns `i32`
/// (0 on success). The module can use host functions like
/// `__CreateElement`, `__SetInlineStyle`, `__AppendElement`, and
/// `__AddStylesheet` to build a DOM tree.
///
/// After execution, styles are resolved, layout is computed, and
/// the result is submitted to the renderer. If a callback is set,
/// a render is triggered automatically.
///
/// Returns 0 on success, or a negative error code on failure.
/// Returns -3 when compiled without the `wasm` feature.
///
/// # Safety
///
/// - `handle` must be a valid, non-zero handle from [`rb_create`].
/// - `wasm_bytes` must point to `wasm_len` bytes of valid WASM or WAT.
#[no_mangle]
pub unsafe extern "C" fn rb_run_wasm_app(
    handle: u64,
    wasm_bytes: *const u8,
    wasm_len: usize,
) -> i32 {
    #[cfg(feature = "wasm")]
    {
        if handle == 0 || wasm_bytes.is_null() {
            return -1;
        }

        // SAFETY: Caller guarantees handle is valid.
        let instance = unsafe { &mut *(handle as *mut RendererInstance) };

        // SAFETY: Caller guarantees wasm_bytes points to wasm_len valid bytes.
        let bytes = unsafe { std::slice::from_raw_parts(wasm_bytes, wasm_len) };

        match run_wasm_app_inner(instance, bytes) {
            Ok(()) => 0,
            Err(_) => -2,
        }
    }

    #[cfg(not(feature = "wasm"))]
    {
        let _ = (handle, wasm_bytes, wasm_len);
        -3 // WASM support not compiled in
    }
}

/// Inner implementation for [`rb_run_wasm_app`], using `anyhow::Result`
/// for ergonomic error handling.
#[cfg(feature = "wasm")]
fn run_wasm_app_inner(instance: &mut RendererInstance, wasm_bytes: &[u8]) -> anyhow::Result<()> {
    use engine::RuntimeState;
    use wasmtime::{Module, Store};

    // 1. Compile the WASM module.
    //    `create_engine()` returns a Pulley-configured engine on iOS
    //    (JIT is forbidden) and the default Cranelift engine elsewhere.
    let wasm_engine = wasm_bridge::create_engine();
    let module = Module::new(&wasm_engine, wasm_bytes)?;

    // 2. Create runtime state and instantiate.
    let mut store = Store::new(
        &wasm_engine,
        RuntimeState::new("https://paws.local".to_string()),
    );
    let linker = wasm_bridge::build_linker(&wasm_engine);
    let wasm_instance = linker.instantiate(&mut store, &module)?;

    // 3. Call the "run" export. The return value is ignored — WASM modules
    //    may return an element ID or a status code depending on convention.
    let run = wasm_instance.get_typed_func::<(), i32>(&mut store, "run")?;
    let _status = run.call(&mut store, ())?;

    // 4. Resolve styles.
    let state = store.data_mut();
    state.doc.resolve_style(&state.style_context);

    // 5. Compute layout tree.
    let mut layout_state = engine::layout::LayoutState::new();
    // Root element is the first child of the document root (node 0).
    let root_id = state
        .doc
        .get_node(0)
        .and_then(|doc_root| doc_root.children.first().copied())
        .unwrap_or(0);

    let layout_tree = layout_state
        .compute_layout_tree(&state.doc, root_id, &engine::layout::MockTextMeasurer)
        .ok_or_else(|| anyhow::anyhow!("failed to compute layout tree"))?;

    // 6. Convert to renderer's LayoutNode.
    let layout_node = crate::convert::layout_box_to_layout_node(&state.doc, &layout_tree, 1);

    // 7. Submit to renderer.
    instance.layout_root = Some(layout_node);

    // 8. Trigger render if callback is set.
    if instance.render_callback.is_some() {
        // Re-borrow through the handle pattern to satisfy borrow checker.
        // We know this is safe because we have exclusive access.
        let handle = instance as *mut RendererInstance as u64;
        rb_trigger_render(handle);
    }

    Ok(())
}
