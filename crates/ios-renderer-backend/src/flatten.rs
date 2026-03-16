use crate::layerize::LayerizeNode;
use crate::types::*;
use std::cell::RefCell;

thread_local! {
    static LAYERIZED_POOL: RefCell<Vec<Vec<LayerizedNode>>> = const { RefCell::new(Vec::new()) };
}

pub fn take_layerized_vec() -> Vec<LayerizedNode> {
    LAYERIZED_POOL.with(|p| {
        let mut pool = p.borrow_mut();
        pool.pop().unwrap_or_else(|| Vec::with_capacity(8))
    })
}

pub fn recycle_layerized_vec(mut v: Vec<LayerizedNode>) {
    for child in v.drain(..) {
        recycle_layerized_vec(child.children);
    }
    LAYERIZED_POOL.with(|p| p.borrow_mut().push(v));
}

pub fn preallocate_pools(capacity: usize) {
    let mut vec = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        vec.push(Vec::with_capacity(8));
    }
    LAYERIZED_POOL.with(|p| p.borrow_mut().extend(vec));
}

pub struct FlattenPassResult {
    pub layered_node: Option<LayerizedNode>,
    pub bubbling_descendants: Vec<LayerizedNode>,
}

pub fn run_flatten(layerize_tree: &LayerizeNode) -> LayerizedTree {
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

fn flatten_bottom_up(node: &LayerizeNode) -> FlattenPassResult {
    let is_layer = node.is_layer;

    let ln = node.cull.layout_node;
    let expected_id = ln.id;

    let kind = if ln.scroll.is_some() {
        LayerKind::ScrollView
    } else if ln.style.transform.is_some() || ln.style.opacity < 1.0 || ln.style.will_change {
        LayerKind::View
    } else {
        LayerKind::View
    };

    let expected_p_kind = if kind == LayerKind::ScrollView {
        ParentKind::ScrollView
    } else {
        ParentKind::Layer
    };

    let mut all_bubbling = take_layerized_vec();

    for child in &node.children {
        let mut child_res = flatten_bottom_up(child);

        if let Some(child_layer) = child_res.layered_node {
            all_bubbling.push(child_layer);
        }
        all_bubbling.append(&mut child_res.bubbling_descendants);
        // The child_res.bubbling_descendants vector is now empty, we can recycle it!
        recycle_layerized_vec(child_res.bubbling_descendants);
    }

    if is_layer || ln.id == 1 {
        let mut z_index = 0;
        for child_layer in &mut all_bubbling {
            child_layer.z_index = z_index;
            child_layer.parent_id = Some(expected_id);
            child_layer.parent_kind = Some(expected_p_kind);
            z_index += 1;
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
                x: 0.,
                y: 0.,
                width: 0.,
                height: 0.,
            }),
        };

        let scroll_size = ln.scroll.as_ref().map(|s| s.content_size);

        let layered = LayerizedNode {
            id: expected_id,
            kind,
            parent_id: None,
            parent_kind: None,
            z_index: 0,
            props,
            scroll_content_size: scroll_size,
            generation: ln.generation,
            children: all_bubbling,
        };

        let empty_bubbling = take_layerized_vec();
        FlattenPassResult {
            layered_node: Some(layered),
            bubbling_descendants: empty_bubbling,
        }
    } else {
        FlattenPassResult {
            layered_node: None,
            bubbling_descendants: all_bubbling,
        }
    }
}
