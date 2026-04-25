//! Network response cache.
//!
//! `CachedEntry` carries the bytes, headers, and bookkeeping fields
//! that a [`CachePolicyProvider`](super::policy::CachePolicyProvider)
//! needs to evaluate freshness. The store itself lives on
//! [`ResourceManager`](super::ResourceManager); this module just
//! defines the value type and an LRU-by-bytes eviction policy.

use std::sync::Arc;
use std::time::Instant;

use super::policy::CachePolicy;

/// A single cache entry. Stored behind an `Arc` on
/// [`ResourceManager`](super::ResourceManager) so callers (the
/// renderer, a style loader) can hold onto the bytes independently of
/// subsequent cache mutations.
pub struct CachedEntry {
    /// Raw response body.
    pub bytes: Arc<Vec<u8>>,

    /// Content-Type / MIME type if known. `None` means "unknown" —
    /// callers should treat it as `application/octet-stream`.
    pub mime_type: Option<String>,

    /// Raw response headers, preserved verbatim so a policy provider
    /// can re-parse them without the engine having to guess which
    /// directives it cares about.
    pub headers: Vec<(String, String)>,

    /// Opaque policy the provider chose for this entry.
    pub policy: CachePolicy,

    /// Wall-clock time the entry was inserted. Used by a
    /// [`CachePolicyProvider`](super::policy::CachePolicyProvider) to
    /// compute freshness against `Cache-Control: max-age` etc.
    ///
    /// Note: there is deliberately no `last_access` field on the
    /// entry itself. LRU ordering is tracked separately by the
    /// injected [`EvictionPolicy`] so `Arc<CachedEntry>` can stay
    /// `Send + Sync` without interior mutability.
    pub inserted_at: Instant,

    /// Cached `ETag` validator, if any. Extracted up front so the
    /// policy provider does not have to re-scan headers on every
    /// conditional request.
    pub etag: Option<String>,

    /// Cached `Last-Modified` validator, if any.
    pub last_modified: Option<String>,
}

impl CachedEntry {
    /// Size of the entry in bytes for eviction accounting. Header
    /// and URL overhead is ignored — the body dominates for image
    /// and script payloads, which are the workloads that will stress
    /// the cache.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

// ── Eviction policy ──────────────────────────────────────────────────

/// Host-injected eviction policy.
///
/// The default implementation [`LruBytesEviction`] is fine for every
/// platform today; the trait exists so a future platform can slot in
/// a native cache (NSURLCache, OkHttp) whose eviction is driven
/// externally.
pub trait EvictionPolicy: Send + 'static {
    /// Called when a new entry is inserted into the cache. `size` is
    /// the entry's body length.
    fn on_insert(&mut self, key: &str, size: usize);

    /// Called when an entry is read from the cache.
    fn on_access(&mut self, key: &str);

    /// Called when an entry is removed from the cache (either by
    /// explicit invalidation or eviction).
    fn on_remove(&mut self, key: &str);

    /// Select the next victim for eviction when the byte budget is
    /// exceeded. Returning `None` signals "no more candidates" — the
    /// caller will stop evicting even if still over budget.
    fn pick_victim(&mut self, current_bytes: usize, budget: usize) -> Option<String>;
}

/// LRU-by-bytes eviction: the least-recently-accessed entry is
/// evicted first when the byte budget is exceeded.
///
/// Implemented as a plain `VecDeque<String>` of keys in access order.
/// `O(n)` on access (we rotate the key to the back); fine at the
/// cache sizes Paws targets today (hundreds, not millions, of
/// entries). Can be swapped for a `LinkedHashMap` if that stops
/// being true.
pub struct LruBytesEviction {
    order: std::collections::VecDeque<String>,
}

impl LruBytesEviction {
    pub fn new() -> Self {
        Self {
            order: std::collections::VecDeque::new(),
        }
    }
}

impl Default for LruBytesEviction {
    fn default() -> Self {
        Self::new()
    }
}

impl EvictionPolicy for LruBytesEviction {
    fn on_insert(&mut self, key: &str, _size: usize) {
        self.order.push_back(key.to_string());
    }

    fn on_access(&mut self, key: &str) {
        // Promote the key to the back (most-recently-used) by
        // removing and re-pushing. O(n), acceptable at current scale.
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos).expect("position was just found");
            self.order.push_back(k);
        }
    }

    fn on_remove(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
    }

    fn pick_victim(&mut self, _current_bytes: usize, _budget: usize) -> Option<String> {
        self.order.pop_front()
    }
}
