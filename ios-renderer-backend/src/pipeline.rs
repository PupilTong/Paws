//! Pipeline orchestrator — runs all five stages per frame.
//!
//! ```text
//! Cull → Layerize → Flatten → Diff → Emit
//! ```
//!
//! [`RendererPipeline::render_frame`] is the single entry point called
//! once per `CAMetalDisplayLink` callback on the main thread.

use crate::cull::{self, Culler};
use crate::diff::{DiffCache, LayerTreeDiffer};
use crate::flatten;
use crate::layerize;
use crate::scroll::ScrollRegistry;
use crate::types::*;

/// Owns the per-frame pipeline state and reusable allocations.
pub(crate) struct RendererPipeline {
    culler: Culler,
    prev_tree: LayerizedTree,
    diff_buffer: LayerTreeDiff,
    diff_cache: DiffCache,
}

impl Default for RendererPipeline {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl RendererPipeline {
    /// Create a new pipeline, pre-allocating buffers for `capacity` layers.
    pub(crate) fn new(capacity: usize) -> Self {
        let mut diff_buffer = LayerTreeDiff::default();
        diff_buffer.created.reserve(capacity);
        diff_buffer.updated.reserve(capacity);
        diff_buffer.removed.reserve(capacity);
        diff_buffer.reordered.reserve(capacity);

        let mut diff_cache = DiffCache::default();
        diff_cache.prev_map.reserve(capacity);
        diff_cache.visited.reserve(capacity);

        Self {
            culler: Culler::new(),
            prev_tree: LayerizedTree::default(),
            diff_buffer,
            diff_cache,
        }
    }

    /// Execute a single frame of the rendering pipeline.
    ///
    /// Writes [`LayerCmd`] values into `out_cmds` in painter's order and
    /// sets `*out_count` to the number of commands written.
    pub(crate) fn render_frame(
        &mut self,
        layout_root: Option<&LayoutNode>,
        viewport: Rect,
        scroll_reg: &ScrollRegistry,
        prefetch_multiplier: f32,
        out_cmds: &mut [LayerCmd],
        out_count: &mut u32,
    ) {
        // ── Stage 1: Cull ──────────────────────────────────────────
        let mut culled = self
            .culler
            .cull(layout_root, viewport, scroll_reg, prefetch_multiplier);

        // ── Stage 2 & 3: Layerize + Flatten ────────────────────────
        let mut layerized_stage = None;
        let next_tree = if let Some(cull_root) = &culled {
            let layerized = layerize::run_layerize(cull_root, None);
            let tree = flatten::run_flatten(&layerized);
            layerized_stage = Some(layerized);
            tree
        } else {
            LayerizedTree::default()
        };

        // ── Stage 4: Diff ──────────────────────────────────────────
        self.diff_buffer.created.clear();
        self.diff_buffer.updated.clear();
        self.diff_buffer.removed.clear();
        self.diff_buffer.reordered.clear();

        LayerTreeDiffer::compute_in_place(
            &self.prev_tree,
            &next_tree,
            &mut self.diff_buffer,
            &mut self.diff_cache,
        );

        // Recycle the OLD tree before overwriting it.
        if let Some(old_root) = self.prev_tree.root.take() {
            flatten::recycle_layerized_vec(vec![old_root]);
        }
        self.prev_tree = next_tree;

        // Recycle intermediate allocations.
        if let Some(layerized) = layerized_stage {
            layerize::recycle_layerize_vec(vec![layerized]);
        }
        if let Some(cull_root) = culled.take() {
            cull::recycle_cull_vec(vec![cull_root]);
        }

        // ── Stage 5: Emit ──────────────────────────────────────────
        let mut emit_count = 0;

        // Removals first.
        for &id in &self.diff_buffer.removed {
            if emit_count < out_cmds.len() {
                out_cmds[emit_count] = LayerCmd::RemoveLayer { id };
                emit_count += 1;
            }
        }

        // Tree walk in painter's order (parents before children).
        if let Some(root) = &self.prev_tree.root {
            Self::emit_node(root, &self.diff_buffer, out_cmds, &mut emit_count);
        }

        *out_count = emit_count as u32;
    }

    /// Emit commands for a single node, then recurse into children.
    fn emit_node(
        node: &LayerizedNode,
        diff: &LayerTreeDiff,
        out_cmds: &mut [LayerCmd],
        count: &mut usize,
    ) {
        let id = node.id;

        // Created / Reparented
        for cmd in &diff.created {
            match cmd {
                LayerCmd::CreateLayer { id: cid, .. } | LayerCmd::ReparentLayer { id: cid, .. }
                    if *cid == id =>
                {
                    if *count < out_cmds.len() {
                        out_cmds[*count] = cmd.clone();
                        *count += 1;
                    }
                }
                _ => {}
            }
        }

        // AttachScroll
        for cmd in diff.created.iter().chain(diff.updated.iter()) {
            if let LayerCmd::AttachScroll { id: cid, .. } = cmd {
                if *cid == id && *count < out_cmds.len() {
                    out_cmds[*count] = cmd.clone();
                    *count += 1;
                }
            }
        }

        // SetZOrder
        for cmd in &diff.reordered {
            if let LayerCmd::SetZOrder { id: cid, .. } = cmd {
                if *cid == id && *count < out_cmds.len() {
                    out_cmds[*count] = cmd.clone();
                    *count += 1;
                }
            }
        }

        // UpdateLayer
        for cmd in &diff.updated {
            if let LayerCmd::UpdateLayer { id: cid, .. } = cmd {
                if *cid == id && *count < out_cmds.len() {
                    out_cmds[*count] = cmd.clone();
                    *count += 1;
                }
            }
        }

        // Recurse children in tree order.
        for child in &node.children {
            Self::emit_node(child, diff, out_cmds, count);
        }
    }
}
