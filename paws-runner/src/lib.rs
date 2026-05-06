//! Headless runner for Paws WASM guests.
//!
//! [`Runner`] wraps [`engine::RuntimeState`] with a builder-style API and
//! viewport configuration, then drives [`wasmtime_engine::run_wasm`] under
//! the hood. It reuses one `wasmtime::Engine` and caches compiled artifacts
//! by exact guest bytes so repeated runs skip wasmtime compilation. Tests and
//! tools use this crate to avoid reinventing the "load wasm → run → inspect
//! DOM" flow.
//!
//! Commit is guest-owned: the runner never calls commit on the host side.
//! A Paws WASM guest that wants its layout computed must call the
//! `__commit` host function (via `rust_wasm_binding::commit()`) before
//! returning from its `run()` export. The viewport defaults to 800x600
//! unless configured via [`RunnerBuilder::viewport`]; it is stored on
//! [`engine::RuntimeState`] ahead of time and the `__commit` handler reads
//! it automatically.
//!
//! # Example
//!
//! ```no_run
//! use paws_runner::Runner;
//!
//! let wasm = std::fs::read("my_guest.wasm").unwrap();
//! let mut runner = Runner::builder().viewport(800.0, 600.0).build();
//! runner.run(&wasm, "run").expect("wasm execution failed");
//! // Inspect the computed DOM:
//! let state = runner.state();
//! assert!(state.doc.root_element_id().is_some());
//! ```

mod error;
mod runner;

pub use error::RunnerError;
pub use runner::{Runner, RunnerBuilder};
