//! The [`Runner`] and [`RunnerBuilder`] types.

use engine::{EngineRenderer, RuntimeState};
use wasmtime::Engine as WasmEngine;
use wasmtime_engine::{
    create_engine, run_component, run_component_with_coverage, run_wasm_with_coverage_and_engine,
    run_wasm_with_engine,
};

use crate::error::RunnerError;

/// Default document URL used when the caller doesn't supply one.
const DEFAULT_URL: &str = "https://example.com";

/// Builder for [`Runner`]. Obtain one from [`Runner::builder`].
pub struct RunnerBuilder<R: EngineRenderer = ()> {
    url: String,
    /// `None` means "don't constrain layout" (Taffy's `Size::MAX_CONTENT`),
    /// matching [`RuntimeState`]'s historical default. Only populated when
    /// the caller explicitly calls [`viewport`](Self::viewport).
    viewport: Option<(f32, f32)>,
    renderer: R,
}

impl RunnerBuilder<()> {
    fn new() -> Self {
        Self {
            url: DEFAULT_URL.to_string(),
            viewport: None,
            renderer: (),
        }
    }
}

impl<R: EngineRenderer> RunnerBuilder<R> {
    /// Sets the document URL. Defaults to `"https://example.com"`.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Sets the viewport that the guest's `__commit` will use for layout.
    /// When unset, layout runs content-sized (Taffy's `MAX_CONTENT`).
    pub fn viewport(mut self, width: f32, height: f32) -> Self {
        self.viewport = Some((width, height));
        self
    }

    /// Swaps in a custom [`EngineRenderer`] backend (e.g. a future wgpu
    /// painter). Defaults to the zero-cost `()` no-op renderer.
    pub fn renderer<R2: EngineRenderer>(self, renderer: R2) -> RunnerBuilder<R2> {
        RunnerBuilder {
            url: self.url,
            viewport: self.viewport,
            renderer,
        }
    }

    /// Finalises the builder and returns a ready-to-use [`Runner`].
    pub fn build(self) -> Runner<R> {
        let state = match self.viewport {
            Some((width, height)) => {
                RuntimeState::with_definite_viewport(self.url, self.renderer, (), width, height)
            }
            None => RuntimeState::with_renderer(self.url, self.renderer, ()),
        };
        Runner {
            state: Some(state),
            engine: create_engine(),
        }
    }
}

/// Headless runner that owns a [`RuntimeState`] and drives WASM guests.
///
/// Does **not** expose a host-side `commit()` method — commits happen only
/// via the `__commit` host function called from WASM. The viewport lives
/// on [`RuntimeState`] so the host-side `__commit` handler reads it
/// automatically when the guest triggers a commit.
///
/// Owns a long-lived [`wasmtime::Engine`] that is reused across every
/// `run*` call. Creating the engine is expensive (JIT setup); reuse makes
/// back-to-back runs cheap.
///
/// The state is held in an `Option` so it can be temporarily moved out
/// during a `run_wasm` call (which takes the state by value). The `Option`
/// is always `Some` between method calls — methods that read the state
/// unwrap it.
pub struct Runner<R: EngineRenderer = ()> {
    state: Option<RuntimeState<R>>,
    engine: WasmEngine,
}

impl Runner<()> {
    /// Returns a builder with default url (`https://example.com`) and
    /// content-sized layout (Taffy's `Size::MAX_CONTENT`).
    pub fn builder() -> RunnerBuilder<()> {
        RunnerBuilder::new()
    }
}

impl<R: EngineRenderer> Runner<R> {
    /// Executes a WASM module and calls its named export (usually `"run"`).
    ///
    /// The underlying [`RuntimeState`] is recovered even on failure, so
    /// [`state`](Self::state) / [`state_mut`](Self::state_mut) remain usable.
    pub fn run(&mut self, wasm: &[u8], func: &str) -> Result<(), RunnerError> {
        let state = self.take_state();
        match run_wasm_with_engine(&self.engine, state, wasm, func) {
            Ok(state) => {
                self.state = Some(state);
                Ok(())
            }
            Err(run_err) => {
                let boxed = *run_err;
                self.state = Some(boxed.state);
                Err(RunnerError { error: boxed.error })
            }
        }
    }

    /// Executes a WASM **component** (produced by `wasm32-wasip2` builds)
    /// by calling its `run` export. Uses the component-model linker path
    /// in [`wasmtime_engine::run_component`], not the core-module linker
    /// used by [`run`](Self::run).
    ///
    /// `func` is accepted for API symmetry with [`run`](Self::run) but
    /// ignored: the component's world (`paws-guest` from `wit/paws.wit`)
    /// names the entry point `run`, so there is only one valid value.
    /// A `debug_assert_eq!` guards against callers passing something
    /// else by mistake — surfaces the surprise during development
    /// rather than silently running the wrong-looking call.
    pub fn run_component(&mut self, wasm: &[u8], func: &str) -> Result<(), RunnerError> {
        debug_assert_eq!(
            func, "run",
            "component-model guests only export `run`; got `{func}`",
        );
        let state = self.take_state();
        match run_component(&self.engine, state, wasm, func) {
            Ok(state) => {
                self.state = Some(state);
                Ok(())
            }
            Err(run_err) => {
                let boxed = *run_err;
                self.state = Some(boxed.state);
                Err(RunnerError { error: boxed.error })
            }
        }
    }

    /// Executes a WASM module like [`run`](Self::run) and additionally
    /// returns profraw bytes if the guest was built with
    /// `rust-wasm-binding`'s `coverage` feature.
    ///
    /// When the guest lacks the coverage exports, returns `Ok(None)`.
    ///
    /// NOTE: this variant uses the core-module linker and is retained
    /// only for internal WAT unit tests. Production guests are
    /// components — see [`run_component_with_coverage`](Self::run_component_with_coverage).
    pub fn run_with_coverage(
        &mut self,
        wasm: &[u8],
        func: &str,
    ) -> Result<Option<Vec<u8>>, RunnerError> {
        let state = self.take_state();
        match run_wasm_with_coverage_and_engine(&self.engine, state, wasm, func) {
            Ok((state, profraw)) => {
                self.state = Some(state);
                Ok(profraw)
            }
            Err(run_err) => {
                let boxed = *run_err;
                self.state = Some(boxed.state);
                Err(RunnerError { error: boxed.error })
            }
        }
    }

    /// Executes a WASM **component** like [`run_component`](Self::run_component)
    /// and additionally returns profraw bytes extracted from the
    /// component's `dump-coverage` export. Returns `Ok(None)` when the
    /// guest was built without the `coverage` feature (the export
    /// exists but yields zero bytes).
    ///
    /// `func` is accepted for API symmetry but ignored: the component
    /// always uses `run`. A `debug_assert_eq!` guards against stale
    /// callers passing something else.
    pub fn run_component_with_coverage(
        &mut self,
        wasm: &[u8],
        func: &str,
    ) -> Result<Option<Vec<u8>>, RunnerError> {
        debug_assert_eq!(
            func, "run",
            "component-model guests only export `run`; got `{func}`",
        );
        let state = self.take_state();
        match run_component_with_coverage(&self.engine, state, wasm, func) {
            Ok((state, profraw)) => {
                self.state = Some(state);
                Ok(profraw)
            }
            Err(run_err) => {
                let boxed = *run_err;
                self.state = Some(boxed.state);
                Err(RunnerError { error: boxed.error })
            }
        }
    }

    /// Moves the `RuntimeState` out of the runner for a by-value wasmtime
    /// call. Callers are expected to restore it before the method returns;
    /// this is a helper to keep the `Option`-unwrapping in one place.
    fn take_state(&mut self) -> RuntimeState<R> {
        self.state
            .take()
            .expect("state is Some between Runner method calls")
    }

    /// Updates the viewport size. The change takes effect on the next
    /// guest-initiated commit; it does not retrigger layout on its own.
    pub fn resize(&mut self, width: f32, height: f32) {
        debug_assert!(
            width.is_finite() && width >= 0.0,
            "viewport width must be finite and non-negative, got {width}"
        );
        debug_assert!(
            height.is_finite() && height >= 0.0,
            "viewport height must be finite and non-negative, got {height}"
        );
        self.state_mut().set_viewport(taffy::Size {
            width: taffy::AvailableSpace::Definite(width),
            height: taffy::AvailableSpace::Definite(height),
        });
    }

    /// Returns the current viewport as stored on [`RuntimeState`].
    ///
    /// `Size::MAX_CONTENT` means "no constraint" — the layout will be
    /// content-sized. A `Size { width: Definite(w), height: Definite(h) }`
    /// value was set via the builder or [`resize`](Self::resize).
    pub fn viewport(&self) -> taffy::Size<taffy::AvailableSpace> {
        self.state().viewport
    }

    /// Borrows the underlying [`RuntimeState`] for DOM / layout inspection.
    pub fn state(&self) -> &RuntimeState<R> {
        self.state
            .as_ref()
            .expect("state is Some between Runner method calls")
    }

    /// Mutable escape hatch for unusual cases (e.g. injecting stylesheets
    /// between runs). Prefer letting the guest drive state changes.
    pub fn state_mut(&mut self) -> &mut RuntimeState<R> {
        self.state
            .as_mut()
            .expect("state is Some between Runner method calls")
    }

    /// Consumes the runner and returns its [`RuntimeState`].
    pub fn into_state(self) -> RuntimeState<R> {
        self.state
            .expect("state is Some between Runner method calls")
    }
}

impl Default for Runner<()> {
    fn default() -> Self {
        Runner::builder().build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taffy::prelude::TaffyMaxContent;

    /// A minimal WAT module that matches the wasmtime-engine linker's
    /// expected imports but does nothing — exercises the happy path.
    const NOOP_WAT: &str = r#"
        (module
            (import "env" "__commit" (func $commit (result i32)))
            (memory (export "memory") 1)
            (func (export "run") (result i32)
                (drop (call $commit))
                (i32.const 0)
            )
        )
    "#;

    /// A WAT module that creates a root <div> sized to 100% width and
    /// 100% height, then calls `__commit`. Used to verify that the viewport
    /// configured on the runner is plumbed through to Taffy's root layout.
    const VIEWPORT_FIT_WAT: &str = r#"
        (module
            (import "env" "__create_element" (func $create (param i32) (result i32)))
            (import "env" "__set_inline_style" (func $set_style (param i32 i32 i32) (result i32)))
            (import "env" "__append_element" (func $append (param i32 i32) (result i32)))
            (import "env" "__commit" (func $commit (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0)  "div\00")
            (data (i32.const 16) "width\00")
            (data (i32.const 32) "100%\00")
            (data (i32.const 48) "height\00")
            (func (export "run") (result i32)
                (local $id i32)
                (local.set $id (call $create (i32.const 0)))
                (drop (call $append (i32.const 0) (local.get $id)))
                (drop (call $set_style (local.get $id) (i32.const 16) (i32.const 32)))
                (drop (call $set_style (local.get $id) (i32.const 48) (i32.const 32)))
                (drop (call $commit))
                (i32.const 0)
            )
        )
    "#;

    #[test]
    fn builder_defaults_to_max_content() {
        let runner = Runner::builder().build();
        assert_eq!(runner.viewport(), taffy::Size::MAX_CONTENT);
    }

    #[test]
    fn builder_applies_url_and_viewport() {
        let runner = Runner::builder()
            .url("https://paws.test")
            .viewport(1024.0, 768.0)
            .build();
        assert_eq!(
            runner.viewport(),
            taffy::Size {
                width: taffy::AvailableSpace::Definite(1024.0),
                height: taffy::AvailableSpace::Definite(768.0),
            }
        );
    }

    #[test]
    fn resize_updates_state_viewport() {
        let mut runner = Runner::builder().viewport(100.0, 100.0).build();
        runner.resize(500.0, 300.0);
        assert_eq!(
            runner.viewport(),
            taffy::Size {
                width: taffy::AvailableSpace::Definite(500.0),
                height: taffy::AvailableSpace::Definite(300.0),
            }
        );
    }

    #[test]
    fn run_noop_wat() {
        let mut runner = Runner::builder().build();
        let wat_bytes = wat::parse_str(NOOP_WAT).expect("valid wat");
        runner.run(&wat_bytes, "run").expect("wasm should run");
    }

    #[test]
    fn viewport_bounds_root_layout() {
        // A 100% / 100% root div should match the configured viewport
        // dimensions after the guest calls commit.
        let mut runner = Runner::builder().viewport(500.0, 400.0).build();
        let wat_bytes = wat::parse_str(VIEWPORT_FIT_WAT).expect("valid wat");
        runner.run(&wat_bytes, "run").expect("wasm should run");

        let node = runner
            .state()
            .doc
            .get_node(engine::NodeId::from(1_u64))
            .expect("root div");
        assert_eq!(node.layout().size.width, 500.0);
        assert_eq!(node.layout().size.height, 400.0);
    }

    #[test]
    fn viewport_resize_takes_effect_on_next_commit() {
        // A WAT that just re-commits — no DOM mutations. Running it after
        // a resize verifies the existing tree is re-laid-out against the
        // updated viewport.
        const RECOMMIT_ONLY_WAT: &str = r#"
            (module
                (import "env" "__commit" (func $commit (result i32)))
                (memory (export "memory") 1)
                (func (export "run") (result i32)
                    (drop (call $commit))
                    (i32.const 0)
                )
            )
        "#;

        let mut runner = Runner::builder().viewport(500.0, 400.0).build();
        let initial_wat = wat::parse_str(VIEWPORT_FIT_WAT).expect("valid wat");
        let recommit_wat = wat::parse_str(RECOMMIT_ONLY_WAT).expect("valid wat");

        runner.run(&initial_wat, "run").expect("first run");
        assert_eq!(
            runner
                .state()
                .doc
                .get_node(engine::NodeId::from(1_u64))
                .unwrap()
                .layout()
                .size,
            taffy::Size {
                width: 500.0,
                height: 400.0
            }
        );

        // Resize and re-commit — the existing div's layout updates.
        runner.resize(800.0, 600.0);
        runner.run(&recommit_wat, "run").expect("second run");
        assert_eq!(
            runner
                .state()
                .doc
                .get_node(engine::NodeId::from(1_u64))
                .unwrap()
                .layout()
                .size,
            taffy::Size {
                width: 800.0,
                height: 600.0
            }
        );
    }

    #[test]
    fn engine_is_reused_across_runs() {
        // Two sequential runs on the same Runner shouldn't allocate a new
        // wasmtime::Engine — this is hard to assert directly, but running
        // many wasms in succession works and completes without leaks.
        let mut runner = Runner::builder().build();
        let wat_bytes = wat::parse_str(NOOP_WAT).expect("valid wat");
        for _ in 0..5 {
            runner.run(&wat_bytes, "run").expect("each run succeeds");
        }
    }
}
