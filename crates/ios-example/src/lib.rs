//! Example app wiring `ios-renderer-backend` with a sample layout tree.
//!
//! Exposes a small `extern "C"` API that Swift can call to create a
//! renderer instance pre-loaded with a scrollable list of colored rows,
//! tick the pipeline each frame, and tear down.
//!
//! # FFI surface
//!
//! | Function                | Thread |
//! |-------------------------|--------|
//! | `example_create`        | Any    |
//! | `example_tick`          | Main   |
//! | `example_update_scroll` | Main   |
//! | `example_destroy`       | Main   |

use ios_renderer_backend::ffi;
use ios_renderer_backend::types::*;

/// Number of rows in the sample scrollable list.
const ROW_COUNT: u64 = 20;
/// Height of each row in points.
const ROW_HEIGHT: f32 = 80.0;
/// Viewport width (iPhone 14 logical width).
const VIEWPORT_W: f32 = 390.0;
/// Viewport height (iPhone 14 logical height).
const VIEWPORT_H: f32 = 844.0;
/// ScrollId for the main scroll container.
const SCROLL_ID: u64 = 2;
/// Default command buffer capacity.
const POOL_CAPACITY: u32 = 1024;

/// Build the sample layout tree: a root view containing a scrollable
/// list of `ROW_COUNT` colored rows.
fn build_sample_tree() -> LayoutNode {
    let total_content_h = ROW_COUNT as f32 * ROW_HEIGHT;

    let rows: Vec<LayoutNode> = (0..ROW_COUNT)
        .map(|i| {
            let hue = (i as f32) / (ROW_COUNT as f32);
            let (r, g, b) = hue_to_rgb(hue);

            LayoutNode {
                id: 100 + i,
                frame: Rect {
                    x: 0.0,
                    y: i as f32 * ROW_HEIGHT,
                    width: VIEWPORT_W,
                    height: ROW_HEIGHT,
                },
                children: vec![],
                scroll: None,
                style: ComputedStyle {
                    opacity: 1.0,
                    transform: None,
                    clip: None,
                    background: Color { r, g, b, a: 1.0 },
                    border_radius: 8.0,
                    will_change: false,
                },
                generation: 1,
            }
        })
        .collect();

    let scroll_container = LayoutNode {
        id: SCROLL_ID,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: VIEWPORT_W,
            height: VIEWPORT_H,
        },
        children: rows,
        scroll: Some(ScrollProps {
            content_size: Size {
                width: VIEWPORT_W,
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
                r: 0.95,
                g: 0.95,
                b: 0.97,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation: 1,
    };

    LayoutNode {
        id: 1,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: VIEWPORT_W,
            height: VIEWPORT_H,
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
    }
}

/// Convert a hue value `[0.0, 1.0]` to an RGB triple.
fn hue_to_rgb(h: f32) -> (f32, f32, f32) {
    let h6 = h * 6.0;
    let sector = h6 as u32;
    let frac = h6 - sector as f32;

    match sector % 6 {
        0 => (1.0, frac, 0.0),
        1 => (1.0 - frac, 1.0, 0.0),
        2 => (0.0, 1.0, frac),
        3 => (0.0, 1.0 - frac, 1.0),
        4 => (frac, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - frac),
    }
}

// ── FFI ──────────────────────────────────────────────────────────────

/// Create a renderer instance pre-loaded with the sample layout tree.
///
/// Returns an opaque handle that must be passed to [`example_destroy`]
/// when no longer needed.
#[no_mangle]
pub extern "C" fn example_create() -> u64 {
    let handle = ffi::rb_create(POOL_CAPACITY);
    let tree = build_sample_tree();

    // SAFETY: handle is valid (just created), tree is a valid LayoutNode.
    unsafe {
        ffi::rb_submit_layout(handle, &tree as *const LayoutNode, (ROW_COUNT + 2) as u32);
    }

    handle
}

/// Run one frame of the rendering pipeline.
///
/// Writes [`LayerCmd`] values into `out_cmds` and sets `*out_count` to
/// the number of commands produced.
///
/// # Safety
///
/// - `handle` must be a valid handle from [`example_create`].
/// - `out_cmds` must point to an array of at least 1024 [`LayerCmd`] values.
/// - `out_count` must point to a valid `u32`.
#[no_mangle]
pub unsafe extern "C" fn example_tick(
    handle: u64,
    timestamp_ns: u64,
    out_cmds: *mut LayerCmd,
    out_count: *mut u32,
) {
    // SAFETY: Caller guarantees handle, out_cmds, out_count are valid.
    unsafe {
        ffi::rb_render_frame(handle, timestamp_ns, out_cmds, out_count);
    }
}

/// Update the scroll offset for the main scroll container.
///
/// Called from `UIScrollViewDelegate.scrollViewDidScroll` in Swift.
#[no_mangle]
pub extern "C" fn example_update_scroll(handle: u64, offset_x: f32, offset_y: f32) {
    ffi::rb_update_scroll_offset(handle, SCROLL_ID, offset_x, offset_y);
}

/// Destroy a renderer instance previously created with [`example_create`].
///
/// Passing `0` is a no-op.
#[no_mangle]
pub extern "C" fn example_destroy(handle: u64) {
    ffi::rb_destroy(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_tree_has_expected_structure() {
        let tree = build_sample_tree();
        assert_eq!(tree.id, 1);
        assert_eq!(tree.children.len(), 1);

        let scroll = &tree.children[0];
        assert_eq!(scroll.id, SCROLL_ID);
        assert!(scroll.scroll.is_some());
        assert_eq!(scroll.children.len(), ROW_COUNT as usize);

        // Check first and last row IDs.
        assert_eq!(scroll.children[0].id, 100);
        assert_eq!(
            scroll.children[ROW_COUNT as usize - 1].id,
            100 + ROW_COUNT - 1
        );
    }

    #[test]
    fn hue_to_rgb_boundaries() {
        let (r, g, b) = hue_to_rgb(0.0);
        assert_eq!((r, g, b), (1.0, 0.0, 0.0)); // Red

        let (r, g, _b) = hue_to_rgb(1.0 / 3.0);
        assert!((g - 1.0).abs() < 0.01); // Green-ish
        assert!(r < 0.01);
    }
}
