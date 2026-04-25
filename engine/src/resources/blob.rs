//! Blob-URL registry.
//!
//! Mirrors the browser `URL.createObjectURL` / `URL.revokeObjectURL`
//! semantics: a guest hands over raw bytes + MIME type, gets back a
//! `blob:paws/<hex>` URL, and can later revoke that URL to release
//! the bytes. The registry never evicts entries on its own — blobs
//! live until explicit revocation or engine shutdown.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fnv::FnvHashMap;

/// A single blob entry backing a `blob:paws/*` URL.
pub struct BlobEntry {
    /// Raw bytes the guest handed over. Stored behind an `Arc` so
    /// the renderer can clone a cheap handle into its own per-node
    /// cache without copying the data.
    pub bytes: Arc<Vec<u8>>,

    /// MIME type supplied by the guest. Unlike network-cache entries
    /// this is required — the whole point of creating a blob URL is
    /// that the guest tells us what it is.
    pub mime_type: String,
}

/// Lookup table of live blob URLs.
pub struct BlobRegistry {
    entries: FnvHashMap<String, Arc<BlobEntry>>,
    /// 64-bit xorshift state used to mint new URLs. Seeded from
    /// `SystemTime` on construction; avoids pulling in `rand` or
    /// `uuid`. At 64 bits collisions are statistically impossible
    /// for any realistic blob count.
    rng_state: u64,
}

impl BlobRegistry {
    /// Creates an empty registry seeded from wall-clock time.
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x1234_5678_9abc_def0);
        // Avoid a zero seed: xorshift stays at zero forever once it
        // reaches zero.
        let seed = if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        };
        Self {
            entries: FnvHashMap::default(),
            rng_state: seed,
        }
    }

    /// Registers `bytes` under a freshly-minted `blob:paws/<hex>` URL
    /// and returns the URL.
    pub fn create(&mut self, bytes: Vec<u8>, mime_type: String) -> String {
        loop {
            let id = self.next_id();
            let url = format!("blob:paws/{id:016x}");
            // Paranoia: if the xorshift ever collides (it won't at 64
            // bits in practice) skip and re-mint rather than
            // overwriting a live entry.
            if !self.entries.contains_key(&url) {
                let entry = Arc::new(BlobEntry {
                    bytes: Arc::new(bytes),
                    mime_type,
                });
                self.entries.insert(url.clone(), entry);
                return url;
            }
        }
    }

    /// Drops the entry for `url`. Returns `true` if a live entry was
    /// removed, `false` if the URL was unknown or already revoked.
    pub fn revoke(&mut self, url: &str) -> bool {
        self.entries.remove(url).is_some()
    }

    /// Looks up a live blob entry. Returns `None` for unknown or
    /// revoked URLs.
    pub fn resolve(&self, url: &str) -> Option<Arc<BlobEntry>> {
        self.entries.get(url).cloned()
    }

    /// Number of live blob URLs. Exposed for tests and diagnostics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Advances the xorshift PRNG and returns the next 64-bit id.
    fn next_id(&mut self) -> u64 {
        // Marsaglia xorshift64* — adequate for id minting.
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

impl Default for BlobRegistry {
    fn default() -> Self {
        Self::new()
    }
}
