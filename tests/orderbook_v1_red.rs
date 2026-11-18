use std::sync::Arc;

use binance_futures_collector::http::orderbook_v1::{
    canonical_snapshot_id, OrderbookDepthV1, OrderbookSnapshotV1, OrderbookV1Publisher,
};

fn candidate(generation: u64, now_ms: i64) -> OrderbookSnapshotV1 {
    let bids = (0..20)
        .map(|index| {
            [
                (10_000 - index).to_string(),
                "0.010000000000000001".to_string(),
            ]
        })
        .collect::<Vec<_>>();
    let asks = (0..20)
        .map(|index| {
            [
                (10_001 + index).to_string(),
                "0.020000000000000003".to_string(),
            ]
        })
        .collect::<Vec<_>>();

    let mut snapshot = OrderbookSnapshotV1 {
        contract_version: "T1_T2_ORDERBOOK_V1".to_string(),
        symbol: "BTCUSDT".to_string(),
        market: "BINANCE_USDM_PERPETUAL".to_string(),
        bids,
        asks,
        snapshot_last_update_id: 900_000,
        first_applied_u: 900_001,
        last_applied_u: 900_010,
        source_timestamp_ms: now_ms,
        observed_at_ms: now_ms,
        decision_watermark: 900_010,
        generation,
        snapshot_id: String::new(),
        continuity: "VERIFIED".to_string(),
        freshness: "FRESH".to_string(),
        depth: OrderbookDepthV1 { bids: 20, asks: 20 },
        complete: true,
    };
    snapshot.snapshot_id = canonical_snapshot_id(&snapshot).expect("canonical identity");
    snapshot
}

#[test]
fn publisher_starts_fail_closed_without_a_snapshot() {
    let publisher = OrderbookV1Publisher::new();
    assert!(publisher.read_fresh("BTCUSDT", 1_800_000_000_000).is_err());
}

#[test]
fn publisher_exposes_only_a_complete_immutable_snapshot() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();
    publisher
        .publish(candidate(generation, now_ms))
        .expect("valid candidate should publish");

    let first = publisher
        .read_fresh("BTCUSDT", now_ms)
        .expect("published snapshot");
    let second = publisher
        .read_fresh("BTCUSDT", now_ms)
        .expect("published snapshot");
    assert!(
        Arc::ptr_eq(&first, &second),
        "readers should share the immutable published Arc"
    );
    assert_eq!(first.decision_watermark, first.last_applied_u);
    assert_eq!(first.bids[0][1], "0.010000000000000001");
    assert_eq!(first.asks[0][1], "0.020000000000000003");
}

#[test]
fn publisher_rejects_noncanonical_symbol_and_market() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut invalid = candidate(generation, now_ms);
    invalid.symbol = "ETHUSDT".to_string();
    invalid.snapshot_id = canonical_snapshot_id(&invalid).unwrap();
    assert!(publisher.publish(invalid).is_err());
}

#[test]
fn publisher_rejects_insufficient_depth_and_inconsistent_depth_metadata() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut invalid = candidate(generation, now_ms);
    invalid.bids.pop();
    invalid.depth.bids = 19;
    invalid.snapshot_id = canonical_snapshot_id(&invalid).unwrap();
    assert!(publisher.publish(invalid).is_err());
}

#[test]
fn publisher_rejects_duplicate_or_unsorted_levels_and_crossed_spread() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut duplicate = candidate(generation, now_ms);
    duplicate.bids[1][0] = duplicate.bids[0][0].clone();
    duplicate.snapshot_id = canonical_snapshot_id(&duplicate).unwrap();
    assert!(publisher.publish(duplicate).is_err());

    let mut crossed = candidate(generation, now_ms);
    crossed.asks[0][0] = "9999".to_string();
    crossed.snapshot_id = canonical_snapshot_id(&crossed).unwrap();
    assert!(publisher.publish(crossed).is_err());
}

#[test]
fn publisher_rejects_sequence_watermark_mismatch_and_unverified_continuity() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut bad_watermark = candidate(generation, now_ms);
    bad_watermark.decision_watermark += 1;
    bad_watermark.snapshot_id = canonical_snapshot_id(&bad_watermark).unwrap();
    assert!(publisher.publish(bad_watermark).is_err());

    let mut unverified = candidate(generation, now_ms);
    unverified.continuity = "UNVERIFIED".to_string();
    unverified.snapshot_id = canonical_snapshot_id(&unverified).unwrap();
    assert!(publisher.publish(unverified).is_err());
}

#[test]
fn invalidation_fences_old_generation_and_removes_readable_snapshot() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let old = candidate(generation, now_ms);
    publisher.publish(old.clone()).expect("initial publish");

    let next_generation = publisher.invalidate();
    assert!(next_generation > generation);
    assert!(publisher.read_fresh("BTCUSDT", now_ms).is_err());
    assert!(
        publisher.publish(old).is_err(),
        "old generation must never republish"
    );
}

#[test]
fn publisher_rejects_stale_and_future_timestamps() {
    let publisher = OrderbookV1Publisher::new();
    let generation = publisher.begin_generation();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let stale = candidate(generation, now_ms - 2_001);
    assert!(publisher.publish(stale).is_err());

    let future = candidate(generation, now_ms + 251);
    assert!(publisher.publish(future).is_err());
}
