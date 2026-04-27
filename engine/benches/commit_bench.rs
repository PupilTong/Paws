//! Engine-level commit benchmarks for style-heavy trees.
//!
//! Unlike `layout_bench`, these benches exercise realistic end-to-end commit
//! paths: stylesheet install, selector matching, style invalidation, and
//! layout. They separate cold setup from warm mutation so performance changes
//! can be attributed to the correct phase.

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

use engine::RuntimeState;

const CARD_COUNT: usize = 120;
const ITEMS_PER_CARD: usize = 6;

fn complex_stylesheet() -> &'static [u8] {
    view_macros::css!(
        r#"
        .app {
            display: block;
            width: 1200px;
        }

        .app > .card {
            display: flex;
            flex-direction: column;
            margin: 4px;
            padding: 6px;
            border-top-width: 1px;
            border-top-style: solid;
        }

        .app > .card[data-kind="primary"] .item {
            padding: 2px;
        }

        .app > .card .list > .item:nth-child(2n + 1) {
            margin-left: 2px;
        }

        .app > .card.featured .list .item:not(.hidden) .badge {
            width: 18px;
            height: 18px;
        }

        .app > .card[data-kind="secondary"] .item[data-state="active"] {
            border-top-width: 2px;
            border-top-style: solid;
        }

        .group-a .label {
            width: 32px;
        }

        .list > .item + .item {
            margin-top: 1px;
        }

        @media (min-width: 900px) {
            .app > .card {
                flex-direction: row;
                gap: 4px;
            }

            .app > .card .item[data-state="active"] .badge {
                width: 20px;
            }
        }
    "#
    )
}

fn setup_complex_runtime() -> (RuntimeState, u32) {
    let mut state = RuntimeState::with_definite_viewport(
        "https://bench.test".to_string(),
        (),
        (),
        1280.0,
        800.0,
    );
    state.add_parsed_stylesheet(complex_stylesheet());

    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_attribute(root, "class".to_string(), "app".to_string())
        .unwrap();

    let mut toggle_target = None;

    for card_index in 0..CARD_COUNT {
        let card = state.create_element("div".to_string());
        state.append_element(root, card).unwrap();

        let card_class = if card_index % 3 == 0 {
            "card featured group-a"
        } else {
            "card group-b"
        };
        state
            .set_attribute(card, "class".to_string(), card_class.to_string())
            .unwrap();
        state
            .set_attribute(
                card,
                "data-kind".to_string(),
                if card_index % 2 == 0 {
                    "primary".to_string()
                } else {
                    "secondary".to_string()
                },
            )
            .unwrap();

        let list = state.create_element("div".to_string());
        state.append_element(card, list).unwrap();
        state
            .set_attribute(list, "class".to_string(), "list".to_string())
            .unwrap();

        for item_index in 0..ITEMS_PER_CARD {
            let item = state.create_element("div".to_string());
            state.append_element(list, item).unwrap();

            let item_class = if item_index % 4 == 0 {
                "item highlighted"
            } else {
                "item"
            };
            state
                .set_attribute(item, "class".to_string(), item_class.to_string())
                .unwrap();
            state
                .set_attribute(
                    item,
                    "data-state".to_string(),
                    if item_index % 2 == 0 {
                        "active".to_string()
                    } else {
                        "inactive".to_string()
                    },
                )
                .unwrap();
            if card_index == CARD_COUNT / 2 && item_index == ITEMS_PER_CARD / 2 {
                toggle_target = Some(item);
            }

            let badge = state.create_element("span".to_string());
            state.append_element(item, badge).unwrap();
            state
                .set_attribute(badge, "class".to_string(), "badge".to_string())
                .unwrap();

            let label = state.create_element("span".to_string());
            state.append_element(item, label).unwrap();
            state
                .set_attribute(label, "class".to_string(), "label".to_string())
                .unwrap();
        }
    }

    (state, toggle_target.expect("toggle target should exist"))
}

fn setup_committed_runtime_for_toggle() -> (RuntimeState, u32) {
    let (mut state, target) = setup_complex_runtime();
    state.commit();
    (state, target)
}

fn bench_commit_cold_complex_selectors(c: &mut Criterion) {
    c.bench_function("commit_cold_complex_selectors_120x6", |b| {
        b.iter_batched(
            setup_complex_runtime,
            |(mut state, _target)| {
                state.commit();
                black_box(state.doc.root_element_id());
            },
            BatchSize::LargeInput,
        )
    });
}

fn bench_resolve_style_cold_complex_selectors(c: &mut Criterion) {
    c.bench_function("resolve_style_cold_complex_selectors_120x6", |b| {
        b.iter_batched(
            setup_complex_runtime,
            |(mut state, _target)| {
                state.doc.resolve_style(&state.style_context);
                black_box(state.doc.root_element_id());
            },
            BatchSize::LargeInput,
        )
    });
}

/// Current invalidation marks the document root dirty on attribute changes, so
/// this still measures a full-tree restyle after toggling one item attribute.
fn bench_commit_full_restyle_after_data_state_toggle(c: &mut Criterion) {
    let (mut state, target) = setup_committed_runtime_for_toggle();
    let mut active = false;

    c.bench_function("commit_full_restyle_after_data_state_toggle_120x6", |b| {
        b.iter(|| {
            active = !active;
            state
                .set_attribute(
                    black_box(target),
                    "data-state".to_string(),
                    if active {
                        "active".to_string()
                    } else {
                        "inactive".to_string()
                    },
                )
                .unwrap();
            state.commit();
            black_box(state.doc.root_element_id());
        })
    });
}

criterion_group!(
    benches,
    bench_commit_cold_complex_selectors,
    bench_resolve_style_cold_complex_selectors,
    bench_commit_full_restyle_after_data_state_toggle,
);
criterion_main!(benches);
