use criterion::{black_box, criterion_group, criterion_main, Criterion};

use taffy::prelude::*; // Use prelude for TaffyMaxContent etc.

use engine::{build_linker, computed_style, hello_engine, layout, RuntimeState};
use wasmtime::{Engine as WasmEngine, Module, Store};

fn bench_computed_style(c: &mut Criterion) {
    let mut state = RuntimeState::default();
    let id = state.create_element("div".to_string()); // returns u32
    state
        .set_inline_style(id, "height".to_string(), "100px".to_string())
        .expect("set style");
    // Verify node exists
    assert!(state.doc.nodes.contains(id as usize));

    c.bench_function("layout_simple", |b| {
        b.iter(|| {
            let mut taffy = TaffyTree::<()>::new();
            let _root = layout::build_layout_tree(&state, id as usize, &mut taffy);
            taffy
                .compute_layout(NodeId::from(0u64), Size::MAX_CONTENT)
                .ok(); // mock
        })
    });

    c.bench_function("computed_style_height", |b| {
        b.iter(|| {
            computed_style(
                black_box(engine::dom::NodeHandle(id as usize)),
                black_box("height"),
            )
        })
    });
}

fn bench_hello_engine(c: &mut Criterion) {
    c.bench_function("hello_engine", |b| b.iter(|| hello_engine()));
}

fn bench_wasm_execution(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "height\00")
  (data (i32.const 32) "100px\00")
  (func (export "run")
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (call $set_style (local.get $id) (i32.const 16) (i32.const 32))
    (drop)
  )
)
"#;

    let engine = WasmEngine::default();
    let module = Module::new(&engine, wat).expect("compile wasm module");
    let linker = build_linker(&engine);
    let mut store = Store::new(&engine, RuntimeState::default());
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate wasm module");
    let run = instance
        .get_typed_func::<(), ()>(&mut store, "run")
        .expect("get run function");

    c.bench_function("wasm_execution", |b| {
        b.iter(|| run.call(&mut store, ()).expect("run wasm"))
    });
}

criterion_group!(
    benches,
    bench_computed_style,
    bench_hello_engine,
    bench_wasm_execution
);
criterion_main!(benches);
