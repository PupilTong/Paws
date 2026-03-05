//! Wasm Bridge: threads together wasmtime, stylo, and taffy.

pub mod wasm;

pub use wasm::{build_linker, read_cstr};

use engine::RuntimeState;
use wasmtime::{Engine as WasmEngine, Module, Store};

/// A tiny demo that wires wasm, layout, and style concepts together.
///
/// Returns a human-readable summary string.
pub fn hello_engine() -> String {
    // 1) Wasmtime: create an engine and compile a minimal module.
    let wasm_engine = WasmEngine::default();
    let wasm_bytes = b"(module)";
    let _module = Module::new(&wasm_engine, wasm_bytes).expect("compile minimal wasm module");
    let _store = Store::new(&wasm_engine, ());

    // 2) DOM & Style: create element and styles
    let mut state = RuntimeState::new("https://example.com".to_string());

    // Create div
    let id = state.create_element("div".to_string());
    state.append_element(0, id).expect("append to doc");

    // Set styles
    let _ = state.set_inline_style(id, "display".to_string(), "block".to_string());
    let _ = state.set_inline_style(id, "height".to_string(), "80px".to_string());
    let _ = state.set_inline_style(id, "width".to_string(), "120px".to_string());

    // Resolve styles first
    state.doc.resolve_style(&state.style_context);

    // 3) Layout: build Taffy tree
    let text_measurer = engine::layout::MockTextMeasurer;
    let layout = engine::layout::compute_layout(&state.doc, id as usize, &text_measurer)
        .expect("get layout");

    // Compute style for verification string
    let display = state
        .doc
        .get_node(id as usize)
        .unwrap()
        .get_computed_style_by_key(&state.style_context, "display");
    let height = state
        .doc
        .get_node(id as usize)
        .unwrap()
        .get_computed_style_by_key(&state.style_context, "height");

    format!(
        "wasm module ok\nlayout={{w:{}, h:{}}}\nstyle={{display:{}, height:{}}}",
        layout.width,
        layout.height,
        display.as_deref().unwrap_or("none"),
        height.as_deref().unwrap_or("none")
    )
}

#[cfg(test)]
mod tests {
    use super::{build_linker, hello_engine};
    use engine::{HostErrorCode, RuntimeState};
    use wasmtime::{Engine as WasmEngine, Module, Store};

    #[test]
    fn hello_engine_works() {
        let msg = hello_engine();
        println!("HELLO ENGINE OUTPUT:\n{}", msg);
        assert!(msg.contains("wasm module ok"));
        assert!(msg.contains("layout={w:120"));
        assert!(msg.contains("style={display:block"));
        assert!(msg.contains("height:80px"));
    }

    #[test]
    fn wasm_injected_bindings_compute_height() {
        let wat = r#"
(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "height\00")
  (data (i32.const 32) "100px\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $id))
    (drop)
    (call $set_style (local.get $id) (i32.const 16) (i32.const 32))
    (drop)
    (local.get $id)
  )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run") // Changed return type to i32
            .expect("get run function");
        let id = run.call(&mut store, ()).expect("run wasm"); // Capture the returned ID

        let state = store.data_mut();
        // Resolve styles first
        state.doc.resolve_style(&state.style_context);

        let height = state
            .doc
            .get_node(id as usize)
            .unwrap()
            .get_computed_style_by_key(&state.style_context, "height")
            .expect("computed height");
        assert_eq!(height, "100px");
    }

    #[test]
    fn wasm_append_element_success_and_idempotent() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 16)))
        (call $append (local.get $parent) (local.get $child))
        (drop)
        (call $append (local.get $parent) (local.get $child))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");
        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, 0);

        let state = store.data();
        let parent = 1;
        let child = 2;

        if let Some(parent_node) = state.doc.get_node(parent) {
            if parent_node.is_element() {
                assert_eq!(parent_node.children, vec![child]);
            } else {
                panic!("Parent not an element");
            }
        } else {
            panic!("Parent not found or not an element");
        }

        if let Some(child_node) = state.doc.get_node(child) {
            if child_node.is_element() {
                assert_eq!(child_node.parent, Some(parent));
            } else {
                panic!("Child not an element");
            }
        } else {
            panic!("Child not found or not an element");
        }
    }

    #[test]
    fn wasm_append_element_invalid_parent_sets_error() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (func (export "run") (result i32)
        (local $child i32)
        (local.set $child (call $create (i32.const 0)))
        (call $append (i32.const 42) (local.get $child))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");
        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, HostErrorCode::InvalidParent.as_i32());

        let state = store.data();
        let error = state.last_error.as_ref().expect("last error set");
        assert_eq!(error.code, HostErrorCode::InvalidParent.as_i32());
    }

    #[test]
    fn wasm_append_elements_success_and_dedup() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__AppendElements" (func $append_many (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child1 i32)
        (local $child2 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child1 (call $create (i32.const 16)))
        (local.set $child2 (call $create (i32.const 32)))
        (i32.store (i32.const 64) (local.get $child1))
        (i32.store (i32.const 68) (local.get $child2))
        (call $append_many (local.get $parent) (i32.const 64) (i32.const 2))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");
        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, 0);

        let state = store.data();
        let parent = 1;
        if let Some(parent_node) = state.doc.get_node(parent) {
            if parent_node.is_element() {
                assert_eq!(parent_node.children, vec![2, 3]);
            } else {
                panic!("Parent not an element");
            }
        } else {
            panic!("Parent not found or not an element");
        }
    }

    #[test]
    fn wasm_append_elements_invalid_child_no_partial_apply() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__AppendElements" (func $append_many (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child1 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child1 (call $create (i32.const 16)))
        (i32.store (i32.const 64) (local.get $child1))
        (i32.store (i32.const 68) (i32.const 99))
        (call $append_many (local.get $parent) (i32.const 64) (i32.const 2))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");
        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());

        let state = store.data();
        let parent_element_opt = state.doc.get_node(1);
        if let Some(node) = parent_element_opt {
            if node.is_element() {
                assert!(node.children.is_empty());
            } else {
                panic!("Parent not element");
            }
        } else {
            panic!("Parent not found");
        }
    }

    #[test]
    fn wasm_destroy_element() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__DestroyElement" (func $destroy (param i32) (result i32)))
    (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 0)))
        (call $destroy (local.get $child))
        (drop)
        (call $append (local.get $parent) (local.get $child))
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");

        // Expect InvalidChild because child is destroyed
        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());

        let state = store.data();
        // Check that child (id 2) is removed from the map
        assert!(state.doc.get_node(2).is_none());
    }

    #[test]
    fn wasm_set_inline_style_invalid_child() {
        let wat = r#"
(module
    (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "height\00")
    (data (i32.const 16) "100px\00")
    (func (export "run") (result i32)
        (call $set_style (i32.const 999) (i32.const 0) (i32.const 16))
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");

        let status = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    #[test]
    fn wasm_add_stylesheet_cascade() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__AddStylesheet" (func $add_css (param i32) (result i32)))
    (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "div { color: red; }\00")
    (func (export "run") (result i32)
        (local $id i32)
        (local.set $id (call $create (i32.const 0)))
        (call $append (i32.const 0) (local.get $id))
        (drop)
        (call $add_css (i32.const 16))
        (drop)
        (local.get $id)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile wasm module");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate wasm module");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run function");
        let id = run.call(&mut store, ()).expect("run wasm");

        let state = store.data_mut();
        // Resolve styles first
        state.doc.resolve_style(&state.style_context);

        let color = state
            .doc
            .get_node(id as usize)
            .unwrap()
            .get_computed_style_by_key(&state.style_context, "color")
            .expect("computed color");

        assert_eq!(color, "rgb(255, 0, 0)");
    }
}
