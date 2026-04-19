//! Store-data wrapper and compile-time main-thread enforcement.
//!
//! wasmtime requires the `Store<T>` data type for wasi-threads to satisfy
//! `T: Clone + Send + 'static`. [`RuntimeState`] is none of those on its own,
//! so [`StoreData`] wraps it in an `Arc<Mutex<_>>`.
//!
//! The Paws invariant — "only the main wasm thread touches `RuntimeState`" —
//! is enforced at Rust compile time by [`MainThreadToken`]. The token is
//! `!Send` and `!Sync`, so rustc refuses to move it across thread boundaries
//! and refuses to capture it in any `Send`-bounded closure (e.g. a
//! wasi-threads worker body). State-mutating host functions take a
//! `&MainThreadToken` as a parameter and acquire the token through
//! thread-local state inside each invocation.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use engine::{EngineRenderer, RuntimeState};
use send_wrapper::SendWrapper;
use wasmtime_wasi_threads::WasiThreadsCtx;

/// Unforgeable proof that the current OS thread is the Paws main wasm thread.
///
/// The `PhantomData<*const ()>` makes this type `!Send` and `!Sync`: the
/// compiler refuses to move a token to another thread or share it through a
/// shared reference that could cross threads. wasi-threads worker closures
/// must be `Send` — they therefore cannot close over a `MainThreadToken`.
///
/// A token can only be obtained through [`Self::current`], which consults a
/// per-thread flag. The flag is set once via [`Self::install`] at Store
/// construction on the thread that will run the main wasm entry point. Worker
/// threads spawned by `wasi:thread-spawn` start with the flag cleared and can
/// never produce a token.
#[derive(Debug)]
pub struct MainThreadToken {
    _not_send: PhantomData<*const ()>,
}

thread_local! {
    static IS_MAIN_THREAD: Cell<bool> = const { Cell::new(false) };
}

impl MainThreadToken {
    /// Marks the current OS thread as the Paws main wasm thread.
    ///
    /// Called once per Store from the thread that will call the guest entry
    /// point. Idempotent when invoked repeatedly on the same thread.
    pub fn install() {
        IS_MAIN_THREAD.with(|cell| cell.set(true));
    }

    /// Returns a token if the current OS thread has been marked as the main
    /// thread via [`Self::install`], otherwise [`None`].
    ///
    /// Worker threads spawned by `wasi:thread-spawn` never have the flag set,
    /// so this returns [`None`] on them.
    pub fn current() -> Option<Self> {
        if IS_MAIN_THREAD.with(Cell::get) {
            Some(Self {
                _not_send: PhantomData,
            })
        } else {
            None
        }
    }
}

/// Per-Store data installed into `wasmtime::Store<StoreData<R>>`.
///
/// Cloned by wasmtime-wasi-threads into every worker thread's Store view, so
/// all fields are `Send + Sync`.
///
/// [`RuntimeState`] contains Stylo types with internal non-`Send` raw
/// pointers, so we wrap it in [`SendWrapper`] — a runtime-checked
/// `Send + Sync` shim that panics if accessed from a thread other than the
/// one that constructed it. Combined with the compile-time [`MainThreadToken`]
/// guard on [`Self::with_state`], the runtime check never triggers in
/// correct Paws code; it's defense-in-depth for the invariant.
pub struct StoreData<R: EngineRenderer> {
    state: Arc<Mutex<SendWrapper<RuntimeState<R>>>>,
    /// wasi-threads context, populated after Store construction by
    /// `run_wasm_inner` before the guest entry point is invoked. Cloned
    /// into every worker thread's Store view by wasi-threads' spawn
    /// trampoline. `None` on WAT tests / non-threaded modules.
    pub(crate) wasi_threads: Option<Arc<WasiThreadsCtx<StoreData<R>>>>,
}

impl<R: EngineRenderer> StoreData<R> {
    /// Wraps the given `RuntimeState` so it can live inside a wasmtime Store.
    ///
    /// Must be called on the Paws main thread; the wrapped `RuntimeState`
    /// panics if accessed from any other thread.
    pub fn new(state: RuntimeState<R>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SendWrapper::new(state))),
            wasi_threads: None,
        }
    }

    /// Consumes the `StoreData` and returns the inner `RuntimeState`.
    ///
    /// Must be called on the same thread that invoked [`Self::new`] — the
    /// inner [`SendWrapper`] panics otherwise. Also panics if other clones
    /// of the inner `Arc` exist (i.e. wasi-threads workers still hold
    /// references).
    pub fn into_state(self) -> RuntimeState<R> {
        let mutex = Arc::try_unwrap(self.state)
            .unwrap_or_else(|_| panic!("StoreData still has live clones"));
        let wrapper = mutex
            .into_inner()
            .unwrap_or_else(|_| panic!("RuntimeState mutex poisoned"));
        wrapper.take()
    }

    /// Access the inner state on the main thread. The `_token` parameter
    /// proves at compile time that the caller is on the main thread: the
    /// token is `!Send`, so this function body cannot be inlined into any
    /// `Send`-bounded closure.
    pub fn with_state<T>(
        &self,
        _token: &MainThreadToken,
        f: impl FnOnce(&mut RuntimeState<R>) -> T,
    ) -> T {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|_| panic!("RuntimeState mutex poisoned"));
        // Dereffing the SendWrapper validates at runtime that we're on the
        // main thread. The MainThreadToken already guarantees this at compile
        // time; this is a backup check.
        f(&mut **guard)
    }

    /// Returns a `Deref`/`DerefMut` guard that holds the state's mutex for
    /// its lifetime. The `_token` parameter proves at compile time that the
    /// caller is on the main thread. Prefer [`Self::with_state`] for host
    /// functions — this helper exists primarily for tests and runners that
    /// want direct field access across multiple statements.
    pub fn lock<'token>(&'token self, _token: &'token MainThreadToken) -> StateGuard<'token, R> {
        StateGuard {
            inner: self
                .state
                .lock()
                .unwrap_or_else(|_| panic!("RuntimeState mutex poisoned")),
        }
    }
}

/// Deref-targeted guard returned by [`StoreData::lock`]. Holds the mutex for
/// its lifetime; drop it to release.
pub struct StateGuard<'token, R: EngineRenderer> {
    inner: MutexGuard<'token, SendWrapper<RuntimeState<R>>>,
}

impl<R: EngineRenderer> Deref for StateGuard<'_, R> {
    type Target = RuntimeState<R>;
    fn deref(&self) -> &RuntimeState<R> {
        &self.inner
    }
}

impl<R: EngineRenderer> DerefMut for StateGuard<'_, R> {
    fn deref_mut(&mut self) -> &mut RuntimeState<R> {
        &mut self.inner
    }
}

impl<R: EngineRenderer> Clone for StoreData<R> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            wasi_threads: self.wasi_threads.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    assert_not_impl_any!(MainThreadToken: Send, Sync);

    assert_impl_all!(StoreData<()>: Send, Sync, Clone);

    #[test]
    fn token_current_none_before_install() {
        // Run on a fresh thread so we know `install()` was not called.
        let handle = std::thread::spawn(|| MainThreadToken::current().is_none());
        assert!(handle.join().unwrap());
    }

    #[test]
    fn token_current_some_after_install() {
        let handle = std::thread::spawn(|| {
            MainThreadToken::install();
            MainThreadToken::current().is_some()
        });
        assert!(handle.join().unwrap());
    }

    #[test]
    fn worker_thread_sees_no_token_after_main_install() {
        MainThreadToken::install();
        let handle = std::thread::spawn(|| MainThreadToken::current().is_some());
        // Worker thread has its own TLS — install on main does NOT propagate.
        assert!(!handle.join().unwrap());
    }
}
