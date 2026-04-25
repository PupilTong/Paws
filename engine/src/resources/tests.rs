//! Unit tests for the resource manager.

use std::sync::Arc;

use super::cache::LruBytesEviction;
use super::policy::{CachePolicy, CachePolicyProvider, Freshness};
use super::{CachedEntry, ResourceManager};

// ── BlobRegistry via ResourceManager ──────────────────────────────────

#[test]
fn create_object_url_returns_blob_paws_prefix() {
    let mut rm = ResourceManager::new();
    let url = rm.create_object_url(b"abc".to_vec(), "image/png".into());
    assert!(
        url.starts_with("blob:paws/"),
        "expected blob:paws/ prefix, got {url}"
    );
    assert_eq!(url.len(), "blob:paws/".len() + 16);
}

#[test]
fn create_object_url_returns_unique_urls() {
    let mut rm = ResourceManager::new();
    let a = rm.create_object_url(b"a".to_vec(), "image/png".into());
    let b = rm.create_object_url(b"b".to_vec(), "image/png".into());
    assert_ne!(a, b);
}

#[test]
fn resolve_blob_returns_bytes_and_mime() {
    let mut rm = ResourceManager::new();
    let url = rm.create_object_url(b"hello".to_vec(), "text/plain".into());
    let entry = rm.resolve_blob(&url).expect("blob should resolve");
    assert_eq!(entry.bytes.as_slice(), b"hello");
    assert_eq!(entry.mime_type, "text/plain");
}

#[test]
fn revoke_object_url_returns_true_once_then_false() {
    let mut rm = ResourceManager::new();
    let url = rm.create_object_url(b"x".to_vec(), "image/png".into());
    assert!(rm.revoke_object_url(&url));
    assert!(!rm.revoke_object_url(&url));
    assert!(rm.resolve_blob(&url).is_none());
}

#[test]
fn revoke_object_url_returns_false_for_unknown_url() {
    let mut rm = ResourceManager::new();
    assert!(!rm.revoke_object_url("blob:paws/0000000000000000"));
}

// ── Network cache ─────────────────────────────────────────────────────

/// Test-only policy that records how often `evaluate` is called and
/// always returns `Fresh`. Replaces the default `()` stub for tests
/// that need to exercise the get/insert paths without tripping the
/// `unimplemented!` in the default evaluator.
#[derive(Default)]
struct AlwaysFreshPolicy;

impl CachePolicyProvider for AlwaysFreshPolicy {
    fn parse_response(&self, _headers: &[(String, String)]) -> CachePolicy {
        CachePolicy::default()
    }
    fn evaluate(&self, _entry: &CachedEntry, _now: std::time::Instant) -> Freshness {
        Freshness::Fresh
    }
    fn conditional_headers(&self, _entry: &CachedEntry) -> Vec<(String, String)> {
        Vec::new()
    }
}

#[test]
fn insert_network_stores_bytes_and_headers_verbatim() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 1024);
    let headers = vec![
        ("Content-Type".to_string(), "image/png".to_string()),
        ("ETag".to_string(), "\"abc\"".to_string()),
        (
            "Last-Modified".to_string(),
            "Wed, 21 Oct 2026 07:28:00 GMT".to_string(),
        ),
    ];
    let entry = rm.insert_network(
        "https://example.com/a.png".into(),
        Arc::new(vec![1, 2, 3]),
        Some("image/png".into()),
        headers.clone(),
    );
    assert_eq!(entry.bytes.as_slice(), &[1, 2, 3]);
    assert_eq!(entry.mime_type.as_deref(), Some("image/png"));
    assert_eq!(entry.headers, headers);
    assert_eq!(entry.etag.as_deref(), Some("\"abc\""));
    assert_eq!(
        entry.last_modified.as_deref(),
        Some("Wed, 21 Oct 2026 07:28:00 GMT")
    );
    assert_eq!(rm.current_bytes(), 3);
}

#[test]
fn get_returns_inserted_entry() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 1024);
    rm.insert_network(
        "https://example.com/a.png".into(),
        Arc::new(vec![9, 9, 9]),
        None,
        vec![],
    );
    let hit = rm.get("https://example.com/a.png").expect("cache hit");
    assert_eq!(hit.bytes.as_slice(), &[9, 9, 9]);
}

#[test]
fn invalidate_removes_entry_and_releases_bytes() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 1024);
    rm.insert_network(
        "https://example.com/a.png".into(),
        Arc::new(vec![1, 2, 3, 4]),
        None,
        vec![],
    );
    assert_eq!(rm.current_bytes(), 4);
    rm.invalidate("https://example.com/a.png");
    assert!(rm.get("https://example.com/a.png").is_none());
    assert_eq!(rm.current_bytes(), 0);
}

#[test]
fn lru_eviction_drops_oldest_when_over_budget() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 10);

    // Insert three 4-byte entries into a 10-byte cache. Third insert
    // should evict the first (oldest) entry.
    rm.insert_network("u/a".into(), Arc::new(vec![1; 4]), None, vec![]);
    rm.insert_network("u/b".into(), Arc::new(vec![2; 4]), None, vec![]);
    rm.insert_network("u/c".into(), Arc::new(vec![3; 4]), None, vec![]);

    assert!(
        rm.current_bytes() <= rm.byte_budget(),
        "should be under budget, got {} / {}",
        rm.current_bytes(),
        rm.byte_budget()
    );
    assert!(rm.get("u/a").is_none(), "oldest entry should be evicted");
    assert!(rm.get("u/c").is_some(), "newest entry should still exist");
}

#[test]
fn lru_eviction_respects_access_order() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 10);

    rm.insert_network("u/a".into(), Arc::new(vec![1; 4]), None, vec![]);
    rm.insert_network("u/b".into(), Arc::new(vec![2; 4]), None, vec![]);
    // Touch `a` so it becomes most-recently-used, making `b` the
    // eviction victim when we insert `c`.
    let _ = rm.get("u/a");
    rm.insert_network("u/c".into(), Arc::new(vec![3; 4]), None, vec![]);

    assert!(rm.get("u/a").is_some(), "touched entry should survive");
    assert!(
        rm.get("u/b").is_none(),
        "untouched older entry should be evicted"
    );
}

#[test]
fn clear_empties_cache_and_resets_bytes() {
    let mut rm: ResourceManager<AlwaysFreshPolicy, LruBytesEviction> =
        ResourceManager::with_parts(AlwaysFreshPolicy, LruBytesEviction::new(), 1024);
    rm.insert_network("u/a".into(), Arc::new(vec![1; 10]), None, vec![]);
    rm.insert_network("u/b".into(), Arc::new(vec![2; 10]), None, vec![]);
    assert_eq!(rm.current_bytes(), 20);
    rm.clear();
    assert!(rm.is_empty());
    assert_eq!(rm.current_bytes(), 0);
}

// ── Policy-stub contract ──────────────────────────────────────────────

#[test]
fn freshness_on_missing_entry_is_missing_not_panic() {
    // Even under the () stub, a cache miss short-circuits without
    // calling the evaluator — so this should not panic.
    let rm = ResourceManager::new();
    assert_eq!(rm.freshness("https://unknown"), Freshness::Missing);
}

#[test]
#[should_panic(expected = "default () CachePolicyProvider")]
fn default_policy_provider_panics_on_evaluate() {
    let mut rm = ResourceManager::new();
    rm.insert_network("u".into(), Arc::new(vec![0]), None, vec![]);
    // Should panic with the stub contract message.
    let _ = rm.freshness("u");
}

#[test]
fn default_policy_provider_conditional_headers_is_empty() {
    let mut rm = ResourceManager::new();
    rm.insert_network(
        "u".into(),
        Arc::new(vec![0]),
        None,
        vec![("ETag".into(), "\"x\"".into())],
    );
    // Stub returns empty even when validators are present — hosts
    // that care plug in a real provider.
    assert!(rm.conditional_headers("u").is_empty());
}
