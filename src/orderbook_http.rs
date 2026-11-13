//! Private, read-only loopback transport for the live Order Book V1 publisher.
//!
//! The listener refuses non-loopback sockets, requires a process-local bearer
//! token, reads one immutable publisher snapshot per request, and never caches.

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};

use crate::orderbook_v1::SnapshotPublisher;

pub const LOOPBACK_ADDR: &str = "127.0.0.1:8787";
const ROUTE_PREFIX: &str = "/internal/v1/orderbook/";
const MAX_REQUEST_BYTES: usize = 4 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_millis(750);

/// Bind the only supported production listener address.
pub async fn bind_loopback() -> io::Result<TcpListener> {
    TcpListener::bind(LOOPBACK_ADDR).await
}

/// Serve the internal V1 resource until the task is cancelled.
///
/// The supplied listener is checked again here so callers cannot accidentally
/// expose the endpoint on a wildcard or external interface.
pub async fn serve(
    listener: TcpListener,
    publisher: Arc<SnapshotPublisher>,
    token: String,
) -> io::Result<()> {
    if token.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "T1_ORDERBOOK_HTTP_TOKEN must not be empty",
        ));
    }

    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Order Book V1 HTTP listener must use a loopback address",
        ));
    }

    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, peer) = listener.accept().await?;
        if !peer.ip().is_loopback() {
            drop(stream);
            continue;
        }

        let permit = match permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                tokio::spawn(async move {
                    let _ = write_response(
                        stream,
                        503,
                        "Service Unavailable",
                        &json!({"error":"capacity_unavailable"}),
                    )
                    .await;
                });
                continue;
            }
        };

        let publisher = publisher.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = timeout(
                IO_TIMEOUT,
                handle_connection(stream, publisher, &token),
            )
            .await;
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    publisher: Arc<SnapshotPublisher>,
    expected_token: &str,
) -> io::Result<()> {
    let mut request = vec![0_u8; MAX_REQUEST_BYTES + 1];
    let mut used = 0;

    loop {
        if used == MAX_REQUEST_BYTES {
            return write_response(
                stream,
                413,
                "Payload Too Large",
                &json!({"error":"request_too_large"}),
            )
            .await;
        }

        let count = stream.read(&mut request[used..]).await?;
        if count == 0 {
            return write_response(
                stream,
                400,
                "Bad Request",
                &json!({"error":"incomplete_request"}),
            )
            .await;
        }
        used += count;

        if request[..used].windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let header_end = request[..used]
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP headers"))?;

    if header_end != used {
        return write_response(
            stream,
            400,
            "Bad Request",
            &json!({"error":"request_body_not_supported"}),
        )
        .await;
    }

    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "headers must be UTF-8"))?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let path = request_parts.next().unwrap_or_default();
    let version = request_parts.next().unwrap_or_default();

    if version != "HTTP/1.1" || request_parts.next().is_some() {
        return write_response(
            stream,
            400,
            "Bad Request",
            &json!({"error":"invalid_request_line"}),
        )
        .await;
    }

    let mut authorization = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return write_response(
                stream,
                400,
                "Bad Request",
                &json!({"error":"invalid_header"}),
            )
            .await;
        };
        if name.eq_ignore_ascii_case("content-length")
            && value.trim().parse::<usize>().unwrap_or(usize::MAX) != 0
        {
            return write_response(
                stream,
                400,
                "Bad Request",
                &json!({"error":"request_body_not_supported"}),
            )
            .await;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return write_response(
                stream,
                400,
                "Bad Request",
                &json!({"error":"transfer_encoding_not_supported"}),
            )
            .await;
        }
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim());
        }
    }

    let expected = format!("Bearer {expected_token}");
    if !authorization.is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
    {
        return write_response(
            stream,
            401,
            "Unauthorized",
            &json!({"error":"unauthorized"}),
        )
        .await;
    }

    if method != "GET" {
        return write_response(
            stream,
            405,
            "Method Not Allowed",
            &json!({"error":"method_not_allowed"}),
        )
        .await;
    }

    let Some(symbol) = path.strip_prefix(ROUTE_PREFIX) else {
        return write_response(stream, 404, "Not Found", &json!({"error":"not_found"})).await;
    };
    if symbol.is_empty() || symbol.contains('/') || !symbol.eq_ignore_ascii_case("btcusdt") {
        return write_response(stream, 404, "Not Found", &json!({"error":"not_found"})).await;
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let snapshot = match publisher.read_fresh(now_ms) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return write_response(
                stream,
                503,
                "Service Unavailable",
                &json!({"error":"snapshot_unavailable"}),
            )
            .await;
        }
    };

    if !snapshot.symbol.eq_ignore_ascii_case(symbol) {
        return write_response(
            stream,
            503,
            "Service Unavailable",
            &json!({"error":"snapshot_unavailable"}),
        )
        .await;
    }

    let body = serde_json::to_vec(snapshot.as_ref())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if body.len() > MAX_RESPONSE_BYTES {
        return write_response(
            stream,
            503,
            "Service Unavailable",
            &json!({"error":"snapshot_exceeds_response_limit"}),
        )
        .await;
    }

    write_raw_response(stream, 200, "OK", "application/json", &body).await
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

async fn write_response(
    stream: TcpStream,
    status: u16,
    reason: &str,
    body: &serde_json::Value,
) -> io::Result<()> {
    let body = serde_json::to_vec(body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_raw_response(stream, status, reason, "application/json", &body).await
}

async fn write_raw_response(
    mut stream: TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

#[allow(dead_code)]
fn _loopback_address_type_is_explicit(_: SocketAddr) {}
