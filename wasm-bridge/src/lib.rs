//! Wasm Bridge: threads together wasmtime, stylo, and taffy.

pub mod wasm;

pub use wasm::{build_linker, read_cstr};

use engine::RuntimeState;
use wasmtime::{Engine as WasmEngine, Module, Store};

/// Create a [`wasmtime::Engine`] configured for the current platform.
///
/// On iOS, JIT compilation is forbidden (no W+X pages), so we target
/// Pulley — wasmtime's portable interpreter. On all other platforms we
/// use the default (Cranelift) configuration.
pub fn create_engine() -> WasmEngine {
    #[allow(unused_mut)]
    let mut config = wasmtime::Config::new();
    #[cfg(target_os = "ios")]
    config.target("pulley64").expect("set pulley64 target");
    WasmEngine::new(&config).expect("create wasmtime engine")
}

/// Compiles and runs a WAT module against a [`RuntimeState`].
///
/// Creates a wasmtime engine, compiles the WAT text, instantiates the module
/// with the standard Paws host functions, and calls the named export.
///
/// The `RuntimeState` is moved into the wasmtime `Store` during execution
/// and returned afterwards — even on error — so the caller can always
/// recover it.
/// Error returned by [`run_wat`] when WASM execution fails.
///
/// Contains the recovered `RuntimeState` so the caller can reuse it.
pub struct RunWatError {
    /// The `RuntimeState` recovered from the wasmtime `Store`.
    pub state: RuntimeState,
    /// The underlying error.
    pub error: anyhow::Error,
}

pub fn run_wat(
    state: RuntimeState,
    wat: &str,
    func_name: &str,
) -> Result<RuntimeState, Box<RunWatError>> {
    let engine = create_engine();
    let module = match Module::new(&engine, wat) {
        Ok(m) => m,
        Err(e) => {
            return Err(Box::new(RunWatError { state, error: e }));
        }
    };
    let linker = build_linker(&engine);
    let mut store = Store::new(&engine, state);

    let result = (|| -> anyhow::Result<()> {
        let instance = linker.instantiate(&mut store, &module)?;
        let run = instance.get_typed_func::<(), i32>(&mut store, func_name)?;
        run.call(&mut store, ())?;
        Ok(())
    })();

    let state = store.into_data();
    match result {
        Ok(()) => Ok(state),
        Err(e) => Err(Box::new(RunWatError { state, error: e })),
    }
}

/// A tiny demo that wires wasm, layout, and style concepts together.
///
/// Returns a human-readable summary string.
pub fn hello_engine() -> String {
    // 1) Wasmtime: create an engine and compile a minimal module.
    let wasm_engine = create_engine();
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
    let mut layout_state = engine::layout::LayoutState::new();
    let layout = layout_state
        .compute_layout(&state.doc, engine::NodeId::from(id as u64), &text_measurer)
        .expect("get layout");

    format!(
        "wasm module ok\nlayout={{w:{}, h:{}}}",
        layout.width, layout.height
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
        assert!(msg.contains("h:80}"));
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

        let mut layout_state = engine::layout::LayoutState::new();
        let layout = layout_state
            .compute_layout(
                &state.doc,
                engine::NodeId::from(id as u64),
                &engine::layout::MockTextMeasurer,
            )
            .expect("layout");
        assert_eq!(layout.height, 100.0);
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
        let parent = engine::NodeId::from(1_u64);
        let child = engine::NodeId::from(2_u64);

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
        let parent = engine::NodeId::from(1_u64);
        if let Some(parent_node) = state.doc.get_node(parent) {
            if parent_node.is_element() {
                assert_eq!(
                    parent_node.children,
                    vec![engine::NodeId::from(2_u64), engine::NodeId::from(3_u64)]
                );
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
        let parent_element_opt = state.doc.get_node(engine::NodeId::from(1_u64));
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
        assert!(state.doc.get_node(engine::NodeId::from(2_u64)).is_none());
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
    (data (i32.const 16) "div { height: 77px; }\00")
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

        let mut layout_state = engine::layout::LayoutState::new();
        let layout = layout_state
            .compute_layout(
                &state.doc,
                engine::NodeId::from(id as u64),
                &engine::layout::MockTextMeasurer,
            )
            .expect("layout");
        assert_eq!(layout.height, 77.0);
    }

    #[test]
    fn wasm_set_attribute_success() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__SetAttribute" (func $set_attr (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "class\00")
    (data (i32.const 32) "foo bar\00")
    (func (export "run") (result i32)
        (local $id i32)
        (local.set $id (call $create (i32.const 0)))
        (call $set_attr (local.get $id) (i32.const 16) (i32.const 32))
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
    }

    #[test]
    fn wasm_set_attribute_invalid_child() {
        let wat = r#"
(module
    (import "env" "__SetAttribute" (func $set_attr (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "class\00")
    (data (i32.const 16) "foo bar\00")
    (func (export "run") (result i32)
        (call $set_attr (i32.const -1) (i32.const 0) (i32.const 16))
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
    fn wasm_commit_triggers_style_and_layout() {
        let wat = r#"
(module
    (import "env" "__CreateElement" (func $create (param i32) (result i32)))
    (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
    (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
    (import "env" "__Commit" (func $commit (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "width\00")
    (data (i32.const 32) "150px\00")
    (data (i32.const 48) "height\00")
    (data (i32.const 64) "75px\00")
    (func (export "run") (result i32)
        (local $id i32)
        (local.set $id (call $create (i32.const 0)))
        (call $append (i32.const 0) (local.get $id))
        (drop)
        (call $set_style (local.get $id) (i32.const 16) (i32.const 32))
        (drop)
        (call $set_style (local.get $id) (i32.const 48) (i32.const 64))
        (drop)
        (call $commit)
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
        let result = run.call(&mut store, ()).expect("run wasm");
        assert_eq!(result, 0, "__Commit should return 0 on success");

        // Verify that commit resolved styles by reading a computed property
        let state = store.data_mut();
        let map = state
            .computed_style_map(1)
            .expect("computed style map for div");
        let width = map
            .get("width", &mut state.doc, &state.style_context)
            .expect("should have computed width");
        match width {
            engine::CSSStyleValue::Unit(u) => assert_eq!(u.value, 150.0),
            engine::CSSStyleValue::Unparsed(s) => assert!(s.contains("150"), "expected 150 in {s}"),
            other => panic!("unexpected width value: {other:?}"),
        }

        // Verify that commit also computed layout (re-commit is a no-op
        // style-wise but returns the same layout tree)
        let layout = store.data_mut().commit();
        assert_eq!(layout.width, 150.0);
        assert_eq!(layout.height, 75.0);
    }
}
