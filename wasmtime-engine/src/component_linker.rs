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

use crate::bindings::paws::host as paws_host;
use engine::{EngineRenderer, HostErrorCode, RuntimeState};
use stylo_atoms::Atom;
use wasmtime::component::{Component, Linker as ComponentLinker, ResourceTable, TypedFunc};
use wasmtime::{AsContextMut, Engine as WasmEngine, Store, StoreContextMut};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::{RunWasmError, WasmCoverageResult};

/// Store-data wrapper that bundles Paws's [`RuntimeState`] with the
/// per-instance WASI context, resource table, and (once the guest is
/// instantiated) a handle to the guest's `invoke-listener` export.
///
/// Implements [`Deref`](std::ops::Deref) / [`DerefMut`] to
/// [`RuntimeState`] so code that inspects state after a run (tests,
/// renderer backends) can continue to write `data.doc.get_node(...)`
/// without knowing about the wrapper.
pub struct HostData<R: EngineRenderer> {
    pub state: RuntimeState<R>,
    wasi: WasiCtx,
    table: ResourceTable,
    /// Typed handle to the guest's `invoke-listener` export, captured
    /// right after instantiation in [`run_component_inner`]. The
    /// custom `dispatch-event` linker registration below pulls this
    /// out to re-enter the guest during the W3C three-phase dispatch
    /// algorithm.
    invoke_listener: Option<TypedFunc<(i32,), ()>>,
}

impl<R: EngineRenderer> HostData<R> {
    fn new(state: RuntimeState<R>) -> Self {
        Self {
            state,
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            invoke_listener: None,
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

/// Builds a [`wasmtime::component::Linker`] with every host import
/// wired in.
///
/// All four Paws interfaces (`dom`, `events`, `shadow`, `stylesheet`)
/// have their bindgen-generated `add_to_linker` implementations
/// invoked, EXCEPT we override `paws:host/events/dispatch-event`: the
/// default registration routes through the `events::Host::dispatch_event`
/// trait method on [`RuntimeState`], but that method has only `&mut
/// self` access and cannot re-enter the guest to invoke listener
/// callbacks. Our replacement closure takes a
/// [`StoreContextMut`](wasmtime::StoreContextMut) and runs the W3C
/// three-phase algorithm, calling the guest's `invoke-listener`
/// export (captured in [`HostData::invoke_listener`]) for every
/// matched listener.
pub fn build_component_linker<R: EngineRenderer>(
    engine: &WasmEngine,
) -> wasmtime::Result<ComponentLinker<HostData<R>>> {
    let mut linker: ComponentLinker<HostData<R>> = ComponentLinker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    // dom / shadow / stylesheet: bindgen auto-registrations are fine.
    paws_host::dom::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;
    paws_host::shadow::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;
    paws_host::stylesheet::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;

    // events: wasmtime's component `Linker` does not allow two
    // registrations of the same instance name (it errors with "map
    // entry `paws:host/events@0.1.0` defined twice"), so we cannot
    // call bindgen's `add_to_linker` AND then re-open the instance
    // to override `dispatch-event`. Instead, register all 14 event
    // functions manually here: the 13 simple ones delegate to the
    // [`events::Host`] trait impl on [`RuntimeState`] (same pattern
    // bindgen generates), and `dispatch-event` is our custom three-
    // phase dispatcher that re-enters the guest via the captured
    // `invoke-listener` typed func.
    register_events_interface::<R>(&mut linker)?;

    Ok(linker)
}

/// Hand-rolled equivalent of `paws_host::events::add_to_linker` that
/// swaps out `dispatch-event` for a store-aware implementation. Every
/// other function delegates to the `events::Host` trait on
/// [`RuntimeState`] via the same host-getter projection bindgen would
/// have used (`|h| &mut h.state`).
fn register_events_interface<R: EngineRenderer>(
    linker: &mut ComponentLinker<HostData<R>>,
) -> wasmtime::Result<()> {
    use paws_host::events::Host as EventsHost;

    let mut inst = linker.instance("paws:host/events@0.1.0")?;

    inst.func_wrap(
        "add-event-listener",
        |mut caller: StoreContextMut<'_, HostData<R>>,
         (target_id, event_type, callback_id, options_flags): (i32, String, i32, i32)|
         -> wasmtime::Result<(i32,)> {
            let host = &mut caller.data_mut().state;
            Ok((EventsHost::add_event_listener(
                host,
                target_id,
                event_type,
                callback_id,
                options_flags,
            ),))
        },
    )?;
    inst.func_wrap(
        "remove-event-listener",
        |mut caller: StoreContextMut<'_, HostData<R>>,
         (target_id, event_type, callback_id, options_flags): (i32, String, i32, i32)|
         -> wasmtime::Result<(i32,)> {
            let host = &mut caller.data_mut().state;
            Ok((EventsHost::remove_event_listener(
                host,
                target_id,
                event_type,
                callback_id,
                options_flags,
            ),))
        },
    )?;
    inst.func_wrap(
        "dispatch-event",
        |mut caller: StoreContextMut<'_, HostData<R>>,
         (target_id, event_type, bubbles, cancelable, composed): (
            i32,
            String,
            bool,
            bool,
            bool,
        )|
         -> wasmtime::Result<(i32,)> {
            let code = dispatch_event_component::<R>(
                &mut caller,
                target_id,
                Atom::from(event_type.as_str()),
                bubbles,
                cancelable,
                composed,
            )?;
            Ok((code,))
        },
    )?;
    inst.func_wrap(
        "stop-propagation",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::stop_propagation(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "stop-immediate-propagation",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::stop_immediate_propagation(
                &mut caller.data_mut().state,
            ),))
        },
    )?;
    inst.func_wrap(
        "prevent-default",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::prevent_default(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "target",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::target(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "current-target",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::current_target(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "phase",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::phase(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "bubbles",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::bubbles(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "cancelable",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::cancelable(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "default-prevented",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::default_prevented(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "composed",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(i32,)> {
            Ok((EventsHost::composed(&mut caller.data_mut().state),))
        },
    )?;
    inst.func_wrap(
        "timestamp",
        |mut caller: StoreContextMut<'_, HostData<R>>, (): ()| -> wasmtime::Result<(f64,)> {
            Ok((EventsHost::timestamp(&mut caller.data_mut().state),))
        },
    )?;

    Ok(())
}

/// Implements the three-phase W3C event dispatch algorithm for the
/// component-model host path. Mirrors the core-module
/// `wasm::dispatch_event_wasm` but drives guest re-entry through
/// [`PawsGuest::call_invoke_listener`] (via the captured
/// [`HostData::invoke_listener`]`TypedFunc`) instead of a raw
/// `wasmtime::Func` fetched from a `Caller`.
fn dispatch_event_component<R: EngineRenderer>(
    caller: &mut StoreContextMut<'_, HostData<R>>,
    target_id: i32,
    event_type: Atom,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
) -> wasmtime::Result<i32> {
    use engine::events::dispatch::build_event_path;
    use engine::events::event::EventPhase;
    use engine::events::Event;

    if target_id < 0 {
        let code = caller
            .data_mut()
            .state
            .set_error(HostErrorCode::InvalidEventTarget, "negative target id");
        return Ok(code);
    }

    if caller
        .data()
        .state
        .current_event
        .as_ref()
        .is_some_and(|e| e.dispatch_flag)
    {
        let code = caller.data_mut().state.set_error(
            HostErrorCode::EventAlreadyDispatching,
            HostErrorCode::EventAlreadyDispatching.message(),
        );
        return Ok(code);
    }

    let target_nid = taffy::NodeId::from(target_id as u64);

    let path = match build_event_path(&caller.data().state.doc, target_nid) {
        Some(p) => p,
        None => {
            let code = caller.data_mut().state.set_error(
                HostErrorCode::InvalidEventTarget,
                "target not found in tree",
            );
            return Ok(code);
        }
    };
    let target_index = path.len() - 1;

    caller.data_mut().state.clear_error();

    // Initialise the event on `RuntimeState` so listeners' host-side
    // accessor calls (event_target / event_phase / etc.) observe a
    // coherent snapshot.
    let mut event = Event::new(event_type.clone(), bubbles, cancelable, composed);
    event.target = Some(target_nid);
    event.dispatch_flag = true;
    caller.data_mut().state.current_event = Some(event);

    let invoke = caller
        .data()
        .invoke_listener
        .ok_or_else(|| wasmtime::format_err!("guest invoke-listener export not captured"))?;

    // Capture phase: every ancestor except the target itself.
    for &node_id in &path[..target_index] {
        if caller
            .data()
            .state
            .current_event
            .as_ref()
            .unwrap()
            .stop_propagation_flag
        {
            break;
        }
        {
            let ev = caller.data_mut().state.current_event.as_mut().unwrap();
            ev.event_phase = EventPhase::Capturing;
            ev.current_target = Some(node_id);
        }
        fire_listeners_on_node::<R>(caller, &invoke, node_id, &event_type, EventPhase::Capturing)?;
    }

    // At-target phase.
    if !caller
        .data()
        .state
        .current_event
        .as_ref()
        .unwrap()
        .stop_propagation_flag
    {
        {
            let ev = caller.data_mut().state.current_event.as_mut().unwrap();
            ev.event_phase = EventPhase::AtTarget;
            ev.current_target = Some(target_nid);
        }
        fire_listeners_on_node::<R>(
            caller,
            &invoke,
            target_nid,
            &event_type,
            EventPhase::AtTarget,
        )?;
    }

    // Bubble phase (if the event bubbles).
    if bubbles {
        for i in (0..target_index).rev() {
            if caller
                .data()
                .state
                .current_event
                .as_ref()
                .unwrap()
                .stop_propagation_flag
            {
                break;
            }
            {
                let ev = caller.data_mut().state.current_event.as_mut().unwrap();
                ev.event_phase = EventPhase::Bubbling;
                ev.current_target = Some(path[i]);
            }
            fire_listeners_on_node::<R>(
                caller,
                &invoke,
                path[i],
                &event_type,
                EventPhase::Bubbling,
            )?;
        }
    }

    // Finalise and tear down the event.
    let canceled = {
        let ev = caller.data_mut().state.current_event.as_mut().unwrap();
        ev.dispatch_flag = false;
        ev.event_phase = EventPhase::None;
        ev.current_target = None;
        ev.default_prevented()
    };
    caller.data_mut().state.current_event = None;

    // Drop `once` listeners that were marked during dispatch.
    for &node_id in &path {
        if let Some(node) = caller.data_mut().state.doc.get_node_mut(node_id) {
            node.event_listeners.retain(|l| !l.removed);
        }
    }

    Ok(if canceled { 0 } else { 1 })
}

/// Fires every listener registered on `node_id` that matches
/// `event_type` + `phase`, invoking each via the typed
/// `invoke-listener` export. Mirrors
/// `wasm::dispatch_listeners_on_node` but uses component-model APIs.
fn fire_listeners_on_node<R: EngineRenderer>(
    caller: &mut StoreContextMut<'_, HostData<R>>,
    invoke: &TypedFunc<(i32,), ()>,
    node_id: taffy::NodeId,
    event_type: &Atom,
    phase: engine::events::event::EventPhase,
) -> wasmtime::Result<()> {
    use engine::events::dispatch::collect_matching_listeners;

    let listeners =
        collect_matching_listeners(&caller.data().state.doc, node_id, event_type, phase);

    for snap in &listeners {
        // The listener may have been removed during an earlier
        // iteration of the same dispatch (via `stop_immediate_propagation`
        // or an explicit `remove_event_listener` call from a handler).
        let active = caller
            .data()
            .state
            .doc
            .get_node(node_id)
            .and_then(|n| n.event_listeners.get(snap.index))
            .is_some_and(|l| !l.removed);
        if !active {
            continue;
        }

        if snap.once {
            if let Some(node) = caller.data_mut().state.doc.get_node_mut(node_id) {
                if let Some(entry) = node.event_listeners.get_mut(snap.index) {
                    entry.removed = true;
                }
            }
        }

        {
            let ev = caller.data_mut().state.current_event.as_mut().unwrap();
            ev.in_passive_listener = snap.passive;
        }

        invoke.call(caller.as_context_mut(), (snap.callback_id as i32,))?;

        {
            let ev = caller.data_mut().state.current_event.as_mut().unwrap();
            ev.in_passive_listener = false;
        }

        if caller
            .data()
            .state
            .current_event
            .as_ref()
            .unwrap()
            .stop_immediate_propagation_flag
        {
            break;
        }
    }

    Ok(())
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
        // Instantiate the component directly against the linker so we
        // can pull typed handles to BOTH `invoke-listener` (needed by
        // the custom `dispatch-event` registration for guest re-entry
        // during three-phase dispatch) and `run` (the entry point).
        // `PawsGuest::instantiate` would give us the same `run`
        // accessor but not expose the underlying `invoke-listener`
        // `Func`, so we skip the convenience wrapper.
        let instance = linker.instantiate(&mut store, &component)?;
        let invoke = instance.get_typed_func::<(i32,), ()>(&mut store, "invoke-listener")?;
        let run = instance.get_typed_func::<(), (i32,)>(&mut store, "run")?;

        // Stash invoke-listener BEFORE calling run so any event
        // dispatched during the initial `run()` reaches its listeners.
        store.data_mut().invoke_listener = Some(invoke);

        let (_exit_code,) = run.call(&mut store, ())?;
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
