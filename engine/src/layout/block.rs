use crate::dom::NodeType;
use taffy::prelude::*;

pub struct LayoutBox {
    pub width: f32,
    pub height: f32,
}

pub fn compute_layout(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    id: usize,
) -> Option<LayoutBox> {
    let mut taffy = TaffyTree::<()>::new();
    let root_node = build_layout_tree(doc, style_context, id, &mut taffy)?;
    taffy.compute_layout(root_node, Size::MAX_CONTENT).ok()?;
    let layout = taffy.layout(root_node).ok()?;
    Some(LayoutBox {
        width: layout.size.width,
        height: layout.size.height,
    })
}

pub fn build_layout_tree(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    root_id: usize,
    taffy: &mut TaffyTree<()>,
) -> Option<NodeId> {
    build_subtree(doc, style_context, root_id, taffy)
}

fn build_subtree(
    doc: &crate::dom::Document,
    style_context: &crate::style::StyleContext,
    node_id: usize,
    taffy: &mut TaffyTree<()>,
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
                if let Some(child_node) = build_subtree(doc, style_context, child_id, taffy) {
                    children.push(child_node);
                }
            }
            taffy.new_with_children(style, &children).ok()
        }
        NodeType::Text => {
            let font_size = 16.0;
            let char_count = node
                .text_content
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            let width = char_count as f32 * font_size * 0.6;
            let height = font_size;

            style.size.width = Dimension::Length(width);
            style.size.height = Dimension::Length(height);

            taffy.new_leaf(style).ok()
        }
        _ => None,
    }
}
