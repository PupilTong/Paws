//! Engine-level stylesheet installation benchmarks.
//!
//! These benchmarks use the parsed stylesheet IR path produced by
//! `view_macros::css!()` because guest-facing styling is expected to arrive as
//! pre-parsed IR, not runtime CSS strings.

use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

use engine::RuntimeState;

fn parsed_stylesheet() -> &'static [u8] {
    view_macros::css!(
        r#"
        div {
            display: block;
            width: 100px;
            height: 24px;
            margin: 1px;
            padding: 2px;
        }

        .row {
            display: flex;
            flex-direction: row;
            gap: 4px;
        }

        .cell {
            flex-grow: 1;
            min-width: 20px;
            height: 16px;
        }

        .featured {
            width: 180px;
            height: 32px;
            z-index: 2;
        }

        @media (min-width: 700px) {
            div {
                width: 220px;
            }

            .featured {
                height: 40px;
            }
        }
    "#
    )
}

fn distinct_parsed_stylesheets() -> [&'static [u8]; 8] {
    [
        view_macros::css!("div { width: 10px; height: 10px; }"),
        view_macros::css!(".row { display: flex; gap: 2px; }"),
        view_macros::css!(".cell { flex-grow: 1; min-width: 12px; }"),
        view_macros::css!(".featured { width: 40px; height: 18px; }"),
        view_macros::css!("@media (min-width: 600px) { div { width: 60px; } }"),
        view_macros::css!("span { display: inline-block; width: 8px; }"),
        view_macros::css!(".stack { display: grid; grid-template-columns: 1fr 1fr; }"),
        view_macros::css!(".hidden { display: none; }"),
    ]
}

fn setup_runtime() -> RuntimeState {
    let mut state = RuntimeState::new("https://bench.test".to_string());
    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
}

fn bench_parsed_stylesheet_initial_install(c: &mut Criterion) {
    let stylesheet = parsed_stylesheet();

    c.bench_function("parsed_stylesheet_initial_install", |b| {
        b.iter_batched(
            setup_runtime,
            |mut state| {
                state.add_parsed_stylesheet(black_box(stylesheet));
                black_box(state);
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_parsed_stylesheet_duplicate_install_noop(c: &mut Criterion) {
    let stylesheet = parsed_stylesheet();
    let mut state = setup_runtime();
    state.add_parsed_stylesheet(stylesheet);

    c.bench_function("parsed_stylesheet_duplicate_install_noop", |b| {
        b.iter(|| {
            black_box(&mut state).add_parsed_stylesheet(black_box(stylesheet));
        })
    });
}

fn bench_parsed_stylesheet_distinct_install_8(c: &mut Criterion) {
    let stylesheets = distinct_parsed_stylesheets();

    c.bench_function("parsed_stylesheet_distinct_install_8", |b| {
        b.iter_batched(
            setup_runtime,
            |mut state| {
                for stylesheet in stylesheets {
                    state.add_parsed_stylesheet(black_box(stylesheet));
                }
                black_box(state);
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    bench_parsed_stylesheet_initial_install,
    bench_parsed_stylesheet_duplicate_install_noop,
    bench_parsed_stylesheet_distinct_install_8,
);
criterion_main!(benches);
