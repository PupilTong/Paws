//! Cache-policy injection trait.
//!
//! The engine does not parse `Cache-Control`, `Expires`, or any other
//! HTTP caching headers itself. That parsing is delegated to a
//! [`CachePolicyProvider`] supplied by the host, so platforms can plug
//! in their native cache (NSURLCache on iOS, OkHttp's `Cache` on
//! Android, `http-cache` on a reqwest-based backend) or a future
//! pure-Rust implementation without reworking every call site in
//! [`IoLayer`](crate::io::IoLayer).
//!
//! The default `()` stub is deliberately lossy: `parse_response` stores
//! nothing, `conditional_headers` produces nothing, and `evaluate`
//! panics with a clear message. This is the "you did not provide a
//! policy, so there is no way to answer this question" contract. The
//! blob-URL and `data:`-URL resolution paths in the engine do not
//! consult the policy provider at all, so guests that only use those
//! schemes never hit the stub.

use std::time::Instant;

use super::cache::CachedEntry;

/// Host-injected cache policy evaluator.
///
/// Implementations are expected to be pure / side-effect-free: all of
/// their inputs are passed by reference, and the engine calls them on
/// the hot path of resource resolution.
pub trait CachePolicyProvider: Send + 'static {
    /// Parse response headers into an opaque [`CachePolicy`] stored on
    /// the cache entry. Implementations may return a zero-sized
    /// policy if they ignore cache-control entirely; in that case the
    /// engine will treat every entry as `Freshness::Missing` until
    /// [`Self::evaluate`] says otherwise.
    fn parse_response(&self, headers: &[(String, String)]) -> CachePolicy;

    /// Answer "is this entry still usable" at a given instant. The
    /// engine invokes this on every cache lookup; implementations
    /// must therefore be cheap.
    fn evaluate(&self, entry: &CachedEntry, now: Instant) -> Freshness;

    /// Produce conditional-revalidation headers (`If-None-Match`,
    /// `If-Modified-Since`) for a stale entry the caller wants to
    /// revalidate over the network. Returns an empty vector when
    /// revalidation is not applicable (e.g. the entry carries no
    /// validators).
    fn conditional_headers(&self, entry: &CachedEntry) -> Vec<(String, String)>;
}

/// Opaque policy stored alongside a cache entry. The struct is
/// intentionally a bag of raw directive strings for now — later PRs
/// will replace it with a structured enum once the full set of
/// directives the engine cares about is nailed down.
#[derive(Debug, Clone, Default)]
pub struct CachePolicy {
    /// Unparsed directive list (e.g. `[("max-age", "60"), ("no-store", "")]`).
    /// Providers that need structured access re-parse this on demand;
    /// providers that do not care leave it empty.
    pub raw_directives: Vec<(String, String)>,
}

/// Freshness classification returned by a [`CachePolicyProvider`].
///
/// The variants follow Chrome's internal cache taxonomy closely
/// enough that a future Chrome-compatible provider can round-trip
/// them without lossy mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// The entry is within its freshness window and can be served
    /// without revalidation.
    Fresh,

    /// The entry is past its freshness window but within a
    /// `stale-while-revalidate` tolerance, so it may be served while
    /// a background revalidation happens.
    StaleRevalidatable,

    /// The entry is past every applicable freshness window and must
    /// be revalidated (or refetched) before being served.
    Stale,

    /// The entry is not in the cache at all.
    Missing,
}

/// No-op policy provider. The default.
///
/// Every method that would require real cache semantics panics with a
/// clear message. The blob-URL and `data:`-URL fast paths in
/// [`IoLayer`](crate::io::IoLayer) never consult the provider, so
/// guests that only use those schemes never hit this stub.
impl CachePolicyProvider for () {
    fn parse_response(&self, _headers: &[(String, String)]) -> CachePolicy {
        // Storing nothing is safe; `evaluate` will panic if anyone
        // tries to look up freshness later.
        CachePolicy::default()
    }

    fn evaluate(&self, _entry: &CachedEntry, _now: Instant) -> Freshness {
        unimplemented!(
            "the default () CachePolicyProvider does not evaluate freshness; \
             inject a real provider via IoLayer::with_policy() or \
             ResourceManager::with_policy()"
        );
    }

    fn conditional_headers(&self, _entry: &CachedEntry) -> Vec<(String, String)> {
        Vec::new()
    }
}
