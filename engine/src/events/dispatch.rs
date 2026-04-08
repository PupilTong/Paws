use crate::runtime::RenderState;
use stylo_atoms::Atom;
use taffy::NodeId;

use crate::dom::Document;
use crate::events::event::EventPhase;
use crate::events::Event;

/// Snapshot of listener data collected before invocation.
///
/// Decouples the dispatch loop from the node borrow so listeners can be
/// invoked without holding a reference to the DOM tree.
#[derive(Debug, Clone)]
pub struct ListenerSnapshot {
    pub callback_id: u32,
    pub passive: bool,
    pub once: bool,
    /// Index in the node's `event_listeners` Vec, used for post-dispatch cleanup.
    pub index: usize,
}

/// Builds the event propagation path from document root to target.
///
/// Returns `None` if any node in the ancestor chain is missing from the
/// document. The returned `Vec` is ordered root-first: `[root, ..., parent, target]`.
pub fn build_event_path<S: RenderState>(
    doc: &Document<S>,
    target_id: NodeId,
) -> Option<Vec<NodeId>> {
    let mut path = Vec::new();
    let mut current = Some(target_id);

    while let Some(id) = current {
        let node = doc.get_node(id)?;
        path.push(id);
        current = node.parent;
    }

    path.reverse();
    Some(path)
}

/// Collects listeners on `node_id` matching the event type and phase.
///
/// Phase-dependent filtering per W3C spec:
/// - `Capturing`: only listeners with `capture: true`
/// - `Bubbling`: only listeners with `capture: false`
/// - `AtTarget`: both capture and non-capture listeners
///
/// Skips entries where `removed` is `true`.
pub fn collect_matching_listeners<S: RenderState>(
    doc: &Document<S>,
    node_id: NodeId,
    event_type: &Atom,
    phase: EventPhase,
) -> Vec<ListenerSnapshot> {
    let Some(node) = doc.get_node(node_id) else {
        return Vec::new();
    };

    node.event_listeners
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            if l.removed || l.event_type != *event_type {
                return false;
            }
            match phase {
                EventPhase::Capturing => l.capture,
                EventPhase::Bubbling => !l.capture,
                EventPhase::AtTarget => true,
                EventPhase::None => false,
            }
        })
        .map(|(i, l)| ListenerSnapshot {
            callback_id: l.callback_id,
            passive: l.passive,
            once: l.once,
            index: i,
        })
        .collect()
}

/// Dispatches an event through the three-phase algorithm using a callback
/// closure for listener invocation.
///
/// This is the pure-Rust dispatch path used for engine-level tests. The
/// wasmtime layer implements its own dispatch loop to handle wasmtime's
/// borrow model and re-entrant WASM calls.
///
/// `invoke` is called for each matching listener with `(callback_id, &mut Event)`.
/// The closure can mutate event flags (e.g. `stopPropagation`, `preventDefault`)
/// to simulate what WASM handlers do via host functions.
///
/// Returns `true` if the event was NOT canceled (i.e. `!defaultPrevented`).
pub fn dispatch_event_with_callback<S, F>(
    doc: &mut Document<S>,
    target_id: NodeId,
    event: &mut Event,
    mut invoke: F,
) -> bool
where
    S: RenderState,
    F: FnMut(u32, &mut Event),
{
    // 1. Build event path
    let path = match build_event_path(doc, target_id) {
        Some(p) => p,
        None => return true,
    };

    let target_index = path.len() - 1;

    // 2. Initialize event
    event.target = Some(target_id);
    event.dispatch_flag = true;
    event.event_phase = EventPhase::None;

    // 3. Capture phase: path[0..target_index] (root to parent-of-target)
    for &node_id in &path[..target_index] {
        if event.stop_propagation_flag {
            break;
        }
        event.event_phase = EventPhase::Capturing;
        event.current_target = Some(node_id);

        invoke_listeners_on_node(doc, node_id, event, &mut invoke);
    }

    // 4. At-target phase
    if !event.stop_propagation_flag {
        event.event_phase = EventPhase::AtTarget;
        event.current_target = Some(target_id);

        invoke_listeners_on_node(doc, target_id, event, &mut invoke);
    }

    // 5. Bubble phase (only if event.bubbles)
    if event.bubbles && !event.stop_propagation_flag {
        // Walk from parent-of-target back to root
        for i in (0..target_index).rev() {
            if event.stop_propagation_flag {
                break;
            }
            event.event_phase = EventPhase::Bubbling;
            event.current_target = Some(path[i]);

            invoke_listeners_on_node(doc, path[i], event, &mut invoke);
        }
    }

    // 6. Finalize
    event.dispatch_flag = false;
    event.event_phase = EventPhase::None;
    event.current_target = None;

    // 7. Clean up removed listeners (once listeners marked during dispatch)
    cleanup_removed_listeners(doc, &path);

    !event.default_prevented()
}

/// Collects and invokes matching listeners on a single node.
///
/// Handles `once` marking and `stopImmediatePropagation`.
fn invoke_listeners_on_node<S, F>(
    doc: &mut Document<S>,
    node_id: NodeId,
    event: &mut Event,
    invoke: &mut F,
) where
    S: RenderState,
    F: FnMut(u32, &mut Event),
{
    let listeners = collect_matching_listeners(doc, node_id, &event.event_type, event.event_phase);

    for snap in &listeners {
        // Re-check removed flag (a previous listener on the same node may
        // have called removeEventListener)
        if let Some(node) = doc.get_node(node_id) {
            if node
                .event_listeners
                .get(snap.index)
                .is_none_or(|l| l.removed)
            {
                continue;
            }
        } else {
            break;
        }

        // Mark `once` listeners for removal
        if snap.once {
            if let Some(node) = doc.get_node_mut(node_id) {
                if let Some(entry) = node.event_listeners.get_mut(snap.index) {
                    entry.removed = true;
                }
            }
        }

        // Set passive flag for preventDefault checking
        event.in_passive_listener = snap.passive;

        invoke(snap.callback_id, event);

        event.in_passive_listener = false;

        if event.stop_immediate_propagation_flag {
            break;
        }
    }
}

/// Removes all listener entries marked as `removed` from nodes in the path.
fn cleanup_removed_listeners<S: RenderState>(doc: &mut Document<S>, path: &[NodeId]) {
    for &node_id in path {
        if let Some(node) = doc.get_node_mut(node_id) {
            node.event_listeners.retain(|l| !l.removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ListenerOptions;
    use crate::runtime::RuntimeState;

    fn setup_tree() -> (RuntimeState, u32, u32, u32) {
        let mut state = RuntimeState::new("https://example.com".to_string());
        let grandparent = state.create_element("div".to_string());
        let parent = state.create_element("section".to_string());
        let child = state.create_element("span".to_string());

        // Attach to document root: root -> grandparent -> parent -> child
        state.append_element(0, grandparent).unwrap();
        state.append_element(grandparent, parent).unwrap();
        state.append_element(parent, child).unwrap();

        (state, grandparent, parent, child)
    }

    fn opts(capture: bool, passive: bool, once: bool) -> ListenerOptions {
        ListenerOptions {
            capture,
            passive,
            once,
        }
    }

    #[test]
    fn test_build_event_path() {
        let (state, grandparent, parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        let path = build_event_path(&state.doc, child_nid).unwrap();

        // root (0) -> grandparent -> parent -> child
        assert_eq!(path.len(), 4);
        assert_eq!(path[0], NodeId::from(0u64));
        assert_eq!(path[1], NodeId::from(grandparent as u64));
        assert_eq!(path[2], NodeId::from(parent as u64));
        assert_eq!(path[3], child_nid);
    }

    #[test]
    fn test_capture_at_target_bubble_order() {
        let (mut state, grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);
        let grandparent_nid = NodeId::from(grandparent as u64);

        // Capture listener on grandparent
        state
            .add_event_listener(
                grandparent,
                Atom::from("click"),
                1,
                opts(true, false, false),
            )
            .unwrap();
        // Bubble listener on grandparent
        state
            .add_event_listener(
                grandparent,
                Atom::from("click"),
                2,
                opts(false, false, false),
            )
            .unwrap();
        // At-target listener on child
        state
            .add_event_listener(child, Atom::from("click"), 3, opts(false, false, false))
            .unwrap();

        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<(u32, NodeId, EventPhase)> = Vec::new();

        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, ev| {
            invocations.push((cb_id, ev.current_target.unwrap(), ev.event_phase));
        });

        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[0], (1, grandparent_nid, EventPhase::Capturing));
        assert_eq!(invocations[1], (3, child_nid, EventPhase::AtTarget));
        assert_eq!(invocations[2], (2, grandparent_nid, EventPhase::Bubbling));
    }

    #[test]
    fn test_stop_propagation_halts_bubbling() {
        let (mut state, grandparent, parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        // Listener on parent (bubble)
        state
            .add_event_listener(parent, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();
        // Listener on grandparent (bubble) — should NOT fire
        state
            .add_event_listener(
                grandparent,
                Atom::from("click"),
                2,
                opts(false, false, false),
            )
            .unwrap();

        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<u32> = Vec::new();

        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, ev| {
            invocations.push(cb_id);
            if cb_id == 1 {
                ev.stop_propagation_flag = true;
            }
        });

        // Only parent listener fires; grandparent is skipped
        assert_eq!(invocations, vec![1]);
    }

    #[test]
    fn test_stop_immediate_propagation() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        // Two listeners on child
        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();
        state
            .add_event_listener(child, Atom::from("click"), 2, opts(false, false, false))
            .unwrap();

        // Without stopImmediatePropagation: both fire
        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, _ev| {
            invocations.push(cb_id);
        });
        assert_eq!(invocations, vec![1, 2]);

        // With stopImmediatePropagation after first: only first fires
        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, ev| {
            invocations.push(cb_id);
            if cb_id == 1 {
                ev.stop_immediate_propagation_flag = true;
                ev.stop_propagation_flag = true;
            }
        });
        assert_eq!(invocations, vec![1]);
    }

    #[test]
    fn test_prevent_default_cancelable() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();

        // Cancelable event with preventDefault
        let mut event = Event::new(Atom::from("click"), true, true, false);
        let result =
            dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |_cb_id, ev| {
                if ev.cancelable && !ev.in_passive_listener {
                    ev.canceled_flag = true;
                }
            });
        assert!(!result); // canceled → returns false
    }

    #[test]
    fn test_prevent_default_non_cancelable() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();

        // Non-cancelable event: preventDefault should be a no-op
        let mut event = Event::new(Atom::from("click"), true, false, false);
        let result =
            dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |_cb_id, ev| {
                if ev.cancelable && !ev.in_passive_listener {
                    ev.canceled_flag = true;
                }
            });
        assert!(result); // not canceled → returns true
    }

    #[test]
    fn test_once_auto_removal() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, true))
            .unwrap();

        // Verify listener exists
        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            1
        );

        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, _ev| {
            invocations.push(cb_id);
        });
        assert_eq!(invocations, vec![1]);

        // After dispatch, the once listener should be removed
        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            0
        );

        // Dispatch again — nothing fires
        let mut event = Event::new(Atom::from("click"), true, true, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, _ev| {
            invocations.push(cb_id);
        });
        assert!(invocations.is_empty());
    }

    #[test]
    fn test_non_bubbling_event() {
        let (mut state, grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        // Bubble listener on grandparent — should NOT fire for non-bubbling event
        state
            .add_event_listener(
                grandparent,
                Atom::from("focus"),
                1,
                opts(false, false, false),
            )
            .unwrap();
        // At-target listener on child
        state
            .add_event_listener(child, Atom::from("focus"), 2, opts(false, false, false))
            .unwrap();

        let mut event = Event::new(Atom::from("focus"), false, false, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, _ev| {
            invocations.push(cb_id);
        });

        assert_eq!(invocations, vec![2]);
    }

    #[test]
    fn test_capture_listener_fires_for_non_bubbling() {
        let (mut state, grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        // Capture listener on grandparent — SHOULD fire even for non-bubbling
        state
            .add_event_listener(
                grandparent,
                Atom::from("focus"),
                1,
                opts(true, false, false),
            )
            .unwrap();
        // At-target on child
        state
            .add_event_listener(child, Atom::from("focus"), 2, opts(false, false, false))
            .unwrap();

        let mut event = Event::new(Atom::from("focus"), false, false, false);
        let mut invocations: Vec<u32> = Vec::new();
        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |cb_id, _ev| {
            invocations.push(cb_id);
        });

        assert_eq!(invocations, vec![1, 2]);
    }

    #[test]
    fn test_passive_listener_flag() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("touchmove"), 1, opts(false, true, false))
            .unwrap();

        let mut event = Event::new(Atom::from("touchmove"), true, true, false);
        let mut was_passive = false;

        dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |_cb_id, ev| {
            was_passive = ev.in_passive_listener;
            // Passive listener: preventDefault should be ignored
            if ev.cancelable && !ev.in_passive_listener {
                ev.canceled_flag = true;
            }
        });

        assert!(was_passive);
        assert!(!event.default_prevented()); // not canceled because passive
    }

    #[test]
    fn test_collect_listeners_phase_filtering() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(true, false, false))
            .unwrap();
        state
            .add_event_listener(child, Atom::from("click"), 2, opts(false, false, false))
            .unwrap();

        let cap = collect_matching_listeners(
            &state.doc,
            child_nid,
            &Atom::from("click"),
            EventPhase::Capturing,
        );
        assert_eq!(cap.len(), 1);
        assert_eq!(cap[0].callback_id, 1);

        let bub = collect_matching_listeners(
            &state.doc,
            child_nid,
            &Atom::from("click"),
            EventPhase::Bubbling,
        );
        assert_eq!(bub.len(), 1);
        assert_eq!(bub[0].callback_id, 2);

        let at = collect_matching_listeners(
            &state.doc,
            child_nid,
            &Atom::from("click"),
            EventPhase::AtTarget,
        );
        assert_eq!(at.len(), 2);
    }

    #[test]
    fn test_listener_dedup() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        // Add same listener twice — should be deduplicated
        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();
        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();

        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            1
        );

        // Different capture flag = different listener
        state
            .add_event_listener(child, Atom::from("click"), 1, opts(true, false, false))
            .unwrap();

        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            2
        );
    }

    #[test]
    fn test_remove_listener() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();
        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            1
        );

        state
            .remove_event_listener(child, Atom::from("click"), 1, false)
            .unwrap();
        assert_eq!(
            state.doc.get_node(child_nid).unwrap().event_listeners.len(),
            0
        );
    }

    #[test]
    fn test_dispatch_returns_true_when_not_canceled() {
        let (mut state, _grandparent, _parent, child) = setup_tree();
        let child_nid = NodeId::from(child as u64);

        state
            .add_event_listener(child, Atom::from("click"), 1, opts(false, false, false))
            .unwrap();

        let mut event = Event::new(Atom::from("click"), true, true, false);
        let result =
            dispatch_event_with_callback(&mut state.doc, child_nid, &mut event, |_cb_id, _ev| {});
        assert!(result);
    }
}
