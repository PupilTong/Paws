//! Wasm Bridge: threads together wasmtime, stylo, and taffy.

pub mod wasm;

pub use wasm::{build_linker, read_cstr};

use engine::RuntimeState;
use wasmtime::{Engine as WasmEngine, MemoryType, Module, SharedMemory, Store};

/// Create a [`wasmtime::Engine`] configured for the current platform.
///
/// On iOS, JIT compilation is forbidden (no W+X pages), so we target
/// Pulley — wasmtime's portable interpreter. On all other platforms we
/// use the default (Cranelift) configuration.
pub fn create_engine() -> WasmEngine {
    let mut config = wasmtime::Config::new();
    config.wasm_threads(true);
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
/// Error returned by [`run_wasm`] when WASM execution fails.
///
/// Contains the recovered `RuntimeState` so the caller can reuse it.
pub struct RunWasmError {
    /// The `RuntimeState` recovered from the wasmtime `Store`.
    pub state: RuntimeState,
    /// The underlying error.
    pub error: anyhow::Error,
}

impl std::fmt::Debug for RunWasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunWasmError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Compiles and runs a binary WASM module against a [`RuntimeState`].
///
/// The `RuntimeState` is always recovered, even on error.
pub fn run_wasm(
    state: RuntimeState,
    wasm_bytes: &[u8],
    func_name: &str,
) -> Result<RuntimeState, Box<RunWasmError>> {
    let engine = create_engine();
    let module = match Module::new(&engine, wasm_bytes) {
        Ok(m) => m,
        Err(e) => {
            return Err(Box::new(RunWasmError { state, error: e }));
        }
    };
    let mut linker = build_linker(&engine);
    let mut store = Store::new(&engine, state);

    // Modules compiled with wasm32-wasip1-threads import shared memory
    // from "env::memory". Provide it via the linker before instantiation.
    if let Err(e) = (|| -> anyhow::Result<()> {
        let mem_ty = MemoryType::shared(17, 16384);
        let shared_mem = SharedMemory::new(&engine, mem_ty)?;
        linker.define(&mut store, "env", "memory", shared_mem)?;
        Ok(())
    })() {
        let state = store.into_data();
        return Err(Box::new(RunWasmError { state, error: e }));
    }

    let result = (|| -> anyhow::Result<()> {
        let instance = linker.instantiate(&mut store, &module)?;
        let run = instance.get_typed_func::<(), i32>(&mut store, func_name)?;
        run.call(&mut store, ())?;
        Ok(())
    })();

    let state = store.into_data();
    match result {
        Ok(()) => Ok(state),
        Err(e) => Err(Box::new(RunWasmError { state, error: e })),
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

    // 3) Layout
    let layout = engine::layout::compute_layout(&mut state.doc, engine::NodeId::from(id as u64))
        .expect("get layout");

    format!(
        "wasm module ok\nlayout={{w:{}, h:{}}}",
        layout.width, layout.height
    )
}

#[cfg(test)]
mod tests {
    use super::{build_linker, hello_engine, run_wasm};
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
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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

        let layout =
            engine::layout::compute_layout(&mut state.doc, engine::NodeId::from(id as u64))
                .expect("layout");
        assert_eq!(layout.height, 100.0);
    }

    #[test]
    fn wasm_append_element_success_and_idempotent() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_elements" (func $append_many (param i32 i32 i32) (result i32)))
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
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_elements" (func $append_many (param i32 i32 i32) (result i32)))
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
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__destroy_element" (func $destroy (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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
    (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
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
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__add_stylesheet" (func $add_css (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
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

        let layout =
            engine::layout::compute_layout(&mut state.doc, engine::NodeId::from(id as u64))
                .expect("layout");
        assert_eq!(layout.height, 77.0);
    }

    #[test]
    fn wasm_set_attribute_success() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
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
    (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
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
    fn run_wasm_success() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "width\00")
  (data (i32.const 32) "200px\00")
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $style (local.get $id) (i32.const 16) (i32.const 32)))
    (i32.const 0)
  )
)
"#;
        let state = RuntimeState::new("https://example.com".to_string());
        let state = run_wasm(state, wat.as_bytes(), "run").expect("run_wasm should succeed");

        // The created element should exist in the returned state.
        let node = state.doc.get_node(engine::NodeId::from(1_u64));
        assert!(node.is_some(), "element should exist after run_wasm");
    }

    #[test]
    fn run_wasm_invalid_wat_returns_error_with_state() {
        let state = RuntimeState::new("https://example.com".to_string());
        match run_wasm(state, b"not valid wat!", "run") {
            Ok(_) => panic!("should fail on invalid WAT"),
            Err(err) => {
                // State should be recovered even on compilation error.
                assert!(
                    err.state
                        .doc
                        .get_node(engine::NodeId::from(0_u64))
                        .is_some(),
                    "root node should still exist in recovered state"
                );
            }
        }
    }

    #[test]
    fn run_wasm_missing_export_returns_error_with_state() {
        let wat = "(module (memory (export \"memory\") 1))";
        let state = RuntimeState::new("https://example.com".to_string());
        match run_wasm(state, wat.as_bytes(), "nonexistent") {
            Ok(_) => panic!("should fail on missing export"),
            Err(err) => {
                // State should be recovered.
                assert!(
                    err.state
                        .doc
                        .get_node(engine::NodeId::from(0_u64))
                        .is_some(),
                    "root node should still exist in recovered state"
                );
            }
        }
    }

    #[test]
    fn wasm_commit_triggers_style_and_layout() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__commit" (func $commit (result i32)))
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
        assert_eq!(result, 0, "__commit should return 0 on success");

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

    // -----------------------------------------------------------------------
    // Tests for new DOM query / traversal host functions
    // -----------------------------------------------------------------------

    #[test]
    fn wasm_get_first_child() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__get_first_child" (func $first_child (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (global $result (mut i32) (i32.const 0))
    (global $empty_result (mut i32) (i32.const 0))
    (export "result" (global $result))
    (export "empty_result" (global $empty_result))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        ;; parent=1, c1=2, c2=3
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; Get first child of parent -> should be c1 (id=2)
        (global.set $result (call $first_child (local.get $parent)))
        ;; Get first child of leaf c2 -> should be -1 (no children)
        (global.set $empty_result (call $first_child (local.get $c2)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let result = instance
            .get_global(&mut store, "result")
            .expect("result global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(result, 2, "first child should be id=2");

        let empty = instance
            .get_global(&mut store, "empty_result")
            .expect("empty_result global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(empty, -1, "leaf node should have no first child");
    }

    #[test]
    fn wasm_get_last_child() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__get_last_child" (func $last_child (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (global $result (mut i32) (i32.const 0))
    (export "result" (global $result))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; last child of parent -> c2 (id=3)
        (global.set $result (call $last_child (local.get $parent)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let result = instance
            .get_global(&mut store, "result")
            .expect("result global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(result, 3, "last child should be id=3");
    }

    #[test]
    fn wasm_get_next_sibling() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__get_next_sibling" (func $next (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (global $next_of_first (mut i32) (i32.const 0))
    (global $next_of_last (mut i32) (i32.const 0))
    (export "next_of_first" (global $next_of_first))
    (export "next_of_last" (global $next_of_last))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; next sibling of c1 -> c2 (id=3)
        (global.set $next_of_first (call $next (local.get $c1)))
        ;; next sibling of c2 -> -1 (none)
        (global.set $next_of_last (call $next (local.get $c2)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let next_of_first = instance
            .get_global(&mut store, "next_of_first")
            .expect("global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(next_of_first, 3, "next sibling of c1 should be c2 (id=3)");

        let next_of_last = instance
            .get_global(&mut store, "next_of_last")
            .expect("global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(next_of_last, -1, "last child has no next sibling");
    }

    #[test]
    fn wasm_get_previous_sibling() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__get_previous_sibling" (func $prev (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (global $prev_of_last (mut i32) (i32.const 0))
    (global $prev_of_first (mut i32) (i32.const 0))
    (export "prev_of_last" (global $prev_of_last))
    (export "prev_of_first" (global $prev_of_first))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; prev sibling of c2 -> c1 (id=2)
        (global.set $prev_of_last (call $prev (local.get $c2)))
        ;; prev sibling of c1 -> -1 (none)
        (global.set $prev_of_first (call $prev (local.get $c1)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let prev_of_last = instance
            .get_global(&mut store, "prev_of_last")
            .expect("global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(prev_of_last, 2, "prev sibling of c2 should be c1 (id=2)");

        let prev_of_first = instance
            .get_global(&mut store, "prev_of_first")
            .expect("global")
            .get(&mut store)
            .i32()
            .expect("i32");
        assert_eq!(prev_of_first, -1, "first child has no previous sibling");
    }

    #[test]
    fn wasm_get_parent_element_and_parent_node() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__get_parent_element" (func $parent_elem (param i32) (result i32)))
    (import "env" "__get_parent_node" (func $parent_node (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (global $pe_of_child (mut i32) (i32.const 0))
    (global $pn_of_child (mut i32) (i32.const 0))
    (global $pe_of_root_child (mut i32) (i32.const 0))
    (global $pn_of_root_child (mut i32) (i32.const 0))
    (export "pe_of_child" (global $pe_of_child))
    (export "pn_of_child" (global $pn_of_child))
    (export "pe_of_root_child" (global $pe_of_root_child))
    (export "pn_of_root_child" (global $pn_of_root_child))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child i32)
        ;; parent=1, child=2
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 16)))
        ;; parent -> root, child -> parent
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $child)))
        ;; parent_element of child -> parent (id=1)
        (global.set $pe_of_child (call $parent_elem (local.get $child)))
        ;; parent_node of child -> parent (id=1)
        (global.set $pn_of_child (call $parent_node (local.get $child)))
        ;; parent_element of parent (whose parent is doc root, not element) -> -1
        (global.set $pe_of_root_child (call $parent_elem (local.get $parent)))
        ;; parent_node of parent -> root (id=0)
        (global.set $pn_of_root_child (call $parent_node (local.get $parent)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let pe = instance
            .get_global(&mut store, "pe_of_child")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(pe, 1, "parent_element of child should be parent (id=1)");

        let pn = instance
            .get_global(&mut store, "pn_of_child")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(pn, 1, "parent_node of child should be parent (id=1)");

        let pe_root = instance
            .get_global(&mut store, "pe_of_root_child")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            pe_root, -1,
            "parent_element of root's direct child should be -1 (root is not an element)"
        );

        let pn_root = instance
            .get_global(&mut store, "pn_of_root_child")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            pn_root, 0,
            "parent_node of root's direct child should be root (id=0)"
        );
    }

    #[test]
    fn wasm_is_connected() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__is_connected" (func $connected (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (global $detached (mut i32) (i32.const -99))
    (global $attached (mut i32) (i32.const -99))
    (export "detached" (global $detached))
    (export "attached" (global $attached))
    (func (export "run") (result i32)
        (local $el i32)
        (local $child i32)
        ;; Create element but don't attach it
        (local.set $el (call $create (i32.const 0)))
        (global.set $detached (call $connected (local.get $el)))
        ;; Attach to root
        (drop (call $append (i32.const 0) (local.get $el)))
        ;; Now create child and append to el
        (local.set $child (call $create (i32.const 16)))
        (drop (call $append (local.get $el) (local.get $child)))
        (global.set $attached (call $connected (local.get $child)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let detached = instance
            .get_global(&mut store, "detached")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(detached, 0, "detached element should not be connected");

        let attached = instance
            .get_global(&mut store, "attached")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            attached, 1,
            "element appended to doc tree should be connected"
        );
    }

    #[test]
    fn wasm_has_attribute_and_remove_attribute() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
    (import "env" "__has_attribute" (func $has_attr (param i32 i32) (result i32)))
    (import "env" "__remove_attribute" (func $rm_attr (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "class\00")
    (data (i32.const 32) "foo\00")
    (data (i32.const 48) "nope\00")
    (global $has_before (mut i32) (i32.const -99))
    (global $has_missing (mut i32) (i32.const -99))
    (global $has_after_remove (mut i32) (i32.const -99))
    (export "has_before" (global $has_before))
    (export "has_missing" (global $has_missing))
    (export "has_after_remove" (global $has_after_remove))
    (func (export "run") (result i32)
        (local $id i32)
        (local.set $id (call $create (i32.const 0)))
        ;; Set class="foo"
        (drop (call $set_attr (local.get $id) (i32.const 16) (i32.const 32)))
        ;; Check has("class") -> 1
        (global.set $has_before (call $has_attr (local.get $id) (i32.const 16)))
        ;; Check has("nope") -> 0
        (global.set $has_missing (call $has_attr (local.get $id) (i32.const 48)))
        ;; Remove class, check again -> 0
        (drop (call $rm_attr (local.get $id) (i32.const 16)))
        (global.set $has_after_remove (call $has_attr (local.get $id) (i32.const 16)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let has_before = instance
            .get_global(&mut store, "has_before")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            has_before, 1,
            "should have 'class' attribute after setting it"
        );

        let has_missing = instance
            .get_global(&mut store, "has_missing")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(has_missing, 0, "should not have 'nope' attribute");

        let has_after = instance
            .get_global(&mut store, "has_after_remove")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(has_after, 0, "should not have 'class' after removal");
    }

    #[test]
    fn wasm_get_attribute() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
    (import "env" "__get_attribute" (func $get_attr (param i32 i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "data-x\00")
    (data (i32.const 32) "hello\00")
    (data (i32.const 48) "nope\00")
    ;; buffer at offset 256
    (global $len (mut i32) (i32.const -99))
    (global $missing_len (mut i32) (i32.const -99))
    (export "len" (global $len))
    (export "missing_len" (global $missing_len))
    (func (export "run") (result i32)
        (local $id i32)
        (local.set $id (call $create (i32.const 0)))
        ;; set data-x="hello"
        (drop (call $set_attr (local.get $id) (i32.const 16) (i32.const 32)))
        ;; get_attribute(id, "data-x", buf@256, 128) -> length of "hello" = 5
        (global.set $len (call $get_attr (local.get $id) (i32.const 16) (i32.const 256) (i32.const 128)))
        ;; get_attribute(id, "nope", buf@256, 128) -> -1 (not found)
        (global.set $missing_len (call $get_attr (local.get $id) (i32.const 48) (i32.const 256) (i32.const 128)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let len = instance
            .get_global(&mut store, "len")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(len, 5, "get_attribute should return length of 'hello'");

        let missing = instance
            .get_global(&mut store, "missing_len")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            missing, -1,
            "get_attribute for missing attr should return -1"
        );

        // Verify buffer contents
        let memory = instance.get_memory(&mut store, "memory").expect("memory");
        let data = memory.data(&store);
        let buf = &data[256..261];
        assert_eq!(buf, b"hello", "buffer should contain 'hello'");
    }

    #[test]
    fn wasm_remove_child() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__remove_child" (func $remove (param i32 i32) (result i32)))
    (import "env" "__get_first_child" (func $first_child (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (global $remove_status (mut i32) (i32.const -99))
    (global $first_after (mut i32) (i32.const -99))
    (export "remove_status" (global $remove_status))
    (export "first_after" (global $first_after))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; Remove c1 from parent
        (global.set $remove_status (call $remove (local.get $parent) (local.get $c1)))
        ;; First child should now be c2
        (global.set $first_after (call $first_child (local.get $parent)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let remove_status = instance
            .get_global(&mut store, "remove_status")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(remove_status, 0, "remove_child should return 0 on success");

        let first_after = instance
            .get_global(&mut store, "first_after")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            first_after, 3,
            "after removing c1, first child should be c2 (id=3)"
        );
    }

    #[test]
    fn wasm_replace_child() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__replace_child" (func $replace (param i32 i32 i32) (result i32)))
    (import "env" "__get_first_child" (func $first_child (param i32) (result i32)))
    (import "env" "__get_last_child" (func $last_child (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "p\00")
    (data (i32.const 48) "a\00")
    (global $replace_status (mut i32) (i32.const -99))
    (global $first_after (mut i32) (i32.const -99))
    (global $last_after (mut i32) (i32.const -99))
    (export "replace_status" (global $replace_status))
    (export "first_after" (global $first_after))
    (export "last_after" (global $last_after))
    (func (export "run") (result i32)
        (local $parent i32)
        (local $c1 i32)
        (local $c2 i32)
        (local $new i32)
        ;; parent=1, c1=2, c2=3, new=4
        (local.set $parent (call $create (i32.const 0)))
        (local.set $c1 (call $create (i32.const 16)))
        (local.set $c2 (call $create (i32.const 32)))
        (local.set $new (call $create (i32.const 48)))
        (drop (call $append (i32.const 0) (local.get $parent)))
        (drop (call $append (local.get $parent) (local.get $c1)))
        (drop (call $append (local.get $parent) (local.get $c2)))
        ;; Replace c1 with new -> parent's children should be [new, c2]
        (global.set $replace_status (call $replace (local.get $parent) (local.get $new) (local.get $c1)))
        (global.set $first_after (call $first_child (local.get $parent)))
        (global.set $last_after (call $last_child (local.get $parent)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let replace_status = instance
            .get_global(&mut store, "replace_status")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(
            replace_status, 0,
            "replace_child should return 0 on success"
        );

        let first = instance
            .get_global(&mut store, "first_after")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(first, 4, "first child should be the new element (id=4)");

        let last = instance
            .get_global(&mut store, "last_after")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(last, 3, "last child should remain c2 (id=3)");
    }

    #[test]
    fn wasm_remove_child_invalid_returns_error() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__remove_child" (func $remove (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (global $result (mut i32) (i32.const 0))
    (export "result" (global $result))
    (func (export "run") (result i32)
        (local $el i32)
        (local.set $el (call $create (i32.const 0)))
        ;; Try to remove el from non-existent parent 99
        (global.set $result (call $remove (i32.const 99) (local.get $el)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        run.call(&mut store, ()).expect("run");

        let result = instance
            .get_global(&mut store, "result")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert!(
            result < 0,
            "remove_child with invalid parent should return error code"
        );
    }

    #[test]
    fn wasm_traversal_negative_id_returns_error() {
        // Verify that passing negative IDs to traversal functions returns error codes
        let wat = r#"
(module
    (import "env" "__get_first_child" (func $first (param i32) (result i32)))
    (import "env" "__get_parent_node" (func $parent (param i32) (result i32)))
    (import "env" "__is_connected" (func $connected (param i32) (result i32)))
    (memory (export "memory") 1)
    (global $r1 (mut i32) (i32.const 0))
    (global $r2 (mut i32) (i32.const 0))
    (global $r3 (mut i32) (i32.const 0))
    (export "r1" (global $r1))
    (export "r2" (global $r2))
    (export "r3" (global $r3))
    (func (export "run") (result i32)
        (global.set $r1 (call $first (i32.const -1)))
        (global.set $r2 (call $parent (i32.const -5)))
        (global.set $r3 (call $connected (i32.const -10)))
        (i32.const 0)
    )
)
"#;
        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(
            &engine,
            RuntimeState::new("https://example.com".to_string()),
        );
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        run.call(&mut store, ()).expect("run");

        let r1 = instance
            .get_global(&mut store, "r1")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        let r2 = instance
            .get_global(&mut store, "r2")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        let r3 = instance
            .get_global(&mut store, "r3")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();

        assert_eq!(r1, HostErrorCode::InvalidChild.as_i32());
        assert_eq!(r2, HostErrorCode::InvalidChild.as_i32());
        assert_eq!(r3, HostErrorCode::InvalidChild.as_i32());
    }
}
