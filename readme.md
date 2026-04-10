# Wardent

A lightweight Rust reverse proxy with built-in rate limiting, bot filtering, and request routing. Sits between Cloudflare and your application server, handling traffic control at the edge of your stack.

## How It Works

Wardent runs as a systemd service, listening for incoming HTTP requests and proxying them to your backend. It intercepts requests before they reach your application, applying rate limits, filtering bots by user agent, enforcing body size limits, and redirecting blocked traffic.

```
Client → Cloudflare → nginx → Wardent → Django (or any backend)
```

## Features

- **Rate Limiting** — configurable requests per minute with burst allowance
- **Bot Filtering** — block crawlers and AI scrapers by user agent (Googlebot, GPTBot, ChatGPT-User, OAI-SearchBot, CCBot, Bingbot, etc.)
- **Selective Access** — allow specific bots through while blocking others (e.g. Claude allowed, ChatGPT blocked)
- **Bot Redirect** — blocked bots get redirected to a configurable URL instead of a generic error page
- **Body Size Enforcement** — reject oversized payloads before they hit your backend
- **Custom Error Pages** — per-status-code error page redirects (429, 403, 413, 408, 502)
- **Timeout Overrides** — per-path timeout configuration for long-running endpoints (Stripe checkout, webhooks, etc.)
- **Language Agnostic** — proxies to any HTTP backend regardless of framework or language

## Configuration

Wardent is configured via a TOML file.

```toml
[server]
listen = "0.0.0.0:8080"
proxy_to = "127.0.0.1:8000"

[rate_limit]
requests_per_minute = 120
burst = 20

[body]
max_size = "5MB"

[bot_filter]
blocked_user_agents = [
    "Googlebot",
    "Google-Extended",
    "GoogleOther",
    "Bingbot",
    "msnbot",
    "GPTBot",
    "ChatGPT-User",
    "OAI-SearchBot",
    "CCBot",
    "facebookexternalhit",
    "FacebookBot",
    "meta-externalagent",
    "DuckDuckBot",
    "YandexBot",
    "Baiduspider",
    "Slurp",
    "Twitterbot",
    "Scrapy",
]
redirect_url = "https://en.wikipedia.org/wiki/Web_scraping"

[error_pages]
429 = "/errors/429.html"
403 = "/errors/403.html"
413 = "/errors/413.html"
408 = "/errors/408.html"
502 = "/errors/502.html"

[timeout_overrides]
"/payments/checkout/" = 300
"/webhooks/stripe/" = 90
```

## Running

Wardent runs as a systemd service in production:

```bash
sudo systemctl start wardent
sudo systemctl enable wardent
sudo systemctl status wardent
```

## Stack

Built with Rust. Deployed via systemd. Designed to sit in front of any HTTP backend as a transparent proxy layer.