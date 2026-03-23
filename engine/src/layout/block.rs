use crate::dom::NodeType;
use crate::layout::text::TextMeasurer;
use crate::style::to_taffy_style;
use style::values::specified::box_::Overflow;
use style::values::specified::font::FONT_MEDIUM_PX;
use taffy::prelude::*;

/// A fully-resolved layout node with absolute position, size, and children.
///
/// Produced by [`LayoutState::compute_layout`] and consumed by
/// the iOS renderer backend's conversion layer to build `LayoutNode` trees.
pub struct LayoutBox {
    /// The DOM node ID this layout box corresponds to.
    pub node_id: taffy::NodeId,
    /// X offset relative to the parent's content box.
    pub x: f32,
    /// Y offset relative to the parent's content box.
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Stacking order. `None` means `auto`.
    pub z_index: Option<i32>,
    /// Overflow behavior on the x-axis.
    pub overflow_x: Overflow,
    /// Overflow behavior on the y-axis.
    pub overflow_y: Overflow,
    /// Background color as `(r, g, b, a)` in 0.0–1.0 range, or `None` if transparent.
    pub background_color: Option<(f32, f32, f32, f32)>,
    pub children: Vec<LayoutBox>,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            node_id: taffy::NodeId::from(0_u64),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            z_index: None,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            background_color: None,
            children: Vec::new(),
        }
    }
}

pub struct LayoutState {
    pub taffy: TaffyTree<taffy::NodeId>,
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

    /// Computes the layout for a subtree rooted at `id`, returning its full tree.
    /// Uses a persistent Taffy instance to reuse allocations.
    pub fn compute_layout(
        &mut self,
        doc: &crate::dom::Document,
        id: taffy::NodeId,
        text_measurer: &dyn TextMeasurer,
    ) -> Option<LayoutBox> {
        self.taffy.clear();
        let root_node = build_layout_tree(doc, id, &mut self.taffy, text_measurer)?;
        self.taffy
            .compute_layout(root_node, Size::MAX_CONTENT)
            .ok()?;
        self.extract_tree(root_node, doc)
    }

    /// Recursively extracts the positioned layout tree from Taffy's results.
    fn extract_tree(&self, taffy_node: NodeId, doc: &crate::dom::Document) -> Option<LayoutBox> {
        let layout = self.taffy.layout(taffy_node).ok()?;
        let node_id = self.taffy.get_node_context(taffy_node).copied()?;

        let children: Vec<LayoutBox> = self
            .taffy
            .children(taffy_node)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|child| self.extract_tree(child, doc))
            .collect();

        // Extract z-index, overflow, and background-color from computed style.
        let (z_index, overflow_x, overflow_y, background_color) = doc
            .get_node(node_id)
            .and_then(|node| node.computed_values.as_ref())
            .map(|cv| {
                use style::values::generics::position::ZIndex;
                let z = match cv.clone_z_index() {
                    ZIndex::Integer(n) => Some(n),
                    ZIndex::Auto => None,
                };

                let bg = extract_background_color(cv);

                (z, cv.clone_overflow_x(), cv.clone_overflow_y(), bg)
            })
            .unwrap_or((None, Overflow::Visible, Overflow::Visible, None));

        Some(LayoutBox {
            node_id,
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
            z_index,
            overflow_x,
            overflow_y,
            background_color,
            children,
        })
    }
}

/// Extracts the background color from computed values as an RGBA tuple.
///
/// Returns `None` for transparent backgrounds (alpha ≈ 0) or non-absolute colors.
fn extract_background_color(
    cv: &style::properties::ComputedValues,
) -> Option<(f32, f32, f32, f32)> {
    use style::values::computed::Color;

    match cv.clone_background_color() {
        Color::Absolute(abs) => {
            let r = abs.components.0;
            let g = abs.components.1;
            let b = abs.components.2;
            let a = abs.alpha;
            // Skip fully transparent backgrounds.
            if a.abs() < f32::EPSILON {
                None
            } else {
                Some((r, g, b, a))
            }
        }
        _ => None,
    }
}

/// Builds a Taffy layout tree from the DOM subtree rooted at `root_id`.
pub(crate) fn build_layout_tree(
    doc: &crate::dom::Document,
    root_id: taffy::NodeId,
    taffy: &mut TaffyTree<taffy::NodeId>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    build_subtree(doc, root_id, taffy, text_measurer)
}

fn build_subtree(
    doc: &crate::dom::Document,
    node_id: taffy::NodeId,
    taffy: &mut TaffyTree<taffy::NodeId>,
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
            let taffy_id = taffy.new_with_children(style, &children).ok()?;
            let _ = taffy.set_node_context(taffy_id, Some(node_id));
            Some(taffy_id)
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

            let w_lp: taffy::LengthPercentage = taffy::style_helpers::length(width);
            let h_lp: taffy::LengthPercentage = taffy::style_helpers::length(height);
            style.size.width = w_lp.into();
            style.size.height = h_lp.into();

            let taffy_id = taffy.new_leaf_with_context(style, node_id).ok()?;
            Some(taffy_id)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Document;
    use crate::layout::text::MockTextMeasurer;
    use markup5ever::QualName;
    use style::shared_lock::SharedRwLock;
    use url::Url;

    #[test]
    fn test_compute_layout_extract_tree() {
        let guard = SharedRwLock::new();
        let mut doc = Document::new(guard, Url::parse("http://test.com").unwrap());
        let mut state = LayoutState::new();
        let measurer = MockTextMeasurer;

        let elem1 = doc.create_element(QualName::new(None, "".into(), "div".into()));
        doc.append_child(doc.root, elem1).unwrap();

        let elem2 = doc.create_element(QualName::new(None, "".into(), "span".into()));
        doc.append_child(elem1, elem2).unwrap();

        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        doc.resolve_style(&style_ctx);

        let layout = state.compute_layout(&doc, elem1, &measurer);
        assert!(layout.is_some());
        let layout = layout.unwrap();
        assert_eq!(layout.children.len(), 1);
    }
}
