//! Compiles every wasm guest under `examples/` at build time (via
//! [`build.rs`](../build.rs)) and exposes the resulting `.wasm` paths
//! through [`example_wasm_path`].
//!
//! The generated file lives in this crate's `OUT_DIR` and defines a
//! single match-based lookup function keyed on the example's rust name
//! (hyphens replaced with underscores, e.g. `example_yew_counter`).

include!(concat!(env!("OUT_DIR"), "/wasm_examples.rs"));
