//! An in-memory HTTP cache.
//!
//! Revisiting a page re-fetched every stylesheet and every image, however
//! recently they had been downloaded. This keeps them, and — when an entry has
//! gone stale but carries a validator — asks the server whether it changed
//! rather than asking for the bytes again.
//!
//! Nothing is written to disk: the cache lives as long as the process, like the
//! cookie jar.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How many entries to keep before evicting the oldest.
const MAX_ENTRIES: usize = 512;

/// How many bytes to keep in total.
const MAX_BYTES: usize = 48 * 1024 * 1024;

/// A response body we may be able to reuse.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    /// The URL the response actually came from, after redirects.
    pub final_url: String,
    /// What the response's headers permit.
    pub policy: CachePolicy,
    /// When the body was stored, for computing its age.
    stored_at: Instant,
}

impl CacheEntry {
    /// How long ago this entry was stored.
    pub fn age(&self) -> Duration {
        self.stored_at.elapsed()
    }
}

/// The caching rules a response declared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CachePolicy {
    /// `Cache-Control: no-store` — must not be kept at all.
    pub no_store: bool,
    /// `Cache-Control: no-cache` — may be kept, but never used without asking.
    pub no_cache: bool,
    /// `Cache-Control: max-age`, in seconds.
    pub max_age: Option<u64>,
    /// `ETag`, for `If-None-Match`.
    pub etag: Option<String>,
    /// `Last-Modified`, for `If-Modified-Since`.
    pub last_modified: Option<String>,
}

impl CachePolicy {
    /// Read the caching rules out of a response's headers.
    pub fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        let get = |name: reqwest::header::HeaderName| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        };

        let cache_control = get(reqwest::header::CACHE_CONTROL).unwrap_or_default();
        let directives: Vec<String> = cache_control
            .split(',')
            .map(|d| d.trim().to_ascii_lowercase())
            .collect();

        let max_age = directives.iter().find_map(|d| {
            d.strip_prefix("max-age")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .and_then(|v| v.trim().parse::<u64>().ok())
        });

        Self {
            no_store: directives.iter().any(|d| d == "no-store"),
            // `must-revalidate` and a zero max-age both mean the same thing to
            // us: keep it, but never serve it without checking.
            no_cache: directives
                .iter()
                .any(|d| d == "no-cache" || d == "must-revalidate")
                || max_age == Some(0),
            max_age,
            etag: get(reqwest::header::ETAG),
            last_modified: get(reqwest::header::LAST_MODIFIED),
        }
    }

    /// Whether an entry with this policy is worth storing.
    pub fn is_storable(&self) -> bool {
        // Something with neither a lifetime nor a validator can never be
        // reused, so keeping it would only cost memory.
        !self.no_store && (self.max_age.is_some() || self.has_validator())
    }

    /// Whether the server gave us something to revalidate with.
    pub fn has_validator(&self) -> bool {
        self.etag.is_some() || self.last_modified.is_some()
    }
}

/// What a cached entry lets us do for a request happening now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheDecision {
    /// Reuse the stored body without contacting the server.
    Fresh,
    /// Ask whether it changed, sending these validators.
    Revalidate {
        if_none_match: Option<String>,
        if_modified_since: Option<String>,
    },
    /// Unusable; fetch it normally.
    Miss,
}

/// Decide what to do with an entry of the given age.
///
/// Age is a parameter rather than read from a clock so the policy can be tested
/// without waiting for time to pass.
pub fn decide(policy: &CachePolicy, age: Duration) -> CacheDecision {
    if policy.no_store {
        return CacheDecision::Miss;
    }

    let revalidate = || {
        if policy.has_validator() {
            CacheDecision::Revalidate {
                if_none_match: policy.etag.clone(),
                if_modified_since: policy.last_modified.clone(),
            }
        } else {
            CacheDecision::Miss
        }
    };

    if policy.no_cache {
        return revalidate();
    }

    match policy.max_age {
        Some(max_age) if age.as_secs() < max_age => CacheDecision::Fresh,
        // Stale, or never had a lifetime: the validator is the only way back.
        _ => revalidate(),
    }
}

/// The process-wide cache.
static CACHE: LazyLock<Mutex<HashMap<String, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Look up a URL, returning a copy of the entry.
pub fn lookup(url: &str) -> Option<CacheEntry> {
    CACHE.lock().ok()?.get(url).cloned()
}

/// Store a response, if its headers allow it.
pub fn store(url: &str, entry: CacheEntry) {
    if !entry.policy.is_storable() {
        return;
    }
    let Ok(mut cache) = CACHE.lock() else {
        return;
    };
    cache.insert(url.to_string(), entry);
    evict_if_needed(&mut cache);
}

/// Build an entry for a response body received now.
pub fn new_entry(
    body: Vec<u8>,
    content_type: Option<String>,
    final_url: String,
    policy: CachePolicy,
) -> CacheEntry {
    CacheEntry {
        body,
        content_type,
        final_url,
        policy,
        stored_at: Instant::now(),
    }
}

/// Mark a revalidated entry as fresh again.
///
/// A 304 means the body we hold is still current, so its age restarts — which
/// is the whole point of asking.
pub fn refresh(url: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        if let Some(entry) = cache.get_mut(url) {
            entry.stored_at = Instant::now();
        }
    }
}

/// Drop the oldest entries until the cache is back within its limits.
fn evict_if_needed(cache: &mut HashMap<String, CacheEntry>) {
    let total_bytes: usize = cache.values().map(|e| e.body.len()).sum();
    if cache.len() <= MAX_ENTRIES && total_bytes <= MAX_BYTES {
        return;
    }

    let mut by_age: Vec<(String, Instant)> = cache
        .iter()
        .map(|(url, entry)| (url.clone(), entry.stored_at))
        .collect();
    // Oldest first.
    by_age.sort_by_key(|(_, stored_at)| *stored_at);

    let mut bytes = total_bytes;
    for (url, _) in by_age {
        if cache.len() <= MAX_ENTRIES && bytes <= MAX_BYTES {
            break;
        }
        if let Some(removed) = cache.remove(&url) {
            bytes -= removed.body.len();
        }
    }
}

/// Empty the cache. Used by tests, and available for a reload.
pub fn clear() {
    if let Ok(mut cache) = CACHE.lock() {
        cache.clear();
    }
}

/// How many entries are held. Diagnostics and tests.
pub fn len() -> usize {
    CACHE.lock().map(|c| c.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn max_age_is_read_from_cache_control() {
        let policy = CachePolicy::from_headers(&headers(&[(
            "cache-control",
            "public, max-age=3600, immutable",
        )]));
        assert_eq!(policy.max_age, Some(3600));
        assert!(!policy.no_store);
        assert!(!policy.no_cache);
    }

    #[test]
    fn no_store_and_no_cache_are_distinguished() {
        let no_store = CachePolicy::from_headers(&headers(&[("cache-control", "no-store")]));
        assert!(no_store.no_store);
        assert!(!no_store.is_storable(), "no-store must not be kept");

        let no_cache = CachePolicy::from_headers(&headers(&[
            ("cache-control", "no-cache"),
            ("etag", "\"v1\""),
        ]));
        assert!(no_cache.no_cache);
        assert!(
            no_cache.is_storable(),
            "no-cache may be kept, it just cannot be served unchecked"
        );
    }

    #[test]
    fn must_revalidate_and_a_zero_lifetime_both_force_a_check() {
        for value in ["must-revalidate", "max-age=0"] {
            let policy = CachePolicy::from_headers(&headers(&[
                ("cache-control", value),
                ("etag", "\"v1\""),
            ]));
            assert!(
                matches!(
                    decide(&policy, Duration::from_secs(0)),
                    CacheDecision::Revalidate { .. }
                ),
                "{value} should force revalidation even at zero age"
            );
        }
    }

    #[test]
    fn a_fresh_entry_is_served_without_a_request() {
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=600")]));
        assert_eq!(
            decide(&policy, Duration::from_secs(59)),
            CacheDecision::Fresh
        );
    }

    #[test]
    fn a_stale_entry_is_revalidated_with_whatever_validator_it_has() {
        let policy = CachePolicy::from_headers(&headers(&[
            ("cache-control", "max-age=60"),
            ("etag", "\"abc\""),
            ("last-modified", "Wed, 20 Aug 2026 10:00:00 GMT"),
        ]));

        assert_eq!(
            decide(&policy, Duration::from_secs(61)),
            CacheDecision::Revalidate {
                if_none_match: Some("\"abc\"".into()),
                if_modified_since: Some("Wed, 20 Aug 2026 10:00:00 GMT".into()),
            }
        );
    }

    #[test]
    fn a_stale_entry_with_no_validator_is_a_miss() {
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=60")]));
        assert_eq!(
            decide(&policy, Duration::from_secs(61)),
            CacheDecision::Miss
        );
    }

    #[test]
    fn a_response_with_no_caching_headers_is_not_stored() {
        let policy = CachePolicy::from_headers(&HeaderMap::new());
        assert!(
            !policy.is_storable(),
            "nothing to revalidate with and no lifetime — keeping it is pure cost"
        );
    }

    #[test]
    fn an_etag_alone_is_worth_storing() {
        // Common for static assets: no lifetime, but a validator, so a revisit
        // costs a 304 rather than the whole body.
        let policy = CachePolicy::from_headers(&headers(&[("etag", "\"v9\"")]));
        assert!(policy.is_storable());
        assert!(matches!(
            decide(&policy, Duration::from_secs(0)),
            CacheDecision::Revalidate { .. }
        ));
    }

    // The cache is process-wide, so these use URLs unique to each test and
    // assert on those keys rather than on the total count — tests run
    // concurrently and would otherwise see each other's entries.

    #[test]
    fn storing_and_looking_up_round_trips() {
        let url = "https://example.com/round-trip.css";
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=60")]));
        store(
            url,
            new_entry(
                b"body{}".to_vec(),
                Some("text/css".into()),
                url.into(),
                policy,
            ),
        );

        let entry = lookup(url).expect("stored entry");
        assert_eq!(entry.body, b"body{}");
        assert_eq!(entry.content_type.as_deref(), Some("text/css"));
        assert!(lookup("https://example.com/never-stored.css").is_none());
    }

    #[test]
    fn an_unstorable_response_is_dropped_rather_than_kept() {
        let url = "https://example.com/no-store-secret";
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "no-store")]));
        store(url, new_entry(b"x".to_vec(), None, url.into(), policy));
        assert!(lookup(url).is_none());
    }

    #[test]
    fn a_revalidated_entry_becomes_fresh_again() {
        let url = "https://example.com/revalidated";
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=600")]));
        store(url, new_entry(b"v1".to_vec(), None, url.into(), policy));

        let before = lookup(url).unwrap().age();
        refresh(url);
        let after = lookup(url).unwrap().age();
        assert!(
            after <= before,
            "a 304 restarts the age: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn eviction_drops_the_oldest_entries_first() {
        // Exercised on a local map: the shared cache would be racing other tests.
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=600")]));
        let mut cache = HashMap::new();
        for i in 0..(MAX_ENTRIES + 20) {
            cache.insert(
                format!("https://example.com/{i}"),
                new_entry(
                    vec![0u8; 8],
                    None,
                    format!("https://example.com/{i}"),
                    policy.clone(),
                ),
            );
        }

        evict_if_needed(&mut cache);
        assert!(
            cache.len() <= MAX_ENTRIES,
            "expected at most {MAX_ENTRIES} entries, got {}",
            cache.len()
        );
    }

    #[test]
    fn eviction_also_respects_the_byte_budget() {
        let policy = CachePolicy::from_headers(&headers(&[("cache-control", "max-age=600")]));
        let mut cache = HashMap::new();
        // Ten entries, each an eighth of the budget: over by a quarter.
        let chunk = MAX_BYTES / 8;
        for i in 0..10 {
            cache.insert(
                format!("https://example.com/big{i}"),
                new_entry(
                    vec![0u8; chunk],
                    None,
                    format!("https://example.com/big{i}"),
                    policy.clone(),
                ),
            );
        }

        evict_if_needed(&mut cache);
        let total: usize = cache.values().map(|e| e.body.len()).sum();
        assert!(
            total <= MAX_BYTES,
            "expected at most {MAX_BYTES} bytes, got {total}"
        );
        assert!(!cache.is_empty(), "eviction should not empty the cache");
    }
}
