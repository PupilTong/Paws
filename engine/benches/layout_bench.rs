//! Engine-level layout benchmarks.
//!
//! These benches intentionally measure warm pure-layout cost on pre-built,
//! pre-styled trees without WASM overhead. Use `commit_bench` for cold setup
//! and end-to-end commit scenarios that include selector matching.

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};
use taffy::prelude::TaffyMaxContent;

use engine::layout::compute_layout_in_place;
use engine::{hit_test_at_point, paint_order_children, NodeId, RuntimeState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Creates a RuntimeState with a flex container and `n` styled children.
fn setup_flex_tree(n: usize) -> (RuntimeState, u32) {
    let mut state = RuntimeState::new("https://bench.test".to_string());
    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_inline_style(root, "display".into(), "flex".into())
        .unwrap();
    state
        .set_inline_style(root, "flex-direction".into(), "row".into())
        .unwrap();
    state
        .set_inline_style(root, "width".into(), "800px".into())
        .unwrap();
    state
        .set_inline_style(root, "height".into(), "600px".into())
        .unwrap();
    state
        .set_inline_style(root, "padding".into(), "10px".into())
        .unwrap();

    for _ in 0..n {
        let child = state.create_element("div".to_string());
        state.append_element(root, child).unwrap();
        state
            .set_inline_style(child, "flex-grow".into(), "1".into())
            .unwrap();
        state
            .set_inline_style(child, "height".into(), "100px".into())
            .unwrap();
        state
            .set_inline_style(child, "margin".into(), "5px".into())
            .unwrap();
    }

    state.doc.resolve_style(&state.style_context);
    (state, root)
}

/// Creates a RuntimeState with a deeply nested chain of `depth` block elements.
fn setup_deep_tree(depth: usize) -> (RuntimeState, u32) {
    let mut state = RuntimeState::new("https://bench.test".to_string());
    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_inline_style(root, "width".into(), "400px".into())
        .unwrap();

    let mut parent = root;
    for _ in 1..depth {
        let child = state.create_element("div".to_string());
        state.append_element(parent, child).unwrap();
        state
            .set_inline_style(child, "padding".into(), "2px".into())
            .unwrap();
        parent = child;
    }

    state.doc.resolve_style(&state.style_context);
    (state, root)
}

/// Creates a root containing one shadow host whose shadow root has:
///
/// `header`, `<slot>`, `footer`
///
/// The slot receives `assigned_children` light-DOM children. This exercises the
/// flat-tree path used by both Taffy traversal and paint-order traversal.
fn setup_shadow_slot_tree(assigned_children: usize) -> (RuntimeState, u32, u32) {
    let mut state = RuntimeState::new("https://bench.test".to_string());

    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_inline_style(root, "display".into(), "block".into())
        .unwrap();
    state
        .set_inline_style(root, "width".into(), "800px".into())
        .unwrap();

    let host = state.create_element("div".to_string());
    state.append_element(root, host).unwrap();
    state
        .set_inline_style(host, "display".into(), "flex".into())
        .unwrap();
    state
        .set_inline_style(host, "width".into(), "800px".into())
        .unwrap();
    state
        .set_inline_style(host, "height".into(), "200px".into())
        .unwrap();

    let shadow_root = state.attach_shadow(host, "open").unwrap();

    let header = state.create_element("div".to_string());
    state.append_element(shadow_root, header).unwrap();
    state
        .set_inline_style(header, "width".into(), "20px".into())
        .unwrap();
    state
        .set_inline_style(header, "height".into(), "20px".into())
        .unwrap();

    let slot = state.create_element("slot".to_string());
    state.append_element(shadow_root, slot).unwrap();

    let footer = state.create_element("div".to_string());
    state.append_element(shadow_root, footer).unwrap();
    state
        .set_inline_style(footer, "width".into(), "20px".into())
        .unwrap();
    state
        .set_inline_style(footer, "height".into(), "20px".into())
        .unwrap();

    for _ in 0..assigned_children {
        let child = state.create_element("div".to_string());
        state.append_element(host, child).unwrap();
        state
            .set_inline_style(child, "width".into(), "10px".into())
            .unwrap();
        state
            .set_inline_style(child, "height".into(), "10px".into())
            .unwrap();
        state
            .set_inline_style(child, "margin".into(), "1px".into())
            .unwrap();
    }

    state.doc.resolve_style(&state.style_context);
    (state, root, host)
}

// ---------------------------------------------------------------------------
// 1. Flex layout — 5 children (comparable to wasm_flex_layout)
// ---------------------------------------------------------------------------
fn bench_flex_layout_5(c: &mut Criterion) {
    let (mut state, root) = setup_flex_tree(5);

    c.bench_function("flex_layout_5_children", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(root as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 2. Flex layout — 50 children (stress test)
// ---------------------------------------------------------------------------
fn bench_flex_layout_50(c: &mut Criterion) {
    let (mut state, root) = setup_flex_tree(50);

    c.bench_function("flex_layout_50_children", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(root as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 3. Deep nested block layout — 50 levels
// ---------------------------------------------------------------------------
fn bench_deep_block_layout(c: &mut Criterion) {
    let (mut state, root) = setup_deep_tree(50);

    c.bench_function("block_layout_depth_50", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(root as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 4. Grid layout — 3x3
// ---------------------------------------------------------------------------
fn bench_grid_layout_3x3(c: &mut Criterion) {
    let mut state = RuntimeState::new("https://bench.test".to_string());
    let grid = state.create_element("div".to_string());
    state.append_element(0, grid).unwrap();
    state
        .set_inline_style(grid, "display".into(), "grid".into())
        .unwrap();
    state
        .set_inline_style(grid, "grid-template-columns".into(), "1fr 1fr 1fr".into())
        .unwrap();
    state
        .set_inline_style(grid, "width".into(), "600px".into())
        .unwrap();
    state
        .set_inline_style(grid, "gap".into(), "10px".into())
        .unwrap();

    for _ in 0..9 {
        let cell = state.create_element("div".to_string());
        state.append_element(grid, cell).unwrap();
        state
            .set_inline_style(cell, "height".into(), "80px".into())
            .unwrap();
    }

    state.doc.resolve_style(&state.style_context);

    c.bench_function("grid_layout_3x3", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(grid as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 5. Mixed layout — nested flex + block
// ---------------------------------------------------------------------------
fn bench_mixed_layout(c: &mut Criterion) {
    let mut state = RuntimeState::new("https://bench.test".to_string());

    // Root flex container
    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_inline_style(root, "display".into(), "flex".into())
        .unwrap();
    state
        .set_inline_style(root, "width".into(), "1000px".into())
        .unwrap();
    state
        .set_inline_style(root, "height".into(), "800px".into())
        .unwrap();

    // 3 flex children, each containing 5 block children
    for _ in 0..3 {
        let col = state.create_element("div".to_string());
        state.append_element(root, col).unwrap();
        state
            .set_inline_style(col, "flex-grow".into(), "1".into())
            .unwrap();
        state
            .set_inline_style(col, "display".into(), "block".into())
            .unwrap();

        for _ in 0..5 {
            let row = state.create_element("div".to_string());
            state.append_element(col, row).unwrap();
            state
                .set_inline_style(row, "height".into(), "40px".into())
                .unwrap();
            state
                .set_inline_style(row, "margin".into(), "5px".into())
                .unwrap();
        }
    }

    state.doc.resolve_style(&state.style_context);

    c.bench_function("mixed_flex_block_layout", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(root as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 6. Shadow host layout — slot-distributed flat tree
// ---------------------------------------------------------------------------
fn bench_shadow_slot_layout_100(c: &mut Criterion) {
    let (mut state, root, _) = setup_shadow_slot_tree(100);

    c.bench_function("shadow_slot_layout_100_assigned_children", |b| {
        b.iter(|| {
            compute_layout_in_place(
                black_box(&mut state.doc),
                black_box(NodeId::from(root as u64)),
                taffy::Size::MAX_CONTENT,
            )
        })
    });
}

// ---------------------------------------------------------------------------
// 7. Shadow host paint order — slot-distributed flat tree
// ---------------------------------------------------------------------------
fn bench_shadow_slot_paint_order_100(c: &mut Criterion) {
    let (state, _, host) = setup_shadow_slot_tree(100);

    c.bench_function("shadow_slot_paint_order_100_assigned_children", |b| {
        b.iter(|| {
            black_box(paint_order_children(
                black_box(&state.doc),
                black_box(NodeId::from(host as u64)),
            ))
        })
    });
}

// ---------------------------------------------------------------------------
// 8. Hit-test — flex 50 children, hit the last one (worst-case z-order walk)
// ---------------------------------------------------------------------------
fn bench_hit_test_flex_50_hit_last(c: &mut Criterion) {
    let (mut state, root) = setup_flex_tree(50);
    compute_layout_in_place(
        &mut state.doc,
        NodeId::from(root as u64),
        taffy::Size::MAX_CONTENT,
    );
    // Pull the last child's centre out of the laid-out tree so the bench
    // doesn't need to know flex sizing rules; the point is whatever the
    // engine actually placed.
    let last_child_id = *state
        .doc
        .get_node(NodeId::from(root as u64))
        .unwrap()
        .children
        .last()
        .unwrap();
    let layout = state.doc.get_node(last_child_id).unwrap().layout();
    let centre = taffy::Point {
        x: layout.location.x + layout.size.width / 2.0,
        y: layout.location.y + layout.size.height / 2.0,
    };

    c.bench_function("hit_test_flex_50_hit_last", |b| {
        b.iter(|| {
            black_box(hit_test_at_point(
                black_box(&state.doc),
                black_box(NodeId::from(root as u64)),
                black_box(centre),
            ))
        })
    });
}

// ---------------------------------------------------------------------------
// 9. Hit-test — flex 50, point outside root (early-rejection path)
// ---------------------------------------------------------------------------
fn bench_hit_test_flex_50_miss(c: &mut Criterion) {
    let (mut state, root) = setup_flex_tree(50);
    compute_layout_in_place(
        &mut state.doc,
        NodeId::from(root as u64),
        taffy::Size::MAX_CONTENT,
    );
    let miss = taffy::Point {
        x: 100_000.0,
        y: 100_000.0,
    };

    c.bench_function("hit_test_flex_50_miss", |b| {
        b.iter(|| {
            black_box(hit_test_at_point(
                black_box(&state.doc),
                black_box(NodeId::from(root as u64)),
                black_box(miss),
            ))
        })
    });
}

// ---------------------------------------------------------------------------
// 10. Hit-test — 50-deep nested chain, point in the innermost leaf
// ---------------------------------------------------------------------------
fn bench_hit_test_deep_50_leaf(c: &mut Criterion) {
    let (mut state, root) = setup_deep_tree(50);
    compute_layout_in_place(
        &mut state.doc,
        NodeId::from(root as u64),
        taffy::Size::MAX_CONTENT,
    );
    // Walk to the innermost descendant.
    let mut leaf = NodeId::from(root as u64);
    while let Some(child) = state
        .doc
        .get_node(leaf)
        .and_then(|n| n.children.first())
        .copied()
    {
        leaf = child;
    }
    let layout = state.doc.get_node(leaf).unwrap().layout();
    // Layout location is parent-relative; recompute absolute via the
    // ancestor chain so the bench feeds in a viewport-space point.
    let mut absolute = layout.location;
    let mut walker = state.doc.get_node(leaf).and_then(|n| n.parent);
    while let Some(parent_id) = walker {
        if let Some(parent_node) = state.doc.get_node(parent_id) {
            absolute.x += parent_node.layout().location.x;
            absolute.y += parent_node.layout().location.y;
            walker = parent_node.parent;
        } else {
            break;
        }
    }
    let centre = taffy::Point {
        x: absolute.x + layout.size.width / 2.0,
        y: absolute.y + layout.size.height / 2.0,
    };

    c.bench_function("hit_test_deep_50_leaf", |b| {
        b.iter(|| {
            black_box(hit_test_at_point(
                black_box(&state.doc),
                black_box(NodeId::from(root as u64)),
                black_box(centre),
            ))
        })
    });
}

// ---------------------------------------------------------------------------
// 11. Hit-test — shadow-slot fixture, point inside a slotted child
// ---------------------------------------------------------------------------
fn bench_hit_test_shadow_slot_100_assigned(c: &mut Criterion) {
    let (mut state, root, host) = setup_shadow_slot_tree(100);
    compute_layout_in_place(
        &mut state.doc,
        NodeId::from(root as u64),
        taffy::Size::MAX_CONTENT,
    );
    // The first slotted light-DOM child is the first entry in the host's
    // children list (only light-DOM children live there directly).
    let first_assigned = *state
        .doc
        .get_node(NodeId::from(host as u64))
        .unwrap()
        .children
        .first()
        .unwrap();
    let host_layout = state
        .doc
        .get_node(NodeId::from(host as u64))
        .unwrap()
        .layout();
    let child_layout = state.doc.get_node(first_assigned).unwrap().layout();
    let centre = taffy::Point {
        x: host_layout.location.x + child_layout.location.x + child_layout.size.width / 2.0,
        y: host_layout.location.y + child_layout.location.y + child_layout.size.height / 2.0,
    };

    c.bench_function("hit_test_shadow_slot_100_assigned", |b| {
        b.iter(|| {
            black_box(hit_test_at_point(
                black_box(&state.doc),
                black_box(NodeId::from(root as u64)),
                black_box(centre),
            ))
        })
    });
}

// ---------------------------------------------------------------------------
// Criterion groups & main
// ---------------------------------------------------------------------------
criterion_group!(
    benches,
    bench_flex_layout_5,
    bench_flex_layout_50,
    bench_deep_block_layout,
    bench_grid_layout_3x3,
    bench_mixed_layout,
    bench_shadow_slot_layout_100,
    bench_shadow_slot_paint_order_100,
    bench_hit_test_flex_50_hit_last,
    bench_hit_test_flex_50_miss,
    bench_hit_test_deep_50_leaf,
    bench_hit_test_shadow_slot_100_assigned,
);
criterion_main!(benches);
