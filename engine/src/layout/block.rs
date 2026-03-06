use crate::dom::NodeType;
use crate::layout::text::TextMeasurer;
use crate::style::to_taffy_style;
use style::values::specified::font::FONT_MEDIUM_PX;
use taffy::prelude::*;

/// The result of a layout computation, containing the final dimensions.
pub struct LayoutBox {
    pub width: f32,
    pub height: f32,
}

pub struct LayoutState {
    pub taffy: TaffyTree<()>,
    // Future incremental node map would go here
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutState {
    pub fn new() -> Self {
        Self {
            taffy: TaffyTree::new(),
        }
    }

    /// Computes the layout for a subtree rooted at `id`, returning its dimensions.
    /// Uses a persistent Taffy instance to reuse allocations.
    pub fn compute_layout(
        &mut self,
        doc: &crate::dom::Document,
        id: usize,
        text_measurer: &dyn TextMeasurer,
    ) -> Option<LayoutBox> {
        self.taffy.clear();
        let root_node = build_layout_tree(doc, id, &mut self.taffy, text_measurer)?;
        self.taffy
            .compute_layout(root_node, Size::MAX_CONTENT)
            .ok()?;
        let layout = self.taffy.layout(root_node).ok()?;
        Some(LayoutBox {
            width: layout.size.width,
            height: layout.size.height,
        })
    }
}

/// Builds a Taffy layout tree from the DOM subtree rooted at `root_id`.
pub(crate) fn build_layout_tree(
    doc: &crate::dom::Document,
    root_id: usize,
    taffy: &mut TaffyTree<()>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    build_subtree(doc, root_id, taffy, text_measurer)
}

fn build_subtree(
    doc: &crate::dom::Document,
    node_id: usize,
    taffy: &mut TaffyTree<()>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    let node = doc.get_node(node_id)?;

    // Direct type-level conversion — no string round-trip.
    let computed = node.computed_values.as_ref()?;
    let mut style = to_taffy_style(computed);

    match node.node_type {
        NodeType::Element => {
            let mut children = Vec::new();
            for &child_id in &node.children {
                if let Some(child_node) = build_subtree(doc, child_id, taffy, text_measurer) {
                    children.push(child_node);
                }
            }
            taffy.new_with_children(style, &children).ok()
        }
        NodeType::Text => {
            let font_size = computed.clone_font_size().computed_size().px();
            let font_size = if font_size > 0.0 {
                font_size
            } else {
                FONT_MEDIUM_PX
            };
            let text = node.text_content.as_deref().unwrap_or("");
            let (width, height) = text_measurer.measure_text(text, font_size, None);

            style.size.width = Dimension::Length(width);
            style.size.height = Dimension::Length(height);

            taffy.new_leaf(style).ok()
        }
        _ => None,
    }
}
