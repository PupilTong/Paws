use codspeed_criterion_compat::{black_box, criterion_group, criterion_main, Criterion};

use engine::RuntimeState;
use wasmtime::{Engine as WasmEngine, Module, Store};
use wasmtime_engine::{build_linker, hello_engine};

// ---------------------------------------------------------------------------
// Helper: compile a WAT module & instantiate with a fresh RuntimeState + linker
// ---------------------------------------------------------------------------
fn setup_wasm(wat: &str) -> (Store<RuntimeState>, wasmtime::Instance) {
    let engine = WasmEngine::default();
    let module = Module::new(&engine, wat).expect("compile wasm module");
    let linker = build_linker(&engine);
    let mut store = Store::new(
        &engine,
        RuntimeState::new("https://example.com".to_string()),
    );
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate wasm module");
    (store, instance)
}

// ---------------------------------------------------------------------------
// 1. Layout — simple (original benchmark)
// ---------------------------------------------------------------------------
fn bench_computed_style(c: &mut Criterion) {
    let mut state = RuntimeState::new("https://example.com".to_string());
    let id = state.create_element("div".to_string()); // returns u32
    state
        .set_inline_style(id, "height".to_string(), "100px".to_string())
        .expect("set style");
    // Verify node exists
    assert!(state
        .doc
        .get_node(engine::NodeId::from(id as u64))
        .is_some());

    c.bench_function("layout_simple", |b| {
        b.iter(|| {
            engine::layout::compute_layout(
                black_box(&mut state.doc),
                black_box(engine::NodeId::from(id as u64)),
            );
        })
    });
}

// ---------------------------------------------------------------------------
// 2. hello_engine
// ---------------------------------------------------------------------------
fn bench_hello_engine(c: &mut Criterion) {
    c.bench_function("hello_engine", |b| b.iter(hello_engine));
}

// ---------------------------------------------------------------------------
// 3. WASM execution — basic create + style (original benchmark)
// ---------------------------------------------------------------------------
fn bench_wasm_execution(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
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

    let (mut store, instance) = setup_wasm(wat);
    let run = instance
        .get_typed_func::<(), ()>(&mut store, "run")
        .expect("get run function");

    c.bench_function("wasm_execution", |b| {
        b.iter(|| run.call(&mut store, ()).expect("run wasm"))
    });
}

// ---------------------------------------------------------------------------
// 4. WASM — Flexbox layout (create + style + append + commit)
// ---------------------------------------------------------------------------
fn bench_wasm_flex_layout(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  ;; tag names
  (data (i32.const 0) "div\00")

  ;; style property names
  (data (i32.const 16) "display\00")
  (data (i32.const 32) "flex-direction\00")
  (data (i32.const 48) "width\00")
  (data (i32.const 64) "height\00")
  (data (i32.const 80) "justify-content\00")
  (data (i32.const 112) "align-items\00")
  (data (i32.const 128) "flex-grow\00")
  (data (i32.const 144) "margin\00")
  (data (i32.const 160) "padding\00")

  ;; style values
  (data (i32.const 256) "flex\00")
  (data (i32.const 272) "row\00")
  (data (i32.const 288) "500px\00")
  (data (i32.const 304) "400px\00")
  (data (i32.const 320) "space-between\00")
  (data (i32.const 336) "center\00")
  (data (i32.const 352) "1\00")
  (data (i32.const 368) "10px\00")
  (data (i32.const 384) "20px\00")
  (data (i32.const 400) "100px\00")
  (data (i32.const 416) "column\00")

  (func (export "setup")
    (local $root i32)
    (local $child i32)
    (local $i i32)

    ;; Create root flex container
    (local.set $root (call $create (i32.const 0)))
    (call $set_style (local.get $root) (i32.const 16) (i32.const 256)) ;; display: flex
    (drop)
    (call $set_style (local.get $root) (i32.const 32) (i32.const 272)) ;; flex-direction: row
    (drop)
    (call $set_style (local.get $root) (i32.const 48) (i32.const 288)) ;; width: 500px
    (drop)
    (call $set_style (local.get $root) (i32.const 64) (i32.const 304)) ;; height: 400px
    (drop)
    (call $set_style (local.get $root) (i32.const 80) (i32.const 320)) ;; justify-content: space-between
    (drop)
    (call $set_style (local.get $root) (i32.const 112) (i32.const 336)) ;; align-items: center
    (drop)
    (call $set_style (local.get $root) (i32.const 160) (i32.const 384)) ;; padding: 20px
    (drop)

    ;; Append root to document root (id=0)
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Create 5 flex children
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 5)))
        (local.set $child (call $create (i32.const 0)))
        (call $set_style (local.get $child) (i32.const 128) (i32.const 352)) ;; flex-grow: 1
        (drop)
        (call $set_style (local.get $child) (i32.const 64) (i32.const 400))  ;; height: 100px
        (drop)
        (call $set_style (local.get $child) (i32.const 144) (i32.const 368)) ;; margin: 10px
        (drop)
        (call $append (local.get $root) (local.get $child))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    ;; Commit — triggers style resolution + layout
    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_flex_layout", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run flex layout"))
    });
}

// ---------------------------------------------------------------------------
// 5. WASM — Deep tree (linear chain of 50 nested divs)
// ---------------------------------------------------------------------------
fn bench_wasm_deep_tree(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 0) "div\00")
  (data (i32.const 16) "padding\00")
  (data (i32.const 32) "2px\00")
  (data (i32.const 48) "width\00")
  (data (i32.const 64) "100%\00")

  (func (export "setup")
    (local $parent i32)
    (local $child i32)
    (local $i i32)

    ;; First element appended to document root
    (local.set $parent (call $create (i32.const 0)))
    (call $set_style (local.get $parent) (i32.const 48) (i32.const 64)) ;; width: 100%
    (drop)
    (call $append (i32.const 0) (local.get $parent))
    (drop)

    ;; Chain 49 more nested children (total depth = 50)
    (local.set $i (i32.const 1))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 50)))
        (local.set $child (call $create (i32.const 0)))
        (call $set_style (local.get $child) (i32.const 16) (i32.const 32)) ;; padding: 2px
        (drop)
        (call $append (local.get $parent) (local.get $child))
        (drop)
        (local.set $parent (local.get $child))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_deep_tree", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run deep tree"))
    });
}

// ---------------------------------------------------------------------------
// 6. WASM — Wide tree (1 root + 200 children)
// ---------------------------------------------------------------------------
fn bench_wasm_wide_tree(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 0) "div\00")
  (data (i32.const 16) "display\00")
  (data (i32.const 32) "flex\00")
  (data (i32.const 48) "flex-wrap\00")
  (data (i32.const 64) "wrap\00")
  (data (i32.const 80) "width\00")
  (data (i32.const 96) "50px\00")
  (data (i32.const 112) "height\00")
  (data (i32.const 128) "50px\00")
  (data (i32.const 144) "1000px\00")

  (func (export "setup")
    (local $root i32)
    (local $child i32)
    (local $i i32)

    ;; Create flex-wrap root
    (local.set $root (call $create (i32.const 0)))
    (call $set_style (local.get $root) (i32.const 16) (i32.const 32))   ;; display: flex
    (drop)
    (call $set_style (local.get $root) (i32.const 48) (i32.const 64))   ;; flex-wrap: wrap
    (drop)
    (call $set_style (local.get $root) (i32.const 80) (i32.const 144))  ;; width: 1000px
    (drop)
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Add 200 children
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 200)))
        (local.set $child (call $create (i32.const 0)))
        (call $set_style (local.get $child) (i32.const 80) (i32.const 96))   ;; width: 50px
        (drop)
        (call $set_style (local.get $child) (i32.const 112) (i32.const 128)) ;; height: 50px
        (drop)
        (call $append (local.get $root) (local.get $child))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_wide_tree", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run wide tree"))
    });
}

// ---------------------------------------------------------------------------
// 7. WASM — Large tree move (create subtree, then re-parent it)
// ---------------------------------------------------------------------------
fn bench_wasm_large_tree_move(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__destroy_element" (func $destroy (param i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 0) "div\00")
  (data (i32.const 16) "width\00")
  (data (i32.const 32) "100px\00")
  (data (i32.const 48) "height\00")
  (data (i32.const 64) "50px\00")

  (func (export "setup")
    (local $container_a i32)
    (local $container_b i32)
    (local $child i32)
    (local $i i32)

    ;; Two containers under document root
    (local.set $container_a (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $container_a))
    (drop)

    (local.set $container_b (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $container_b))
    (drop)

    ;; Add 50 children to container A
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 50)))
        (local.set $child (call $create (i32.const 0)))
        (call $set_style (local.get $child) (i32.const 16) (i32.const 32)) ;; width: 100px
        (drop)
        (call $set_style (local.get $child) (i32.const 48) (i32.const 64)) ;; height: 50px
        (drop)
        (call $append (local.get $container_a) (local.get $child))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    ;; Commit with children in container A
    (call $commit)
    (drop)

    ;; Destroy container A (removes it and all descendants)
    (call $destroy (local.get $container_a))
    (drop)

    ;; Rebuild 50 children under container B (simulates a large tree move)
    (local.set $i (i32.const 0))
    (block $break2
      (loop $loop2
        (br_if $break2 (i32.ge_u (local.get $i) (i32.const 50)))
        (local.set $child (call $create (i32.const 0)))
        (call $set_style (local.get $child) (i32.const 16) (i32.const 32))
        (drop)
        (call $set_style (local.get $child) (i32.const 48) (i32.const 64))
        (drop)
        (call $append (local.get $container_b) (local.get $child))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop2)
      )
    )

    ;; Re-commit
    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_large_tree_move", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run large tree move"))
    });
}

// ---------------------------------------------------------------------------
// 8. WASM — Remove nodes (create tree, then destroy subtrees)
// ---------------------------------------------------------------------------
fn bench_wasm_remove_nodes(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__destroy_element" (func $destroy (param i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 0) "div\00")

  ;; Store child IDs for later removal (offset 64..64+40*4=224)
  (global $child_base (mut i32) (i32.const 64))

  (func (export "setup")
    (local $root i32)
    (local $child i32)
    (local $i i32)

    ;; Root container
    (local.set $root (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Create 40 children, store their IDs in linear memory
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 40)))
        (local.set $child (call $create (i32.const 0)))
        (call $append (local.get $root) (local.get $child))
        (drop)
        ;; Store id at memory[64 + i*4]
        (i32.store
          (i32.add (i32.const 64) (i32.mul (local.get $i) (i32.const 4)))
          (local.get $child)
        )
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)

    ;; Destroy all 40 children
    (local.set $i (i32.const 0))
    (block $break2
      (loop $loop2
        (br_if $break2 (i32.ge_u (local.get $i) (i32.const 40)))
        (call $destroy
          (i32.load (i32.add (i32.const 64) (i32.mul (local.get $i) (i32.const 4))))
        )
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop2)
      )
    )

    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_remove_nodes", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run remove nodes"))
    });
}

// ---------------------------------------------------------------------------
// 9. WASM — Add large stylesheet
// ---------------------------------------------------------------------------
fn bench_wasm_add_large_stylesheet(c: &mut Criterion) {
    // Build a large CSS string with many rules at compile time
    let mut css = String::with_capacity(8192);
    for i in 0..100 {
        css.push_str(&format!(
            ".class-{i} {{ width: {w}px; height: {h}px; margin: {m}px; padding: {p}px; \
             color: rgb({r},{g},{b}); background-color: #{bg:06x}; \
             display: flex; align-items: center; justify-content: center; }}\n",
            i = i,
            w = 50 + i,
            h = 30 + i,
            m = i % 20,
            p = i % 15,
            r = i % 256,
            g = (i * 3) % 256,
            b = (i * 7) % 256,
            bg = (i * 12345) % 0xFFFFFF,
        ));
    }
    css.push('\0'); // null-terminate for WASM

    // Build WAT that embeds a pointer/length reference and calls __add_stylesheet
    // We'll place the CSS string at offset 0 in WASM memory.
    let css_len = css.len();
    let pages_needed = (css_len / 65536) + 1;

    let wat = format!(
        r#"
(module
  (import "env" "__add_stylesheet" (func $add_css (param i32) (result i32)))
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") {pages})
  (func (export "setup")
    (local $root i32)
    (local $child i32)
    (local $i i32)

    ;; Add the stylesheet
    (call $add_css (i32.const 0))
    (drop)

    ;; Create a root element and some children that will match the selectors
    (local.set $root (call $create (i32.const {tag_offset})))
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Create 20 children with class attributes matching the stylesheet
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 20)))
        (local.set $child (call $create (i32.const {tag_offset})))
        (call $set_attr (local.get $child) (i32.const {class_attr_offset}) (i32.const {class_val_offset}))
        (drop)
        (call $append (local.get $root) (local.get $child))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)
  )
)
"#,
        pages = pages_needed,
        tag_offset = css_len,
        class_attr_offset = css_len + 16,
        class_val_offset = css_len + 32,
    );

    // Build the WASM memory data: CSS string + tag name + attribute strings
    let engine = WasmEngine::default();
    let module = Module::new(&engine, &wat).expect("compile wasm module");
    let linker = build_linker(&engine);
    let mut store = Store::new(
        &engine,
        RuntimeState::new("https://example.com".to_string()),
    );
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate wasm module");

    // Write data into WASM memory
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("get memory");
    memory.data_mut(&mut store)[..css_len].copy_from_slice(css.as_bytes());

    // Write "div\0" at tag_offset
    let tag_data = b"div\0";
    memory.data_mut(&mut store)[css_len..css_len + 4].copy_from_slice(tag_data);

    // Write "class\0" at class_attr_offset
    let class_attr = b"class\0";
    memory.data_mut(&mut store)[css_len + 16..css_len + 22].copy_from_slice(class_attr);

    // Write "class-5\0" at class_val_offset
    let class_val = b"class-5\0";
    memory.data_mut(&mut store)[css_len + 32..css_len + 40].copy_from_slice(class_val);

    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_add_large_stylesheet", |b| {
        b.iter(|| {
            setup
                .call(&mut store, ())
                .expect("run add large stylesheet")
        })
    });
}

// ---------------------------------------------------------------------------
// 10. WASM — Complex selectors (descendant, child, sibling, pseudo-class)
// ---------------------------------------------------------------------------
fn bench_wasm_complex_selectors(c: &mut Criterion) {
    // CSS with complex selectors: descendant, child, attribute, pseudo-class, combinators
    let css = concat!(
        "div > .container .item { color: red; }\n",
        "div .wrapper > span:first-child { font-size: 14px; }\n",
        "div div div .deep { margin: 5px; }\n",
        ".a .b .c .d { padding: 10px; }\n",
        "div:nth-child(2n+1) { background: blue; }\n",
        "div:last-child > .item { border: 1px solid black; }\n",
        ".container > div + div { margin-left: 10px; }\n",
        "div.wrapper .item:not(.hidden) { display: block; }\n",
        ".a > .b ~ .c { color: green; }\n",
        "[data-type=\"primary\"] .label { font-weight: bold; }\n",
        "\0"
    );

    let css_len = css.len();

    let wat = format!(
        r#"
(module
  (import "env" "__add_stylesheet" (func $add_css (param i32) (result i32)))
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (func (export "setup")
    (local $root i32)
    (local $container i32)
    (local $wrapper i32)
    (local $child i32)
    (local $deep i32)
    (local $i i32)

    ;; Add complex stylesheet
    (call $add_css (i32.const 0))
    (drop)

    ;; Build a DOM tree that exercises the complex selectors:
    ;; <div class="a">                              [root]
    ;;   <div class="container b">                  [container * 3]
    ;;     <div class="wrapper c">                  [wrapper]
    ;;       <div class="item d" data-type="primary">
    ;;         <div class="deep label">             [deep]

    (local.set $root (call $create (i32.const {tag})))
    (call $set_attr (local.get $root) (i32.const {cls_attr}) (i32.const {cls_a}))
    (drop)
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Create 3 container branches to exercise sibling combinators
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 3)))

        ;; container
        (local.set $container (call $create (i32.const {tag})))
        (call $set_attr (local.get $container) (i32.const {cls_attr}) (i32.const {cls_container_b}))
        (drop)
        (call $append (local.get $root) (local.get $container))
        (drop)

        ;; wrapper
        (local.set $wrapper (call $create (i32.const {tag})))
        (call $set_attr (local.get $wrapper) (i32.const {cls_attr}) (i32.const {cls_wrapper_c}))
        (drop)
        (call $append (local.get $container) (local.get $wrapper))
        (drop)

        ;; item child
        (local.set $child (call $create (i32.const {tag})))
        (call $set_attr (local.get $child) (i32.const {cls_attr}) (i32.const {cls_item_d}))
        (drop)
        (call $set_attr (local.get $child) (i32.const {data_attr}) (i32.const {data_val}))
        (drop)
        (call $append (local.get $wrapper) (local.get $child))
        (drop)

        ;; deep label
        (local.set $deep (call $create (i32.const {tag})))
        (call $set_attr (local.get $deep) (i32.const {cls_attr}) (i32.const {cls_deep_label}))
        (drop)
        (call $append (local.get $child) (local.get $deep))
        (drop)

        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)
  )
)
"#,
        tag = css_len,                  // "div\0"
        cls_attr = css_len + 16,        // "class\0"
        cls_a = css_len + 32,           // "a\0"
        cls_container_b = css_len + 48, // "container b\0"
        cls_wrapper_c = css_len + 64,   // "wrapper c\0"
        cls_item_d = css_len + 80,      // "item d\0"
        cls_deep_label = css_len + 96,  // "deep label\0"
        data_attr = css_len + 112,      // "data-type\0"
        data_val = css_len + 128,       // "primary\0"
    );

    let engine = WasmEngine::default();
    let module = Module::new(&engine, &wat).expect("compile wasm module");
    let linker = build_linker(&engine);
    let mut store = Store::new(
        &engine,
        RuntimeState::new("https://example.com".to_string()),
    );
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate wasm module");

    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("get memory");
    let mem = memory.data_mut(&mut store);

    // Write CSS at offset 0
    mem[..css_len].copy_from_slice(css.as_bytes());

    // String table after CSS
    let strings: &[(&[u8], usize)] = &[
        (b"div\0", css_len),
        (b"class\0", css_len + 16),
        (b"a\0", css_len + 32),
        (b"container b\0", css_len + 48),
        (b"wrapper c\0", css_len + 64),
        (b"item d\0", css_len + 80),
        (b"deep label\0", css_len + 96),
        (b"data-type\0", css_len + 112),
        (b"primary\0", css_len + 128),
    ];
    for (data, offset) in strings {
        mem[*offset..*offset + data.len()].copy_from_slice(data);
    }

    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_complex_selectors", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run complex selectors"))
    });
}

// ---------------------------------------------------------------------------
// 11. WASM — Grid layout
// ---------------------------------------------------------------------------
fn bench_wasm_grid_layout(c: &mut Criterion) {
    let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__commit" (func $commit (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 0) "div\00")
  (data (i32.const 16) "display\00")
  (data (i32.const 32) "grid\00")
  (data (i32.const 48) "grid-template-columns\00")
  (data (i32.const 80) "repeat(4, 1fr)\00")
  (data (i32.const 112) "grid-template-rows\00")
  (data (i32.const 144) "auto\00")
  (data (i32.const 160) "gap\00")
  (data (i32.const 176) "10px\00")
  (data (i32.const 192) "width\00")
  (data (i32.const 208) "800px\00")
  (data (i32.const 224) "min-height\00")
  (data (i32.const 240) "50px\00")

  (func (export "setup")
    (local $grid i32)
    (local $cell i32)
    (local $i i32)

    ;; Create grid container
    (local.set $grid (call $create (i32.const 0)))
    (call $set_style (local.get $grid) (i32.const 16) (i32.const 32))    ;; display: grid
    (drop)
    (call $set_style (local.get $grid) (i32.const 48) (i32.const 80))    ;; grid-template-columns
    (drop)
    (call $set_style (local.get $grid) (i32.const 112) (i32.const 144))  ;; grid-template-rows: auto
    (drop)
    (call $set_style (local.get $grid) (i32.const 160) (i32.const 176))  ;; gap: 10px
    (drop)
    (call $set_style (local.get $grid) (i32.const 192) (i32.const 208))  ;; width: 800px
    (drop)
    (call $append (i32.const 0) (local.get $grid))
    (drop)

    ;; Add 16 grid cells (4x4)
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_u (local.get $i) (i32.const 16)))
        (local.set $cell (call $create (i32.const 0)))
        (call $set_style (local.get $cell) (i32.const 224) (i32.const 240)) ;; min-height: 50px
        (drop)
        (call $append (local.get $grid) (local.get $cell))
        (drop)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (call $commit)
    (drop)
  )
)
"#;

    let (mut store, instance) = setup_wasm(wat);
    let setup = instance
        .get_typed_func::<(), ()>(&mut store, "setup")
        .expect("get setup function");

    c.bench_function("wasm_grid_layout", |b| {
        b.iter(|| setup.call(&mut store, ()).expect("run grid layout"))
    });
}

// ---------------------------------------------------------------------------
// 12. Layout with text nodes — measures text measurement overhead
// ---------------------------------------------------------------------------
fn bench_layout_with_text(c: &mut Criterion) {
    let mut state = RuntimeState::new("https://example.com".to_string());
    let flex = state.create_element("div".to_string());
    state.append_element(0, flex).unwrap();
    state
        .set_inline_style(flex, "display".to_string(), "flex".to_string())
        .unwrap();
    state
        .set_inline_style(flex, "width".to_string(), "600px".to_string())
        .unwrap();
    state
        .set_inline_style(flex, "flex-wrap".to_string(), "wrap".to_string())
        .unwrap();

    for _ in 0..5 {
        let child = state.create_element("div".to_string());
        state.append_element(flex, child).unwrap();
        state
            .set_inline_style(child, "display".to_string(), "flex".to_string())
            .unwrap();
        let text_id = state.create_text_node("Some example text!!".to_string());
        state.append_element(child, text_id).unwrap();
    }

    // Resolve styles once before benchmarking layout
    state.doc.resolve_style(&state.style_context);

    c.bench_function("layout_with_text", |b| {
        b.iter(|| {
            engine::layout::compute_layout(
                black_box(&mut state.doc),
                black_box(engine::NodeId::from(flex as u64)),
            );
        })
    });
}

// ---------------------------------------------------------------------------
// 13. Text-heavy layout — deep tree with many text nodes
// ---------------------------------------------------------------------------
fn bench_text_heavy_layout(c: &mut Criterion) {
    let mut state = RuntimeState::new("https://example.com".to_string());
    let root = state.create_element("div".to_string());
    state.append_element(0, root).unwrap();
    state
        .set_inline_style(root, "display".to_string(), "flex".to_string())
        .unwrap();
    state
        .set_inline_style(root, "flex-direction".to_string(), "column".to_string())
        .unwrap();
    state
        .set_inline_style(root, "width".to_string(), "800px".to_string())
        .unwrap();

    // 3 levels deep, ~20 text nodes total
    for _ in 0..4 {
        let section = state.create_element("div".to_string());
        state.append_element(root, section).unwrap();
        state
            .set_inline_style(section, "display".to_string(), "flex".to_string())
            .unwrap();
        state
            .set_inline_style(section, "flex-direction".to_string(), "column".to_string())
            .unwrap();

        for _ in 0..5 {
            let item = state.create_element("div".to_string());
            state.append_element(section, item).unwrap();
            state
                .set_inline_style(item, "display".to_string(), "flex".to_string())
                .unwrap();
            let text_id =
                state.create_text_node("The quick brown fox jumps over the lazy dog".to_string());
            state.append_element(item, text_id).unwrap();
        }
    }

    state.doc.resolve_style(&state.style_context);

    c.bench_function("text_heavy_layout", |b| {
        b.iter(|| {
            engine::layout::compute_layout(
                black_box(&mut state.doc),
                black_box(engine::NodeId::from(root as u64)),
            );
        })
    });
}

// ---------------------------------------------------------------------------
// Criterion groups & main
// ---------------------------------------------------------------------------
criterion_group!(
    benches,
    bench_computed_style,
    bench_hello_engine,
    bench_wasm_execution,
    bench_wasm_flex_layout,
    bench_wasm_deep_tree,
    bench_wasm_wide_tree,
    bench_wasm_large_tree_move,
    bench_wasm_remove_nodes,
    bench_wasm_add_large_stylesheet,
    bench_wasm_complex_selectors,
    bench_wasm_grid_layout,
    bench_layout_with_text,
    bench_text_heavy_layout,
);
criterion_main!(benches);
