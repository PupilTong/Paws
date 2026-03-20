use ios_renderer_backend::ffi::*;
use ios_renderer_backend::types::*;
use std::mem;

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

// ── Demo layout frame correctness ────────────────────────────────────

#[test]
fn demo_layout_produces_correct_50x50_frames() {
    let handle = rb_create(1024);

    // Submit the demo layout and render (push model triggers internal render).
    rb_submit_demo_layout(handle, 390.0, 844.0);

    // Pull-model render to capture commands.
    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };

    let cmds = &cmds[..count as usize];

    // Collect UpdateLayer commands for the 4 rows (ids 100-103).
    for row_id in 100u64..104u64 {
        let update = cmds
            .iter()
            .find(|c| matches!(c, LayerCmd::UpdateLayer { id, .. } if *id == row_id));
        assert!(update.is_some(), "expected UpdateLayer for row id={row_id}");
        if let Some(LayerCmd::UpdateLayer { props, .. }) = update {
            assert_eq!(
                props.frame.width, 50.0,
                "row {row_id} width should be 50, got {}",
                props.frame.width
            );
            assert_eq!(
                props.frame.height, 50.0,
                "row {row_id} height should be 50, got {}",
                props.frame.height
            );
            assert!(
                props.background.a > 0.0,
                "row {row_id} should have non-transparent background"
            );
        }
    }

    // Scroll container should be created as ScrollView.
    let scroll_create = cmds.iter().find(|c| {
        matches!(
            c,
            LayerCmd::CreateLayer {
                id: 2,
                kind: LayerKind::ScrollView
            }
        )
    });
    assert!(
        scroll_create.is_some(),
        "scroll container (id=2) should be created as ScrollView"
    );

    // Rows should be reparented to scroll container.
    for row_id in 100u64..104u64 {
        let reparent = cmds.iter().find(
            |c| matches!(c, LayerCmd::ReparentLayer { id, new_parent: 2, .. } if *id == row_id),
        );
        assert!(
            reparent.is_some(),
            "row {row_id} should be reparented to scroll container (id=2)"
        );
    }

    rb_destroy(handle);
}

// ── Struct size/offset assertions (C FFI correctness) ────────────────

#[test]
fn struct_sizes_match_c_layout() {
    assert_eq!(mem::size_of::<Rect>(), 16, "Rect: 4 × f32 = 16");
    assert_eq!(mem::size_of::<Color>(), 16, "Color: 4 × f32 = 16");
    assert_eq!(mem::size_of::<Size>(), 8, "Size: 2 × f32 = 8");
    assert_eq!(
        mem::size_of::<Transform3D>(),
        64,
        "Transform3D: 16 × f32 = 64"
    );
    assert_eq!(mem::size_of::<LayerProps>(), 128, "LayerProps total size");
}

#[test]
fn layer_props_field_offsets_are_correct() {
    assert_eq!(mem::offset_of!(LayerProps, frame), 0);
    assert_eq!(mem::offset_of!(LayerProps, opacity), 16);
    assert_eq!(mem::offset_of!(LayerProps, background), 20);
    assert_eq!(mem::offset_of!(LayerProps, border_radius), 36);
    assert_eq!(mem::offset_of!(LayerProps, has_transform), 40);
    assert_eq!(mem::offset_of!(LayerProps, transform), 44);
    assert_eq!(mem::offset_of!(LayerProps, has_clip), 108);
    assert_eq!(mem::offset_of!(LayerProps, clip), 112);
}

// ── First frame emits create+update for all visible nodes ────────────

#[test]
fn first_frame_emits_all_nodes_with_correct_frames() {
    let handle = rb_create(1024);

    let child = LayoutNode {
        id: 2,
        frame: Rect {
            x: 10.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
        },
        children: vec![],
        scroll: None,
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation: 1,
    };
    let root = LayoutNode {
        id: 1,
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        },
        children: vec![child],
        scroll: None,
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation: 1,
    };

    // SAFETY: handle valid, root is valid LayoutNode.
    unsafe { rb_submit_layout(handle, &root as *const LayoutNode, 2) };

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0u32;
    // SAFETY: handle valid, buffer valid.
    unsafe { rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count) };

    let cmds = &cmds[..count as usize];

    // Both nodes should have CreateLayer + UpdateLayer.
    let creates: Vec<u64> = cmds
        .iter()
        .filter_map(|c| match c {
            LayerCmd::CreateLayer { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert!(creates.contains(&1), "root should have CreateLayer");
    assert!(creates.contains(&2), "child should have CreateLayer");

    // Child UpdateLayer should have correct absolute frame.
    let child_update = cmds
        .iter()
        .find(|c| matches!(c, LayerCmd::UpdateLayer { id: 2, .. }))
        .expect("child should have UpdateLayer");
    if let LayerCmd::UpdateLayer { props, .. } = child_update {
        assert_eq!(props.frame.x, 10.0, "child absolute x");
        assert_eq!(props.frame.y, 20.0, "child absolute y");
        assert_eq!(props.frame.width, 80.0, "child width");
        assert_eq!(props.frame.height, 60.0, "child height");
        assert_eq!(props.background.r, 1.0, "child bg red");
    }

    rb_destroy(handle);
}
