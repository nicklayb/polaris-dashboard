//! Caching name resolver for ureq.
//!
//! ureq resolves the host of every request, even when it reuses a pooled
//! connection, and its default resolver caches nothing. Polling a dashboard
//! therefore sends one DNS query per HTTP request. This resolver keeps
//! answers for a configurable duration and forgets a host as soon as a
//! request to it fails at the transport level, so a DNS change still
//! propagates quickly.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::NextTimeout;

use crate::clock::{Clock, SystemClock};

/// Resolver caching the answers of an inner resolver for `time_to_live`.
///
/// Cloning is cheap and clones share the same cache, which lets the caller
/// hand one clone to the ureq agent and keep another to invalidate entries.
pub struct CachingResolver<R = DefaultResolver, C = SystemClock> {
    shared: Arc<SharedCache<R, C>>,
}

struct SharedCache<R, C> {
    resolver: R,
    clock: C,
    time_to_live: Duration,
    entries: Mutex<HashMap<String, CachedAddresses>>,
}

struct CachedAddresses {
    addresses: ResolvedSocketAddrs,
    resolved_at: Instant,
}

impl CachingResolver {
    /// Caches the system resolver for `time_to_live`.
    pub fn with_system_resolver(time_to_live: Duration) -> Self {
        Self::new(DefaultResolver::default(), SystemClock, time_to_live)
    }
}

impl<R: Resolver, C: Clock> CachingResolver<R, C> {
    /// Caches the answers of `resolver` for `time_to_live`, as measured by
    /// `clock`. A zero `time_to_live` disables caching.
    pub fn new(resolver: R, clock: C, time_to_live: Duration) -> Self {
        Self {
            shared: Arc::new(SharedCache {
                resolver,
                clock,
                time_to_live,
                entries: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Forgets the cached addresses of the host targeted by `uri`.
    pub fn invalidate(&self, uri: &Uri) {
        if let Some(key) = cache_key(uri) {
            self.entries().remove(&key);
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, CachedAddresses>> {
        // A poisoned cache only holds plain addresses, still safe to reuse.
        self.shared
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn fresh_addresses(&self, key: &str, now: Instant) -> Option<ResolvedSocketAddrs> {
        self.entries()
            .get(key)
            .filter(|cached| now.duration_since(cached.resolved_at) < self.shared.time_to_live)
            .map(|cached| cached.addresses.clone())
    }

    fn store(&self, key: String, addresses: &ResolvedSocketAddrs, now: Instant) {
        let cached = CachedAddresses {
            addresses: addresses.clone(),
            resolved_at: now,
        };
        self.entries().insert(key, cached);
    }
}

impl<R: Resolver, C: Clock> Resolver for CachingResolver<R, C> {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let Some(key) = cache_key(uri) else {
            return self.shared.resolver.resolve(uri, config, timeout);
        };
        let now = self.shared.clock.now();
        if let Some(addresses) = self.fresh_addresses(&key, now) {
            return Ok(addresses);
        }
        let addresses = self.shared.resolver.resolve(uri, config, timeout)?;
        self.store(key, &addresses, now);
        Ok(addresses)
    }

    fn empty(&self) -> ResolvedSocketAddrs {
        self.shared.resolver.empty()
    }
}

impl<R, C> Clone for CachingResolver<R, C> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<R, C> fmt::Debug for CachingResolver<R, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachingResolver")
            .field("time_to_live", &self.shared.time_to_live)
            .finish_non_exhaustive()
    }
}

/// Tells whether a failed request may have been caused by a stale address,
/// meaning the cached host should be resolved again.
pub fn is_connection_failure(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Io(_)
            | ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::Timeout(_)
    )
}

/// `host:port` of `uri`, with the scheme default port when omitted.
fn cache_key(uri: &Uri) -> Option<String> {
    DefaultResolver::host_and_port(uri.scheme()?, uri.authority()?)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const TIME_TO_LIVE: Duration = Duration::from_secs(60);
    const POWER_PLANT_URL: &str = "http://power-plant.springfield.local:3000/api/ds/query";
    const KWIK_E_MART_URL: &str = "http://kwik-e-mart.springfield.local:3000/api/ds/query";
    const POWER_PLANT_ADDRESS: &str = "10.0.7.1:3000";

    /// Inner resolver counting lookups and answering a fixed address.
    #[derive(Debug, Default)]
    struct CountingResolver {
        lookups: Arc<AtomicUsize>,
    }

    impl Resolver for CountingResolver {
        fn resolve(
            &self,
            _uri: &Uri,
            _config: &Config,
            _timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            let mut addresses = self.empty();
            addresses.push(POWER_PLANT_ADDRESS.parse::<SocketAddr>().unwrap());
            Ok(addresses)
        }
    }

    /// Clock only moving when the test advances it.
    #[derive(Clone)]
    struct ManualClock {
        start: Instant,
        elapsed: Arc<Mutex<Duration>>,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                start: Instant::now(),
                elapsed: Arc::new(Mutex::new(Duration::ZERO)),
            }
        }

        fn advance(&self, duration: Duration) {
            *self.elapsed.lock().unwrap() += duration;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            self.start + *self.elapsed.lock().unwrap()
        }
    }

    struct Fixture {
        resolver: CachingResolver<CountingResolver, ManualClock>,
        clock: ManualClock,
        lookups: Arc<AtomicUsize>,
    }

    fn fixture(time_to_live: Duration) -> Fixture {
        let inner = CountingResolver::default();
        let lookups = Arc::clone(&inner.lookups);
        let clock = ManualClock::new();
        Fixture {
            resolver: CachingResolver::new(inner, clock.clone(), time_to_live),
            clock,
            lookups,
        }
    }

    fn resolve(resolver: &impl Resolver, url: &str) -> ResolvedSocketAddrs {
        resolver
            .resolve(
                &url.parse().unwrap(),
                &Config::default(),
                NextTimeout::default(),
            )
            .unwrap()
    }

    #[test]
    fn reuses_answer_within_time_to_live() {
        let fixture = fixture(TIME_TO_LIVE);

        let first_addresses = resolve(&fixture.resolver, POWER_PLANT_URL);
        fixture.clock.advance(TIME_TO_LIVE - Duration::from_secs(1));
        let second_addresses = resolve(&fixture.resolver, POWER_PLANT_URL);

        assert_eq!(first_addresses, second_addresses);
        assert_eq!(fixture.lookups.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resolves_again_once_expired() {
        let fixture = fixture(TIME_TO_LIVE);

        resolve(&fixture.resolver, POWER_PLANT_URL);
        fixture.clock.advance(TIME_TO_LIVE);
        resolve(&fixture.resolver, POWER_PLANT_URL);

        assert_eq!(fixture.lookups.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn invalidation_forces_a_new_lookup() {
        let fixture = fixture(TIME_TO_LIVE);
        let handle = fixture.resolver.clone();

        resolve(&fixture.resolver, POWER_PLANT_URL);
        handle.invalidate(&POWER_PLANT_URL.parse().unwrap());
        resolve(&fixture.resolver, POWER_PLANT_URL);

        assert_eq!(fixture.lookups.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn caches_each_host_separately() {
        let fixture = fixture(TIME_TO_LIVE);

        resolve(&fixture.resolver, POWER_PLANT_URL);
        resolve(&fixture.resolver, KWIK_E_MART_URL);
        resolve(&fixture.resolver, KWIK_E_MART_URL);

        assert_eq!(fixture.lookups.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn zero_time_to_live_disables_caching() {
        let fixture = fixture(Duration::ZERO);

        resolve(&fixture.resolver, POWER_PLANT_URL);
        resolve(&fixture.resolver, POWER_PLANT_URL);

        assert_eq!(fixture.lookups.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn transport_errors_are_connection_failures() {
        assert!(is_connection_failure(&ureq::Error::ConnectionFailed));
        assert!(is_connection_failure(&ureq::Error::HostNotFound));
        assert!(!is_connection_failure(&ureq::Error::StatusCode(500)));
    }
}
