//! Stage 3 — Flatten (bottom-up merge).
//!
//! Walks the layerized tree bottom-up, merging non-layer nodes into their
//! nearest qualifying ancestor's layer. Assigns monotonically increasing
//! `z_index` values within each layer to preserve painter's order.

use crate::layerize::LayerizeNode;
use crate::types::*;
use std::cell::RefCell;

// ── Thread-local vec pool ─────────────────────────────────────────────

thread_local! {
    static LAYERIZED_POOL: RefCell<Vec<Vec<LayerizedNode>>> =
        const { RefCell::new(Vec::new()) };
}

fn take_layerized_vec() -> Vec<LayerizedNode> {
    LAYERIZED_POOL.with(|p| {
        p.borrow_mut()
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(8))
    })
}

/// Drain and return a `Vec<LayerizedNode>` to the pool for reuse.
pub(crate) fn recycle_layerized_vec(mut v: Vec<LayerizedNode>) {
    for child in v.drain(..) {
        recycle_layerized_vec(child.children);
    }
    LAYERIZED_POOL.with(|p| p.borrow_mut().push(v));
}

/// Pre-fill the pool.
pub(crate) fn preallocate_pools(capacity: usize) {
    LAYERIZED_POOL.with(|p| {
        let mut pool = p.borrow_mut();
        for _ in 0..capacity {
            pool.push(Vec::with_capacity(8));
        }
    });
}

// ── Intermediate result ───────────────────────────────────────────────

struct FlattenPassResult {
    layered_node: Option<LayerizedNode>,
    bubbling_descendants: Vec<LayerizedNode>,
}

// ── Public entry point ────────────────────────────────────────────────

/// Run the flatten pass, producing a [`LayerizedTree`] snapshot.
pub(crate) fn run_flatten(layerize_tree: &LayerizeNode) -> LayerizedTree {
    let mut res = flatten_bottom_up(layerize_tree);

    if let Some(root) = res.layered_node.take() {
        LayerizedTree { root: Some(root) }
    } else if let Some(mut first) = res.bubbling_descendants.into_iter().next() {
        first.parent_id = None;
        first.parent_kind = None;
        LayerizedTree { root: Some(first) }
    } else {
        LayerizedTree { root: None }
    }
}

// ── Recursive flatten ─────────────────────────────────────────────────

fn flatten_bottom_up(node: &LayerizeNode) -> FlattenPassResult {
    let ln = node.cull.layout_node;

    let kind = if ln.scroll.is_some() {
        LayerKind::ScrollView
    } else {
        LayerKind::View
    };

    let parent_kind_for_children = if kind == LayerKind::ScrollView {
        ParentKind::ScrollView
    } else {
        ParentKind::Layer
    };

    // Collect all children's bubbling layers.
    let mut all_bubbling = take_layerized_vec();

    for child in &node.children {
        let mut child_res = flatten_bottom_up(child);

        if let Some(child_layer) = child_res.layered_node {
            all_bubbling.push(child_layer);
        }
        all_bubbling.append(&mut child_res.bubbling_descendants);
        recycle_layerized_vec(child_res.bubbling_descendants);
    }

    if node.is_layer || ln.id == 1 {
        // This node gets its own layer — absorb all bubbling descendants.
        for (z_index, child_layer) in all_bubbling.iter_mut().enumerate() {
            child_layer.z_index = z_index as u32;
            child_layer.parent_id = Some(ln.id);
            child_layer.parent_kind = Some(parent_kind_for_children);
        }

        let props = LayerProps {
            frame: node.cull.absolute_frame,
            opacity: ln.style.opacity,
            background: ln.style.background,
            border_radius: ln.style.border_radius,
            has_transform: ln.style.transform.is_some(),
            transform: ln.style.transform.unwrap_or_default(),
            has_clip: ln.style.clip.is_some(),
            clip: ln.style.clip.unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
        };

        let scroll_size = ln.scroll.as_ref().map(|s| s.content_size);

        let layered = LayerizedNode {
            id: ln.id,
            kind,
            parent_id: None,
            parent_kind: None,
            z_index: 0,
            props,
            scroll_content_size: scroll_size,
            generation: ln.generation,
            children: all_bubbling,
        };

        FlattenPassResult {
            layered_node: Some(layered),
            bubbling_descendants: take_layerized_vec(),
        }
    } else {
        // Not a layer — bubble everything up.
        FlattenPassResult {
            layered_node: None,
            bubbling_descendants: all_bubbling,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cull::CullNode;

    fn simple_layout(id: NodeId) -> LayoutNode {
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
    fn single_layer_root() {
        let layout = simple_layout(1);
        let cull = CullNode {
            layout_node: &layout,
            absolute_frame: layout.frame,
            children: vec![],
        };
        let layerize = LayerizeNode {
            cull: &cull,
            is_layer: true,
            children: vec![],
        };

        let tree = run_flatten(&layerize);
        let root = tree.root.unwrap();
        assert_eq!(root.id, 1);
        assert_eq!(root.kind, LayerKind::View);
        assert!(root.children.is_empty());
    }

    #[test]
    fn non_layer_children_merge_into_parent() {
        let parent_layout = simple_layout(1);
        let child_layout = simple_layout(2);

        let child_cull = CullNode {
            layout_node: &child_layout,
            absolute_frame: child_layout.frame,
            children: vec![],
        };
        let parent_cull = CullNode {
            layout_node: &parent_layout,
            absolute_frame: parent_layout.frame,
            children: vec![child_cull],
        };

        let child_layerize = LayerizeNode {
            cull: &parent_cull.children[0],
            is_layer: false,
            children: vec![],
        };
        let parent_layerize = LayerizeNode {
            cull: &parent_cull,
            is_layer: true,
            children: vec![child_layerize],
        };

        let tree = run_flatten(&parent_layerize);
        let root = tree.root.unwrap();
        assert_eq!(root.id, 1);
        // Non-layer child doesn't produce its own LayerizedNode
        assert!(root.children.is_empty());
    }

    #[test]
    fn scroll_node_produces_scroll_view_kind() {
        let mut layout = simple_layout(1);
        layout.scroll = Some(ScrollProps {
            content_size: Size {
                width: 500.0,
                height: 500.0,
            },
            overflow_x: Overflow::Scroll,
            overflow_y: Overflow::Scroll,
        });

        let cull = CullNode {
            layout_node: &layout,
            absolute_frame: layout.frame,
            children: vec![],
        };
        let layerize = LayerizeNode {
            cull: &cull,
            is_layer: true,
            children: vec![],
        };

        let tree = run_flatten(&layerize);
        let root = tree.root.unwrap();
        assert_eq!(root.kind, LayerKind::ScrollView);
        assert!(root.scroll_content_size.is_some());
    }
}
