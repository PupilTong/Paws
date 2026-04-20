//! Host-side bindings generated from `wit/paws.wit`.
//!
//! `wasmtime::component::bindgen!` emits:
//!   - One trait per imported interface (`dom::Host`, `events::Host`,
//!     `shadow::Host`, `stylesheet::Host`) that the host implements.
//!   - A `PawsGuest` type that wraps a guest component instance and
//!     exposes its exports (`run`, `invoke-listener`) as typed methods.
//!   - `PawsGuest::add_to_linker(...)` which registers all host
//!     imports on a `wasmtime::component::Linker<T>`.
//!
//! Impls of the four `Host` traits live in [`host_impl`]; they
//! delegate to methods on [`engine::RuntimeState`] (the same calls
//! the old `env::__*` host functions made) and preserve the
//! non-negative-id / negative-error-code `s32` return convention.

// Paws host functions are synchronous. wasmtime 43's `bindgen!` does
// not accept an `async` option (see error list in the macro source) —
// the sync default is what we want, so no override is configured.
wasmtime::component::bindgen!({
    path: "../wit",
    world: "paws-guest",
});
