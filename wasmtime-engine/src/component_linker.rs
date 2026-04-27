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

    // dom / shadow / stylesheet / resources: bindgen auto-registrations
    // are fine — none of these need the store to re-enter the guest.
    paws_host::dom::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;
    paws_host::shadow::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;
    paws_host::stylesheet::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
        &mut h.state
    })?;
    paws_host::resources::add_to_linker::<HostData<R>, RuntimeStateData<R>>(&mut linker, |h| {
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

    // Thirteen of the fourteen event functions are straight
    // delegations to an `EventsHost::<method>(&mut state, args...)`
    // call — exactly the shape `paws_host::events::add_to_linker`
    // would have generated. Two macro arms cover them all: `delegate!`
    // binds a component-ABI name to an `EventsHost` method with a
    // matching parameter tuple. Keeping the arms typed in the
    // invocation means a signature drift in the WIT schema will fail
    // at the macro call-site, not at some later runtime mismatch.
    //
    // `dispatch-event` is the odd one out — it needs the store to
    // re-enter the guest — so it's wrapped manually below.
    macro_rules! delegate {
        // Accessor / mutator with args, returning i32.
        ($name:literal, $method:ident ( $( $arg:ident : $ty:ty ),+ $(,)? ) -> i32) => {
            inst.func_wrap(
                $name,
                |mut caller: StoreContextMut<'_, HostData<R>>,
                 ( $( $arg ),+ ,): ( $( $ty ),+ ,)|
                 -> wasmtime::Result<(i32,)> {
                    let host = &mut caller.data_mut().state;
                    Ok((EventsHost::$method(host $(, $arg)+),))
                },
            )?;
        };
        // Zero-arg accessor returning a scalar (i32 or f64).
        ($name:literal, $method:ident () -> $ret:ty) => {
            inst.func_wrap(
                $name,
                |mut caller: StoreContextMut<'_, HostData<R>>, (): ()|
                 -> wasmtime::Result<($ret,)> {
                    Ok((EventsHost::$method(&mut caller.data_mut().state),))
                },
            )?;
        };
    }

    delegate!(
        "add-event-listener",
        add_event_listener(target_id: i32, event_type: String, callback_id: i32, options_flags: i32) -> i32
    );
    delegate!(
        "remove-event-listener",
        remove_event_listener(target_id: i32, event_type: String, callback_id: i32, options_flags: i32) -> i32
    );
    delegate!("stop-propagation", stop_propagation() -> i32);
    delegate!("stop-immediate-propagation", stop_immediate_propagation() -> i32);
    delegate!("prevent-default", prevent_default() -> i32);
    delegate!("target", target() -> i32);
    delegate!("current-target", current_target() -> i32);
    delegate!("phase", phase() -> i32);
    delegate!("bubbles", bubbles() -> i32);
    delegate!("cancelable", cancelable() -> i32);
    delegate!("default-prevented", default_prevented() -> i32);
    delegate!("composed", composed() -> i32);
    delegate!("timestamp", timestamp() -> f64);

    // `dispatch-event`: the three-phase algorithm needs the store so
    // it can call back into the guest via the captured typed
    // `invoke-listener` func. Handled inline, not through the macro.
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

    Ok(())
}

/// Implements the three-phase W3C event dispatch algorithm for the
/// component-model host path. Mirrors the core-module
/// `wasm::dispatch_event_wasm` but drives guest re-entry through
/// [`PawsGuest::call_invoke_listener`] (via the captured
/// [`HostData::invoke_listener`]`TypedFunc`) instead of a raw
/// `wasmtime::Func` fetched from a `Caller`.
pub(crate) fn dispatch_event_component<R: EngineRenderer>(
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
        let entry = caller
            .data()
            .state
            .doc
            .get_node(node_id)
            .and_then(|n| n.event_listeners.get(snap.index));

        // Index-stability invariant: `remove_event_listener` during
        // an active dispatch sets `removed = true` instead of
        // physically deleting the entry (the retain happens once
        // after the whole algorithm finishes). If that ever changes
        // and entries get re-packed during dispatch, `snap.index`
        // becomes stale and we could misfire the wrong handler —
        // fail loudly in debug rather than silently corrupt dispatch.
        debug_assert!(
            entry.is_none_or(|l| l.callback_id == snap.callback_id),
            "listener at snap.index does not match snapshot callback_id \
             — did remove_event_listener start physically reordering entries?",
        );

        let active = entry.is_some_and(|l| !l.removed);
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

/// Compiles a WASM component, runs its `run` export, and extracts
/// profraw coverage bytes via the component's `dump-coverage` export.
///
/// `dump-coverage` is part of the `paws-guest` world so every Paws
/// guest has it, but the default `paws_main!` body returns an empty
/// `Vec<u8>` unless the guest was built with
/// `rust-wasm-binding/coverage`. An empty vec is surfaced as
/// `Ok(None)` so callers can skip the lcov step without branching on
/// content length.
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
    with_coverage: bool,
) -> WasmCoverageResult<R> {
    let (mut store, instance) = instantiate_and_run_component(engine, state, wasm_bytes)?;

    if !with_coverage {
        return Ok((store.into_data().into_state(), None));
    }

    // Coverage data is recorded into the guest's WASM memory by minicov,
    // so we must extract it from the same instance that just ran `run`.
    // `dump-coverage` is part of the `paws-guest` world and the
    // `paws_main!` macro emits a default empty implementation when the
    // guest lacks the `coverage` Cargo feature; an empty Vec surfaces
    // here as `Ok(None)` so callers can skip the lcov step without
    // branching on length.
    let result: wasmtime::Result<Option<Vec<u8>>> = (|| {
        let dump_coverage =
            instance.get_typed_func::<(), (Vec<u8>,)>(&mut store, "dump-coverage")?;
        let (bytes,) = dump_coverage.call(&mut store, ())?;
        if bytes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(bytes))
        }
    })();

    match result {
        Ok(coverage) => Ok((store.into_data().into_state(), coverage)),
        Err(error) => Err(Box::new(RunWasmError {
            state: store.into_data().into_state(),
            error,
        })),
    }
}

/// Result of [`instantiate_and_run_component`]: a live wasmtime store
/// holding the component instance plus the typed handle to the
/// component's `Instance`. Returned as a named alias because clippy's
/// `type_complexity` rule otherwise flags the bare tuple.
type ComponentRunHandle<R> =
    Result<(Store<HostData<R>>, wasmtime::component::Instance), Box<RunWasmError<R>>>;

/// Shared component-instantiation path. Compiles `wasm_bytes`, builds a
/// fresh [`Store`] holding [`HostData`], instantiates the component
/// against the standard linker, captures the `invoke-listener` typed
/// handle on `HostData`, and calls the guest's `run` export.
///
/// Returns the live store + the wasmtime `Instance` so the caller can
/// either drop both (one-shot path) or keep the store alive for further
/// host-driven dispatches via [`ComponentSession`].
fn instantiate_and_run_component<R: EngineRenderer>(
    engine: &WasmEngine,
    state: RuntimeState<R>,
    wasm_bytes: &[u8],
) -> ComponentRunHandle<R> {
    let component = match Component::new(engine, wasm_bytes) {
        Ok(c) => c,
        Err(error) => return Err(Box::new(RunWasmError { state, error })),
    };
    let linker = match build_component_linker::<R>(engine) {
        Ok(l) => l,
        Err(error) => return Err(Box::new(RunWasmError { state, error })),
    };
    let mut store = Store::new(engine, HostData::new(state));

    let result = (|| -> wasmtime::Result<wasmtime::component::Instance> {
        let instance = linker.instantiate(&mut store, &component)?;
        let invoke = instance.get_typed_func::<(i32,), ()>(&mut store, "invoke-listener")?;
        let run = instance.get_typed_func::<(), (i32,)>(&mut store, "run")?;

        store.data_mut().invoke_listener = Some(invoke);

        let (_exit_code,) = run.call(&mut store, ())?;
        Ok(instance)
    })();

    match result {
        Ok(instance) => Ok((store, instance)),
        Err(error) => Err(Box::new(RunWasmError {
            state: store.into_data().into_state(),
            error,
        })),
    }
}

/// Outcome of a host-driven pointer-event dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Hit-test found no element under the point; no listener fired.
    NoHit,
    /// Hit-test resolved a target and the dispatch path ran. `target` is
    /// the slab id that received the event; `default_prevented` reflects
    /// whether any listener called `preventDefault` on a cancelable event.
    Dispatched {
        target: i32,
        default_prevented: bool,
    },
}

/// A live component-model guest paired with its wasmtime [`Store`].
///
/// Created by [`ComponentSession::start`] (which compiles, instantiates,
/// and calls `run` once). After `run` returns the session stays alive so
/// the host can dispatch further events into the guest — most importantly
/// pointer events from the renderer, but any
/// [`dispatch_event`](Self::dispatch_event_at) flow that needs to re-enter
/// the guest's `invoke-listener` export.
///
/// The store is moved into the session and recovered by
/// [`ComponentSession::into_state`], which mirrors how
/// [`run_component`] always returns the [`RuntimeState`] even on error.
pub struct ComponentSession<R: EngineRenderer> {
    store: Store<HostData<R>>,
}

impl<R: EngineRenderer> ComponentSession<R> {
    /// Compiles `wasm_bytes`, instantiates the component, captures the
    /// `invoke-listener` typed-func handle, and calls the guest's `run`
    /// export. Returns a session that owns the live store and can
    /// dispatch further events.
    ///
    /// Like [`run_component`], the [`RuntimeState`] is always recovered
    /// on error and returned inside [`RunWasmError`].
    pub fn start(
        engine: &WasmEngine,
        state: RuntimeState<R>,
        wasm_bytes: &[u8],
    ) -> Result<Self, Box<RunWasmError<R>>> {
        let (store, _instance) = instantiate_and_run_component(engine, state, wasm_bytes)?;
        Ok(Self { store })
    }

    /// Hit-tests the point `(x, y)` against the document. If an element
    /// is hit, dispatches `event_type` to it through the existing W3C
    /// three-phase path with `bubbles=true, cancelable=true,
    /// composed=true` (W3C `click` defaults). On miss, returns
    /// [`DispatchOutcome::NoHit`] without firing anything.
    ///
    /// Coordinates are in the same space as `final_layout.location` of
    /// top-level elements — for the iOS renderer that's CSS-pixel
    /// viewport space, top-left origin.
    pub fn dispatch_pointer_event(
        &mut self,
        x: f32,
        y: f32,
        event_type: &str,
    ) -> wasmtime::Result<DispatchOutcome> {
        let root = taffy::NodeId::from(0u64);
        let point = taffy::Point { x, y };
        let hit = engine::hit_test_at_point(&self.store.data().state.doc, root, point);
        let Some(target) = hit else {
            return Ok(DispatchOutcome::NoHit);
        };

        let mut caller = self.store.as_context_mut();
        let target_id = u64::from(target) as i32;
        let code = dispatch_event_component::<R>(
            &mut caller,
            target_id,
            Atom::from(event_type),
            true,
            true,
            true,
        )?;

        // `dispatch_event_component` returns 1 when the event was not
        // canceled, 0 when canceled, negative when an error code was
        // recorded on `RuntimeState::last_error`.
        Ok(DispatchOutcome::Dispatched {
            target: target_id,
            default_prevented: code == 0,
        })
    }

    /// Borrows the underlying [`RuntimeState`] for read-only inspection.
    /// Useful for tests and renderer code that needs to look up DOM /
    /// layout state between dispatches without consuming the session.
    pub fn state(&self) -> &RuntimeState<R> {
        &self.store.data().state
    }

    /// Mutable borrow of the underlying [`RuntimeState`].
    pub fn state_mut(&mut self) -> &mut RuntimeState<R> {
        &mut self.store.data_mut().state
    }

    /// Drops the session, recovering the [`RuntimeState`] so the caller
    /// can release any backend-owned resources (UIKit views, GL buffers,
    /// …) attached to it.
    pub fn into_state(self) -> RuntimeState<R> {
        self.store.into_data().into_state()
    }
}
