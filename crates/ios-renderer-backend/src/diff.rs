use crate::types::*;
use fnv::{FnvHashMap, FnvHashSet};

#[derive(Default)]
pub struct DiffCache {
    pub prev_map: FnvHashMap<LayerId, *const LayerizedNode>,
    pub visited: FnvHashSet<LayerId>,
}

impl DiffCache {
    pub fn clear(&mut self) {
        self.prev_map.clear();
        self.visited.clear();
    }
}

pub struct LayerTreeDiffer;

impl LayerTreeDiffer {
    pub fn compute_in_place(
        prev: &LayerizedTree,
        next: &LayerizedTree,
        diff: &mut LayerTreeDiff,
        cache: &mut DiffCache,
    ) {
        cache.clear();

        if prev.root.is_none() && next.root.is_none() {
            return;
        }

        Self::get_changed_prev(prev.root.as_ref(), next.root.as_ref(), &mut cache.prev_map);

        if let Some(n_root) = &next.root {
            Self::diff_recursive(
                prev.root.as_ref(),
                n_root,
                diff,
                &cache.prev_map,
                &mut cache.visited,
            );
        }

        for id in cache.prev_map.keys() {
            if !cache.visited.contains(id) {
                diff.removed.push(*id);
            }
        }
    }

    pub fn compute(prev: &LayerizedTree, next: &LayerizedTree) -> LayerTreeDiff {
        let mut diff = LayerTreeDiff::default();
        let mut cache = DiffCache::default();
        Self::compute_in_place(prev, next, &mut diff, &mut cache);
        diff
    }

    fn get_changed_prev(
        prev: Option<&LayerizedNode>,
        next: Option<&LayerizedNode>,
        map: &mut FnvHashMap<LayerId, *const LayerizedNode>,
    ) {
        let p = match prev {
            Some(p) => p,
            None => return,
        };

        if let Some(n) = next {
            if p.id == n.id && p.generation == n.generation {
                return;
            }
        }

        // SAFETY: Pointer is only valid for duration of this diff pass, which is single-threaded synchronous and data is owned by parent pipeline.
        map.insert(p.id, p as *const LayerizedNode);

        if let Some(n) = next {
            if p.id == n.id {
                for p_child in &p.children {
                    let n_child = n.children.iter().find(|c| c.id == p_child.id);
                    Self::get_changed_prev(Some(p_child), n_child, map);
                }
                return;
            }
        }

        Self::add_all_to_map(p, map);
    }

    fn add_all_to_map(node: &LayerizedNode, map: &mut FnvHashMap<LayerId, *const LayerizedNode>) {
        map.insert(node.id, node as *const LayerizedNode);
        for child in &node.children {
            Self::add_all_to_map(child, map);
        }
    }

    fn diff_recursive<'a>(
        prev: Option<&'a LayerizedNode>,
        next: &'a LayerizedNode,
        diff: &mut LayerTreeDiff,
        prev_map: &FnvHashMap<LayerId, *const LayerizedNode>,
        visited: &mut FnvHashSet<LayerId>,
    ) {
        if visited.contains(&next.id) {
            return;
        }
        visited.insert(next.id);

        let p_node = prev
            .filter(|p| p.id == next.id)
            .or_else(|| prev_map.get(&next.id).map(|&ptr| unsafe { &*ptr }));

        if let Some(p) = p_node {
            if p.generation == next.generation {
                // Exact match, no updates inside this subtree.
                // We don't recurse into children because they haven't changed.
                return;
            }

            if p.props != next.props {
                diff.updated.push(LayerCmd::UpdateLayer {
                    id: next.id,
                    props: next.props,
                });
            }
            if p.parent_id != next.parent_id || p.parent_kind != next.parent_kind {
                if let (Some(parent_id), Some(parent_type)) = (next.parent_id, next.parent_kind) {
                    diff.created.push(LayerCmd::ReparentLayer {
                        id: next.id,
                        new_parent: parent_id,
                        parent_type,
                    });
                }
            }
            if p.z_index != next.z_index {
                diff.reordered.push(LayerCmd::SetZOrder {
                    id: next.id,
                    index: next.z_index,
                });
            }
            if p.scroll_content_size != next.scroll_content_size {
                if let (Some(size), Some(parent_id)) = (next.scroll_content_size, next.parent_id) {
                    diff.updated.push(LayerCmd::AttachScroll {
                        id: next.id,
                        parent_id,
                        content_size: size,
                    });
                }
            }
        } else {
            diff.created.push(LayerCmd::CreateLayer {
                id: next.id,
                kind: next.kind,
            });
            if let (Some(parent_id), Some(parent_type)) = (next.parent_id, next.parent_kind) {
                diff.created.push(LayerCmd::ReparentLayer {
                    id: next.id,
                    new_parent: parent_id,
                    parent_type,
                });
            }
            if let (Some(size), Some(parent_id)) = (next.scroll_content_size, next.parent_id) {
                diff.updated.push(LayerCmd::AttachScroll {
                    id: next.id,
                    parent_id,
                    content_size: size,
                });
            }
            diff.reordered.push(LayerCmd::SetZOrder {
                id: next.id,
                index: next.z_index,
            });
            diff.updated.push(LayerCmd::UpdateLayer {
                id: next.id,
                props: next.props,
            });
        }

        for next_child in &next.children {
            let p_child = p_node.and_then(|p| p.children.iter().find(|c| c.id == next_child.id));
            Self::diff_recursive(p_child, next_child, diff, prev_map, visited);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_unchanged_skipped() {
        let prev = LayerizedTree {
            root: Some(LayerizedNode {
                id: 1,
                kind: LayerKind::View,
                parent_id: None,
                parent_kind: None,
                z_index: 0,
                props: LayerProps {
                    frame: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    opacity: 1.0,
                    background: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    border_radius: 0.0,
                    has_transform: false,
                    transform: Transform3D::default(),
                    has_clip: false,
                    clip: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                },
                scroll_content_size: None,
                generation: 1,
                children: vec![LayerizedNode {
                    id: 2,
                    kind: LayerKind::View,
                    parent_id: Some(1),
                    parent_kind: Some(ParentKind::Layer),
                    z_index: 1,
                    props: LayerProps {
                        frame: Rect {
                            x: 10.0,
                            y: 10.0,
                            width: 50.0,
                            height: 50.0,
                        },
                        opacity: 1.0,
                        background: Color {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                        border_radius: 0.0,
                        has_transform: false,
                        transform: Transform3D::default(),
                        has_clip: false,
                        clip: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        },
                    },
                    scroll_content_size: None,
                    generation: 1,
                    children: vec![],
                }],
            }),
        };

        // Generation unchanged
        let next = prev.clone();
        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.created.is_empty());
        assert!(diff.updated.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.reordered.is_empty());
    }

    #[test]
    fn test_diff_node_opacity_changed() {
        let mut prev = LayerizedTree {
            root: Some(LayerizedNode {
                id: 1,
                kind: LayerKind::View,
                parent_id: None,
                parent_kind: None,
                z_index: 0,
                props: LayerProps {
                    frame: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    opacity: 1.0,
                    background: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    border_radius: 0.0,
                    has_transform: false,
                    transform: Transform3D::default(),
                    has_clip: false,
                    clip: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                },
                scroll_content_size: None,
                generation: 1,
                children: vec![],
            }),
        };

        let mut next = prev.clone();
        next.root.as_mut().unwrap().props.opacity = 0.5;
        next.root.as_mut().unwrap().generation = 2; // Incremented generation!

        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.created.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.updated.len(), 1);
        if let LayerCmd::UpdateLayer { id, props } = &diff.updated[0] {
            assert_eq!(*id, 1);
            assert_eq!(props.opacity, 0.5);
        } else {
            panic!("Expected UpdateLayer");
        }
    }
}
