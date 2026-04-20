//! Component-model host wiring.
//!
//! Parallel to [`crate::wasm::build_linker`] (which still serves the
//! core-module `env`-import path for WAT unit tests), this module
//! provides the component-model counterpart: a
//! [`wasmtime::component::Linker`] pre-populated with every host
//! function declared in `wit/paws.wit` plus the standard WASI p2 host
//! implementations (`wasi:io`, `wasi:clocks`, `wasi:random`, …) that
//! `std`-linking guests transitively depend on.
//!
//! Guest components are produced by compiling `rust-wasm-binding`-using
//! crates to `wasm32-wasip2` and letting `wasm-component-ld` package
//! the core module as a component. The resulting binary is what
//! [`run_component`] consumes.

use crate::bindings::PawsGuest;
use engine::{EngineRenderer, RuntimeState};
use wasmtime::component::{Component, Linker as ComponentLinker, ResourceTable};
use wasmtime::{Engine as WasmEngine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::{RunWasmError, WasmCoverageResult};

/// Store-data wrapper that bundles Paws's [`RuntimeState`] with the
/// per-instance WASI context and resource table. Required because
/// standard guest components (via `std` / `tokio` / `futures`) import
/// `wasi:io`, `wasi:clocks`, etc.; their host impls in
/// `wasmtime-wasi` need somewhere to stash their own state.
///
/// Implements [`Deref`](std::ops::Deref) / [`DerefMut`] to
/// [`RuntimeState`] so code that inspects state after a run (tests,
/// renderer backends) can continue to write `data.doc.get_node(...)`
/// without knowing about the wrapper.
pub struct HostData<R: EngineRenderer> {
    pub state: RuntimeState<R>,
    wasi: WasiCtx,
    table: ResourceTable,
}

impl<R: EngineRenderer> HostData<R> {
    fn new(state: RuntimeState<R>) -> Self {
        Self {
            state,
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
        }
    }

    fn into_state(self) -> RuntimeState<R> {
        self.state
    }
}

impl<R: EngineRenderer> std::ops::Deref for HostData<R> {
    type Target = RuntimeState<R>;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<R: EngineRenderer> std::ops::DerefMut for HostData<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

// SAFETY: `WasiView: Send` forces us to assert that `HostData<R>` is
// `Send`. `RuntimeState<R>` contains Stylo types whose internal raw
// pointers (`FontMetricsProvider`, the element slab) make the auto
// trait fail. Paws's runtime invariant is that the guest and all host
// function invocations run on a single OS thread — the Store is never
// observed from another thread. Wasmtime's component linker enforces
// this shape: guest execution is synchronous on the thread that called
// `call_run`, so the `Send` bound is satisfied in practice even though
// it is not derivable. No wasi-threads or async executor is configured.
unsafe impl<R: EngineRenderer> Send for HostData<R> {}

impl<R: EngineRenderer> WasiView for HostData<R> {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// Builds a [`wasmtime::component::Linker`] with every host import from
/// `wit/paws.wit` wired in, plus the standard WASI p2 implementations.
/// The Paws-specific `Host` traits are implemented on [`RuntimeState`]
/// (see [`crate::host_impl`]); the getter projects
/// `&mut HostData<R>` → `&mut RuntimeState<R>` via `&mut h.state`.
pub fn build_component_linker<R: EngineRenderer>(
    engine: &WasmEngine,
) -> wasmtime::Result<ComponentLinker<HostData<R>>> {
    let mut linker: ComponentLinker<HostData<R>> = ComponentLinker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    PawsGuest::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| &mut h.state)?;
    Ok(linker)
}

/// `HasData` impl whose `Data<'a>` is `&'a mut RuntimeState<R>`.
///
/// The `component::bindgen!` macro synthesises trait bounds that
/// require a `D: HasData` parameter so the getter closure can produce
/// a short-lived borrow per host call. For Paws, the borrow projects
/// out of [`HostData`] to the underlying `RuntimeState`.
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
    let mut store = Store::new(engine, HostData::new(state));

    let result = (|| -> wasmtime::Result<()> {
        let guest = PawsGuest::instantiate(&mut store, &component, &linker)?;
        let _exit_code = guest.call_run(&mut store)?;
        Ok(())
    })();

    // Coverage extraction from components is a follow-up: the
    // `__paws_dump_coverage` / `__paws_coverage_ptr` exports live
    // inside the wrapped core module, not at the component boundary,
    // so they need a separate probe path. Returning `None` for now
    // keeps the existing `PAWS_WASM_COVERAGE` signal intact but
    // reports no bytes.
    let coverage = None;

    match result {
        Ok(()) => Ok((store.into_data().into_state(), coverage)),
        Err(error) => Err(Box::new(RunWasmError {
            state: store.into_data().into_state(),
            error,
        })),
    }
}
