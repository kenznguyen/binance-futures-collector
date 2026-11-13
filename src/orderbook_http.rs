//! Contract tests for the private T1 -> T2 Order Book V1 HTTP transport.
//!
//! These tests intentionally precede the implementation (C2.2 TDD RED).

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task::JoinHandle,
    };

    use crate::{
        orderbook_http::serve,
        orderbook_v1::{
            OrderbookDepthV1, OrderbookSnapshotV1, SnapshotPublisher, CONTRACT_VERSION,
        },
    };

    const TOKEN: &str = "test-only-loopback-token";

    async fn ready_publisher() -> Arc<SnapshotPublisher> {
        let publisher = Arc::new(SnapshotPublisher::new());
        let generation = publisher.begin_connection_generation();
        publisher.note_reanchor_attempt().unwrap();
        publisher.mark_validating(generation).unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis();
        publisher
            .publish_if_current(OrderbookSnapshotV1 {
                contract_version: CONTRACT_VERSION.to_string(),
                symbol: "BTCUSDT".to_string(),
                market: "USDT-M-PERPETUAL".to_string(),
                bids: vec![["67234.10".to_string(), "0.012500".to_string()]],
                asks: vec![["67234.20".to_string(), "0.015000".to_string()]],
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
                depth: OrderbookDepthV1 { bids: 1, asks: 1 },
            })
            .unwrap();

        publisher
    }

    async fn start_server(
        publisher: Arc<SnapshotPublisher>,
    ) -> (String, JoinHandle<std::io::Result<()>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(serve(listener, publisher, TOKEN.to_string()));
        (address, task)
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
        assert!(response.to_ascii_lowercase().contains("cache-control: no-store"));
        let body = response.split("\r\n\r\n").nth(1).unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["contractVersion"], CONTRACT_VERSION);
        assert_eq!(json["symbol"], "BTCUSDT");
        assert_eq!(json["bids"][0][0], "67234.10");
        assert_eq!(json["bids"][0][1], "0.012500");
        assert_eq!(json["asks"][0][0], "67234.20");
        assert_eq!(json["depth"]["bids"], 1);
        assert_eq!(json["depth"]["asks"], 1);

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
}
