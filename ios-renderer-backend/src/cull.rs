//! Stage 1 — Viewport culling with prefetch region.
//!
//! Walks the [`LayoutNode`] tree and produces a [`CullNode`] tree
//! containing only the nodes whose absolute frames intersect the
//! expanded prefetch rectangle. Scroll offsets are read from the
//! [`ScrollRegistry`] with `Acquire` ordering.

use crate::scroll::ScrollRegistry;
use crate::types::*;
use std::cell::RefCell;

// ── Thread-local vec pool ─────────────────────────────────────────────

thread_local! {
    static CULL_VEC_POOL: RefCell<Vec<Vec<CullNode<'static>>>> =
        const { RefCell::new(Vec::new()) };
}

/// Take a recycled `Vec` from the thread-local pool, or allocate a new one.
pub(crate) fn take_cull_vec<'a>() -> Vec<CullNode<'a>> {
    CULL_VEC_POOL.with(|p| {
        let mut pool = p.borrow_mut();
        if let Some(v) = pool.pop() {
            // SAFETY: The vec was drained (empty) before being returned to the
            // pool, so it contains no live references. Transmuting the lifetime
            // parameter on an empty Vec is sound.
            unsafe { std::mem::transmute::<Vec<CullNode<'_>>, Vec<CullNode<'_>>>(v) }
        } else {
            Vec::with_capacity(8)
        }
    })
}

/// Drain and return a `Vec` to the thread-local pool for reuse.
pub(crate) fn recycle_cull_vec(mut v: Vec<CullNode<'_>>) {
    for child in v.drain(..) {
        recycle_cull_vec(child.children);
    }
    // SAFETY: The vec has been fully drained above, so it holds no live
    // references. Transmuting the now-irrelevant lifetime to 'static on
    // an empty Vec is sound.
    CULL_VEC_POOL.with(|p| {
        p.borrow_mut()
            .push(unsafe { std::mem::transmute::<Vec<CullNode<'_>>, Vec<CullNode<'_>>>(v) });
    });
}

/// Pre-fill the thread-local pool with empty vecs to avoid allocation
/// during the first few frames.
pub(crate) fn preallocate_pools(capacity: usize) {
    CULL_VEC_POOL.with(|p| {
        let mut pool = p.borrow_mut();
        for _ in 0..capacity {
            pool.push(Vec::with_capacity(8));
        }
    });
}

// ── Types ─────────────────────────────────────────────────────────────

/// A node that survived culling, with an absolute screen-space frame.
pub(crate) struct CullNode<'a> {
    pub(crate) layout_node: &'a LayoutNode,
    pub(crate) absolute_frame: Rect,
    pub(crate) children: Vec<CullNode<'a>>,
}

// ── Culler ─────────────────────────────────────────────────────────────

/// Stateless culling pass. Call [`Culler::cull`] each frame.
pub(crate) struct Culler;

impl Default for Culler {
    fn default() -> Self {
        Self::new()
    }
}

impl Culler {
    pub(crate) fn new() -> Self {
        Self
    }

    /// Run the cull pass over the layout tree.
    ///
    /// `prefetch_multiplier` expands the viewport (default `1.5`) so that
    /// nodes just outside the visible area are kept for smooth scrolling.
    pub(crate) fn cull<'a>(
        &mut self,
        layout_tree: Option<&'a LayoutNode>,
        viewport: Rect,
        scroll_reg: &ScrollRegistry,
        prefetch_multiplier: f32,
    ) -> Option<CullNode<'a>> {
        let root = layout_tree?;

        let expand_x = viewport.width * (prefetch_multiplier - 1.0) / 2.0;
        let expand_y = viewport.height * (prefetch_multiplier - 1.0) / 2.0;
        let prefetch_rect = Rect {
            x: viewport.x - expand_x,
            y: viewport.y - expand_y,
            width: viewport.width * prefetch_multiplier,
            height: viewport.height * prefetch_multiplier,
        };

        Self::cull_recursive(root, 0.0, 0.0, &prefetch_rect, scroll_reg)
    }

    fn cull_recursive<'a>(
        node: &'a LayoutNode,
        abs_x: f32,
        abs_y: f32,
        prefetch_rect: &Rect,
        scroll_reg: &ScrollRegistry,
    ) -> Option<CullNode<'a>> {
        let absolute_frame = Rect {
            x: node.frame.x + abs_x,
            y: node.frame.y + abs_y,
            width: node.frame.width,
            height: node.frame.height,
        };

        if !intersects(&absolute_frame, prefetch_rect) {
            return None;
        }

        // Compute the absolute offset for children. If this node is a
        // scroll container, subtract its scroll offset so children shift
        // in screen space. The container itself is unaffected.
        let mut child_abs_x = abs_x + node.frame.x;
        let mut child_abs_y = abs_y + node.frame.y;
        if node.scroll.is_some() {
            scroll_reg.take_dirty(node.id);
            let (sx, sy) = scroll_reg.get_offset(node.id);
            child_abs_x -= sx;
            child_abs_y -= sy;
        }

        let mut culled_children = take_cull_vec();
        for child in &node.children {
            if let Some(c) =
                Self::cull_recursive(child, child_abs_x, child_abs_y, prefetch_rect, scroll_reg)
            {
                culled_children.push(c);
            }
        }

        Some(CullNode {
            layout_node: node,
            absolute_frame,
            children: culled_children,
        })
    }
}

/// AABB intersection test.
fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scroll::ScrollRegistry;

    fn leaf(id: NodeId, x: f32, y: f32, w: f32, h: f32) -> LayoutNode {
        LayoutNode {
            id,
            frame: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            children: vec![],
            scroll: None,
            style: ComputedStyle {
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
            },
            generation: 1,
        }
    }

    #[test]
    fn node_outside_viewport_is_culled() {
        let root = LayoutNode {
            children: vec![leaf(2, 500.0, 500.0, 50.0, 50.0)],
            ..leaf(1, 0.0, 0.0, 100.0, 100.0)
        };
        let reg = ScrollRegistry::new();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let mut culler = Culler::new();
        let result = culler.cull(Some(&root), viewport, &reg, 1.0).unwrap();
        assert!(
            result.children.is_empty(),
            "off-screen child should be culled"
        );
    }

    #[test]
    fn node_inside_viewport_is_kept() {
        let root = LayoutNode {
            children: vec![leaf(2, 10.0, 10.0, 50.0, 50.0)],
            ..leaf(1, 0.0, 0.0, 100.0, 100.0)
        };
        let reg = ScrollRegistry::new();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let mut culler = Culler::new();
        let result = culler.cull(Some(&root), viewport, &reg, 1.0).unwrap();
        assert_eq!(result.children.len(), 1);
    }

    #[test]
    fn prefetch_expands_visible_region() {
        // Child is at (220, 0) with size 50x50, viewport is 200x200.
        // With 1.0 multiplier right edge = 200, child starts at 220 → culled.
        // With 1.5 multiplier prefetch right edge = 250, child starts at 220 → kept.
        let root = LayoutNode {
            children: vec![leaf(2, 220.0, 0.0, 50.0, 50.0)],
            ..leaf(1, 0.0, 0.0, 400.0, 400.0)
        };
        let reg = ScrollRegistry::new();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let mut culler = Culler::new();

        let narrow = culler.cull(Some(&root), viewport, &reg, 1.0).unwrap();
        assert!(narrow.children.is_empty());

        let wide = culler.cull(Some(&root), viewport, &reg, 1.5).unwrap();
        assert_eq!(wide.children.len(), 1);
    }

    #[test]
    fn empty_tree_returns_none() {
        let reg = ScrollRegistry::new();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mut culler = Culler::new();
        assert!(culler.cull(None, viewport, &reg, 1.5).is_none());
    }

    #[test]
    fn scroll_offset_shifts_children() {
        let child = leaf(2, 10.0, 10.0, 50.0, 50.0);
        let mut root = leaf(1, 0.0, 0.0, 100.0, 100.0);
        root.scroll = Some(ScrollProps {
            content_size: Size {
                width: 500.0,
                height: 500.0,
            },
            overflow_x: Overflow::Scroll,
            overflow_y: Overflow::Scroll,
        });
        root.children = vec![child];

        let mut reg = ScrollRegistry::new();
        reg.insert(
            1,
            10,
            None,
            Size {
                width: 500.0,
                height: 500.0,
            },
            0.0,
            0.0,
        );
        // Scroll down by 200 — the child at (10,10) moves to (10, -190) in screen space
        reg.update_offset(1, 0.0, 200.0);

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mut culler = Culler::new();
        let result = culler.cull(Some(&root), viewport, &reg, 1.0).unwrap();
        // Child should be culled because it's at y=-190 which is above viewport
        assert!(result.children.is_empty());
    }
}
