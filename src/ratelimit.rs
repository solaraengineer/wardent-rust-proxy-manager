use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use hyper::{Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{warn, error};

use crate::config::{RateLimitConfig, ErrorRedirects};

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

const BAN_DURATION: Duration = Duration::from_secs(3600); // 1 hour
const MAX_VIOLATIONS: u32 = 3;

struct ViolationRecord {
    count: u32,
    first_violation: Instant,
}

pub struct RateLimit {
    limiters: DashMap<IpAddr, Arc<Limiter>>,
    violations: DashMap<IpAddr, ViolationRecord>,
    banned: DashMap<IpAddr, Instant>,
    quota: Quota,
}

impl RateLimit {
    pub fn new(config: &RateLimitConfig) -> Self {
        let rpm = NonZeroU32::new(config.requests_per_minute)
            .expect("requests_per_minute must be > 0");
        let burst = NonZeroU32::new(config.burst_size)
            .expect("burst_size must be > 0");

        let quota = Quota::per_minute(rpm).allow_burst(burst);

        Self {
            limiters: DashMap::new(),
            violations: DashMap::new(),
            banned: DashMap::new(),
            quota,
        }
    }

    /// Check if an IP is banned or rate limited.
    /// Returns Some(Response) with 302 redirect if blocked, None if allowed.
    pub fn check_rate_limit(
        &self,
        ip: IpAddr,
        redirects: &ErrorRedirects,
    ) -> Option<Response<Full<Bytes>>> {
        // 1. Check if IP is banned
        if let Some(ban_expiry) = self.banned.get(&ip) {
            if Instant::now() < *ban_expiry {
                let remaining = ban_expiry.duration_since(Instant::now());
                error!(ip = %ip, remaining_secs = remaining.as_secs(), "Banned IP attempted request");
                return Some(redirect(&redirects.banned));
            } else {
                self.banned.remove(&ip);
                self.violations.remove(&ip);
            }
        }

        // 2. Check rate limit
        let limiter = self
            .limiters
            .entry(ip)
            .or_insert_with(|| Arc::new(RateLimiter::direct(self.quota)))
            .clone();

        match limiter.check() {
            Ok(_) => None,
            Err(_) => {
                let should_ban = {
                    let mut entry = self
                        .violations
                        .entry(ip)
                        .or_insert_with(|| ViolationRecord {
                            count: 0,
                            first_violation: Instant::now(),
                        });

                    entry.count += 1;
                    warn!(ip = %ip, violations = entry.count, "Rate limit exceeded");
                    entry.count >= MAX_VIOLATIONS
                };

                if should_ban {
                    let ban_until = Instant::now() + BAN_DURATION;
                    self.banned.insert(ip, ban_until);
                    error!(ip = %ip, duration_secs = BAN_DURATION.as_secs(), "IP banned");
                    return Some(redirect(&redirects.banned));
                }

                Some(redirect(&redirects.rate_limited))
            }
        }
    }

    /// Periodic cleanup of expired bans and stale entries.
    pub fn cleanup(&self) {
        let now = Instant::now();

        self.banned.retain(|ip, expiry| {
            if now >= *expiry {
                warn!(ip = %ip, "Ban expired, removing");
                false
            } else {
                true
            }
        });

        self.violations.retain(|ip, _| self.banned.contains_key(ip));

        if self.limiters.len() > 10_000 {
            warn!("Rate limiter map exceeded 10k entries, clearing");
            self.limiters.clear();
        }
    }
}

fn redirect(location: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .header("Content-Length", "0")
        .body(Full::new(Bytes::new()))
        .unwrap()
}
