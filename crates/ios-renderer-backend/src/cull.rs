use crate::scroll::ScrollRegistry;
use crate::types::*;
use std::cell::RefCell;

thread_local! {
    static CULL_VEC_POOL: RefCell<Vec<Vec<CullNode<'static>>>> = const { RefCell::new(Vec::new()) };
}

pub fn take_cull_vec<'a>() -> Vec<CullNode<'a>> {
    CULL_VEC_POOL.with(|p| {
        let mut pool = p.borrow_mut();
        if let Some(v) = pool.pop() {
            unsafe { std::mem::transmute(v) }
        } else {
            Vec::with_capacity(8)
        }
    })
}

pub fn recycle_cull_vec<'a>(mut v: Vec<CullNode<'a>>) {
    for child in v.drain(..) {
        recycle_cull_vec(child.children);
    }
    CULL_VEC_POOL.with(|p| p.borrow_mut().push(unsafe { std::mem::transmute(v) }));
}

pub fn preallocate_pools(capacity: usize) {
    let mut vec = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        vec.push(Vec::with_capacity(8));
    }
    CULL_VEC_POOL.with(|p| p.borrow_mut().extend(vec));
}

pub struct CullNode<'a> {
    pub layout_node: &'a LayoutNode,
    pub absolute_frame: Rect,
    pub children: Vec<CullNode<'a>>,
}

pub struct Culler;

pub fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

impl Default for Culler {
    fn default() -> Self {
        Self::new()
    }
}

impl Culler {
    pub fn new() -> Self {
        Self
    }

    pub fn cull<'a>(
        &mut self,
        layout_tree: Option<&'a LayoutNode>,
        viewport: Rect,
        scroll_reg: &ScrollRegistry,
        prefetch_multiplier: f32,
    ) -> Option<CullNode<'a>> {
        let root = layout_tree?;

        let prefetch_rect = Rect {
            x: viewport.x - viewport.width * (prefetch_multiplier - 1.0) / 2.0,
            y: viewport.y - viewport.height * (prefetch_multiplier - 1.0) / 2.0,
            width: viewport.width * prefetch_multiplier,
            height: viewport.height * prefetch_multiplier,
        };

        // If any scroll node is dirty, we must visit all scroll nodes to update them.
        // Actually, we can just recursively check and build the tree.
        Self::cull_recursive(root, 0.0, 0.0, &prefetch_rect, scroll_reg)
    }

    fn cull_recursive<'a>(
        layout_node: &'a LayoutNode,
        mut absolute_offset_x: f32,
        mut absolute_offset_y: f32,
        prefetch_rect: &Rect,
        scroll_reg: &ScrollRegistry,
    ) -> Option<CullNode<'a>> {
        if layout_node.scroll.is_some() {
            scroll_reg.take_dirty(layout_node.id);
            let (sx, sy) = scroll_reg.get_offset(layout_node.id);
            absolute_offset_x -= sx;
            absolute_offset_y -= sy;
        }

        let abs_frame = Rect {
            x: layout_node.frame.x + absolute_offset_x,
            y: layout_node.frame.y + absolute_offset_y,
            width: layout_node.frame.width,
            height: layout_node.frame.height,
        };

        if !intersects(&abs_frame, prefetch_rect) {
            return None; // Culled!
        }

        let mut culled_children = take_cull_vec();
        for layout_child in &layout_node.children {
            if let Some(c_node) = Self::cull_recursive(
                layout_child,
                absolute_offset_x + layout_node.frame.x,
                absolute_offset_y + layout_node.frame.y,
                prefetch_rect,
                scroll_reg,
            ) {
                culled_children.push(c_node);
            }
        }

        Some(CullNode {
            layout_node,
            absolute_frame: abs_frame,
            children: culled_children,
        })
    }
}
