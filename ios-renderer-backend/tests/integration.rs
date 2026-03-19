use ios_renderer_backend::ffi::*;
use ios_renderer_backend::types::*;

fn make_scroll_node(id: NodeId, generation: u64, children: Vec<LayoutNode>) -> LayoutNode {
    LayoutNode {
        id,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        children,
        scroll: Some(ScrollProps {
            content_size: Size {
                width: 500.0,
                height: 500.0,
            },
            overflow_x: Overflow::Scroll,
            overflow_y: Overflow::Scroll,
        }),
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation,
    }
}

#[test]
fn initial_frame_produces_commands() {
    let handle = rb_create(1024);

    let n4 = make_scroll_node(4, 1, vec![]);
    let n3 = make_scroll_node(3, 1, vec![n4]);
    let n2 = make_scroll_node(2, 1, vec![n3]);
    let n1 = make_scroll_node(1, 1, vec![n2]);

    // SAFETY: handle is valid from rb_create, root points to valid LayoutNode.
    unsafe { rb_submit_layout(handle, &n1 as *const LayoutNode, 4) };

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;

    // SAFETY: handle valid, cmds has 1024 entries, count is valid u32.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert!(count > 0, "Initial frame must produce commands");

    rb_destroy(handle);
}

#[test]
fn scroll_update_unchanged_generation_produces_no_diff() {
    let handle = rb_create(1024);

    let n4 = make_scroll_node(4, 1, vec![]);
    let n3 = make_scroll_node(3, 1, vec![n4]);
    let n2 = make_scroll_node(2, 1, vec![n3]);
    let n1 = make_scroll_node(1, 1, vec![n2]);

    // SAFETY: handle is valid, root points to valid LayoutNode.
    unsafe { rb_submit_layout(handle, &n1 as *const LayoutNode, 4) };

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;

    // Initial frame
    // SAFETY: handle valid, cmds has 1024 entries, count is valid u32.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };

    // Update scroll offset (no generation change)
    rb_update_scroll_offset(handle, 3, 10.0, 10.0);

    let mut count2 = 0u32;
    // SAFETY: handle valid, cmds has 1024 entries, count2 is valid u32.
    unsafe { rb_render_frame(handle, 16, cmds.as_mut_ptr(), &mut count2) };

    assert_eq!(
        count2, 0,
        "Expected 0 commands on pure scroll update with unchanged generation"
    );

    rb_destroy(handle);
}

#[test]
fn empty_layout_produces_no_commands() {
    let handle = rb_create(256);

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 256];
    let mut count = 0u32;

    // SAFETY: handle valid, cmds has 256 entries, count is valid u32.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert_eq!(count, 0, "Empty layout should produce no commands");

    rb_destroy(handle);
}

#[test]
fn generation_bump_with_prop_change_produces_update() {
    let handle = rb_create(1024);

    // Frame 1: initial tree with opacity 1.0
    let root = make_scroll_node(1, 1, vec![]);
    // SAFETY: handle valid, root is valid LayoutNode.
    unsafe { rb_submit_layout(handle, &root as *const LayoutNode, 1) };

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert!(count > 0, "Initial frame should produce commands");

    // Frame 2: bump generation AND change opacity
    let mut root2 = make_scroll_node(1, 2, vec![]);
    root2.style.opacity = 0.5;
    // SAFETY: handle valid, root2 is valid LayoutNode.
    unsafe { rb_submit_layout(handle, &root2 as *const LayoutNode, 1) };

    let mut count2 = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 16, cmds.as_mut_ptr(), &mut count2) };

    assert!(
        count2 > 0,
        "Generation bump with property change should produce update commands"
    );

    rb_destroy(handle);
}

#[test]
fn node_removal_produces_remove_command() {
    let handle = rb_create(1024);

    // Frame 1: tree with child
    let child = make_scroll_node(2, 1, vec![]);
    let root = make_scroll_node(1, 1, vec![child]);
    // SAFETY: handle valid, root is valid LayoutNode.
    unsafe { rb_submit_layout(handle, &root as *const LayoutNode, 2) };

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };
    assert!(count > 0);

    // Frame 2: remove child, bump generation
    let root2 = make_scroll_node(1, 2, vec![]);
    // SAFETY: handle valid, root2 is valid LayoutNode.
    unsafe { rb_submit_layout(handle, &root2 as *const LayoutNode, 1) };

    let mut count2 = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 16, cmds.as_mut_ptr(), &mut count2) };

    // Should include a RemoveLayer for the removed child
    let remove_cmds: Vec<_> = cmds[..count2 as usize]
        .iter()
        .filter(|c| matches!(c, LayerCmd::RemoveLayer { .. }))
        .collect();
    assert!(
        !remove_cmds.is_empty(),
        "Removing a child should produce RemoveLayer commands"
    );

    rb_destroy(handle);
}
