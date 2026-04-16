//! Error type returned by [`Runner::run`](crate::Runner::run) and
//! [`Runner::run_with_coverage`](crate::Runner::run_with_coverage).

/// Wraps a [`wasmtime::Error`] from WASM execution.
///
/// The `RuntimeState` is always recovered inside the `Runner` before this
/// error is returned, so callers can continue to call `runner.state()` /
/// `runner.state_mut()` after a failed `run()`.
#[derive(Debug)]
pub struct RunnerError {
    pub error: wasmtime::Error,
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wasm execution failed: {}", self.error)
    }
}

impl std::error::Error for RunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}
