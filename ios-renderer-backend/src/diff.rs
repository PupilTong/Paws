//! Stage 4 — Layer tree diff.
//!
//! Compares the current frame's [`LayerizedTree`] against the previous
//! frame's snapshot and produces a minimal [`LayerTreeDiff`].
//!
//! Subtrees whose root `generation` is unchanged are skipped in O(1).
//! The diff pass is single-threaded and synchronous.

use crate::types::*;
use fnv::{FnvHashMap, FnvHashSet};

/// Reusable scratch space for the diff pass. Kept across frames to avoid
/// repeated allocation.
#[derive(Default)]
pub(crate) struct DiffCache {
    pub(crate) prev_map: FnvHashMap<LayerId, *const LayerizedNode>,
    pub(crate) visited: FnvHashSet<LayerId>,
}

impl DiffCache {
    pub(crate) fn clear(&mut self) {
        self.prev_map.clear();
        self.visited.clear();
    }
}

/// Stateless differ. All mutable state lives in [`DiffCache`] and
/// [`LayerTreeDiff`], which are owned by the pipeline.
pub(crate) struct LayerTreeDiffer;

impl LayerTreeDiffer {
    /// Diff `prev` against `next`, appending commands to `diff`.
    ///
    /// `cache` is reused across frames — call `cache.clear()` is done
    /// internally at the start of each pass.
    pub(crate) fn compute_in_place(
        prev: &LayerizedTree,
        next: &LayerizedTree,
        diff: &mut LayerTreeDiff,
        cache: &mut DiffCache,
    ) {
        cache.clear();

        if prev.root.is_none() && next.root.is_none() {
            return;
        }

        Self::build_prev_map(prev.root.as_ref(), next.root.as_ref(), &mut cache.prev_map);

        if let Some(n_root) = &next.root {
            Self::diff_recursive(
                prev.root.as_ref(),
                n_root,
                diff,
                &cache.prev_map,
                &mut cache.visited,
            );
        }

        // Any prev node not visited in the next tree has been removed.
        for id in cache.prev_map.keys() {
            if !cache.visited.contains(id) {
                diff.removed.push(*id);
            }
        }
    }

    /// Convenience wrapper that allocates fresh buffers.
    #[cfg(test)]
    fn compute(prev: &LayerizedTree, next: &LayerizedTree) -> LayerTreeDiff {
        let mut diff = LayerTreeDiff::default();
        let mut cache = DiffCache::default();
        Self::compute_in_place(prev, next, &mut diff, &mut cache);
        diff
    }

    /// Build a map of prev-tree nodes that may have changed, so we can
    /// look them up by id during the forward pass.
    fn build_prev_map(
        prev: Option<&LayerizedNode>,
        next: Option<&LayerizedNode>,
        map: &mut FnvHashMap<LayerId, *const LayerizedNode>,
    ) {
        let p = match prev {
            Some(p) => p,
            None => return,
        };

        // If both trees have the same node with unchanged generation, skip.
        if let Some(n) = next {
            if p.id == n.id && p.generation == n.generation {
                return;
            }
        }

        // SAFETY: Pointer is only valid for the duration of this diff pass,
        // which is single-threaded and synchronous. The LayerizedTree that
        // owns the data is borrowed immutably by the caller for the entire
        // pass lifetime.
        map.insert(p.id, p as *const LayerizedNode);

        if let Some(n) = next {
            if p.id == n.id {
                for p_child in &p.children {
                    let n_child = n.children.iter().find(|c| c.id == p_child.id);
                    Self::build_prev_map(Some(p_child), n_child, map);
                }
                return;
            }
        }

        Self::add_all_to_map(p, map);
    }

    fn add_all_to_map(node: &LayerizedNode, map: &mut FnvHashMap<LayerId, *const LayerizedNode>) {
        // SAFETY: Same as build_prev_map — pointer valid for diff pass lifetime.
        map.insert(node.id, node as *const LayerizedNode);
        for child in &node.children {
            Self::add_all_to_map(child, map);
        }
    }

    fn diff_recursive(
        prev: Option<&LayerizedNode>,
        next: &LayerizedNode,
        diff: &mut LayerTreeDiff,
        prev_map: &FnvHashMap<LayerId, *const LayerizedNode>,
        visited: &mut FnvHashSet<LayerId>,
    ) {
        if visited.contains(&next.id) {
            return;
        }
        visited.insert(next.id);

        // SAFETY: Pointers in prev_map are valid for the diff pass lifetime
        // (see build_prev_map). We only dereference inside this synchronous,
        // single-threaded pass while the owning LayerizedTree is borrowed.
        let p_node = prev
            .filter(|p| p.id == next.id)
            .or_else(|| prev_map.get(&next.id).map(|&ptr| unsafe { &*ptr }));

        if let Some(p) = p_node {
            // Generation unchanged → skip entire subtree.
            if p.generation == next.generation {
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
            // New node — emit full creation sequence.
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

        // Recurse into children.
        for next_child in &next.children {
            let p_child = p_node.and_then(|p| p.children.iter().find(|c| c.id == next_child.id));
            Self::diff_recursive(p_child, next_child, diff, prev_map, visited);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_props(opacity: f32) -> LayerProps {
        LayerProps {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            opacity,
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
        }
    }

    fn make_tree(generation: u64, opacity: f32) -> LayerizedTree {
        LayerizedTree {
            root: Some(LayerizedNode {
                id: 1,
                kind: LayerKind::View,
                parent_id: None,
                parent_kind: None,
                z_index: 0,
                props: make_props(opacity),
                scroll_content_size: None,
                generation,
                children: vec![],
            }),
        }
    }

    #[test]
    fn unchanged_generation_produces_empty_diff() {
        let prev = make_tree(1, 1.0);
        let next = prev.clone();
        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.created.is_empty());
        assert!(diff.updated.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.reordered.is_empty());
    }

    #[test]
    fn opacity_change_produces_update() {
        let prev = make_tree(1, 1.0);
        let next = make_tree(2, 0.5);
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

    #[test]
    fn new_node_produces_create_commands() {
        let prev = LayerizedTree { root: None };
        let next = make_tree(1, 1.0);
        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.removed.is_empty());
        assert!(!diff.created.is_empty());
        assert!(diff
            .created
            .iter()
            .any(|c| matches!(c, LayerCmd::CreateLayer { id: 1, .. })));
    }

    #[test]
    fn removed_node_appears_in_removed() {
        let prev = make_tree(1, 1.0);
        let next = LayerizedTree { root: None };
        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.created.is_empty());
        assert!(diff.removed.contains(&1));
    }

    #[test]
    fn both_empty_produces_nothing() {
        let prev = LayerizedTree { root: None };
        let next = LayerizedTree { root: None };
        let diff = LayerTreeDiffer::compute(&prev, &next);

        assert!(diff.created.is_empty());
        assert!(diff.updated.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.reordered.is_empty());
    }
}
