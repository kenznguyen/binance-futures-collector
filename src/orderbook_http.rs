//! Private, read-only loopback transport for the live Order Book V1 publisher.
//!
//! The listener refuses non-loopback sockets, requires a process-local bearer
//! token, reads one immutable publisher snapshot per request, and never caches.

use std::{io, sync::Arc, time::Duration};

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
const MAX_BEARER_TOKEN_BYTES: usize = MAX_REQUEST_BYTES - "Bearer ".len();
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 8;
const MAX_REJECTED_CONNECTIONS: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_millis(750);

/// Bounded response buffer used to serialize snapshots without an unbounded
/// intermediate allocation.
struct BoundedResponseWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl std::io::Write for BoundedResponseWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // RED scaffold: the regression test below must reject this unbounded
        // append before the writer is wired into the HTTP response path.
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
    if token.is_empty() || token.len() > MAX_BEARER_TOKEN_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "T1_ORDERBOOK_HTTP_TOKEN must be non-empty and fit within the 4 KiB request limit",
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
    let rejection_permits = Arc::new(Semaphore::new(MAX_REJECTED_CONNECTIONS));
    loop {
        let (stream, peer) = listener.accept().await?;
        if !peer.ip().is_loopback() {
            drop(stream);
            continue;
        }

        let permit = match permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let rejection_permit = match rejection_permits.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        drop(stream);
                        continue;
                    }
                };
                tokio::spawn(async move {
                    let _permit = rejection_permit;
                    let _ = timeout(
                        IO_TIMEOUT,
                        write_response(
                            stream,
                            503,
                            "Service Unavailable",
                            &json!({"error":"capacity_unavailable"}),
                        ),
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
            let _ = timeout(IO_TIMEOUT, handle_connection(stream, publisher, &token)).await;
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
        if used > MAX_REQUEST_BYTES {
            return write_response(
                stream,
                413,
                "Payload Too Large",
                &json!({"error":"request_too_large"}),
            )
            .await;
        }

        if request[..used]
            .windows(4)
            .any(|window| window == b"\r\n\r\n")
        {
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
            if authorization.is_some() {
                return write_response(
                    stream,
                    400,
                    "Bad Request",
                    &json!({"error":"duplicate_authorization"}),
                )
                .await;
            }
            authorization = Some(value.trim());
        }
    }

    let expected = format!("Bearer {expected_token}");
    if !authorization.is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes())) {
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
    // Both values are bounded by the 4 KiB HTTP request/token limits. Pad to a
    // fixed width and always traverse the full bound; length mismatch is folded
    // into the accumulated difference rather than returning early.
    let mut left_padded = [0_u8; MAX_REQUEST_BYTES];
    let mut right_padded = [0_u8; MAX_REQUEST_BYTES];
    let left_len = left.len().min(MAX_REQUEST_BYTES);
    let right_len = right.len().min(MAX_REQUEST_BYTES);
    left_padded[..left_len].copy_from_slice(&left[..left_len]);
    right_padded[..right_len].copy_from_slice(&right[..right_len]);

    let mut difference = (left.len() != right.len()) as u8;
    for index in 0..MAX_REQUEST_BYTES {
        difference |= left_padded[index] ^ right_padded[index];
    }
    difference == 0
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

#[cfg(test)]
mod tests {
    use std::{io, sync::Arc, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task::JoinHandle,
    };

    use super::{constant_time_eq, serve, BoundedResponseWriter};
    use crate::orderbook_v1::{
        canonical_snapshot_id, OrderbookDepthV1, OrderbookSnapshotV1, SnapshotPublisher,
        CONTRACT_VERSION,
    };

    const TOKEN: &str = "test-only-loopback-token";

    async fn ready_publisher() -> Arc<SnapshotPublisher> {
        let publisher = Arc::new(SnapshotPublisher::new());
        let generation = publisher.begin_connection_generation();
        publisher.note_reanchor_attempt().unwrap();
        publisher.mark_validating(generation).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut snapshot = OrderbookSnapshotV1 {
            contract_version: CONTRACT_VERSION.to_string(),
            symbol: "BTCUSDT".to_string(),
            market: "BINANCE_USDM_PERPETUAL".to_string(),
            bids: (0..20)
                .map(|index| {
                    [
                        format!("{:.2}", 67234.10 - index as f64),
                        "0.012500".to_string(),
                    ]
                })
                .collect(),
            asks: (0..20)
                .map(|index| {
                    [
                        format!("{:.2}", 67234.20 + index as f64),
                        "0.015000".to_string(),
                    ]
                })
                .collect(),
            snapshot_last_update_id: 900_000,
            first_applied_u: 900_001,
            last_applied_u: 900_010,
            decision_watermark: 900_010,
            source_timestamp_ms: now_ms,
            observed_at_ms: now_ms,
            generation,
            snapshot_id: "btc-900010-g1".to_string(),
            continuity: "VERIFIED".to_string(),
            freshness: "FRESH".to_string(),
            complete: true,
            depth: OrderbookDepthV1 { bids: 20, asks: 20 },
        };
        snapshot.snapshot_id = canonical_snapshot_id(&snapshot).unwrap();
        publisher.publish_if_current(snapshot).unwrap();

        publisher
    }

    async fn start_server(
        publisher: Arc<SnapshotPublisher>,
    ) -> (String, JoinHandle<io::Result<()>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(serve(listener, publisher, TOKEN.to_string()));
        (address, task)
    }

    async fn raw_request(address: &str, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(response).unwrap()
    }

    async fn request(address: &str, method: &str, path: &str, token: Option<&str>) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let authorization = token
            .map(|value| format!("Authorization: Bearer {value}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{authorization}Connection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(response).unwrap()
    }

    #[tokio::test]
    async fn ready_snapshot_is_returned_as_one_exact_v1_document_without_numeric_coercion() {
        let publisher = ready_publisher().await;
        let (address, server) = start_server(publisher).await;
        let response = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response
            .to_ascii_lowercase()
            .contains("content-type: application/json"));
        assert!(response
            .to_ascii_lowercase()
            .contains("cache-control: no-store"));
        assert!(response
            .to_ascii_lowercase()
            .contains("x-content-type-options: nosniff"));
        assert!(response.to_ascii_lowercase().contains("connection: close"));
        let body = response.split("\r\n\r\n").nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["contractVersion"], CONTRACT_VERSION);
        assert_eq!(json["symbol"], "BTCUSDT");
        assert_eq!(json["market"], "BINANCE_USDM_PERPETUAL");
        assert_eq!(json["bids"].as_array().unwrap().len(), 20);
        assert_eq!(json["asks"].as_array().unwrap().len(), 20);
        assert_eq!(json["bids"][0][0], "67234.10");
        assert_eq!(json["bids"][0][1], "0.012500");
        assert_eq!(json["asks"][0][0], "67234.20");
        assert_eq!(json["depth"]["bids"], 20);
        assert_eq!(json["depth"]["asks"], 20);

        server.abort();
    }

    #[tokio::test]
    async fn missing_or_invalid_local_token_is_rejected_without_snapshot_disclosure() {
        let publisher = ready_publisher().await;
        let (address, server) = start_server(publisher).await;
        let response = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some("wrong-token"),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
        assert!(!response.contains("67234.10"));
        server.abort();
    }

    #[tokio::test]
    async fn duplicate_authorization_headers_are_rejected_regardless_of_order() {
        let (address, server) = start_server(ready_publisher().await).await;
        let requests = [
            format!(
                "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer wrong-token\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n"
            ),
            format!(
                "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nAuthorization: Bearer wrong-token\r\nConnection: close\r\n\r\n"
            ),
        ];

        for request in requests {
            let response = raw_request(&address, request.as_bytes()).await;
            assert!(response.starts_with("HTTP/1.1 400"), "{response}");
            assert!(!response.contains("67234.10"));
        }

        server.abort();
    }

    #[test]
    fn bearer_comparison_rejects_content_and_length_mismatches() {
        assert!(constant_time_eq(b"Bearer token", b"Bearer token"));
        assert!(!constant_time_eq(b"Bearer token", b"Bearer tokenx"));
        assert!(!constant_time_eq(b"Bearer token", b"Bearer tokez"));
    }

    #[tokio::test]
    async fn serve_rejects_empty_and_overlong_bearer_tokens_before_accepting() {
        for token in [String::new(), "x".repeat(4090)] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(100),
                serve(listener, Arc::new(SnapshotPublisher::new()), token),
            )
            .await;
            assert!(
                matches!(result, Ok(Err(error)) if error.kind() == io::ErrorKind::InvalidInput),
                "empty/overlong token must fail startup immediately"
            );
        }
    }

    #[tokio::test]
    async fn serve_rejects_a_non_loopback_listener() {
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let result = serve(
            listener,
            Arc::new(SnapshotPublisher::new()),
            TOKEN.to_string(),
        )
        .await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn oversized_request_headers_return_413() {
        let (address, server) = start_server(ready_publisher().await).await;
        let request = format!(
            "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nX-Fill: {}\r\n\r\n",
            "x".repeat(5000)
        );
        let response = raw_request(&address, request.as_bytes()).await;
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        server.abort();
    }

    #[tokio::test]
    async fn request_bodies_and_transfer_encoding_are_rejected() {
        let (address, server) = start_server(ready_publisher().await).await;
        let body = format!(
            "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 1\r\n\r\nx"
        );
        let body_response = raw_request(&address, body.as_bytes()).await;
        let chunked = format!(
            "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nTransfer-Encoding: chunked\r\n\r\n"
        );
        let chunked_response = raw_request(&address, chunked.as_bytes()).await;
        assert!(body_response.starts_with("HTTP/1.1 400"), "{body_response}");
        assert!(
            chunked_response.starts_with("HTTP/1.1 400"),
            "{chunked_response}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn io_deadline_closes_a_client_that_never_finishes_headers() {
        let (address, server) = start_server(ready_publisher().await).await;
        let mut stream = TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(b"GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(
            response.is_empty(),
            "timed-out request must not receive data"
        );
        server.abort();
    }

    #[tokio::test]
    async fn excess_concurrent_clients_receive_bounded_capacity_rejection() {
        let (address, server) = start_server(ready_publisher().await).await;
        let mut held = Vec::new();
        for _ in 0..8 {
            let mut stream = TcpStream::connect(&address).await.unwrap();
            stream
                .write_all(b"GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\n")
                .await
                .unwrap();
            held.push(stream);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut excess = TcpStream::connect(&address).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), excess.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert!(response.contains("capacity_unavailable"), "{response}");
        drop(held);
        server.abort();
    }

    #[test]
    fn bounded_response_writer_never_retains_more_than_its_limit() {
        let mut writer = BoundedResponseWriter {
            bytes: Vec::new(),
            limit: 4,
            exceeded: false,
        };

        let result = std::io::Write::write_all(&mut writer, b"12345");
        assert!(result.is_err(), "writer must reject bytes beyond its limit");
        assert!(writer.bytes.len() <= writer.limit);
        assert!(writer.exceeded);
    }

    #[tokio::test]
    async fn oversized_snapshot_is_not_truncated_or_served() {
        let publisher = ready_publisher().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut snapshot = publisher.read_fresh(now_ms).unwrap().as_ref().clone();
        let generation = publisher.begin_connection_generation();
        publisher.note_reanchor_attempt().unwrap();
        publisher.mark_validating(generation).unwrap();
        snapshot.generation = generation;
        snapshot.source_timestamp_ms = now_ms;
        snapshot.observed_at_ms = now_ms;
        snapshot.bids = (0..300)
            .map(|index| [format!("{:080}", 500_000 - index), "1".repeat(80)])
            .collect();
        snapshot.asks = (0..300)
            .map(|index| [format!("{:080}", 500_001 + index), "1".repeat(80)])
            .collect();
        snapshot.depth.bids = snapshot.bids.len();
        snapshot.depth.asks = snapshot.asks.len();
        snapshot.snapshot_id = crate::orderbook_v1::canonical_snapshot_id(&snapshot).unwrap();
        publisher.publish_if_current(snapshot).unwrap();

        let (address, server) = start_server(publisher).await;
        let response = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert!(
            response.contains("snapshot_exceeds_response_limit"),
            "{response}"
        );
        assert!(!response.contains("500000"));
        server.abort();
    }

    #[tokio::test]
    async fn broken_publisher_never_serves_a_cached_last_good_snapshot() {
        let publisher = ready_publisher().await;
        publisher.mark_broken();
        let (address, server) = start_server(publisher).await;
        let response = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert!(!response.contains("67234.10"));
        server.abort();
    }

    #[tokio::test]
    async fn only_get_for_the_published_symbol_is_accepted() {
        let publisher = ready_publisher().await;
        let (address, server) = start_server(publisher).await;
        let wrong_symbol = request(
            &address,
            "GET",
            "/internal/v1/orderbook/ethusdt",
            Some(TOKEN),
        )
        .await;
        let wrong_method = request(
            &address,
            "POST",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;

        assert!(wrong_symbol.starts_with("HTTP/1.1 404"), "{wrong_symbol}");
        assert!(wrong_method.starts_with("HTTP/1.1 405"), "{wrong_method}");
        server.abort();
    }

    #[tokio::test]
    async fn no_snapshot_and_stopped_publishers_fail_closed_over_http() {
        let publisher = Arc::new(SnapshotPublisher::new());
        let (address, server) = start_server(publisher).await;
        let empty = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(empty.starts_with("HTTP/1.1 503"), "{empty}");
        assert!(empty.contains("snapshot_unavailable"), "{empty}");
        assert!(!empty.contains("67234.10"));
        server.abort();

        let publisher = ready_publisher().await;
        publisher.mark_stopped();
        let (address, server) = start_server(publisher).await;
        let stopped = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(stopped.starts_with("HTTP/1.1 503"), "{stopped}");
        assert!(stopped.contains("snapshot_unavailable"), "{stopped}");
        assert!(!stopped.contains("67234.10"));
        server.abort();
    }

    #[tokio::test]
    async fn reanchoring_fetching_or_validating_publisher_never_serves_cached_snapshot() {
        let publisher = ready_publisher().await;
        publisher.begin_reanchor();
        let (address, server) = start_server(publisher.clone()).await;
        let reanchoring = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(reanchoring.starts_with("HTTP/1.1 503"), "{reanchoring}");
        assert!(
            reanchoring.contains("snapshot_unavailable"),
            "{reanchoring}"
        );
        assert!(!reanchoring.contains("67234.10"));
        server.abort();

        publisher.note_reanchor_attempt().unwrap();
        let (address, server) = start_server(publisher.clone()).await;
        let fetching = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(fetching.starts_with("HTTP/1.1 503"), "{fetching}");
        assert!(fetching.contains("snapshot_unavailable"), "{fetching}");
        assert!(!fetching.contains("67234.10"));
        server.abort();

        publisher.mark_validating(publisher.generation()).unwrap();
        let (address, server) = start_server(publisher).await;
        let validating = request(
            &address,
            "GET",
            "/internal/v1/orderbook/btcusdt",
            Some(TOKEN),
        )
        .await;
        assert!(validating.starts_with("HTTP/1.1 503"), "{validating}");
        assert!(validating.contains("snapshot_unavailable"), "{validating}");
        assert!(!validating.contains("67234.10"));
        server.abort();
    }
}
