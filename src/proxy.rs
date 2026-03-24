use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode, Uri};
use std::time::Duration;
use tracing::{error, info, instrument};

use crate::config::Config;

#[instrument(skip_all, fields(method = %req.method(), path = %req.uri().path()))]
pub async fn forward(
    req: Request<Incoming>,
    config: &Config,
    client_ip: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    let timeout_secs = config.timeout_for_path(&path);
    let timeout = Duration::from_secs(timeout_secs);

    info!(
        client_ip = client_ip,
        timeout_secs = timeout_secs,
        "Forwarding request"
    );

    let body_result = tokio::time::timeout(timeout, collect_body(req, config)).await;

    let (parts, body_bytes) = match body_result {
        Ok(Ok(result)) => result,
        Ok(Err(response)) => return Ok(response),
        Err(_) => {
            error!("Timeout reading request body");
            return Ok(status_response(StatusCode::GATEWAY_TIMEOUT));
        }
    };

    let upstream_uri = format!(
        "{}{}",
        config.proxy.upstream.trim_end_matches('/'),
        parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
    );

    let upstream_uri: Uri = match upstream_uri.parse() {
        Ok(uri) => uri,
        Err(e) => {
            error!(error = %e, "Failed to parse upstream URI");
            return Ok(status_response(StatusCode::BAD_GATEWAY));
        }
    };

    let mut builder = Request::builder()
        .method(method)
        .uri(upstream_uri);

    for (name, value) in parts.headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if matches!(
            name_str.as_str(),
            "connection" | "keep-alive" | "transfer-encoding" | "te" | "trailer" | "upgrade"
        ) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder.header("X-Forwarded-For", client_ip);
    builder = builder.header("X-Forwarded-Proto", "https");
    builder = builder.header("X-Wardent-Secret", &config.proxy.secret_key);

    let outgoing = builder
        .body(Full::new(body_bytes.clone()))
        .expect("Failed to build outgoing request");

    let upstream_result = tokio::time::timeout(
        timeout,
        send_upstream(outgoing, &config.proxy.upstream),
    )
        .await;

    match upstream_result {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => {
            error!(error = %e, "Upstream request failed");
            Ok(status_response(StatusCode::BAD_GATEWAY))
        }
        Err(_) => {
            error!(path = path, timeout_secs = timeout_secs, "Upstream timeout");
            Ok(status_response(StatusCode::GATEWAY_TIMEOUT))
        }
    }
}

async fn collect_body(
    req: Request<Incoming>,
    config: &Config,
) -> Result<(hyper::http::request::Parts, Bytes), Response<Full<Bytes>>> {
    let max_size = config.limits.max_body_size;

    if let Some(content_length) = req.headers().get("content-length") {
        if let Ok(len_str) = content_length.to_str() {
            if let Ok(len) = len_str.parse::<u64>() {
                if len > max_size {
                    return Err(status_response(StatusCode::PAYLOAD_TOO_LARGE));
                }
            }
        }
    }

    let (parts, body) = req.into_parts();

    let collected = body.collect().await;
    match collected {
        Ok(collected) => {
            let body_bytes = collected.to_bytes();
            if body_bytes.len() as u64 > max_size {
                return Err(status_response(StatusCode::PAYLOAD_TOO_LARGE));
            }
            Ok((parts, body_bytes))
        }
        Err(_) => Err(status_response(StatusCode::BAD_GATEWAY)),
    }
}

async fn send_upstream(
    req: Request<Full<Bytes>>,
    _upstream_base: &str,
) -> Result<Response<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;

    let client: Client<_, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http();

    let response = client.request(req).await?;
    let (parts, body) = response.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    Ok(Response::from_parts(parts, Full::new(body_bytes)))
}

fn status_response(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Length", "0")
        .body(Full::new(Bytes::new()))
        .unwrap()
}