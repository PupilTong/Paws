use ios_renderer_backend::ffi::*;
use ios_renderer_backend::types::*;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn make_scroll_node(id: NodeId, generation: u64, children: Vec<LayoutNode>) -> LayoutNode {
    LayoutNode {
        id,
        frame: Rect {
            x: 0.,
            y: 0.,
            width: 100.,
            height: 100.,
        },
        children,
        scroll: Some(ScrollProps {
            content_size: Size {
                width: 500.,
                height: 500.,
            },
            overflow_x: Overflow::Scroll,
            overflow_y: Overflow::Scroll,
        }),
        style: ComputedStyle {
            opacity: 1.0,
            transform: None,
            clip: None,
            background: Color {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 1.,
            },
            border_radius: 0.0,
            will_change: false,
        },
        generation,
    }
}

#[test]
fn test_integration_frames() {
    let _profiler = dhat::Profiler::builder().testing().build();

    let handle = rb_create(1024);

    // Initial frame
    let n4 = make_scroll_node(4, 1, vec![]); // This node will actually get moved by n3's scroll
    let n3 = make_scroll_node(3, 1, vec![n4]);
    let n2 = make_scroll_node(2, 1, vec![n3]);
    let n1 = make_scroll_node(1, 1, vec![n2]);

    rb_submit_layout(handle, &n1 as *const LayoutNode, 3);

    let mut cmds = vec![LayerCmd::RemoveLayer { id: 0 }; 1024];
    let mut count = 0;

    rb_render_frame(handle, 0, cmds.as_mut_ptr(), &mut count);
    assert!(count > 0);

    // Frame 2: Update innermost scroll offset
    let stats_before = dhat::HeapStats::get();

    rb_update_scroll_offset(handle, 3, 10.0, 10.0);

    let mut count2 = 0;
    rb_render_frame(handle, 16, cmds.as_mut_ptr(), &mut count2);

    let stats_after = dhat::HeapStats::get();

    // Assert ZERO heap allocations on the hot path (blocks_created should not increase)
    if stats_after.total_blocks > stats_before.total_blocks {
        println!(
            "Heap allocation detected on hot path! (Blocks: {})",
            stats_after.total_blocks - stats_before.total_blocks
        );
    }

    assert_eq!(
        count2, 0,
        "Expected 0 commands on pure scroll update inside bounds"
    );

    rb_destroy(handle);
}
