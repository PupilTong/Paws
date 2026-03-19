//! Stage 2 — Layer promotion (layerization).
//!
//! Determines which [`CullNode`]s require their own `CALayer` /
//! `UIScrollView` on the Swift side. Subtrees are processed in parallel
//! via [`rayon::scope`].

use crate::cull::CullNode;
use crate::types::*;
use std::sync::{Mutex, OnceLock};

// ── Global vec pools (cross-thread via Mutex) ─────────────────────────

fn layerize_pool() -> &'static Mutex<Vec<Vec<LayerizeNode<'static>>>> {
    static POOL: OnceLock<Mutex<Vec<Vec<LayerizeNode<'static>>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

fn temp_pool() -> &'static Mutex<Vec<Vec<std::mem::MaybeUninit<LayerizeNode<'static>>>>> {
    static POOL: OnceLock<Mutex<Vec<Vec<std::mem::MaybeUninit<LayerizeNode<'static>>>>>> =
        OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

/// Take a recycled `Vec` from the global pool, or allocate a new one.
pub(crate) fn take_layerize_vec<'a>() -> Vec<LayerizeNode<'a>> {
    let mut pool = layerize_pool().lock().unwrap();
    if let Some(v) = pool.pop() {
        // SAFETY: The vec was drained (empty) before being returned to the
        // pool, so it contains no live references. Transmuting the lifetime
        // parameter on an empty Vec is sound.
        unsafe { std::mem::transmute::<Vec<LayerizeNode<'_>>, Vec<LayerizeNode<'_>>>(v) }
    } else {
        Vec::with_capacity(8)
    }
}

/// Drain and return a `Vec` to the global pool for reuse.
pub(crate) fn recycle_layerize_vec(mut v: Vec<LayerizeNode<'_>>) {
    for child in v.drain(..) {
        recycle_layerize_vec(child.children);
    }
    // SAFETY: The vec has been fully drained above, so it holds no live
    // references. Transmuting the now-irrelevant lifetime to 'static on
    // an empty Vec is sound.
    layerize_pool()
        .lock()
        .unwrap()
        .push(unsafe { std::mem::transmute::<Vec<LayerizeNode<'_>>, Vec<LayerizeNode<'_>>>(v) });
}

fn take_temp_vec<'a>() -> Vec<std::mem::MaybeUninit<LayerizeNode<'a>>> {
    let mut pool = temp_pool().lock().unwrap();
    if let Some(v) = pool.pop() {
        // SAFETY: Same as take_layerize_vec — empty vec, lifetime is irrelevant.
        unsafe {
            std::mem::transmute::<
                Vec<std::mem::MaybeUninit<LayerizeNode<'_>>>,
                Vec<std::mem::MaybeUninit<LayerizeNode<'_>>>,
            >(v)
        }
    } else {
        Vec::with_capacity(8)
    }
}

fn recycle_temp_vec(mut v: Vec<std::mem::MaybeUninit<LayerizeNode<'_>>>) {
    v.clear();
    // SAFETY: Same as recycle_layerize_vec — empty vec, lifetime is irrelevant.
    temp_pool().lock().unwrap().push(unsafe {
        std::mem::transmute::<
            Vec<std::mem::MaybeUninit<LayerizeNode<'_>>>,
            Vec<std::mem::MaybeUninit<LayerizeNode<'_>>>,
        >(v)
    });
}

/// Pre-fill the global pools to avoid allocation on first frames.
pub(crate) fn preallocate_pools(capacity: usize) {
    {
        let mut pool = layerize_pool().lock().unwrap();
        for _ in 0..capacity {
            pool.push(Vec::with_capacity(8));
        }
    }
    {
        let mut pool = temp_pool().lock().unwrap();
        for _ in 0..capacity {
            pool.push(Vec::with_capacity(8));
        }
    }
}

// ── Types ─────────────────────────────────────────────────────────────

/// A layerized node: wraps a [`CullNode`] with a flag indicating whether
/// it requires its own native layer.
pub(crate) struct LayerizeNode<'a> {
    pub(crate) cull: &'a CullNode<'a>,
    pub(crate) is_layer: bool,
    pub(crate) children: Vec<LayerizeNode<'a>>,
}

// ── Logic ─────────────────────────────────────────────────────────────

/// Determine whether `node` must receive its own `CALayer` / `UIScrollView`.
///
/// A node is promoted when any of these conditions hold:
/// 1. `scroll.is_some()` → becomes `UIScrollView`
/// 2. Has a `clip` differing from parent bounds
/// 3. Has a non-identity `transform`
/// 4. Has `opacity < 1.0` (independent compositing)
/// 5. `will_change == true`
pub(crate) fn needs_layer(node: &LayoutNode, parent_frame: Option<Rect>) -> bool {
    if node.scroll.is_some() {
        return true;
    }
    if let Some(clip) = node.style.clip {
        if parent_frame != Some(clip) {
            return true;
        }
    }
    if let Some(t) = node.style.transform {
        if t != Transform3D::default() {
            return true;
        }
    }
    if node.style.opacity < 1.0 {
        return true;
    }
    node.style.will_change
}

/// Run the layerize pass, processing subtrees in parallel via rayon.
pub(crate) fn run_layerize<'a>(
    cull: &'a CullNode<'a>,
    parent_frame: Option<Rect>,
) -> LayerizeNode<'a> {
    let is_layer = needs_layer(cull.layout_node, parent_frame);
    let my_frame = cull.absolute_frame;

    let mut children = take_layerize_vec();
    if cull.children.is_empty() {
        return LayerizeNode {
            cull,
            is_layer,
            children,
        };
    }

    // Use MaybeUninit + rayon::scope to write results in parallel without
    // requiring the output type to be Send (it borrows non-Send CullNode refs).
    let mut temp = take_temp_vec();
    if temp.capacity() < cull.children.len() {
        temp.reserve(cull.children.len());
    }
    // SAFETY: We are setting the length to match the number of children we
    // will initialize in the rayon scope below. Each slot is written to
    // exactly once by a single spawned task. After the scope completes,
    // all slots are guaranteed to be initialized.
    unsafe {
        temp.set_len(cull.children.len());
    }

    let ptr_val = temp.as_mut_ptr() as usize;

    rayon::scope(|s| {
        for (i, child) in cull.children.iter().enumerate() {
            s.spawn(move |_| {
                // SAFETY: Each task writes to a unique index `i` in the
                // pre-sized `temp` array. No two tasks share an index.
                // The pointer is derived from a valid `Vec` allocation that
                // outlives the rayon scope.
                let p = ptr_val as *mut std::mem::MaybeUninit<LayerizeNode>;
                let res = run_layerize(child, Some(my_frame));
                unsafe { p.add(i).write(std::mem::MaybeUninit::new(res)) };
            });
        }
    });

    for t in temp.drain(..) {
        // SAFETY: All elements were initialized in the rayon scope above.
        children.push(unsafe { t.assume_init() });
    }
    recycle_temp_vec(temp);

    LayerizeNode {
        cull,
        is_layer,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_style() -> ComputedStyle {
        ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        }
    }

    fn make_layout(id: NodeId, style: ComputedStyle) -> LayoutNode {
        LayoutNode {
            id,
            frame: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            children: vec![],
            scroll: None,
            style,
            generation: 1,
        }
    }

    #[test]
    fn plain_node_does_not_need_layer() {
        let node = make_layout(1, simple_style());
        assert!(!needs_layer(&node, None));
    }

    #[test]
    fn scroll_node_needs_layer() {
        let mut node = make_layout(1, simple_style());
        node.scroll = Some(ScrollProps {
            content_size: Size {
                width: 500.0,
                height: 500.0,
            },
            overflow_x: Overflow::Scroll,
            overflow_y: Overflow::Scroll,
        });
        assert!(needs_layer(&node, None));
    }

    #[test]
    fn opacity_below_one_needs_layer() {
        let mut style = simple_style();
        style.opacity = 0.5;
        let node = make_layout(1, style);
        assert!(needs_layer(&node, None));
    }

    #[test]
    fn will_change_needs_layer() {
        let mut style = simple_style();
        style.will_change = true;
        let node = make_layout(1, style);
        assert!(needs_layer(&node, None));
    }

    #[test]
    fn non_identity_transform_needs_layer() {
        let mut style = simple_style();
        let mut t = Transform3D::default();
        t.m[12] = 10.0; // translate X
        style.transform = Some(t);
        let node = make_layout(1, style);
        assert!(needs_layer(&node, None));
    }

    #[test]
    fn identity_transform_does_not_need_layer() {
        let mut style = simple_style();
        style.transform = Some(Transform3D::default());
        let node = make_layout(1, style);
        assert!(!needs_layer(&node, None));
    }
}
