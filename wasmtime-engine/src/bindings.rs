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

wasmtime::component::bindgen!({
    path: "../wit",
    world: "paws-guest",
    // Enable async = false — Paws host functions are synchronous.
});
