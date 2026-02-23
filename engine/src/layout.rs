use crate::dom::NodeType;
use crate::runtime::RuntimeState;
use crate::style::computed_style;
use taffy::prelude::*;

pub fn build_layout_tree(
    state: &RuntimeState,
    root_id: usize,
    taffy: &mut TaffyTree<()>,
) -> Option<NodeId> {
    // 1. Traverse DOM
    // 2. Create Taffy nodes
    // 3. Compute styles (width/height/display) and convert to Taffy Style
    // 4. Return Taffy NodeId

    // Recursive helper
    build_subtree(state, root_id, taffy)
}

fn build_subtree(
    state: &RuntimeState,
    node_id: usize,
    taffy: &mut TaffyTree<()>,
) -> Option<NodeId> {
    let node = state.doc.get_node(node_id)?;
    // node is &Node.

    let display = computed_style(state, node_id, "display").unwrap_or_else(|| "inline".to_string());
    let width_str = computed_style(state, node_id, "width").unwrap_or_else(|| "auto".to_string());
    let height_str = computed_style(state, node_id, "height").unwrap_or_else(|| "auto".to_string());

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
                if let Some(child_node) = build_subtree(state, child_id, taffy) {
                    children.push(child_node);
                }
            }

            // If explicit size not set, and has children, Taffy handles it.
            // If it's a leaf element (no children), currently it collapses unless size set.

            taffy.new_with_children(style, &children).ok()
        }
        NodeType::Text => {
            // Measure text
            // Mock: font-size * char_count.
            // Assume default font-size 16px.
            let font_size = 16.0;
            // In real world, we'd check computed style "font-size".

            let char_count = node
                .text_content
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0);
            let width = char_count as f32 * font_size * 0.6; // approx width per char
            let height = font_size; // line height approx

            style.size.width = Dimension::Length(width);
            style.size.height = Dimension::Length(height);

            taffy.new_leaf(style).ok()
        }
        _ => None,
    }
}
