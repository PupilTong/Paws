//! Engine-level layout benchmarks.
//!
//! These benchmarks measure pure layout performance without WASM overhead,
//! isolating the Taffy trait implementation on `Document`.

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};

use engine::layout::compute_layout_in_place;
use engine::{NodeId, RuntimeState};

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
            )
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
);
criterion_main!(benches);
