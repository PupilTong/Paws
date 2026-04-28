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
const DEEP_CHAIN_COUNT: usize = 96;

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

fn deep_descendant_stylesheet() -> &'static [u8] {
    view_macros::css!(
        r#"
        .bench-root {
            display: block;
            width: 1200px;
        }

        .bench-root > .region {
            display: block;
            padding: 1px;
        }

        .bench-root > .region > .section > .panel > .row > .cell > .stack > .leaf {
            display: block;
            width: 12px;
            height: 8px;
        }

        .bench-root .region[data-zone="primary"] > .section[data-depth="even"] .panel .row > .cell[data-kind="metric"] .stack > .leaf.target[data-state="active"] {
            width: 28px;
        }

        .bench-root .region .section .panel[data-mode="dense"] > .row .cell .leaf.target {
            height: 12px;
        }

        .bench-root .region.featured > .section .panel > .row .cell .stack > .leaf.target.special {
            margin-left: 2px;
        }

        .bench-root .region .section .panel .row:nth-child(1) > .cell .leaf.target:not(.hidden) {
            padding-left: 1px;
        }

        .bench-root .missing-a .missing-b .missing-c .missing-d .leaf.target {
            width: 80px;
        }

        .bench-root .region .missing-b .missing-c .missing-d .leaf.target {
            width: 81px;
        }

        .bench-root .missing-a > .section > .missing-c .row .leaf.target {
            width: 82px;
        }

        .bench-root [data-missing="a"] .region .section .panel .leaf.target {
            width: 83px;
        }

        .bench-root .region[data-zone="ghost"] .section .panel .row .cell .leaf.target {
            width: 84px;
        }

        .bench-root .region .section[data-depth="missing"] .panel .row .cell .leaf.target {
            width: 85px;
        }

        .bench-root .region .section .panel[data-mode="ghost"] .row .cell .leaf.target {
            width: 86px;
        }

        .bench-root .region .section .panel .row .cell[data-kind="ghost"] .stack .leaf.target {
            width: 87px;
        }

        .bench-root .region .section .panel .row .missing-stack > .leaf.target {
            width: 88px;
        }

        .bench-root .ghost-one .ghost-two .ghost-three .ghost-four .ghost-five .leaf.target {
            height: 40px;
        }

        .bench-root .region > .ghost-two > .panel > .row > .cell > .stack > .leaf.target {
            height: 41px;
        }

        .bench-root .region .section > .ghost-three > .row > .cell > .stack > .leaf.target {
            height: 42px;
        }

        .bench-root .region .section .panel > .ghost-four > .cell > .stack > .leaf.target {
            height: 43px;
        }

        .bench-root .region .section .panel .row > .ghost-five > .stack > .leaf.target {
            height: 44px;
        }

        .bench-root .region .section .panel .row .cell > .ghost-six > .leaf.target {
            height: 45px;
        }
    "#
    )
}

fn setup_deep_descendant_runtime() -> (RuntimeState, u32) {
    let mut state = RuntimeState::with_definite_viewport(
        "https://bench.test".to_string(),
        (),
        (),
        1280.0,
        800.0,
    );
    state.add_parsed_stylesheet(deep_descendant_stylesheet());

    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_attribute(root, "class".to_string(), "bench-root".to_string())
        .unwrap();

    let mut toggle_target = None;

    for chain_index in 0..DEEP_CHAIN_COUNT {
        let region = state.create_element("div".to_string());
        state.append_element(root, region).unwrap();
        state
            .set_attribute(
                region,
                "class".to_string(),
                if chain_index % 4 == 0 {
                    "region featured".to_string()
                } else {
                    "region".to_string()
                },
            )
            .unwrap();
        state
            .set_attribute(
                region,
                "data-zone".to_string(),
                if chain_index % 2 == 0 {
                    "primary".to_string()
                } else {
                    "secondary".to_string()
                },
            )
            .unwrap();

        let section = state.create_element("div".to_string());
        state.append_element(region, section).unwrap();
        state
            .set_attribute(section, "class".to_string(), "section".to_string())
            .unwrap();
        state
            .set_attribute(
                section,
                "data-depth".to_string(),
                if chain_index % 2 == 0 {
                    "even".to_string()
                } else {
                    "odd".to_string()
                },
            )
            .unwrap();

        let panel = state.create_element("div".to_string());
        state.append_element(section, panel).unwrap();
        state
            .set_attribute(panel, "class".to_string(), "panel".to_string())
            .unwrap();
        state
            .set_attribute(
                panel,
                "data-mode".to_string(),
                if chain_index % 3 == 0 {
                    "dense".to_string()
                } else {
                    "compact".to_string()
                },
            )
            .unwrap();

        let row = state.create_element("div".to_string());
        state.append_element(panel, row).unwrap();
        state
            .set_attribute(row, "class".to_string(), "row".to_string())
            .unwrap();

        let cell = state.create_element("div".to_string());
        state.append_element(row, cell).unwrap();
        state
            .set_attribute(cell, "class".to_string(), "cell".to_string())
            .unwrap();
        state
            .set_attribute(
                cell,
                "data-kind".to_string(),
                if chain_index % 2 == 0 {
                    "metric".to_string()
                } else {
                    "summary".to_string()
                },
            )
            .unwrap();

        let stack = state.create_element("div".to_string());
        state.append_element(cell, stack).unwrap();
        state
            .set_attribute(stack, "class".to_string(), "stack".to_string())
            .unwrap();

        let leaf = state.create_element("span".to_string());
        state.append_element(stack, leaf).unwrap();
        state
            .set_attribute(
                leaf,
                "class".to_string(),
                if chain_index % 4 == 0 {
                    "leaf target special".to_string()
                } else {
                    "leaf target".to_string()
                },
            )
            .unwrap();
        state
            .set_attribute(
                leaf,
                "data-state".to_string(),
                if chain_index % 2 == 0 {
                    "active".to_string()
                } else {
                    "inactive".to_string()
                },
            )
            .unwrap();

        if chain_index == DEEP_CHAIN_COUNT / 2 {
            toggle_target = Some(leaf);
        }
    }

    (state, toggle_target.expect("toggle target should exist"))
}

fn setup_committed_deep_descendant_runtime_for_toggle() -> (RuntimeState, u32) {
    let (mut state, target) = setup_deep_descendant_runtime();
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

fn bench_resolve_style_cold_deep_descendant_selectors(c: &mut Criterion) {
    c.bench_function("resolve_style_cold_deep_descendant_selectors_96x8", |b| {
        b.iter_batched(
            setup_deep_descendant_runtime,
            |(mut state, _target)| {
                state.doc.resolve_style(&state.style_context);
                black_box(state.doc.root_element_id());
            },
            BatchSize::LargeInput,
        )
    });
}

/// Measures a full-tree restyle on an already-mounted tree. This is the warm
/// equivalent of first-screen style work: the whole document is dirty, but DOM
/// construction and stylesheet parsing are outside the measured iteration.
fn bench_commit_full_restyle_after_viewport_change(c: &mut Criterion) {
    let (mut state, _target) = setup_committed_runtime_for_toggle();
    let mut wide = true;

    c.bench_function("commit_full_restyle_after_viewport_change_120x6", |b| {
        b.iter(|| {
            wide = !wide;
            let width = if wide { 1280.0 } else { 800.0 };
            state.set_viewport(taffy::Size {
                width: taffy::AvailableSpace::Definite(black_box(width)),
                height: taffy::AvailableSpace::Definite(800.0),
            });
            state.commit();
            black_box(state.doc.root_element_id());
        })
    });
}

/// Measures the dirty-subtree path after toggling one item attribute. The
/// changed item, its descendants, and following sibling subtrees can be
/// restyled without selector/cascade work for the rest of the document.
fn bench_commit_incremental_restyle_after_data_state_toggle(c: &mut Criterion) {
    let (mut state, target) = setup_committed_runtime_for_toggle();
    let mut active = false;

    c.bench_function(
        "commit_incremental_restyle_after_data_state_toggle_120x6",
        |b| {
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
        },
    );
}

fn bench_commit_incremental_restyle_after_deep_leaf_toggle(c: &mut Criterion) {
    let (mut state, target) = setup_committed_deep_descendant_runtime_for_toggle();
    let mut active = false;

    c.bench_function(
        "commit_incremental_restyle_after_deep_leaf_toggle_96x8",
        |b| {
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
        },
    );
}

criterion_group!(
    benches,
    bench_commit_cold_complex_selectors,
    bench_resolve_style_cold_complex_selectors,
    bench_resolve_style_cold_deep_descendant_selectors,
    bench_commit_full_restyle_after_viewport_change,
    bench_commit_incremental_restyle_after_data_state_toggle,
    bench_commit_incremental_restyle_after_deep_leaf_toggle,
);
criterion_main!(benches);
