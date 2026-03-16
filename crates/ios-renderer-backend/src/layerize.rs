use crate::cull::CullNode;
use crate::types::*;

use std::sync::{Mutex, OnceLock};

fn layerize_pool() -> &'static Mutex<Vec<Vec<LayerizeNode<'static>>>> {
    static POOL: OnceLock<Mutex<Vec<Vec<LayerizeNode<'static>>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

fn temp_pool() -> &'static Mutex<Vec<Vec<std::mem::MaybeUninit<LayerizeNode<'static>>>>> {
    static POOL: OnceLock<Mutex<Vec<Vec<std::mem::MaybeUninit<LayerizeNode<'static>>>>>> =
        OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn take_layerize_vec<'a>() -> Vec<LayerizeNode<'a>> {
    let mut pool = layerize_pool().lock().unwrap();
    if let Some(v) = pool.pop() {
        unsafe { std::mem::transmute(v) }
    } else {
        Vec::with_capacity(8)
    }
}

pub fn recycle_layerize_vec<'a>(mut v: Vec<LayerizeNode<'a>>) {
    for child in v.drain(..) {
        recycle_layerize_vec(child.children);
    }
    layerize_pool()
        .lock()
        .unwrap()
        .push(unsafe { std::mem::transmute(v) });
}

pub fn take_temp_vec<'a>() -> Vec<std::mem::MaybeUninit<LayerizeNode<'a>>> {
    let mut pool = temp_pool().lock().unwrap();
    if let Some(v) = pool.pop() {
        unsafe { std::mem::transmute(v) }
    } else {
        Vec::with_capacity(8)
    }
}

pub fn recycle_temp_vec<'a>(mut v: Vec<std::mem::MaybeUninit<LayerizeNode<'a>>>) {
    v.clear();
    temp_pool()
        .lock()
        .unwrap()
        .push(unsafe { std::mem::transmute(v) });
}

pub fn preallocate_pools(capacity: usize) {
    let mut vec1 = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        vec1.push(Vec::with_capacity(8));
    }
    layerize_pool().lock().unwrap().extend(vec1);

    let mut vec2 = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        vec2.push(Vec::with_capacity(8));
    }
    temp_pool().lock().unwrap().extend(vec2);
}

pub struct LayerizeNode<'a> {
    pub cull: &'a CullNode<'a>,
    pub is_layer: bool,
    pub children: Vec<LayerizeNode<'a>>,
}

pub fn needs_layer(node: &LayoutNode, parent_frame: Option<Rect>) -> bool {
    if node.scroll.is_some() {
        return true;
    }
    if let Some(clip) = node.style.clip {
        if let Some(pf) = parent_frame {
            if clip != pf {
                return true;
            }
        } else {
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
    if node.style.will_change {
        return true;
    }
    false
}

pub fn run_layerize<'a>(cull: &'a CullNode<'a>, parent_frame: Option<Rect>) -> LayerizeNode<'a> {
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

    let mut temp = take_temp_vec();
    // Safely reserve capacity if needed
    if temp.capacity() < cull.children.len() {
        temp.reserve(cull.children.len() - temp.capacity());
    }
    unsafe {
        temp.set_len(cull.children.len());
    }

    let ptr_val = temp.as_mut_ptr() as usize;

    rayon::scope(|s| {
        for (i, child) in cull.children.iter().enumerate() {
            s.spawn(move |_| {
                let p = ptr_val as *mut std::mem::MaybeUninit<LayerizeNode>;
                let res = run_layerize(child, Some(my_frame));
                unsafe { p.add(i).write(std::mem::MaybeUninit::new(res)) };
            });
        }
    });

    for t in temp.drain(..) {
        children.push(unsafe { t.assume_init() });
    }
    recycle_temp_vec(temp);

    LayerizeNode {
        cull,
        is_layer,
        children,
    }
}
