//! Resource-management layer.
//!
//! `ResourceManager` is the engine-owned registry for everything a
//! guest fetches under a URL: network responses (cached with a
//! host-injected policy), inline blobs registered via
//! `URL.createObjectURL`-style APIs, and — at the plumbing level —
//! any other scheme the guest might hand the renderer.
//!
//! The module is split into three files:
//!
//! - [`blob`] — `BlobRegistry`, fully implemented. Backs the
//!   `create_object_url_with_raw_data` / `revoke_object_url` host
//!   API. This is the only part of the module that is end-to-end
//!   functional today; it is all the shipping examples need.
//!
//! - [`cache`] — `CachedEntry` + `EvictionPolicy` + `LruBytesEviction`.
//!   The entry type and LRU-by-bytes eviction are fully implemented;
//!   the network-cache insert/lookup path is implemented too, but
//!   freshness evaluation is delegated to the injected
//!   `CachePolicyProvider` (see below).
//!
//! - [`policy`] — `CachePolicyProvider` trait + default `()` stub.
//!   Real HTTP cache semantics (Cache-Control, ETag revalidation,
//!   stale-while-revalidate) land in follow-up PRs; the trait
//!   surface is the hand-off point.
//!
//! See the module-level doc on each of these files for more detail.
//!
//! The parameter layout `ResourceManager<P, E>` with defaults
//! `P = ()` and `E = LruBytesEviction` means existing call sites
//! that don't care about policy or eviction compile unchanged.

use std::sync::Arc;

use fnv::FnvHashMap;

pub mod blob;
pub mod cache;
pub mod policy;

#[cfg(test)]
mod tests;

pub use blob::{BlobEntry, BlobRegistry};
pub use cache::{CachedEntry, EvictionPolicy, LruBytesEviction};
pub use policy::{CachePolicy, CachePolicyProvider, Freshness};

/// Default byte budget for the network cache when a host does not
/// override it. 64 MiB is large enough that no realistic example
/// will evict on its own; hosts that need a smaller cap (constrained
/// mobile devices) can set their own via
/// [`ResourceManager::with_byte_budget`].
pub const DEFAULT_BYTE_BUDGET: usize = 64 * 1024 * 1024;

/// Central resource manager owned by [`IoLayer`](crate::io::IoLayer).
///
/// Owns two independent registries:
///
/// - **Network cache** (`entries`): bounded by a byte budget, evicted
///   by the injected [`EvictionPolicy`]. Freshness is answered by the
///   injected [`CachePolicyProvider`].
/// - **Blob registry** (`blob`): unbounded, explicit revocation only.
///   Does not count toward the byte budget — blobs are guest-owned
///   and the guest is responsible for revoking them.
pub struct ResourceManager<P: CachePolicyProvider = (), E: EvictionPolicy = LruBytesEviction> {
    policy: P,
    eviction: E,
    entries: FnvHashMap<String, Arc<CachedEntry>>,
    blob: BlobRegistry,
    byte_budget: usize,
    current_bytes: usize,
}

impl ResourceManager<(), LruBytesEviction> {
    /// Creates a resource manager with the default policy (`()` —
    /// panics on freshness evaluation) and default eviction
    /// (`LruBytesEviction`) and the default byte budget.
    pub fn new() -> Self {
        Self::with_parts((), LruBytesEviction::new(), DEFAULT_BYTE_BUDGET)
    }
}

impl Default for ResourceManager<(), LruBytesEviction> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: CachePolicyProvider, E: EvictionPolicy> ResourceManager<P, E> {
    /// Creates a manager from explicit parts. Hosts that want to
    /// inject a real HTTP cache policy go through here.
    pub fn with_parts(policy: P, eviction: E, byte_budget: usize) -> Self {
        Self {
            policy,
            eviction,
            entries: FnvHashMap::default(),
            blob: BlobRegistry::new(),
            byte_budget,
            current_bytes: 0,
        }
    }

    /// Replaces the byte budget, evicting entries if the new budget
    /// is smaller than the current footprint.
    pub fn with_byte_budget(mut self, byte_budget: usize) -> Self {
        self.byte_budget = byte_budget;
        self.enforce_budget();
        self
    }

    // ── Blob registry (fully implemented) ───────────────────────────

    /// Registers raw bytes under a new `blob:paws/<hex>` URL and
    /// returns the URL. Mirrors `URL.createObjectURL`.
    pub fn create_object_url(&mut self, bytes: Vec<u8>, mime_type: String) -> String {
        self.blob.create(bytes, mime_type)
    }

    /// Drops the registration for a blob URL. Returns `true` on
    /// first revocation, `false` if the URL was already unknown.
    /// Mirrors `URL.revokeObjectURL`.
    pub fn revoke_object_url(&mut self, url: &str) -> bool {
        self.blob.revoke(url)
    }

    /// Looks up a blob URL. Returns `None` for unknown or revoked
    /// URLs.
    pub fn resolve_blob(&self, url: &str) -> Option<Arc<BlobEntry>> {
        self.blob.resolve(url)
    }

    // ── Network cache (scaffolded; evaluation delegated) ────────────

    /// Returns the cached entry for `url`, or `None` on a miss.
    /// Notifies the eviction policy so it can promote the entry in
    /// its LRU ordering.
    pub fn get(&mut self, url: &str) -> Option<Arc<CachedEntry>> {
        let entry = self.entries.get(url).cloned()?;
        self.eviction.on_access(url);
        Some(entry)
    }

    /// Inserts a network response into the cache. Uses the injected
    /// policy provider to parse headers into a [`CachePolicy`];
    /// extracts `ETag` / `Last-Modified` for conditional revalidation.
    /// Evicts older entries as needed to stay under the byte budget.
    pub fn insert_network(
        &mut self,
        url: String,
        bytes: Arc<Vec<u8>>,
        mime_type: Option<String>,
        headers: Vec<(String, String)>,
    ) -> Arc<CachedEntry> {
        let now = std::time::Instant::now();
        let policy = self.policy.parse_response(&headers);
        let etag = header_value(&headers, "etag");
        let last_modified = header_value(&headers, "last-modified");

        let size = bytes.len();
        let entry = Arc::new(CachedEntry {
            bytes,
            mime_type,
            headers,
            policy,
            inserted_at: now,
            etag,
            last_modified,
        });

        // Remove any prior entry at the same URL so byte accounting
        // stays accurate.
        if let Some(old) = self.entries.remove(&url) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size());
            self.eviction.on_remove(&url);
        }

        self.entries.insert(url.clone(), entry.clone());
        self.eviction.on_insert(&url, size);
        self.current_bytes += size;
        self.enforce_budget();

        entry
    }

    /// Removes the entry for `url`, if any.
    pub fn invalidate(&mut self, url: &str) {
        if let Some(old) = self.entries.remove(url) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size());
            self.eviction.on_remove(url);
        }
    }

    /// Drops every entry. Does not touch the blob registry.
    pub fn clear(&mut self) {
        for key in self.entries.keys().cloned().collect::<Vec<_>>() {
            self.eviction.on_remove(&key);
        }
        self.entries.clear();
        self.current_bytes = 0;
    }

    /// Number of cached network entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the network cache is empty. Blob registry state is
    /// not considered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current cached bytes across all network entries.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Configured byte budget.
    pub fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    // ── Policy hooks (delegated to the provider) ────────────────────

    /// Reports the freshness of a cached entry. Returns
    /// [`Freshness::Missing`] without consulting the policy for a
    /// cache miss. For a hit, delegates to the injected
    /// [`CachePolicyProvider::evaluate`] — which, under the default
    /// `()` provider, panics.
    pub fn freshness(&self, url: &str) -> Freshness {
        match self.entries.get(url) {
            Some(entry) => self.policy.evaluate(entry, std::time::Instant::now()),
            None => Freshness::Missing,
        }
    }

    /// Produces conditional-revalidation headers for the entry at
    /// `url`. Empty when the URL is unknown or the provider has no
    /// validators to offer.
    pub fn conditional_headers(&self, url: &str) -> Vec<(String, String)> {
        match self.entries.get(url) {
            Some(entry) => self.policy.conditional_headers(entry),
            None => Vec::new(),
        }
    }

    // ── Internals ───────────────────────────────────────────────────

    /// Drops entries until `current_bytes <= byte_budget`.
    fn enforce_budget(&mut self) {
        while self.current_bytes > self.byte_budget {
            match self
                .eviction
                .pick_victim(self.current_bytes, self.byte_budget)
            {
                Some(key) => {
                    if let Some(old) = self.entries.remove(&key) {
                        self.current_bytes = self.current_bytes.saturating_sub(old.size());
                    }
                    // Note: we don't re-call on_remove here — the
                    // eviction policy already removed the key from
                    // its own bookkeeping via pick_victim.
                }
                None => break,
            }
        }
    }
}

/// Case-insensitive header lookup. Returns the first matching value,
/// trimmed of surrounding whitespace.
fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim().to_string())
}

// ── ResourceResolver impl ─────────────────────────────────────────────

/// Exposes the blob registry and the in-memory network cache through
/// the object-safe [`ResourceResolver`](crate::ResourceResolver)
/// surface so renderers can resolve `blob:paws/*` and already-cached
/// `data:` URLs without knowing the policy / eviction type params.
impl<P: CachePolicyProvider, E: EvictionPolicy> crate::ResourceResolver for ResourceManager<P, E> {
    fn resolve(&self, url: &str) -> Option<Arc<Vec<u8>>> {
        if url.starts_with("blob:paws/") {
            return self.resolve_blob(url).map(|e| e.bytes.clone());
        }
        // `get()` takes &mut self for LRU bookkeeping; a renderer
        // needs a &self-shaped resolver. Fall back to the raw
        // hashmap read — an uncounted touch here is fine because
        // the same hit path on the mutable cache ran during
        // commit()'s pre-resolution pass.
        self.entries.get(url).map(|e| e.bytes.clone())
    }
}
