# Wardent

A lightweight, high-performance reverse proxy and API gateway written in raw Rust. No frameworks. Single binary. Sits between Nginx and your application server as a security and filtering layer.

## What it does

Wardent intercepts every incoming request before it reaches your backend. If a request fails any check, it gets killed instantly — your application never knows it existed.

- **Rate limiting** — 40 requests/minute per IP with configurable burst. Uses in-memory token bucket algorithm.
- **IP banning** — Exceed the rate limit 3 times and you're banned for 1 hour. Automatic expiry, no manual intervention.
- **Bot filtering** — Blocks known crawler user-agents (Google, Microsoft, OpenAI, Meta, and others) via compiled regex. Redirects them to a URL of your choice.
- **Body size enforcement** — Rejects payloads over 5MB before your backend wastes resources reading them.
- **Per-path timeouts** — Default 5 second timeout with configurable overrides for slow routes like payment processing.
- **Header forwarding** — Passes X-Forwarded-For and X-Forwarded-Proto, strips hop-by-hop headers.
- **Error redirects** — All errors return 302 redirects to your application's styled error pages instead of raw JSON.

## Architecture

```
Client → Nginx (:443) → Wardent (:8080) → Django (:8000)
```

Wardent runs as a systemd service directly on the host. Not containerized. Internal services (databases, caches, monitoring) bypass wardent entirely and communicate with the backend directly.

## Request lifecycle

```
Incoming request
  │
  ├─ IP banned? ──────────── → 302 /error/403/
  │
  ├─ Rate limit exceeded? ── → 302 /error/429/
  │   └─ 3rd violation? ──── → ban IP for 1 hour
  │
  ├─ Blocked user-agent? ─── → 301 Wikipedia
  │
  ├─ Body > 5MB? ─────────── → 302 /error/413/
  │
  ├─ Forward to backend
  │   ├─ Timeout? ─────────── → 302 /error/408/
  │   └─ Backend down? ────── → 302 /error/502/
  │
  └─ Response back to client
```

## Configuration

Everything is controlled through `wardent.toml`:

```toml
[server]
listen_addr = "0.0.0.0:8080"

[proxy]
upstream = "http://127.0.0.1:8000"

[limits]
max_body_size = 5_242_880  # 5MB
default_timeout_secs = 5

[rate_limit]
requests_per_minute = 40
burst_size = 20

[filter]
blocked_user_agents = [
    "Googlebot",
    "GPTBot",
    "facebookexternalhit",
    # ... see wardent.toml for full list
]
redirect_url = "https://en.wikipedia.org/wiki/Web_scraping"

[error_redirects]
rate_limited = "/error/429/"
banned = "/error/403/"
body_too_large = "/error/413/"
timeout = "/error/408/"
bad_gateway = "/error/502/"

[[timeout_override]]
path = "/create-checkout-session/"
timeout_secs = 300

[[timeout_override]]
path = "/webhook/stripe/"
timeout_secs = 90
```

## Build & run

```bash
cargo build --release
./target/release/wardent wardent.toml
```

Set log level with the `RUST_LOG` environment variable:

```bash
RUST_LOG=wardent=debug ./target/release/wardent wardent.toml
```

## Tech stack

| Crate | Purpose |
|-------|---------|
| tokio | Async runtime |
| hyper | HTTP server and client |
| dashmap | Concurrent hash maps for rate limiting, bans |
| governor | Token bucket rate limiter |
| regex | User-agent pattern matching |
| toml + serde | Config parsing |
| tracing | Structured logging |

## Project structure

```
src/
├── main.rs        Entry point, server loop, request handler wiring
├── config.rs      TOML config parser and deserialization
├── proxy.rs       Request forwarding, timeout enforcement, body size checks
├── filter.rs      User-agent regex matching and bot blocking
├── ratelimit.rs   Per-IP rate limiting, violation tracking, IP banning
└── tcp.rs         Timeout resolution helper
```

## Performance

Wardent adds approximately 0.1–0.5ms of latency per request. All checks are O(1) lookups against in-memory data structures. Regex patterns are compiled once at startup. Zero heap allocation in the hot path for most requests.

Memory usage stays minimal — 10,000 tracked IPs consume roughly 2MB of RAM.

## License

Private. Not open source.
