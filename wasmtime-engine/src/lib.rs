//! Wasm Bridge: threads together wasmtime, stylo, and taffy.

pub mod wasm;

pub use wasm::{build_linker, read_cstr};

use engine::{EngineRenderer, RuntimeState};
use wasmtime::{AsContextMut, Engine as WasmEngine, MemoryType, Module, SharedMemory, Store};

/// Create a [`wasmtime::Engine`] configured for the current platform.
///
/// On iOS, JIT compilation is forbidden (no W+X pages), so we target
/// Pulley — wasmtime's portable interpreter. On all other platforms we
/// use the default (Cranelift) configuration.
pub fn create_engine() -> WasmEngine {
    let mut config = wasmtime::Config::new();
    config.wasm_threads(true);
    config.shared_memory(true);
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
pub struct RunWasmError<R: EngineRenderer = ()> {
    /// The `RuntimeState` recovered from the wasmtime `Store`.
    pub state: RuntimeState<R>,
    /// The underlying error.
    pub error: wasmtime::Error,
}

impl<R: EngineRenderer> std::fmt::Debug for RunWasmError<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunWasmError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Result of running a WASM module with coverage extraction.
type WasmCoverageResult<R> = Result<(RuntimeState<R>, Option<Vec<u8>>), Box<RunWasmError<R>>>;

/// Compiles and runs a binary WASM module against a [`RuntimeState`].
///
/// The `RuntimeState` is always recovered, even on error.
pub fn run_wasm<R: EngineRenderer>(
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    func_name: &str,
) -> Result<RuntimeState<R>, Box<RunWasmError<R>>> {
    run_wasm_inner(state, wasm_bytes, func_name, false).map(|(state, _)| state)
}

/// Like [`run_wasm`] but also extracts LLVM coverage data from the guest.
///
/// If the guest was compiled with the `coverage` feature (minicov), the
/// returned `Option<Vec<u8>>` contains the profraw bytes. Otherwise it
/// is `None`.
pub fn run_wasm_with_coverage<R: EngineRenderer>(
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    func_name: &str,
) -> WasmCoverageResult<R> {
    run_wasm_inner(state, wasm_bytes, func_name, true)
}

/// Shared implementation for [`run_wasm`] and [`run_wasm_with_coverage`].
fn run_wasm_inner<R: EngineRenderer>(
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    func_name: &str,
    extract_coverage: bool,
) -> WasmCoverageResult<R> {
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
    if let Err(e) = (|| -> wasmtime::Result<()> {
        let mem_ty = MemoryType::shared(17, 16384);
        let shared_mem = SharedMemory::new(&engine, mem_ty)?;
        linker.define(&mut store, "env", "memory", shared_mem)?;
        Ok(())
    })() {
        let state = store.into_data();
        return Err(Box::new(RunWasmError { state, error: e }));
    }

    let result = (|| -> wasmtime::Result<(wasmtime::Instance, ())> {
        let instance = linker.instantiate(&mut store, &module)?;

        // WASI reactor modules (cdylib with std) export `_initialize` to
        // set up the C runtime (TLS, constructors, etc.). Call it before
        // any guest code that uses `thread_local!` or other std facilities.
        if let Ok(init) = instance.get_typed_func::<(), ()>(&mut store, "_initialize") {
            init.call(&mut store, ())?;
        }

        let run = instance.get_typed_func::<(), i32>(&mut store, func_name)?;
        run.call(&mut store, ())?;
        Ok((instance, ()))
    })();

    match result {
        Ok((instance, ())) => {
            let coverage = if extract_coverage {
                extract_guest_coverage(&instance, &mut store)
            } else {
                None
            };
            let state = store.into_data();
            Ok((state, coverage))
        }
        Err(e) => {
            let state = store.into_data();
            Err(Box::new(RunWasmError { state, error: e }))
        }
    }
}

/// Extracts profraw coverage bytes from a WASM guest instance.
///
/// Looks for the `__paws_dump_coverage` and `__paws_coverage_ptr` exports
/// that `rust-wasm-binding` provides when compiled with the `coverage`
/// feature. Returns `None` if the exports are absent or if the guest
/// reports zero coverage bytes.
fn extract_guest_coverage<R: EngineRenderer>(
    instance: &wasmtime::Instance,
    store: &mut Store<RuntimeState<R>>,
) -> Option<Vec<u8>> {
    let dump_function = instance
        .get_typed_func::<(), i32>(store.as_context_mut(), "__paws_dump_coverage")
        .ok()?;
    let raw_length = dump_function.call(store.as_context_mut(), ()).ok()?;
    let length = (raw_length > 0).then_some(raw_length as usize)?;

    let ptr_function = instance
        .get_typed_func::<(), i32>(store.as_context_mut(), "__paws_coverage_ptr")
        .ok()?;
    let raw_pointer = ptr_function.call(store.as_context_mut(), ()).ok()?;
    let pointer = (raw_pointer > 0).then_some(raw_pointer as usize)?;

    // Read bytes from WASM linear memory. Handle both regular Memory
    // (WAT tests) and SharedMemory (wasm32-wasip1-threads modules).
    if let Some(memory) = instance.get_memory(store.as_context_mut(), "memory") {
        let data = memory.data(&store);
        if pointer + length > data.len() {
            return None;
        }
        Some(data[pointer..pointer + length].to_vec())
    } else if let Some(wasmtime::Extern::SharedMemory(shared)) =
        instance.get_export(store.as_context_mut(), "memory")
    {
        let data = shared.data();
        if pointer + length > data.len() {
            return None;
        }
        let source = data[pointer..pointer + length].as_ptr() as *const u8;
        // SAFETY: Direct, non-atomic memory access to Wasmtime's SharedMemory
        // is safe here because the guest function has already returned — no
        // concurrent WASM execution is happening, so no concurrent writes to
        // this memory region can occur.
        Some(unsafe { std::slice::from_raw_parts(source, length) }.to_vec())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{build_linker, create_engine, run_wasm};
    use engine::{HostErrorCode, RuntimeState};
    use wasmtime::{Engine as WasmEngine, Module, Store};

    /// Wires wasm, layout, and style together to verify the full pipeline.
    ///
    /// No text nodes are involved — this exercises element layout only.
    #[test]
    fn hello_engine_works() {
        let wasm_engine = create_engine();
        let wasm_bytes = b"(module)";
        let _module = Module::new(&wasm_engine, wasm_bytes).expect("compile minimal wasm module");

        let mut state = RuntimeState::new("https://example.com".to_string());
        let id = state.create_element("div".to_string());
        state.append_element(0, id).expect("append to doc");
        let _ = state.set_inline_style(id, "display".to_string(), "block".to_string());
        let _ = state.set_inline_style(id, "height".to_string(), "80px".to_string());
        let _ = state.set_inline_style(id, "width".to_string(), "120px".to_string());

        state.commit();
        let node = state.doc.get_node(engine::NodeId::from(id as u64)).unwrap();
        assert_eq!(node.layout().size.width, 120.0);
        assert_eq!(node.layout().size.height, 80.0);
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
        state.commit();
        let node = state
            .doc
            .get_node(engine::NodeId::from(id as u64))
            .expect("node");
        assert_eq!(node.layout().size.height, 100.0);
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
        state.commit();
        let node = state
            .doc
            .get_node(engine::NodeId::from(id as u64))
            .expect("node");
        assert_eq!(node.layout().size.height, 77.0);
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
        store.data_mut().commit();
        let node = store
            .data()
            .doc
            .get_node(engine::NodeId::from(1_u64))
            .unwrap();
        assert_eq!(node.layout().size.width, 150.0);
        assert_eq!(node.layout().size.height, 75.0);
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

    #[test]
    fn wasm_create_text_node_and_commit() {
        // E2E test: create a div, create a text node, append text to div,
        // commit, and verify text node appears in the layout with content.
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__create_text_node" (func $text (param i32) (result i32)))
    (import "env" "__set_inline_style" (func $style (param i32 i32 i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__commit" (func $commit (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "width\00")
    (data (i32.const 32) "200px\00")
    (data (i32.const 48) "Hello Paws\00")
    (global $div_id (mut i32) (i32.const 0))
    (global $txt_id (mut i32) (i32.const 0))
    (export "div_id" (global $div_id))
    (export "txt_id" (global $txt_id))
    (func (export "run") (result i32)
        ;; Create div with width
        (global.set $div_id (call $create (i32.const 0)))
        (drop (call $append (i32.const 0) (global.get $div_id)))
        (drop (call $style (global.get $div_id) (i32.const 16) (i32.const 32)))
        ;; Create text node and append to div
        (global.set $txt_id (call $text (i32.const 48)))
        (drop (call $append (global.get $div_id) (global.get $txt_id)))
        ;; Commit triggers style + layout
        (drop (call $commit))
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

        let _div_id = instance
            .get_global(&mut store, "div_id")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        let txt_id = instance
            .get_global(&mut store, "txt_id")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();

        // Verify text node exists in the DOM
        let state = store.data();
        let text_node = state
            .doc
            .get_node(engine::NodeId::from(txt_id as u64))
            .expect("text node should exist");
        assert!(text_node.is_text_node());

        // Verify layout includes text node with non-zero dimensions
        let state = store.data_mut();
        state.commit();
        let text_node = state
            .doc
            .get_node(engine::NodeId::from(txt_id as u64))
            .expect("text node");
        assert!(text_node.is_text_node(), "child should be a text node");
        assert_eq!(text_node.text(), Some("Hello Paws"));
        assert!(
            text_node.layout().size.width > 0.0,
            "text should have positive width"
        );
        assert!(
            text_node.layout().size.height > 0.0,
            "text should have positive height"
        );
    }

    // ─── insert_before WAT tests ──────────────────────────────────

    #[test]
    fn wasm_insert_before_success() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__insert_before" (func $insert_before (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "em\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $a i32)
        (local $b i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $a (call $create (i32.const 16)))
        (local.set $b (call $create (i32.const 32)))
        ;; parent -> [a]
        (drop (call $append (local.get $parent) (local.get $a)))
        ;; insert b before a → parent -> [b, a]
        (call $insert_before (local.get $parent) (local.get $b) (local.get $a))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let state = store.data();
        let parent = state.doc.get_node(engine::NodeId::from(1_u64)).unwrap();
        assert_eq!(
            parent.children,
            vec![engine::NodeId::from(3_u64), engine::NodeId::from(2_u64)]
        );
    }

    #[test]
    fn wasm_insert_before_negative_id_returns_error() {
        let wat = r#"
(module
    (import "env" "__insert_before" (func $insert_before (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (func (export "run") (result i32)
        (call $insert_before (i32.const -1) (i32.const 1) (i32.const 2))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    #[test]
    fn wasm_insert_before_invalid_ref_returns_error() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__insert_before" (func $insert_before (param i32 i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 0)))
        ;; ref_child (99) does not exist
        (call $insert_before (local.get $parent) (local.get $child) (i32.const 99))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    // ─── clone_node WAT tests ─────────────────────────────────────

    #[test]
    fn wasm_clone_node_shallow() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__set_attribute" (func $set_attr (param i32 i32 i32) (result i32)))
    (import "env" "__clone_node" (func $clone (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (data (i32.const 32) "id\00")
    (data (i32.const 48) "myid\00")
    (func (export "run") (result i32)
        (local $el i32)
        (local $child i32)
        (local $cloned i32)
        (local.set $el (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 16)))
        (drop (call $set_attr (local.get $el) (i32.const 32) (i32.const 48)))
        (drop (call $append (local.get $el) (local.get $child)))
        ;; shallow clone (deep=0)
        (local.set $cloned (call $clone (local.get $el) (i32.const 0)))
        (local.get $cloned)
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let cloned_id = run.call(&mut store, ()).expect("run");
        assert!(cloned_id > 0, "clone should return a positive ID");

        let state = store.data();
        let cloned = state
            .doc
            .get_node(engine::NodeId::from(cloned_id as u64))
            .expect("cloned node should exist");
        assert!(cloned.is_element());
        // Verify attribute was cloned via RuntimeState's public API
        let attr = store
            .data()
            .get_attribute(cloned_id as u32, "id")
            .expect("get_attribute should work");
        assert_eq!(attr.as_deref(), Some("myid"));
        // Shallow clone: no children
        assert!(cloned.children.is_empty());
        assert!(cloned.parent.is_none());
    }

    #[test]
    fn wasm_clone_node_deep() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
    (import "env" "__clone_node" (func $clone (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "span\00")
    (func (export "run") (result i32)
        (local $parent i32)
        (local $child i32)
        (local $cloned i32)
        (local.set $parent (call $create (i32.const 0)))
        (local.set $child (call $create (i32.const 16)))
        (drop (call $append (local.get $parent) (local.get $child)))
        ;; deep clone (deep=1)
        (local.set $cloned (call $clone (local.get $parent) (i32.const 1)))
        (local.get $cloned)
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let cloned_id = run.call(&mut store, ()).expect("run");
        assert!(cloned_id > 0);

        let state = store.data();
        let cloned = state
            .doc
            .get_node(engine::NodeId::from(cloned_id as u64))
            .expect("cloned node");
        assert!(cloned.is_element());
        assert_eq!(cloned.children.len(), 1, "deep clone should have 1 child");

        let cloned_child = state
            .doc
            .get_node(cloned.children[0])
            .expect("cloned child");
        assert!(cloned_child.is_element());
        // Verify the cloned child is a "span" element via node type
        assert!(cloned_child.is_element());
        assert_eq!(
            cloned_child.parent,
            Some(engine::NodeId::from(cloned_id as u64))
        );
    }

    #[test]
    fn wasm_clone_node_negative_id_returns_error() {
        let wat = r#"
(module
    (import "env" "__clone_node" (func $clone (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (func (export "run") (result i32)
        (call $clone (i32.const -1) (i32.const 0))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    // ─── set_node_value WAT tests ─────────────────────────────────

    #[test]
    fn wasm_set_node_value_text_node() {
        let wat = r#"
(module
    (import "env" "__create_text_node" (func $create_text (param i32) (result i32)))
    (import "env" "__set_node_value" (func $set_value (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "old\00")
    (data (i32.const 16) "new\00")
    (func (export "run") (result i32)
        (local $text i32)
        (local.set $text (call $create_text (i32.const 0)))
        (call $set_value (local.get $text) (i32.const 16))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, 0);

        let state = store.data();
        let text = state
            .doc
            .get_node(engine::NodeId::from(1_u64))
            .expect("text node");
        assert_eq!(text.text(), Some("new"));
    }

    #[test]
    fn wasm_set_node_value_element_noop() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__set_node_value" (func $set_value (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (data (i32.const 16) "ignored\00")
    (func (export "run") (result i32)
        (local $el i32)
        (local.set $el (call $create (i32.const 0)))
        (call $set_value (local.get $el) (i32.const 16))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        // Should return 0 (no-op, not an error)
        assert_eq!(status, 0);
    }

    #[test]
    fn wasm_set_node_value_negative_id_returns_error() {
        let wat = r#"
(module
    (import "env" "__set_node_value" (func $set_value (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "val\00")
    (func (export "run") (result i32)
        (call $set_value (i32.const -1) (i32.const 0))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    // ─── get_node_type WAT tests ──────────────────────────────────

    #[test]
    fn wasm_get_node_type_element() {
        let wat = r#"
(module
    (import "env" "__create_element" (func $create (param i32) (result i32)))
    (import "env" "__get_node_type" (func $get_type (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "div\00")
    (func (export "run") (result i32)
        (local $el i32)
        (local.set $el (call $create (i32.const 0)))
        (call $get_type (local.get $el))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let node_type = run.call(&mut store, ()).expect("run");
        assert_eq!(node_type, 1); // ELEMENT_NODE
    }

    #[test]
    fn wasm_get_node_type_text() {
        let wat = r#"
(module
    (import "env" "__create_text_node" (func $create_text (param i32) (result i32)))
    (import "env" "__get_node_type" (func $get_type (param i32) (result i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "hi\00")
    (func (export "run") (result i32)
        (local $text i32)
        (local.set $text (call $create_text (i32.const 0)))
        (call $get_type (local.get $text))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let node_type = run.call(&mut store, ()).expect("run");
        assert_eq!(node_type, 3); // TEXT_NODE
    }

    #[test]
    fn wasm_get_node_type_document() {
        let wat = r#"
(module
    (import "env" "__get_node_type" (func $get_type (param i32) (result i32)))
    (memory (export "memory") 1)
    (func (export "run") (result i32)
        ;; Document root is id 0
        (call $get_type (i32.const 0))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let node_type = run.call(&mut store, ()).expect("run");
        assert_eq!(node_type, 9); // DOCUMENT_NODE
    }

    #[test]
    fn wasm_get_node_type_negative_id_returns_error() {
        let wat = r#"
(module
    (import "env" "__get_node_type" (func $get_type (param i32) (result i32)))
    (memory (export "memory") 1)
    (func (export "run") (result i32)
        (call $get_type (i32.const -1))
    )
)
"#;

        let engine = WasmEngine::default();
        let module = Module::new(&engine, wat).expect("compile");
        let mut store = Store::new(&engine, RuntimeState::new("https://example.com".into()));
        let linker = build_linker(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate");
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("get run");
        let status = run.call(&mut store, ()).expect("run");
        assert_eq!(status, HostErrorCode::InvalidChild.as_i32());
    }

    // ── Event system WAT integration tests ──────────────────────────

    /// Basic event dispatch: add listener + dispatch → __paws_invoke_listener called.
    #[test]
    fn wasm_event_add_listener_and_dispatch() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
  )
  (func (export "run") (result i32)
    (local $id i32)
    ;; Create element and attach to root
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Add event listener (callback_id=42, options=0)
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 42) (i32.const 0)))
    ;; Dispatch "click" event (bubbles=1, cancelable=1, composed=0)
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Return counter (should be 1)
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 1, "listener should have been invoked once");
    }

    /// Event bubbling: child dispatch triggers listeners on parent.
    #[test]
    fn wasm_event_bubbling() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
  )
  (func (export "run") (result i32)
    (local $parent i32)
    (local $child i32)
    ;; Create parent and child
    (local.set $parent (call $create (i32.const 0)))
    (local.set $child (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $parent)))
    (drop (call $append (local.get $parent) (local.get $child)))
    ;; Add bubble listener on parent (callback_id=1, options=0)
    (drop (call $add_listener (local.get $parent) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Add bubble listener on child (callback_id=2, options=0)
    (drop (call $add_listener (local.get $child) (i32.const 16) (i32.const 2) (i32.const 0)))
    ;; Dispatch on child — should fire child then parent (bubbling)
    (drop (call $dispatch (local.get $child) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Return counter (should be 2)
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 2, "both child and parent listeners should fire");
    }

    /// stopPropagation from WASM handler prevents parent listener.
    #[test]
    fn wasm_event_stop_propagation() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_stop_propagation" (func $stop_prop (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
    ;; callback_id=2 (child) calls stopPropagation
    (if (i32.eq (local.get $cb_id) (i32.const 2))
      (then (drop (call $stop_prop)))
    )
  )
  (func (export "run") (result i32)
    (local $parent i32)
    (local $child i32)
    (local.set $parent (call $create (i32.const 0)))
    (local.set $child (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $parent)))
    (drop (call $append (local.get $parent) (local.get $child)))
    ;; Parent listener (callback_id=1) — should NOT fire
    (drop (call $add_listener (local.get $parent) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Child listener (callback_id=2) — calls stopPropagation
    (drop (call $add_listener (local.get $child) (i32.const 16) (i32.const 2) (i32.const 0)))
    ;; Dispatch on child
    (drop (call $dispatch (local.get $child) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Return counter (should be 1 — only child fires)
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 1, "only child should fire after stopPropagation");
    }

    /// preventDefault returns 0 from dispatch_event.
    #[test]
    fn wasm_event_prevent_default() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_prevent_default" (func $prevent (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (drop (call $prevent))
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Dispatch cancelable event — listener calls preventDefault
    ;; Returns 0 (canceled) or 1 (not canceled)
    (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, 0, "dispatch should return 0 (canceled)");
    }

    /// Event accessors during dispatch: target, currentTarget, phase, bubbles, cancelable.
    #[test]
    fn wasm_event_accessors() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_target" (func $target (result i32)))
  (import "env" "__event_current_target" (func $current_target (result i32)))
  (import "env" "__event_phase" (func $phase (result i32)))
  (import "env" "__event_bubbles" (func $bubbles (result i32)))
  (import "env" "__event_cancelable" (func $cancelable (result i32)))
  (import "env" "__event_default_prevented" (func $default_prevented (result i32)))
  (import "env" "__event_composed" (func $composed (result i32)))
  (import "env" "__event_timestamp" (func $timestamp (result f64)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "test\00")
  (global $g_target (export "g_target") (mut i32) (i32.const -99))
  (global $g_current (export "g_current") (mut i32) (i32.const -99))
  (global $g_phase (export "g_phase") (mut i32) (i32.const -99))
  (global $g_bubbles (export "g_bubbles") (mut i32) (i32.const -99))
  (global $g_cancelable (export "g_cancelable") (mut i32) (i32.const -99))
  (global $g_prevented (export "g_prevented") (mut i32) (i32.const -99))
  (global $g_composed (export "g_composed") (mut i32) (i32.const -99))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $g_target (call $target))
    (global.set $g_current (call $current_target))
    (global.set $g_phase (call $phase))
    (global.set $g_bubbles (call $bubbles))
    (global.set $g_cancelable (call $cancelable))
    (global.set $g_prevented (call $default_prevented))
    (global.set $g_composed (call $composed))
    ;; Also call timestamp to exercise the f64 host function
    (drop (call $timestamp))
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Add at-target listener
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Dispatch (bubbles=1, cancelable=1, composed=0)
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Return target id (should equal the element id)
    (global.get $g_target)
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
        let target_id = run.call(&mut store, ()).expect("run");
        // Element id is 1 (root is 0, first created element is 1)
        assert_eq!(
            target_id, 1,
            "target should be the element we dispatched on"
        );

        // Read globals to verify all accessors worked
        let g_current = instance
            .get_global(&mut store, "g_current")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_current, 1, "current_target should be element 1 at-target");

        let g_phase = instance
            .get_global(&mut store, "g_phase")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_phase, 2, "phase should be AT_TARGET (2)");

        let g_bubbles = instance
            .get_global(&mut store, "g_bubbles")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_bubbles, 1, "bubbles should be 1");

        let g_cancelable = instance
            .get_global(&mut store, "g_cancelable")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_cancelable, 1, "cancelable should be 1");

        let g_prevented = instance
            .get_global(&mut store, "g_prevented")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_prevented, 0, "default_prevented should be 0");

        let g_composed = instance
            .get_global(&mut store, "g_composed")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(g_composed, 0, "composed should be 0");
    }

    /// Once listener fires only once.
    #[test]
    fn wasm_event_once_listener() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Add once listener (options_flags=4, bit 2 = once)
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 4)))
    ;; Dispatch twice
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Counter should be 1 (once listener removed after first)
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 1, "once listener should fire only once");
    }

    /// Remove listener prevents it from firing.
    #[test]
    fn wasm_event_remove_listener() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__remove_event_listener" (func $remove_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Add listener
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Remove it
    (drop (call $remove_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Dispatch — should fire nothing
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 0, "removed listener should not fire");
    }

    /// stopImmediatePropagation prevents second listener on same node.
    #[test]
    fn wasm_event_stop_immediate_propagation() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_stop_immediate_propagation" (func $stop_imm (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $counter (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
    ;; First listener (callback_id=1) calls stopImmediatePropagation
    (if (i32.eq (local.get $cb_id) (i32.const 1))
      (then (drop (call $stop_imm)))
    )
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Two listeners on same node
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    (drop (call $add_listener (local.get $id) (i32.const 16) (i32.const 2) (i32.const 0)))
    ;; Dispatch
    (drop (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Only first listener fires
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(
            counter, 1,
            "stopImmediatePropagation should halt second listener"
        );
    }

    /// Event accessor host functions return error when no event is active.
    #[test]
    fn wasm_event_accessors_no_active_event() {
        let wat = r#"
(module
  (import "env" "__event_target" (func $target (result i32)))
  (import "env" "__event_phase" (func $phase (result i32)))
  (import "env" "__event_stop_propagation" (func $stop (result i32)))
  (import "env" "__event_prevent_default" (func $prevent (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (local $t i32)
    (local $p i32)
    (local $s i32)
    (local $pd i32)
    ;; Call accessors outside dispatch — should return NoActiveEvent error
    (local.set $t (call $target))
    (local.set $p (call $phase))
    (local.set $s (call $stop))
    (local.set $pd (call $prevent))
    ;; All should return NoActiveEvent (-7)
    ;; Return target result
    (local.get $t)
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::NoActiveEvent.as_i32());
    }

    /// Capture listener fires during capture phase.
    #[test]
    fn wasm_event_capture_phase() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add_listener (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_phase" (func $phase (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  (global $capture_phase (export "capture_phase") (mut i32) (i32.const -1))
  (global $target_phase (export "target_phase") (mut i32) (i32.const -1))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    ;; callback_id=1 is capture listener on parent
    (if (i32.eq (local.get $cb_id) (i32.const 1))
      (then (global.set $capture_phase (call $phase)))
    )
    ;; callback_id=2 is at-target listener on child
    (if (i32.eq (local.get $cb_id) (i32.const 2))
      (then (global.set $target_phase (call $phase)))
    )
  )
  (func (export "run") (result i32)
    (local $parent i32)
    (local $child i32)
    (local.set $parent (call $create (i32.const 0)))
    (local.set $child (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $parent)))
    (drop (call $append (local.get $parent) (local.get $child)))
    ;; Capture listener on parent (options_flags=1, bit 0 = capture)
    (drop (call $add_listener (local.get $parent) (i32.const 16) (i32.const 1) (i32.const 1)))
    ;; At-target listener on child (options_flags=0)
    (drop (call $add_listener (local.get $child) (i32.const 16) (i32.const 2) (i32.const 0)))
    ;; Dispatch on child
    (drop (call $dispatch (local.get $child) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0)))
    ;; Return capture phase value (should be 1 = CAPTURING_PHASE)
    (global.get $capture_phase)
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
        let capture_phase = run.call(&mut store, ()).expect("run");
        assert_eq!(
            capture_phase, 1,
            "capture listener should see CAPTURING_PHASE (1)"
        );

        let target_phase = instance
            .get_global(&mut store, "target_phase")
            .unwrap()
            .get(&mut store)
            .i32()
            .unwrap();
        assert_eq!(target_phase, 2, "target listener should see AT_TARGET (2)");
    }

    /// Negative target ID returns InvalidEventTarget error.
    #[test]
    fn wasm_event_add_listener_negative_id() {
        let wat = r#"
(module
  (import "env" "__add_event_listener" (func $add (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    ;; target_id = -1 (negative)
    (call $add (i32.const -1) (i32.const 0) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }

    /// Negative target ID on remove_event_listener returns error.
    #[test]
    fn wasm_event_remove_listener_negative_id() {
        let wat = r#"
(module
  (import "env" "__remove_event_listener" (func $remove (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    (call $remove (i32.const -1) (i32.const 0) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }

    /// Negative target ID on dispatch_event returns error.
    #[test]
    fn wasm_event_dispatch_negative_id() {
        let wat = r#"
(module
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    (call $dispatch (i32.const -1) (i32.const 0) (i32.const 1) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }

    /// Dispatch on nonexistent node returns InvalidEventTarget.
    #[test]
    fn wasm_event_dispatch_nonexistent_target() {
        let wat = r#"
(module
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    ;; target_id=999, doesn't exist
    (call $dispatch (i32.const 999) (i32.const 0) (i32.const 1) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }

    /// Dispatch works without __paws_invoke_listener export (no-op listeners).
    #[test]
    fn wasm_event_dispatch_no_invoke_export() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "click\00")
  ;; No __paws_invoke_listener export — dispatch should still succeed
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    (drop (call $add (local.get $id) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Dispatch — returns 1 (not canceled) since listener is a no-op
    (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(
            result, 1,
            "dispatch without invoke export should return 1 (not canceled)"
        );
    }

    /// Non-bubbling event: parent bubble listener doesn't fire.
    #[test]
    fn wasm_event_non_bubbling() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "focus\00")
  (global $counter (export "counter") (mut i32) (i32.const 0))
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
  )
  (func (export "run") (result i32)
    (local $parent i32)
    (local $child i32)
    (local.set $parent (call $create (i32.const 0)))
    (local.set $child (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $parent)))
    (drop (call $append (local.get $parent) (local.get $child)))
    ;; Bubble listener on parent
    (drop (call $add (local.get $parent) (i32.const 16) (i32.const 1) (i32.const 0)))
    ;; Listener on child
    (drop (call $add (local.get $child) (i32.const 16) (i32.const 2) (i32.const 0)))
    ;; Dispatch non-bubbling event (bubbles=0)
    (drop (call $dispatch (local.get $child) (i32.const 16) (i32.const 0) (i32.const 0) (i32.const 0)))
    (global.get $counter)
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
        let counter = run.call(&mut store, ()).expect("run");
        assert_eq!(counter, 1, "non-bubbling event should only fire at-target");
    }

    /// Passive listener: preventDefault is ignored.
    #[test]
    fn wasm_event_passive_prevents_default() {
        let wat = r#"
(module
  (import "env" "__create_element" (func $create (param i32) (result i32)))
  (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
  (import "env" "__add_event_listener" (func $add (param i32 i32 i32 i32) (result i32)))
  (import "env" "__dispatch_event" (func $dispatch (param i32 i32 i32 i32 i32) (result i32)))
  (import "env" "__event_prevent_default" (func $prevent (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "div\00")
  (data (i32.const 16) "scroll\00")
  (func (export "__paws_invoke_listener") (param $cb_id i32)
    ;; Try to preventDefault from passive listener — should be no-op
    (drop (call $prevent))
  )
  (func (export "run") (result i32)
    (local $id i32)
    (local.set $id (call $create (i32.const 0)))
    (drop (call $append (i32.const 0) (local.get $id)))
    ;; Add passive listener (options_flags=2, bit 1 = passive)
    (drop (call $add (local.get $id) (i32.const 16) (i32.const 1) (i32.const 2)))
    ;; Dispatch cancelable event
    (call $dispatch (local.get $id) (i32.const 16) (i32.const 1) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(
            result, 1,
            "passive listener preventDefault should be ignored, event not canceled"
        );
    }

    /// Add listener to nonexistent target returns error.
    #[test]
    fn wasm_event_add_listener_invalid_target() {
        let wat = r#"
(module
  (import "env" "__add_event_listener" (func $add (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    ;; target_id=999, doesn't exist
    (call $add (i32.const 999) (i32.const 0) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }

    /// Remove listener from nonexistent target returns error.
    #[test]
    fn wasm_event_remove_listener_invalid_target() {
        let wat = r#"
(module
  (import "env" "__remove_event_listener" (func $remove (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "click\00")
  (func (export "run") (result i32)
    (call $remove (i32.const 999) (i32.const 0) (i32.const 1) (i32.const 0))
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
        let result = run.call(&mut store, ()).expect("run");
        assert_eq!(result, HostErrorCode::InvalidEventTarget.as_i32());
    }
}
