use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use engine::RuntimeState;
use paws_examples::example_wasm_path;
use wasmtime::Engine as WasmEngine;
use wasmtime_engine::{create_engine, run_component};

const BASE_URL: &str = "https://example.com";
const VIEWPORT_WIDTH: f32 = 800.0;
const VIEWPORT_HEIGHT: f32 = 600.0;

fn fresh_state() -> RuntimeState {
    RuntimeState::with_definite_viewport(
        BASE_URL.to_string(),
        (),
        (),
        VIEWPORT_WIDTH,
        VIEWPORT_HEIGHT,
    )
}

fn load_example_component(name: &str) -> Vec<u8> {
    let path = example_wasm_path(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

fn run_example_component(
    engine: &WasmEngine,
    state: RuntimeState,
    wasm: &[u8],
    example_name: &str,
) {
    let state = run_component(engine, state, black_box(wasm), "run")
        .unwrap_or_else(|e| panic!("failed to run {example_name}: {}", e.error));
    black_box(state);
}

fn bench_component_example(
    c: &mut Criterion,
    bench_name: &'static str,
    example_name: &'static str,
) {
    let engine = create_engine();
    let wasm = load_example_component(example_name);

    c.bench_function(bench_name, |b| {
        b.iter_batched(
            fresh_state,
            |state| run_example_component(&engine, state, &wasm, example_name),
            BatchSize::SmallInput,
        )
    });
}

fn bench_component_basic_element(c: &mut Criterion) {
    bench_component_example(c, "component_basic_element", "example_basic_element");
}

fn bench_component_nested_elements(c: &mut Criterion) {
    bench_component_example(c, "component_nested_elements", "example_nested_elements");
}

fn bench_component_stylesheet_cascade(c: &mut Criterion) {
    bench_component_example(
        c,
        "component_stylesheet_cascade",
        "example_stylesheet_cascade",
    );
}

fn bench_component_parsed_stylesheet(c: &mut Criterion) {
    bench_component_example(
        c,
        "component_parsed_stylesheet",
        "example_parsed_stylesheet",
    );
}

fn bench_component_destroy_rebuild(c: &mut Criterion) {
    bench_component_example(c, "component_destroy_rebuild", "example_destroy_rebuild");
}

fn bench_component_commit_full(c: &mut Criterion) {
    bench_component_example(c, "component_commit_full", "example_commit_full");
}

fn bench_component_event_dispatch(c: &mut Criterion) {
    bench_component_example(c, "component_event_dispatch", "example_event_dispatch");
}

fn bench_component_inline_image(c: &mut Criterion) {
    bench_component_example(c, "component_inline_image", "example_inline_image");
}

criterion_group!(
    benches,
    bench_component_basic_element,
    bench_component_nested_elements,
    bench_component_stylesheet_cascade,
    bench_component_parsed_stylesheet,
    bench_component_destroy_rebuild,
    bench_component_commit_full,
    bench_component_event_dispatch,
    bench_component_inline_image,
);
criterion_main!(benches);
