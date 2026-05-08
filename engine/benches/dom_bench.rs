//! Engine-level DOM mutation benchmarks.
//!
//! These benches isolate tree mutation cost from style resolution and layout.

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

use engine::{NodeId, RuntimeState};

const CHILD_COUNT: usize = 1_000;

fn setup_parent_with_orphans(child_count: usize) -> (RuntimeState, u32, Vec<u32>) {
    let mut state = RuntimeState::new("https://bench.test".to_string());
    let parent = state.create_element("div".to_string());
    state.append_element(0, parent).unwrap();

    let mut children = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        children.push(state.create_element("div".to_string()));
    }

    (state, parent, children)
}

fn setup_parent_with_children(child_count: usize) -> (RuntimeState, u32, Vec<u32>) {
    let (mut state, parent, children) = setup_parent_with_orphans(child_count);
    state.append_elements(parent, &children).unwrap();
    (state, parent, children)
}

fn parent_child_count(state: &RuntimeState, parent: u32) -> usize {
    state
        .doc
        .get_node(NodeId::from(parent as u64))
        .map(|node| node.children.len())
        .unwrap_or_default()
}

fn bench_append_elements_batch_1000_orphans(c: &mut Criterion) {
    c.bench_function("append_elements_batch_1000_orphans", |b| {
        b.iter_batched(
            || setup_parent_with_orphans(CHILD_COUNT),
            |(mut state, parent, children)| {
                state
                    .append_elements(black_box(parent), black_box(&children))
                    .unwrap();
                black_box(parent_child_count(&state, parent));
            },
            BatchSize::LargeInput,
        )
    });
}

fn bench_append_elements_reappend_1000_existing_children(c: &mut Criterion) {
    c.bench_function("append_elements_reappend_1000_existing_children", |b| {
        b.iter_batched(
            || setup_parent_with_children(CHILD_COUNT),
            |(mut state, parent, children)| {
                state
                    .append_elements(black_box(parent), black_box(&children))
                    .unwrap();
                black_box(parent_child_count(&state, parent));
            },
            BatchSize::LargeInput,
        )
    });
}

criterion_group!(
    benches,
    bench_append_elements_batch_1000_orphans,
    bench_append_elements_reappend_1000_existing_children,
);
criterion_main!(benches);
