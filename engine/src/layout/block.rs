use crate::dom::NodeType;
use crate::layout::text::TextMeasurer;
use crate::style::to_taffy_style;
use fnv::FnvHashMap;
use style::values::specified::font::FONT_MEDIUM_PX;
use taffy::prelude::*;

/// A fully-resolved layout node with absolute position, size, and children.
///
/// Produced by [`LayoutState::compute_layout`] and consumed by
/// the iOS renderer backend's conversion layer to build `LayoutNode` trees.
pub struct LayoutBox {
    /// X offset relative to the parent's content box.
    pub x: f32,
    /// Y offset relative to the parent's content box.
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub children: Vec<LayoutBox>,
}

pub struct LayoutState {
    pub taffy: TaffyTree<()>,
    /// Maps Taffy node IDs back to DOM node IDs, populated during tree build.
    taffy_to_dom: FnvHashMap<NodeId, usize>,
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
            taffy_to_dom: FnvHashMap::default(),
        }
    }

    /// Computes the layout for a subtree rooted at `id`, returning its full tree.
    /// Uses a persistent Taffy instance to reuse allocations.
    pub fn compute_layout(
        &mut self,
        doc: &crate::dom::Document,
        id: usize,
        text_measurer: &dyn TextMeasurer,
    ) -> Option<LayoutBox> {
        self.taffy.clear();
        self.taffy_to_dom.clear();
        let root_node = build_layout_tree(
            doc,
            id,
            &mut self.taffy,
            &mut self.taffy_to_dom,
            text_measurer,
        )?;
        self.taffy
            .compute_layout(root_node, Size::MAX_CONTENT)
            .ok()?;
        self.extract_tree(root_node)
    }

    /// Recursively extracts the positioned layout tree from Taffy's results.
    fn extract_tree(&self, taffy_node: NodeId) -> Option<LayoutBox> {
        let layout = self.taffy.layout(taffy_node).ok()?;

        let children: Vec<LayoutBox> = self
            .taffy
            .children(taffy_node)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|child| self.extract_tree(child))
            .collect();

        Some(LayoutBox {
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
            children,
        })
    }
}

/// Builds a Taffy layout tree from the DOM subtree rooted at `root_id`.
///
/// Populates `node_map` with the mapping from Taffy node IDs to DOM node IDs.
pub(crate) fn build_layout_tree(
    doc: &crate::dom::Document,
    root_id: usize,
    taffy: &mut TaffyTree<()>,
    node_map: &mut FnvHashMap<NodeId, usize>,
    text_measurer: &dyn TextMeasurer,
) -> Option<NodeId> {
    build_subtree(doc, root_id, taffy, node_map, text_measurer)
}

fn build_subtree(
    doc: &crate::dom::Document,
    node_id: usize,
    taffy: &mut TaffyTree<()>,
    node_map: &mut FnvHashMap<NodeId, usize>,
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
                if let Some(child_node) =
                    build_subtree(doc, child_id, taffy, node_map, text_measurer)
                {
                    children.push(child_node);
                }
            }
            let taffy_id = taffy.new_with_children(style, &children).ok()?;
            node_map.insert(taffy_id, node_id);
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

            let taffy_id = taffy.new_leaf(style).ok()?;
            node_map.insert(taffy_id, node_id);
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
        doc.append_child(0, elem1).unwrap();

        let elem2 = doc.create_element(QualName::new(None, "".into(), "span".into()));
        doc.append_child(elem1, elem2).unwrap();

        // Ensure there's a computed values cache so to_taffy_style doesn't bail early.
        // Actually Document::resolve_style will ensure `computed_values` is Some(...)
        let url = Url::parse("http://test.com").unwrap();
        let style_ctx = crate::style::StyleContext::new(url);
        // wait, we can just resolve style on the document
        doc.resolve_style(&style_ctx);

        let layout = state.compute_layout(&doc, elem1, &measurer);
        assert!(layout.is_some());
        let layout = layout.unwrap();
        assert_eq!(layout.children.len(), 1);
    }
}
