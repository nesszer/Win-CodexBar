//! Local HTTP server for scriptable usage/cost JSON.

use clap::Args;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::usage::ProviderSelection;
use crate::core::{FetchContext, ProviderId, SourceMode, instantiate_provider};
use crate::cost_scanner::CostScanner;

#[derive(Args, Debug, Clone)]
pub struct ServeArgs {
    /// Local HTTP port
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Response cache TTL in seconds
    #[arg(long = "refresh-interval", default_value = "60")]
    pub refresh_interval: u64,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", args.port)).await?;
    eprintln!(
        "CodexBar server listening on http://127.0.0.1:{}",
        args.port
    );

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream).await {
                tracing::debug!("serve client error: {error}");
            }
        });
    }
}

async fn handle_client(mut stream: TcpStream) -> anyhow::Result<()> {
    let mut buffer = vec![0_u8; 8192];
    let n = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let response = match parse_request(&request) {
        Ok(request) => route_request(&request).await,
        Err(status) => json_response(status, serde_json::json!({ "error": "bad request" })),
    };
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn route_request(request: &ServeRequest) -> String {
    if request.method != "GET" {
        return json_response(405, serde_json::json!({ "error": "method not allowed" }));
    }
    if !allowed_host(&request.host) {
        return json_response(403, serde_json::json!({ "error": "forbidden host" }));
    }

    match request.path.as_str() {
        "/health" => json_response(
            200,
            serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }),
        ),
        "/usage" => usage_response(request.query.get("provider").map(String::as_str)).await,
        "/cost" => cost_response(request.query.get("provider").map(String::as_str)).await,
        _ => json_response(404, serde_json::json!({ "error": "not found" })),
    }
}

async fn usage_response(provider: Option<&str>) -> String {
    let selection = match ProviderSelection::from_arg(provider) {
        Ok(selection) => selection,
        Err(error) => {
            return json_response(400, serde_json::json!({ "error": error.to_string() }));
        }
    };
    let ctx = FetchContext {
        source_mode: SourceMode::Auto,
        include_credits: true,
        web_timeout: 60,
        verbose: false,
        manual_cookie_header: None,
        api_key: None,
        workspace_id: None,
        api_region: None,
    };

    let mut results = Vec::new();
    for provider_id in selection.as_list() {
        let provider = instantiate_provider(provider_id);
        match provider.fetch_usage(&ctx).await {
            Ok(result) => results.push(serde_json::json!({
                "provider": provider_id.cli_name(),
                "source": result.source_label,
                "usage": result.usage,
                "cost": result.cost,
            })),
            Err(error) => results.push(serde_json::json!({
                "provider": provider_id.cli_name(),
                "error": error.to_string(),
            })),
        }
    }
    json_response(200, serde_json::Value::Array(results))
}

async fn cost_response(provider: Option<&str>) -> String {
    let selection = match ProviderSelection::from_arg(provider) {
        Ok(selection) => selection,
        Err(error) => {
            return json_response(400, serde_json::json!({ "error": error.to_string() }));
        }
    };
    let scanner = CostScanner::new(30);
    let mut results = Vec::new();
    for provider_id in selection.as_list() {
        let (supported, summary) = match provider_id {
            ProviderId::Codex => (true, scanner.scan_codex()),
            ProviderId::Claude => (true, scanner.scan_claude()),
            _ => (false, Default::default()),
        };
        if supported {
            results.push(serde_json::json!({
                "provider": provider_id.cli_name(),
                "supported": true,
                "days_scanned": 30,
                "cost": {
                    "total_usd": summary.total_cost_usd,
                    "currency": "USD"
                },
                "tokens": {
                    "input": summary.input_tokens,
                    "output": summary.output_tokens,
                    "cached": summary.cached_tokens
                },
                "sessions_count": summary.sessions_count,
                "by_model": summary.by_model,
            }));
        } else {
            results.push(serde_json::json!({
                "provider": provider_id.cli_name(),
                "supported": false,
                "error": "Local cost scanning not available for this provider"
            }));
        }
    }
    json_response(200, serde_json::Value::Array(results))
}

#[derive(Debug)]
struct ServeRequest {
    method: String,
    path: String,
    host: String,
    query: std::collections::HashMap<String, String>,
}

fn parse_request(raw: &str) -> Result<ServeRequest, u16> {
    let mut lines = raw.split("\r\n");
    let first = lines.next().ok_or(400_u16)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().ok_or(400_u16)?.to_uppercase();
    let target = parts.next().ok_or(400_u16)?;
    if parts.next().is_none() || !target.starts_with('/') {
        return Err(400);
    }

    let mut hosts = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(400);
        };
        if name.trim().eq_ignore_ascii_case("host") {
            hosts.push(value.trim().to_string());
        }
    }
    if hosts.len() != 1 {
        return Err(400);
    }

    let (path, query) = parse_target(target);
    Ok(ServeRequest {
        method,
        path,
        host: hosts.remove(0),
        query,
    })
}

fn parse_target(target: &str) -> (String, std::collections::HashMap<String, String>) {
    let Some((path, query_string)) = target.split_once('?') else {
        return (target.to_string(), Default::default());
    };
    let query = query_string
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((url_decode(key), url_decode(value)))
        })
        .collect();
    (path.to_string(), query)
}

fn allowed_host(host: &str) -> bool {
    let trimmed = host.trim();
    if trimmed.is_empty() || trimmed.contains(',') {
        return false;
    }
    let without_port = if let Some(rest) = trimmed.strip_prefix('[') {
        let Some((addr, port)) = rest.split_once(']') else {
            return false;
        };
        if !port.is_empty() && !valid_port_suffix(port) {
            return false;
        }
        format!("[{addr}]")
    } else {
        let segments: Vec<_> = trimmed.split(':').collect();
        match segments.as_slice() {
            [host] => host.to_string(),
            [host, port] if valid_port(port) => host.to_string(),
            _ => return false,
        }
    };
    matches!(
        without_port.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "localhost." | "[::1]"
    )
}

fn valid_port_suffix(raw: &str) -> bool {
    raw.is_empty() || raw.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(raw: &str) -> bool {
    raw.parse::<u16>().is_ok_and(|port| port > 0)
}

fn url_decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut bytes = raw.as_bytes().iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'+' {
            out.push(' ');
        } else if byte == b'%' {
            let hi = bytes.next();
            let lo = bytes.next();
            if let (Some(hi), Some(lo)) = (hi, lo)
                && let Ok(value) =
                    u8::from_str_radix(std::str::from_utf8(&[hi, lo]).unwrap_or_default(), 16)
            {
                out.push(value as char);
            }
        } else {
            out.push(byte as char);
        }
    }
    out
}

fn json_response(status: u16, payload: serde_json::Value) -> String {
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_hosts() {
        assert!(allowed_host("127.0.0.1:8080"));
        assert!(allowed_host("localhost"));
        assert!(allowed_host("[::1]:8080"));
        assert!(!allowed_host("example.com"));
        assert!(!allowed_host("127.0.0.1, example.com"));
    }

    #[test]
    fn parses_usage_route_provider_query() {
        let request =
            parse_request("GET /usage?provider=deepseek HTTP/1.1\r\nHost: localhost:8080\r\n\r\n")
                .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/usage");
        assert_eq!(request.query.get("provider"), Some(&"deepseek".to_string()));
    }
}
