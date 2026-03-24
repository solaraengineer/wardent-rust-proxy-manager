mod config;
mod filter;
mod proxy;
mod ratelimit;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use config::Config;
use filter::Filter;
use ratelimit::RateLimit;

struct AppState {
    config: Config,
    filter: Filter,
    rate_limiter: RateLimit,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wardent=info".into()),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "wardent.toml".to_string());

    let config = Config::load(&config_path)?;
    info!(
        listen = %config.server.listen_addr,
        upstream = %config.proxy.upstream,
        "Wardent starting"
    );
    info!(
        max_body = config.limits.max_body_size,
        default_timeout = config.limits.default_timeout_secs,
        rate_limit_rpm = config.rate_limit.requests_per_minute,
        "Limits configured"
    );
    for rule in &config.timeout_override {
        info!(
            path = %rule.path,
            timeout_secs = rule.timeout_secs,
            "Timeout override loaded"
        );
    }

    let state = Arc::new(AppState {
        filter: Filter::new(&config.filter),
        rate_limiter: RateLimit::new(&config.rate_limit),
        config,
    });

    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            cleanup_state.rate_limiter.cleanup();
        }
    });

    let addr: SocketAddr = state.config.server.listen_addr.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "Listening");

    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!(error = %e, "Failed to accept connection");
                continue;
            }
        };

        let state = state.clone();
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let state = state.clone();
                let remote_ip = remote_addr.ip();
                async move {
                    handle_request(req, &state, remote_ip).await
                }
            });

            if let Err(err) = http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                if !err.is_incomplete_message() {
                    warn!(error = %err, "Connection error");
                }
            }
        });
    }
}

/// Extract the real client IP from proxy headers.
/// Stack: client -> WAF -> nginx -> Wardent
/// The first IP in X-Forwarded-For is the real client.
fn extract_client_ip(req: &Request<Incoming>, remote_addr: std::net::IpAddr) -> String {
    // X-Forwarded-For: <client>, <waf>, <nginx>
    if let Some(xff) = req.headers().get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            if let Some(first_ip) = xff_str.split(',').next() {
                let trimmed = first_ip.trim();
                if trimmed.parse::<std::net::IpAddr>().is_ok() {
                    return trimmed.to_string();
                }
            }
        }
    }

    // Fallback: X-Real-IP
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            let trimmed = ip_str.trim();
            if trimmed.parse::<std::net::IpAddr>().is_ok() {
                return trimmed.to_string();
            }
        }
    }

    // Last resort: raw TCP addr (will be nginx's IP, but better than nothing)
    remote_addr.to_string()
}

async fn handle_request(
    req: Request<Incoming>,
    state: &AppState,
    remote_addr: std::net::IpAddr,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let client_ip = extract_client_ip(&req, remote_addr);
    let ip: std::net::IpAddr = client_ip
        .parse()
        .unwrap_or_else(|_| "0.0.0.0".parse().unwrap());

    info!(client_ip = %client_ip, remote_addr = %remote_addr, "Request received");

    // 1. Rate limit check (now per actual client, not nginx)
    if let Some(response) = state.rate_limiter.check_rate_limit(ip, &state.config.error_redirects) {
        return Ok(response);
    }

    // 2. User-agent filter
    let user_agent = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok());

    if let Some(response) = state.filter.check_user_agent(user_agent) {
        return Ok(response);
    }

    // 3. Forward to upstream
    proxy::forward(req, &state.config, &client_ip).await
}