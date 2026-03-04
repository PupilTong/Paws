use crate::dom::NodeType;
use crate::layout::text::TextMeasurer;
use taffy::prelude::*;

/// The result of a layout computation, containing the final dimensions.
pub struct LayoutBox {
    pub width: f32,
    pub height: f32,
}

/// Computes the layout for a subtree rooted at `id`, returning its dimensions.
pub fn compute_layout(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    id: usize,
    text_measurer: &dyn TextMeasurer,
) -> Option<LayoutBox> {
    let mut taffy = TaffyTree::<()>::new();
    let root_node = build_layout_tree(doc, style_context, id, &mut taffy, text_measurer)?;
    taffy.compute_layout(root_node, Size::MAX_CONTENT).ok()?;
    let layout = taffy.layout(root_node).ok()?;
    Some(LayoutBox {
        width: layout.size.width,
        height: layout.size.height,
    })
}

/// Builds a Taffy layout tree from the DOM subtree rooted at `root_id`.
pub fn build_layout_tree(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    root_id: usize,
    taffy: &mut TaffyTree<()>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    build_subtree(doc, style_context, root_id, taffy, text_measurer)
}

fn build_subtree(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    node_id: usize,
    taffy: &mut TaffyTree<()>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    let node = doc.get_node(node_id)?;

    let display = node
        .get_computed_style_by_key(style_context, "display")
        .unwrap_or_else(|| "inline".to_string());
    let width_str = node
        .get_computed_style_by_key(style_context, "width")
        .unwrap_or_else(|| "auto".to_string());
    let height_str = node
        .get_computed_style_by_key(style_context, "height")
        .unwrap_or_else(|| "auto".to_string());

    fn parse_dim(s: &str) -> Dimension {
        if s == "auto" {
            Dimension::Auto
        } else if s.ends_with("px") {
            if let Ok(val) = s.trim_end_matches("px").parse::<f32>() {
                Dimension::Length(val)
            } else {
                Dimension::Auto
            }
        } else {
            Dimension::Auto
        }
    }

    let mut style = Style {
        display: match display.as_str() {
            "none" => Display::None,
            "block" => Display::Block,
            "flex" => Display::Flex,
            "grid" => Display::Grid,
            _ => Display::Block,
        },
        size: Size {
            width: parse_dim(&width_str),
            height: parse_dim(&height_str),
        },
        ..Default::default()
    };

    match node.node_type {
        NodeType::Element => {
            let mut children = Vec::new();
            for &child_id in &node.children {
                if let Some(child_node) =
                    build_subtree(doc, style_context, child_id, taffy, text_measurer)
                {
                    children.push(child_node);
                }
            }
            taffy.new_with_children(style, &children).ok()
        }
        NodeType::Text => {
            let font_size = node
                .get_computed_style_by_key(style_context, "font-size")
                .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
                .unwrap_or(16.0);
            let text = node.text_content.as_deref().unwrap_or("");
            let (width, height) = text_measurer.measure_text(text, font_size, None);

            style.size.width = Dimension::Length(width);
            style.size.height = Dimension::Length(height);

            taffy.new_leaf(style).ok()
        }
        _ => None,
    }
}
