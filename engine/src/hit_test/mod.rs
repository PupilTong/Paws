//! Hit testing — locate the topmost element under a viewport point.
//!
//! Given a point in CSS-pixel viewport coordinates (top-left origin), walks
//! the layout tree in CSS paint order (front-to-back, deepest-first) and
//! returns the deepest element whose final box contains the point. The
//! result is the [`taffy::NodeId`] callers feed back into the W3C event
//! dispatch path.
//!
//! Hit-testability filter (MVP):
//! - Only [`PawsElement::is_element`] nodes can be returned.
//! - The node must have computed style ([`PawsElement::has_style`]).
//! - The node's rect must have non-zero area.
//!
//! Not yet honoured (follow-up): `pointer-events: none`, `visibility:
//! hidden`, and `overflow: hidden` clipping.

use taffy::{NodeId, Point};

use crate::dom::Document;
use crate::layout::paint_order_children;
use crate::runtime::RenderState;

/// Returns the deepest hit-testable element at `point`, or `None` if no
/// element's rect contains the point.
///
/// `point` is in the same coordinate space as `final_layout.location` of
/// the immediate children of `root` — for the document root that's the
/// viewport's top-left-origin CSS-pixel space, which is what the iOS
/// renderer (and all other planned backends) emit on user input.
///
/// `root` is typically `taffy::NodeId::from(0)` (the [`Document`] root)
/// but any subtree root works — useful for scoped hit-testing inside an
/// inert container or shadow root.
///
/// The traversal walks **all** descendants regardless of whether
/// intermediate ancestors' rects contain the point, so absolutely
/// positioned children that overflow their parent are still reachable.
/// This is O(N) per call where N is the number of styled descendants.
pub fn hit_test_at_point<S: RenderState>(
    doc: &Document<S>,
    root: NodeId,
    point: Point<f32>,
) -> Option<NodeId> {
    hit_test_node(doc, root, point)
}

/// Recursive worker. `point` is in the parent's coordinate space — i.e.
/// the same space as `node`'s `final_layout.location`.
fn hit_test_node<S: RenderState>(
    doc: &Document<S>,
    node_id: NodeId,
    point: Point<f32>,
) -> Option<NodeId> {
    let node = doc.get_node(node_id)?;
    let layout = node.layout();
    let location = layout.location;
    let size = layout.size;

    // Translate the point into the node's local coordinate space so
    // children — whose `final_layout.location` is parent-relative — see
    // it in their own parent's space.
    let local_point = Point {
        x: point.x - location.x,
        y: point.y - location.y,
    };

    // Front-to-back: descendants painted later (on top) win over earlier
    // ones at the same point.
    let children = paint_order_children(doc, node_id);
    for &child_id in children.iter().rev() {
        if let Some(hit) = hit_test_node(doc, child_id, local_point) {
            return Some(hit);
        }
    }

    let in_rect = point.x >= location.x
        && point.x < location.x + size.width
        && point.y >= location.y
        && point.y < location.y + size.height;

    if !in_rect {
        return None;
    }

    if !node.is_element() || !node.has_style() || size.width <= 0.0 || size.height <= 0.0 {
        return None;
    }

    Some(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::dispatch::dispatch_event_with_callback;
    use crate::events::{Event, EventPhase, ListenerOptions};
    use crate::layout::compute_layout_in_place;
    use crate::runtime::RuntimeState;
    use stylo_atoms::Atom;
    use taffy::prelude::TaffyMaxContent;

    fn add_styled_child(state: &mut RuntimeState, parent: u32, styles: &[(&str, &str)]) -> u32 {
        let id = state.create_element("div".to_string());
        state.append_element(parent, id).unwrap();
        for &(prop, value) in styles {
            state
                .set_inline_style(id, prop.to_string(), value.to_string())
                .unwrap();
        }
        id
    }

    fn build_layout(state: &mut RuntimeState, root: u32) {
        state.doc.resolve_style(&state.style_context);
        compute_layout_in_place(
            &mut state.doc,
            NodeId::from(root as u64),
            taffy::Size::MAX_CONTENT,
        );
    }

    fn pt(x: f32, y: f32) -> Point<f32> {
        Point { x, y }
    }

    /// A single fixed-size box: hit inside, miss outside.
    #[test]
    fn single_box_hit_and_miss() {
        let mut state = RuntimeState::new("https://test.example".into());
        let root = state.create_element("div".into());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "width".into(), "300px".into())
            .unwrap();
        state
            .set_inline_style(root, "height".into(), "300px".into())
            .unwrap();
        let box_id = add_styled_child(
            &mut state,
            root,
            &[
                ("position", "absolute"),
                ("left", "10px"),
                ("top", "10px"),
                ("width", "100px"),
                ("height", "50px"),
            ],
        );
        build_layout(&mut state, root);

        let root_id = NodeId::from(root as u64);
        let box_nid = NodeId::from(box_id as u64);

        assert_eq!(
            hit_test_at_point(&state.doc, root_id, pt(50.0, 30.0)),
            Some(box_nid)
        );
        // (5, 5) is inside the root rect but outside the box → hit the root.
        assert_eq!(
            hit_test_at_point(&state.doc, root_id, pt(5.0, 5.0)),
            Some(root_id)
        );
        // (400, 400) is past the root's 300×300 rect → no hit.
        assert_eq!(
            hit_test_at_point(&state.doc, root_id, pt(400.0, 400.0)),
            None
        );
    }

    /// Nested boxes: the deepest containing box wins.
    #[test]
    fn nested_returns_deepest_hit() {
        let mut state = RuntimeState::new("https://test.example".into());
        let outer = state.create_element("div".into());
        state.append_element(0, outer).unwrap();
        state
            .set_inline_style(outer, "width".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(outer, "height".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(outer, "position".into(), "relative".into())
            .unwrap();

        let inner = add_styled_child(
            &mut state,
            outer,
            &[
                ("position", "absolute"),
                ("left", "75px"),
                ("top", "75px"),
                ("width", "50px"),
                ("height", "50px"),
            ],
        );
        build_layout(&mut state, outer);

        let outer_id = NodeId::from(outer as u64);
        let inner_id = NodeId::from(inner as u64);

        assert_eq!(
            hit_test_at_point(&state.doc, outer_id, pt(100.0, 100.0)),
            Some(inner_id)
        );
        assert_eq!(
            hit_test_at_point(&state.doc, outer_id, pt(10.0, 10.0)),
            Some(outer_id)
        );
    }

    /// Two siblings sharing the same rect: the higher z-index wins.
    #[test]
    fn higher_z_index_wins() {
        let mut state = RuntimeState::new("https://test.example".into());
        let root = state.create_element("div".into());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();
        state
            .set_inline_style(root, "width".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(root, "height".into(), "200px".into())
            .unwrap();

        let common = [
            ("position", "absolute"),
            ("left", "0px"),
            ("top", "0px"),
            ("width", "100px"),
            ("height", "100px"),
        ];
        let bottom = add_styled_child(&mut state, root, &common);
        let top = add_styled_child(&mut state, root, &common);
        // Bump `top` above `bottom` via z-index.
        state
            .set_inline_style(top, "z-index".into(), "1".into())
            .unwrap();
        state
            .set_inline_style(bottom, "z-index".into(), "0".into())
            .unwrap();
        build_layout(&mut state, root);

        let root_id = NodeId::from(root as u64);
        assert_eq!(
            hit_test_at_point(&state.doc, root_id, pt(50.0, 50.0)),
            Some(NodeId::from(top as u64))
        );
        // Confirm the bottom node is still in the document; it just lost the hit.
        assert!(state.doc.get_node(NodeId::from(bottom as u64)).is_some());
    }

    /// Negative z-index siblings paint behind the parent's content; a sibling
    /// with z-index 0 (positioned) hits before a negative-z sibling.
    #[test]
    fn negative_z_loses_to_zero_z() {
        let mut state = RuntimeState::new("https://test.example".into());
        let root = state.create_element("div".into());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();
        state
            .set_inline_style(root, "z-index".into(), "0".into())
            .unwrap();
        state
            .set_inline_style(root, "width".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(root, "height".into(), "200px".into())
            .unwrap();

        let neg = add_styled_child(
            &mut state,
            root,
            &[
                ("position", "absolute"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "100px"),
                ("height", "100px"),
                ("z-index", "-1"),
            ],
        );
        let zero = add_styled_child(
            &mut state,
            root,
            &[
                ("position", "absolute"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "100px"),
                ("height", "100px"),
                ("z-index", "0"),
            ],
        );
        build_layout(&mut state, root);

        let root_id = NodeId::from(root as u64);
        assert_eq!(
            hit_test_at_point(&state.doc, root_id, pt(50.0, 50.0)),
            Some(NodeId::from(zero as u64))
        );
        let _ = neg;
    }

    /// Shadow DOM: the slot's flat-tree child (the slotted light-DOM node)
    /// should be returned, not the `<slot>` element itself.
    #[test]
    fn shadow_slotted_child_is_hit() {
        let mut state = RuntimeState::new("https://test.example".into());
        let host = state.create_element("div".into());
        state.append_element(0, host).unwrap();
        state
            .set_inline_style(host, "display".into(), "block".into())
            .unwrap();
        state
            .set_inline_style(host, "width".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(host, "height".into(), "100px".into())
            .unwrap();

        let shadow_root = state.attach_shadow(host, "open").unwrap();

        let slot = state.create_element("slot".into());
        state.append_element(shadow_root, slot).unwrap();
        state
            .set_inline_style(slot, "display".into(), "block".into())
            .unwrap();
        state
            .set_inline_style(slot, "width".into(), "200px".into())
            .unwrap();
        state
            .set_inline_style(slot, "height".into(), "100px".into())
            .unwrap();

        let light = add_styled_child(&mut state, host, &[("width", "200px"), ("height", "100px")]);

        state.commit();

        let host_id = NodeId::from(host as u64);
        let hit = hit_test_at_point(&state.doc, host_id, pt(50.0, 50.0));
        assert_eq!(
            hit,
            Some(NodeId::from(light as u64)),
            "slotted light-DOM child should be the hit, not the <slot> element"
        );
        // The slot element must never be the answer.
        assert_ne!(hit, Some(NodeId::from(slot as u64)));
    }

    /// Empty document (only the root node 0) returns `None`.
    #[test]
    fn empty_document_returns_none() {
        let state = RuntimeState::new("https://test.example".into());
        let root_id = NodeId::from(0u64);
        assert_eq!(hit_test_at_point(&state.doc, root_id, pt(10.0, 10.0)), None);
    }

    /// Full pipeline: hit-test → engine dispatch with capture and bubble
    /// listeners on the path. Confirms the returned NodeId is what the
    /// existing event-dispatch infrastructure expects as `target_id`.
    #[test]
    fn hit_then_dispatch_runs_full_propagation_path() {
        let mut state = RuntimeState::new("https://test.example".into());

        // Root → inner (the click target).
        let root = state.create_element("div".into());
        state.append_element(0, root).unwrap();
        state
            .set_inline_style(root, "width".into(), "300px".into())
            .unwrap();
        state
            .set_inline_style(root, "height".into(), "300px".into())
            .unwrap();
        state
            .set_inline_style(root, "position".into(), "relative".into())
            .unwrap();

        let inner = add_styled_child(
            &mut state,
            root,
            &[
                ("position", "absolute"),
                ("left", "50px"),
                ("top", "50px"),
                ("width", "100px"),
                ("height", "100px"),
            ],
        );
        build_layout(&mut state, root);

        // Capture + bubble listeners on root, at-target on inner.
        let click = Atom::from("click");
        state
            .add_event_listener(
                root,
                click.clone(),
                10,
                ListenerOptions {
                    capture: true,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();
        state
            .add_event_listener(
                root,
                click.clone(),
                20,
                ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();
        state
            .add_event_listener(
                inner,
                click.clone(),
                30,
                ListenerOptions {
                    capture: false,
                    passive: false,
                    once: false,
                },
            )
            .unwrap();

        let target = hit_test_at_point(&state.doc, NodeId::from(root as u64), pt(100.0, 100.0))
            .expect("hit-test should resolve a node");
        assert_eq!(target, NodeId::from(inner as u64));

        let mut event = Event::new(click.clone(), true, true, true);
        let mut firings: Vec<(u32, EventPhase)> = Vec::new();
        let not_canceled =
            dispatch_event_with_callback(&mut state.doc, target, &mut event, |cb_id, ev| {
                firings.push((cb_id, ev.event_phase));
            });
        assert!(not_canceled);
        assert_eq!(
            firings,
            vec![
                (10, EventPhase::Capturing),
                (30, EventPhase::AtTarget),
                (20, EventPhase::Bubbling),
            ],
            "capture → at-target → bubble order must match W3C dispatch"
        );
    }
}
