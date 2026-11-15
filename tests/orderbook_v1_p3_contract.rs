use std::{collections::BTreeSet, io, sync::Arc, time::Duration};

use binance_futures_collector::{
    orderbook_http::{bind_loopback, serve},
    orderbook_v1::{OrderbookDepthV1, OrderbookSnapshotV1, SnapshotPublisher, CONTRACT_VERSION},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const TOKEN: &str = "p3-test-token";

fn candidate(
    generation: u64,
    market: &str,
    bid_count: usize,
    ask_count: usize,
    reported_bids: usize,
    reported_asks: usize,
    source_timestamp_ms: i64,
) -> OrderbookSnapshotV1 {
    let bids = (0..bid_count)
        .map(|index| [(10_000 - index).to_string(), "1.000".to_string()])
        .collect::<Vec<_>>();
    let asks = (0..ask_count)
        .map(|index| [(10_001 + index).to_string(), "2.000".to_string()])
        .collect::<Vec<_>>();

    OrderbookSnapshotV1 {
        contract_version: CONTRACT_VERSION.to_string(),
        symbol: "BTCUSDT".to_string(),
        market: market.to_string(),
        bids,
        asks,
        snapshot_last_update_id: 900_000,
        first_applied_u: 900_001,
        last_applied_u: 900_010,
        decision_watermark: 900_010,
        source_timestamp_ms,
        observed_at_ms: source_timestamp_ms,
        generation,
        snapshot_id: format!("btcusdt-{generation}-900010"),
        continuity: "VERIFIED".to_string(),
        freshness: "FRESH".to_string(),
        complete: true,
        depth: OrderbookDepthV1 {
            bids: reported_bids,
            asks: reported_asks,
        },
    }
}

fn publisher() -> Arc<SnapshotPublisher> {
    Arc::new(SnapshotPublisher::new())
}

fn begin_publish(publisher: &SnapshotPublisher) -> u64 {
    let generation = publisher.begin_connection_generation();
    publisher.note_reanchor_attempt().unwrap();
    publisher.mark_validating(generation).unwrap();
    generation
}

async fn start_server(publisher: Arc<SnapshotPublisher>) -> (String, JoinHandle<io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let task = tokio::spawn(serve(listener, publisher, TOKEN.to_string()));
    (address, task)
}

async fn request(address: &str, token: Option<&str>) -> String {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let authorization = token
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /internal/v1/orderbook/btcusdt HTTP/1.1\r\nHost: localhost\r\n{authorization}Connection: close\r\n\r\n"
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

fn publish_valid(publisher: &SnapshotPublisher, generation: u64) {
    let now = chrono::Utc::now().timestamp_millis();
    publisher
        .publish_if_current(candidate(
            generation,
            "BINANCE_USDM_PERPETUAL",
            20,
            20,
            20,
            20,
            now,
        ))
        .unwrap();
}

#[test]
fn publisher_rejects_noncanonical_market_metadata() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let wrong_market = candidate(generation, "USDT-M-PERPETUAL", 20, 20, 20, 20, now);
    assert!(
        publisher.publish_if_current(wrong_market).is_err(),
        "Publisher must reject a noncanonical V1 market identifier"
    );
}

#[test]
fn publisher_rejects_noncanonical_symbol_metadata() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let mut wrong_symbol =
        candidate(generation, "BINANCE_USDM_PERPETUAL", 20, 20, 20, 20, now);
    wrong_symbol.symbol = "ETHUSDT".to_string();
    assert!(
        publisher.publish_if_current(wrong_symbol).is_err(),
        "BTCUSDT Publisher must reject a different symbol"
    );
}

#[test]
fn publisher_rejects_non_v1_contract_version() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let mut wrong_version =
        candidate(generation, "BINANCE_USDM_PERPETUAL", 20, 20, 20, 20, now);
    wrong_version.contract_version = "T1_T2_ORDERBOOK_V0".to_string();
    assert!(
        publisher.publish_if_current(wrong_version).is_err(),
        "Publisher must reject a contract version other than V1"
    );
}

#[test]
fn publisher_rejects_insufficient_bid_depth() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let shallow = candidate(generation, "BINANCE_USDM_PERPETUAL", 19, 20, 19, 20, now);
    assert!(
        publisher.publish_if_current(shallow).is_err(),
        "Publisher must enforce the 20-level minimum bid depth"
    );
}

#[test]
fn publisher_rejects_insufficient_ask_depth() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let shallow = candidate(generation, "BINANCE_USDM_PERPETUAL", 20, 19, 20, 19, now);
    assert!(
        publisher.publish_if_current(shallow).is_err(),
        "Publisher must enforce the 20-level minimum ask depth"
    );
}

#[test]
fn publisher_rejects_inconsistent_depth_metadata() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let now = chrono::Utc::now().timestamp_millis();

    let inconsistent = candidate(generation, "BINANCE_USDM_PERPETUAL", 20, 20, 19, 20, now);
    assert!(
        publisher.publish_if_current(inconsistent).is_err(),
        "Published depth metadata must equal the actual bid/ask array lengths"
    );
}

#[tokio::test]
async fn authenticated_response_matches_exact_v1_schema_market_and_depth() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    publish_valid(&publisher, generation);
    let (address, server) = start_server(publisher).await;

    let response = request(&address, Some(TOKEN)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let body = response.split("\r\n\r\n").nth(1).unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();

    let actual = json
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected = [
        "contractVersion",
        "symbol",
        "market",
        "bids",
        "asks",
        "snapshotLastUpdateId",
        "firstAppliedU",
        "lastAppliedU",
        "decisionWatermark",
        "sourceTimestampMs",
        "observedAtMs",
        "generation",
        "snapshotId",
        "continuity",
        "freshness",
        "complete",
        "depth",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert_eq!(json["contractVersion"], "T1_T2_ORDERBOOK_V1");
    assert_eq!(json["symbol"], "BTCUSDT");
    assert_eq!(json["market"], "BINANCE_USDM_PERPETUAL");
    assert_eq!(json["bids"].as_array().unwrap().len(), 20);
    assert_eq!(json["asks"].as_array().unwrap().len(), 20);
    assert_eq!(json["depth"]["bids"], 20);
    assert_eq!(json["depth"]["asks"], 20);
    assert!(json["bids"][0][0].is_string());
    assert!(json["bids"][0][1].is_string());
    assert_eq!(json["bids"][0][1], "1.000");

    server.abort();
}

#[tokio::test]
async fn missing_bearer_token_is_rejected_without_snapshot_disclosure() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    publish_valid(&publisher, generation);
    let (address, server) = start_server(publisher).await;

    let response = request(&address, None).await;
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(!response.contains("10000"));

    server.abort();
}

#[tokio::test]
async fn incorrect_bearer_token_is_rejected_without_snapshot_disclosure() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    publish_valid(&publisher, generation);
    let (address, server) = start_server(publisher).await;

    let response = request(&address, Some("incorrect-token")).await;
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(!response.contains("10000"));

    server.abort();
}

#[tokio::test]
async fn production_bind_helper_uses_only_the_documented_loopback_socket() {
    let listener = bind_loopback().await.unwrap();
    let address = listener.local_addr().unwrap();
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_eq!(address.port(), 8787);
}

#[tokio::test]
async fn stale_published_snapshot_is_never_returned_by_http() {
    let publisher = publisher();
    let generation = begin_publish(&publisher);
    let stale = chrono::Utc::now().timestamp_millis() - 2_001;
    publisher
        .publish_if_current(candidate(
            generation,
            "BINANCE_USDM_PERPETUAL",
            20,
            20,
            20,
            20,
            stale,
        ))
        .unwrap();
    let (address, server) = start_server(publisher).await;

    let response = request(&address, Some(TOKEN)).await;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(!response.contains("10000"));

    server.abort();
}
