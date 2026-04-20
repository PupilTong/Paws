//! Component-model host wiring.
//!
//! Parallel to [`crate::wasm::build_linker`] (which still serves the old
//! core-module `env`-import path for WAT unit tests), this module
//! provides the component-model counterpart: a
//! [`wasmtime::component::Linker`] pre-populated with every host
//! function declared in `wit/paws.wit` plus a convenience wrapper for
//! loading, instantiating, and running a guest component.
//!
//! Guest components are produced by compiling `rust-wasm-binding`-using
//! crates to `wasm32-wasip2` and letting `wasm-component-ld` package
//! the core module as a component. The resulting binary is what
//! [`run_component`] consumes.

use crate::bindings::PawsGuest;
use engine::{EngineRenderer, RuntimeState};
use wasmtime::component::{Component, Linker as ComponentLinker};
use wasmtime::{Engine as WasmEngine, Store};

use crate::{RunWasmError, WasmCoverageResult};

/// Builds a [`wasmtime::component::Linker`] with every host import from
/// `wit/paws.wit` wired in. The host-getter is the identity function on
/// the `RuntimeState<R>` store data, since the four generated `Host`
/// traits are implemented directly on `RuntimeState` (see
/// [`crate::host_impl`]).
pub fn build_component_linker<R: EngineRenderer>(
    engine: &WasmEngine,
) -> wasmtime::Result<ComponentLinker<RuntimeState<R>>> {
    let mut linker: ComponentLinker<RuntimeState<R>> = ComponentLinker::new(engine);
    PawsGuest::add_to_linker::<RuntimeState<R>, RuntimeStateData<R>>(&mut linker, |s| s)?;
    Ok(linker)
}

/// `HasData` impl whose `Data<'a>` is `&'a mut RuntimeState<R>`.
///
/// The `component::bindgen!` macro synthesises trait bounds that require
/// a `D: HasData` parameter so the getter closure can produce a
/// short-lived borrow per host call. For Paws, the borrow is the store
/// data itself — no intermediate state.
pub struct RuntimeStateData<R>(std::marker::PhantomData<R>);

impl<R: EngineRenderer> wasmtime::component::HasData for RuntimeStateData<R> {
    type Data<'a> = &'a mut RuntimeState<R>;
}

/// Compiles a WASM component and runs its `run` export against a
/// [`RuntimeState`]. Mirrors [`crate::run_wasm_with_engine`] but for
/// components; like that function it always recovers the `RuntimeState`
/// (even on error) so callers can reuse it.
pub fn run_component<R: EngineRenderer>(
    engine: &WasmEngine,
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    _func_name: &str,
) -> Result<RuntimeState<R>, Box<RunWasmError<R>>> {
    run_component_inner(engine, state, wasm_bytes, false).map(|(state, _)| state)
}

/// Compiles a WASM component, runs its `run` export, and (if the
/// component exports them) extracts guest coverage bytes. The coverage
/// exports live outside the WIT world and are only present when guests
/// are built with the `coverage` Cargo feature — absent exports yield
/// `Ok(None)` rather than an error.
pub fn run_component_with_coverage<R: EngineRenderer>(
    engine: &WasmEngine,
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    _func_name: &str,
) -> WasmCoverageResult<R> {
    run_component_inner(engine, state, wasm_bytes, true)
}

fn run_component_inner<R: EngineRenderer>(
    engine: &WasmEngine,
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
    _with_coverage: bool,
) -> WasmCoverageResult<R> {
    let component = match Component::new(engine, wasm_bytes) {
        Ok(c) => c,
        Err(error) => return Err(Box::new(RunWasmError { state, error })),
    };
    let linker = match build_component_linker::<R>(engine) {
        Ok(l) => l,
        Err(error) => return Err(Box::new(RunWasmError { state, error })),
    };
    let mut store = Store::new(engine, state);

    let result = (|| -> wasmtime::Result<()> {
        let guest = PawsGuest::instantiate(&mut store, &component, &linker)?;
        let _exit_code = guest.call_run(&mut store)?;
        Ok(())
    })();

    // Coverage extraction from components is a follow-up: the
    // `__paws_dump_coverage` / `__paws_coverage_ptr` exports live inside
    // the wrapped core module, not at the component boundary, so they
    // need a separate probe path. Returning `None` for now keeps the
    // existing `PAWS_WASM_COVERAGE` signal intact but reports no bytes.
    let coverage = None;

    match result {
        Ok(()) => Ok((store.into_data(), coverage)),
        Err(error) => Err(Box::new(RunWasmError {
            state: store.into_data(),
            error,
        })),
    }
}
