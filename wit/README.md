# `wit/` — Paws host interface (component model)

`paws.wit` describes the host functions that guest WASM modules call into
— the same surface currently exposed as `#[link(wasm_import_module =
"env")]` / `#[link(wasm_import_module = "paws")]` externs in
`rust-wasm-binding/src/lib.rs`.

## Status

Schema-only. No Cargo target compiles against this file yet. The code
migration that consumes it lives in a follow-up PR (see PR2 below).

Validate with:

```
wasm-tools component wit wit/paws.wit
```

## How this will be used

- **Host** (`wasmtime-engine`): `wasmtime::component::bindgen!({ path:
  "../wit" })` generates a trait to implement on `RuntimeState<R>`, and
  an `add_to_linker` to wire into `wasmtime::component::Linker`.

- **Guest** (`rust-wasm-binding`): `wit_bindgen::generate!({ path:
  "../wit", world: "paws-guest" })` emits the raw import declarations
  that replace today's hand-written `extern "C"` block. Public wrapper
  functions keep the same signatures so examples and the Yew fork do
  not need call-site rewrites.

## Rollout plan

- **PR0 (merged, d75dfb8)** — removed wasi-threads scaffolding; unified
  standalone and Yew builds on `wasm32-wasip1`.
- **PR1 (this PR)** — ship the WIT schema.
- **PR2** — consume the schema: flip host imports to the component
  model, flip targets to `wasm32-wasip2` (or `wasm32-wasip3` once the
  toolchain ships libc artifacts for it; as of nightly-2026-04-18 it
  does not), update `build.rs` to produce components, migrate the Yew
  fork's binding layer.

## Convention

Non-negative `s32` return values carry an id / size / boolean.
Negative values encode a `HostErrorCode` variant
(`engine/src/runtime.rs`). This preserves the pre-migration i32 ABI so
`rust-wasm-binding`'s public API stays unchanged during PR2.
