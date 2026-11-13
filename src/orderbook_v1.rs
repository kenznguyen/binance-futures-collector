use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::models::{DepthSnapshotRest, DepthUpdateWs};
use crate::utils::{compare_normalized_decimals, validate_orderbook_level};

pub const CONTRACT_VERSION: &str = "T1_BTCUSDT_ORDERBOOK_V1";
pub const REQUIRED_DEPTH: usize = 20;
pub const MAX_BOOK_AGE_MS: i64 = 2_000;
pub const MAX_FUTURE_SKEW_MS: i64 = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveDiffRejection {
    Freshness(String),
    Sequence(String),
    Shape(String),
}

impl std::fmt::Display for LiveDiffRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Freshness(detail) => write!(formatter, "freshness rejected: {detail}"),
            Self::Sequence(detail) => write!(formatter, "sequence rejected: {detail}"),
            Self::Shape(detail) => write!(formatter, "shape rejected: {detail}"),
        }
    }
}

impl std::error::Error for LiveDiffRejection {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderbookV1State {
    Initializing,
    Buffering,
    FetchingSnapshot,
    Reanchoring,
    Validating,
    Ready,
    Broken,
    Stopped,
}

pub type DepthLevel = [String; 2];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderbookSnapshotV1 {
    pub contract_version: String,
    pub symbol: String,
    pub market: String,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
    pub snapshot_last_update_id: u64,
    pub first_applied_u: u64,
    pub last_applied_u: u64,
    pub decision_watermark: u64,
    pub source_timestamp_ms: i64,
    pub observed_at_ms: i64,
    pub generation: u64,
    pub snapshot_id: String,
    pub continuity: String,
    pub freshness: String,
    pub complete: bool,
    pub bid_depth: usize,
    pub ask_depth: usize,
}

struct PublicationState {
    generation: u64,
    state: OrderbookV1State,
    reanchor_attempts: u8,
    snapshot: Option<Arc<OrderbookSnapshotV1>>,
}

/// A single lock protects generation changes, invalidation and immutable publication.
/// Readers retain an Arc to a complete snapshot; no partially built candidate is exposed.
pub struct SnapshotPublisher {
    inner: RwLock<PublicationState>,
}

impl Default for SnapshotPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotPublisher {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(PublicationState {
                generation: 0,
                state: OrderbookV1State::Initializing,
                reanchor_attempts: 0,
                snapshot: None,
            }),
        }
    }

    pub fn begin_connection_generation(&self) -> u64 {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        inner.generation = inner.generation.saturating_add(1);
        inner.reanchor_attempts = 0;
        inner.snapshot = None;
        inner.state = OrderbookV1State::Buffering;
        inner.generation
    }

    pub fn mark_validating(&self, generation: u64) -> Result<()> {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        if inner.generation != generation || inner.state != OrderbookV1State::FetchingSnapshot {
            bail!("stale BTCUSDT anchor cannot enter validation");
        }
        inner.state = OrderbookV1State::Validating;
        Ok(())
    }

    pub fn note_reanchor_attempt(&self) -> Result<u8> {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        if inner.reanchor_attempts >= 3 {
            inner.generation = inner.generation.saturating_add(1);
            inner.snapshot = None;
            inner.state = OrderbookV1State::Broken;
            bail!("BTCUSDT V1 exhausted three re-anchor attempts");
        }
        inner.reanchor_attempts += 1;
        inner.state = OrderbookV1State::FetchingSnapshot;
        Ok(inner.reanchor_attempts)
    }

    pub fn begin_reanchor(&self) -> u64 {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        inner.generation = inner.generation.saturating_add(1);
        inner.snapshot = None;
        inner.state = OrderbookV1State::Reanchoring;
        inner.generation
    }

    pub fn mark_broken(&self) {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        inner.generation = inner.generation.saturating_add(1);
        inner.snapshot = None;
        inner.state = OrderbookV1State::Broken;
    }

    pub fn mark_stopped(&self) {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        inner.generation = inner.generation.saturating_add(1);
        inner.snapshot = None;
        inner.state = OrderbookV1State::Stopped;
    }

    pub fn publish_if_current(&self, snapshot: OrderbookSnapshotV1) -> Result<()> {
        let mut inner = self
            .inner
            .write()
            .expect("snapshot publisher lock poisoned");
        if snapshot.generation != inner.generation {
            bail!("stale order-book generation");
        }
        if !matches!(
            inner.state,
            OrderbookV1State::FetchingSnapshot
                | OrderbookV1State::Validating
                | OrderbookV1State::Ready
        ) {
            bail!("order-book publisher is not in a publishable lifecycle state");
        }
        if !snapshot.complete || snapshot.continuity != "VERIFIED" || snapshot.freshness != "FRESH"
        {
            bail!("refusing to publish an incomplete or unverified snapshot");
        }
        inner.snapshot = Some(Arc::new(snapshot));
        inner.reanchor_attempts = 0;
        inner.state = OrderbookV1State::Ready;
        Ok(())
    }

    pub fn read_fresh(&self, decision_time_ms: i64) -> Result<Arc<OrderbookSnapshotV1>> {
        let snapshot = self
            .inner
            .read()
            .expect("snapshot publisher lock poisoned")
            .snapshot
            .clone()
            .context("BTCUSDT V1 snapshot is unavailable")?;
        validate_timestamp(snapshot.source_timestamp_ms, decision_time_ms, "source")?;
        validate_timestamp(snapshot.observed_at_ms, decision_time_ms, "observation")?;
        Ok(snapshot)
    }

    pub fn read(&self) -> Option<Arc<OrderbookSnapshotV1>> {
        self.inner
            .read()
            .expect("snapshot publisher lock poisoned")
            .snapshot
            .clone()
    }

    pub fn state(&self) -> OrderbookV1State {
        self.inner
            .read()
            .expect("snapshot publisher lock poisoned")
            .state
    }

    pub fn generation(&self) -> u64 {
        self.inner
            .read()
            .expect("snapshot publisher lock poisoned")
            .generation
    }
}

type Levels = HashMap<String, DepthLevel>;

fn insert_levels(
    target: &mut Levels,
    raw_levels: &[[String; 2]],
    side: &str,
    reject_duplicates: bool,
) -> Result<()> {
    let mut seen = HashSet::new();
    for [price, quantity] in raw_levels {
        let (price_key, quantity_key) = validate_orderbook_level(price, quantity)
            .with_context(|| format!("invalid {side} level"))?;
        if !seen.insert(price_key.clone()) && reject_duplicates {
            bail!("duplicate normalized {side} price level");
        }
        if quantity_key == "0" {
            target.remove(&price_key);
        } else {
            target.insert(price_key, [price.clone(), quantity.clone()]);
        }
    }
    Ok(())
}

fn validate_snapshot_order(raw_levels: &[[String; 2]], side: &str, descending: bool) -> Result<()> {
    let mut previous: Option<String> = None;
    for [price, quantity] in raw_levels {
        let (key, _) = validate_orderbook_level(price, quantity)?;
        if let Some(previous_key) = previous.as_ref() {
            let ordering = compare_normalized_decimals(previous_key, &key);
            let valid = if descending {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            };
            if !valid {
                bail!("{side} snapshot levels must be strictly price-sorted");
            }
        }
        previous = Some(key.to_owned());
    }
    Ok(())
}

fn trim_levels(levels: &mut Levels, depth_limit: usize, highest_first: bool) {
    if levels.len() <= depth_limit {
        return;
    }
    let mut keys: Vec<String> = levels.keys().cloned().collect();
    keys.sort_by(|left, right| {
        let order = compare_normalized_decimals(left, right);
        if highest_first {
            order.reverse()
        } else {
            order
        }
    });
    for key in keys.into_iter().skip(depth_limit) {
        levels.remove(&key);
    }
}

fn sorted_levels(levels: Levels, descending: bool) -> Vec<DepthLevel> {
    let mut result: Vec<(String, DepthLevel)> = levels.into_iter().collect();
    result.sort_by(|left, right| {
        let order = compare_normalized_decimals(&left.0, &right.0);
        if descending {
            order.reverse()
        } else {
            order
        }
    });
    result.into_iter().map(|(_, level)| level).collect()
}

/// Build a fully validated BTCUSDT V1 snapshot from one REST anchor and its ordered WS diffs.
/// The caller must supply events in receive order; this function never sorts or repairs a chain.
pub fn build_snapshot(
    snapshot: &DepthSnapshotRest,
    updates: &[DepthUpdateWs],
    generation: u64,
    observed_at_ms: i64,
    decision_time_ms: i64,
) -> Result<OrderbookSnapshotV1> {
    if updates.is_empty() {
        bail!("a REST anchor without an exchange-timestamped WS bridge is not publishable");
    }

    validate_snapshot_order(&snapshot.bids, "bid", true)?;
    validate_snapshot_order(&snapshot.asks, "ask", false)?;

    let mut bids = HashMap::new();
    let mut asks = HashMap::new();
    insert_levels(&mut bids, &snapshot.bids, "bid", true)?;
    insert_levels(&mut asks, &snapshot.asks, "ask", true)?;

    let next_update_id = snapshot
        .last_update_id
        .checked_add(1)
        .context("snapshot update id overflow")?;
    let mut bridge_index = None;
    for (index, update) in updates.iter().enumerate() {
        validate_update_header(update)?;
        if update.final_update_id <= snapshot.last_update_id {
            continue;
        }
        if update.first_update_id <= next_update_id && next_update_id <= update.final_update_id {
            bridge_index = Some(index);
            break;
        }
        bail!("buffered diff chain has no exact V1 snapshot bridge");
    }
    let bridge_index = bridge_index.context("no buffered diff bridges the REST snapshot")?;

    let mut previous_u = snapshot.last_update_id;
    let mut first_applied_u = 0;
    let mut last_event_timestamp_ms = 0;

    for (index, update) in updates.iter().enumerate().skip(bridge_index) {
        validate_update_header(update)?;
        if index == bridge_index {
            if !(update.first_update_id <= next_update_id
                && next_update_id <= update.final_update_id)
            {
                bail!("first applied diff does not satisfy the exact V1 bridge predicate");
            }
            first_applied_u = update.first_update_id;
        } else if update.prev_update_id != previous_u || update.final_update_id <= previous_u {
            bail!("WS diff sequence is discontinuous or regressing");
        }

        insert_levels(&mut bids, &update.bids, "bid", true)?;
        insert_levels(&mut asks, &update.asks, "ask", true)?;
        previous_u = update.final_update_id;
        last_event_timestamp_ms = update.event_time;
    }

    if first_applied_u == 0 || last_event_timestamp_ms <= 0 {
        bail!("missing first-applied watermark or exchange event timestamp");
    }

    if bids.len() < REQUIRED_DEPTH || asks.len() < REQUIRED_DEPTH {
        bail!("candidate book does not meet the minimum depth on both sides");
    }

    let best_bid = bids
        .keys()
        .max_by(|left, right| compare_normalized_decimals(left, right))
        .context("empty bid side")?;
    let best_ask = asks
        .keys()
        .min_by(|left, right| compare_normalized_decimals(left, right))
        .context("empty ask side")?;
    if compare_normalized_decimals(best_bid, best_ask).is_ge() {
        bail!("candidate order book is locked or crossed");
    }

    validate_timestamp(last_event_timestamp_ms, decision_time_ms, "source")?;
    validate_timestamp(observed_at_ms, decision_time_ms, "observation")?;

    let bid_levels = sorted_levels(bids, true);
    let ask_levels = sorted_levels(asks, false);
    let last_applied_u = previous_u;

    Ok(OrderbookSnapshotV1 {
        contract_version: CONTRACT_VERSION.to_string(),
        symbol: "BTCUSDT".to_string(),
        market: "BINANCE_USDM_PERPETUAL".to_string(),
        bid_depth: bid_levels.len(),
        ask_depth: ask_levels.len(),
        bids: bid_levels,
        asks: ask_levels,
        snapshot_last_update_id: snapshot.last_update_id,
        first_applied_u,
        last_applied_u,
        decision_watermark: last_applied_u,
        source_timestamp_ms: last_event_timestamp_ms,
        observed_at_ms,
        generation,
        snapshot_id: format!(
            "btcusdt-{generation}-{last_applied_u}-{last_event_timestamp_ms}-{observed_at_ms}"
        ),
        continuity: "VERIFIED".to_string(),
        freshness: "FRESH".to_string(),
        complete: true,
    })
}

/// Apply one live WS diff to an immutable published snapshot. The previous snapshot
/// remains untouched; callers publish the returned candidate only if its generation is current.
pub fn apply_live_diff(
    current: &OrderbookSnapshotV1,
    update: &DepthUpdateWs,
    depth_limit: usize,
    observed_at_ms: i64,
    decision_time_ms: i64,
) -> Result<OrderbookSnapshotV1> {
    apply_live_diff_classified(
        current,
        update,
        depth_limit,
        observed_at_ms,
        decision_time_ms,
    )
    .map_err(anyhow::Error::new)
}

/// Apply one live diff and retain a stable typed rejection class for diagnostics.
/// Validation order intentionally matches the original fail-closed path:
/// header -> sequence -> levels/structure -> source and observation freshness.
pub fn apply_live_diff_classified(
    current: &OrderbookSnapshotV1,
    update: &DepthUpdateWs,
    depth_limit: usize,
    observed_at_ms: i64,
    decision_time_ms: i64,
) -> std::result::Result<OrderbookSnapshotV1, LiveDiffRejection> {
    validate_update_header(update)
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;

    if update.prev_update_id != current.last_applied_u
        || update.final_update_id <= current.last_applied_u
    {
        return Err(LiveDiffRejection::Sequence(
            "live WS diff does not continue the published watermark".to_string(),
        ));
    }

    let mut bids: Levels = current
        .bids
        .iter()
        .map(|level| {
            let key = crate::utils::normalize_plain_decimal(&level[0])?;
            Ok((key, level.clone()))
        })
        .collect::<Result<_>>()
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;
    let mut asks: Levels = current
        .asks
        .iter()
        .map(|level| {
            let key = crate::utils::normalize_plain_decimal(&level[0])?;
            Ok((key, level.clone()))
        })
        .collect::<Result<_>>()
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;

    insert_levels(&mut bids, &update.bids, "bid", true)
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;
    insert_levels(&mut asks, &update.asks, "ask", true)
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;
    trim_levels(&mut bids, depth_limit, true);
    trim_levels(&mut asks, depth_limit, false);
    if depth_limit < REQUIRED_DEPTH || bids.len() < REQUIRED_DEPTH || asks.len() < REQUIRED_DEPTH {
        return Err(LiveDiffRejection::Shape(
            "live candidate book does not meet the minimum depth".to_string(),
        ));
    }

    let best_bid = bids
        .keys()
        .max_by(|left, right| compare_normalized_decimals(left, right))
        .context("empty bid side")
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;
    let best_ask = asks
        .keys()
        .min_by(|left, right| compare_normalized_decimals(left, right))
        .context("empty ask side")
        .map_err(|error| LiveDiffRejection::Shape(format!("{error:#}")))?;
    if compare_normalized_decimals(best_bid, best_ask).is_ge() {
        return Err(LiveDiffRejection::Shape(
            "live candidate book is locked or crossed".to_string(),
        ));
    }

    validate_timestamp(update.event_time, decision_time_ms, "source")
        .map_err(|error| LiveDiffRejection::Freshness(format!("{error:#}")))?;
    validate_timestamp(observed_at_ms, decision_time_ms, "observation")
        .map_err(|error| LiveDiffRejection::Freshness(format!("{error:#}")))?;

    let bid_levels = sorted_levels(bids, true);
    let ask_levels = sorted_levels(asks, false);
    let mut next = current.clone();
    next.bids = bid_levels;
    next.asks = ask_levels;
    next.bid_depth = next.bids.len();
    next.ask_depth = next.asks.len();
    next.last_applied_u = update.final_update_id;
    next.decision_watermark = update.final_update_id;
    next.source_timestamp_ms = update.event_time;
    next.observed_at_ms = observed_at_ms;
    next.snapshot_id = format!(
        "btcusdt-{}-{}-{}-{}",
        next.generation, next.last_applied_u, next.source_timestamp_ms, next.observed_at_ms
    );
    next.freshness = "FRESH".to_string();
    next.complete = true;
    Ok(next)
}

fn validate_update_header(update: &DepthUpdateWs) -> Result<()> {
    if update.symbol != "BTCUSDT" || update.event_type != "depthUpdate" {
        bail!("V1 accepts only BTCUSDT depthUpdate events");
    }
    if update.first_update_id == 0
        || update.final_update_id == 0
        || update.first_update_id > update.final_update_id
        || update.prev_update_id >= update.final_update_id
        || update.event_time <= 0
    {
        bail!("malformed WS depth update header");
    }
    Ok(())
}

fn validate_timestamp(timestamp_ms: i64, now_ms: i64, label: &str) -> Result<()> {
    if timestamp_ms > now_ms.saturating_add(MAX_FUTURE_SKEW_MS) {
        bail!("{label} timestamp exceeds the allowed future skew");
    }
    if now_ms.saturating_sub(timestamp_ms) > MAX_BOOK_AGE_MS {
        bail!("{label} timestamp is stale");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DepthSnapshotRest, DepthUpdateWs};

    const NOW: i64 = 1_800_000_000_000;

    fn snapshot() -> DepthSnapshotRest {
        DepthSnapshotRest {
            last_update_id: 100,
            bids: (0..20)
                .map(|index| [format!("{}", 10_000 - index), "1.000".to_string()])
                .collect(),
            asks: (0..20)
                .map(|index| [format!("{}", 10_001 + index), "2.000".to_string()])
                .collect(),
        }
    }

    fn update(first: u64, last: u64, previous: u64, event_time: i64) -> DepthUpdateWs {
        DepthUpdateWs {
            event_type: "depthUpdate".to_string(),
            event_time,
            transaction_time: event_time,
            symbol: "BTCUSDT".to_string(),
            first_update_id: first,
            final_update_id: last,
            prev_update_id: previous,
            bids: vec![],
            asks: vec![],
        }
    }

    fn valid_updates() -> Vec<DepthUpdateWs> {
        vec![
            update(99, 101, 100, NOW - 100),
            update(102, 103, 101, NOW - 50),
        ]
    }

    #[test]
    fn exact_bridge_and_pu_chain_build_a_complete_snapshot() {
        let result = build_snapshot(&snapshot(), &valid_updates(), 7, NOW - 10, NOW).unwrap();
        assert_eq!(result.generation, 7);
        assert_eq!(result.first_applied_u, 99);
        assert_eq!(result.last_applied_u, 103);
        assert_eq!(result.decision_watermark, 103);
        assert_eq!(result.source_timestamp_ms, NOW - 50);
        assert_eq!(result.bid_depth, 20);
        assert_eq!(result.ask_depth, 20);
        assert!(result.complete);
        assert_eq!(result.continuity, "VERIFIED");
        assert_eq!(result.freshness, "FRESH");
    }

    #[test]
    fn serialized_snapshot_matches_t2_v1_wire_field_names_and_string_levels() {
        let result = build_snapshot(&snapshot(), &valid_updates(), 7, NOW - 10, NOW).unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["contractVersion"], CONTRACT_VERSION);
        assert_eq!(value["symbol"], "BTCUSDT");
        assert_eq!(value["market"], "BINANCE_USDM_PERPETUAL");
        assert_eq!(value["continuity"], "VERIFIED");
        assert_eq!(value["freshness"], "FRESH");
        assert_eq!(value["decisionWatermark"], 103);
        assert!(value["bids"][0].is_array());
        assert!(value["bids"][0][0].is_string());
        assert!(value["bids"][0][1].is_string());
    }

    #[test]
    fn legacy_pu_attach_bridge_is_rejected() {
        let mut diffs = vec![update(110, 120, 100, NOW - 100)];
        diffs[0].bids.push(["9999".into(), "1".into()]);
        assert!(build_snapshot(&snapshot(), &diffs, 1, NOW, NOW).is_err());
    }

    #[test]
    fn sequence_gap_duplicate_or_regression_is_rejected() {
        let mut gap = valid_updates();
        gap[1].prev_update_id = 102;
        assert!(build_snapshot(&snapshot(), &gap, 1, NOW, NOW).is_err());

        let mut duplicate = valid_updates();
        duplicate[1].final_update_id = 101;
        duplicate[1].prev_update_id = 101;
        assert!(build_snapshot(&snapshot(), &duplicate, 1, NOW, NOW).is_err());
    }

    #[test]
    fn negative_and_zero_prices_or_negative_quantities_are_rejected() {
        let mut invalid_snapshot = snapshot();
        invalid_snapshot.bids[0][0] = "-1".into();
        assert!(build_snapshot(&invalid_snapshot, &valid_updates(), 1, NOW, NOW).is_err());

        let mut invalid_update = valid_updates();
        invalid_update[0].bids.push(["9999".into(), "-0.1".into()]);
        assert!(build_snapshot(&snapshot(), &invalid_update, 1, NOW, NOW).is_err());
    }

    #[test]
    fn duplicate_normalized_levels_and_unsorted_snapshot_are_rejected() {
        let mut invalid = snapshot();
        invalid.bids[1][0] = invalid.bids[0][0].clone();
        assert!(build_snapshot(&invalid, &valid_updates(), 1, NOW, NOW).is_err());

        let mut unsorted = snapshot();
        unsorted.asks.swap(0, 1);
        assert!(build_snapshot(&unsorted, &valid_updates(), 1, NOW, NOW).is_err());
    }

    #[test]
    fn insufficient_depth_and_crossed_books_are_rejected() {
        let mut shallow = snapshot();
        shallow.bids.pop();
        assert!(build_snapshot(&shallow, &valid_updates(), 1, NOW, NOW).is_err());

        let mut crossed = snapshot();
        crossed.asks[0][0] = "9999".into();
        assert!(build_snapshot(&crossed, &valid_updates(), 1, NOW, NOW).is_err());
    }

    #[test]
    fn source_and_observation_freshness_boundaries_are_enforced() {
        let mut stale_source = valid_updates();
        stale_source[1].event_time = NOW - MAX_BOOK_AGE_MS - 1;
        assert!(build_snapshot(&snapshot(), &stale_source, 1, NOW, NOW).is_err());
        assert!(build_snapshot(
            &snapshot(),
            &valid_updates(),
            1,
            NOW - MAX_BOOK_AGE_MS - 1,
            NOW
        )
        .is_err());

        let mut future_source = valid_updates();
        future_source[1].event_time = NOW + MAX_FUTURE_SKEW_MS + 1;
        assert!(build_snapshot(&snapshot(), &future_source, 1, NOW, NOW).is_err());
    }

    #[test]
    fn publisher_invalidates_old_snapshot_and_fences_late_generation() {
        let publisher = SnapshotPublisher::new();
        let generation = publisher.begin_reanchor();
        publisher.note_reanchor_attempt().unwrap();
        let candidate =
            build_snapshot(&snapshot(), &valid_updates(), generation, NOW, NOW).unwrap();
        publisher.publish_if_current(candidate.clone()).unwrap();
        assert!(publisher.read().is_some());

        let next_generation = publisher.begin_reanchor();
        publisher.note_reanchor_attempt().unwrap();
        assert!(publisher.read().is_none());
        assert!(publisher.publish_if_current(candidate).is_err());

        let next =
            build_snapshot(&snapshot(), &valid_updates(), next_generation, NOW, NOW).unwrap();
        publisher.publish_if_current(next).unwrap();
        assert_eq!(publisher.state(), OrderbookV1State::Ready);
    }

    #[test]
    fn live_diff_requires_exact_pu_and_publishes_a_new_immutable_watermark() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();
        let mut next_event = update(104, 104, 103, NOW - 20);
        next_event.bids.push(["9999".into(), "3".into()]);
        let next = apply_live_diff(&current, &next_event, 20, NOW - 5, NOW).unwrap();
        assert_eq!(current.last_applied_u, 103);
        assert_eq!(next.last_applied_u, 104);
        assert_eq!(next.decision_watermark, 104);
        assert_ne!(current.snapshot_id, next.snapshot_id);

        next_event.prev_update_id = 102;
        assert!(apply_live_diff(&current, &next_event, 20, NOW - 5, NOW).is_err());
    }

    #[test]
    fn live_zero_quantity_deletes_level_without_mutating_prior_snapshot() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();
        let mut event = update(104, 104, 103, NOW - 20);
        event.bids.push(["9981".into(), "0".into()]);
        event.bids.push(["9980".into(), "3".into()]);

        let next = apply_live_diff(&current, &event, 20, NOW - 5, NOW).unwrap();
        assert!(current.bids.iter().any(|level| level[0] == "9981"));
        assert!(!next.bids.iter().any(|level| level[0] == "9981"));
        assert!(next.bids.iter().any(|level| level[0] == "9980"));
    }

    #[test]
    fn live_duplicate_normalized_prices_and_crossed_book_are_rejected() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();
        let mut duplicate = update(104, 104, 103, NOW - 20);
        duplicate.bids = vec![["9999".into(), "1".into()], ["9999.0".into(), "2".into()]];
        assert!(apply_live_diff(&current, &duplicate, 20, NOW - 5, NOW).is_err());

        let mut crossed = update(104, 104, 103, NOW - 20);
        crossed.asks.push(["9999".into(), "1".into()]);
        assert!(apply_live_diff(&current, &crossed, 20, NOW - 5, NOW).is_err());
    }

    #[test]
    fn reanchor_budget_stops_after_three_attempts_and_resets_on_new_connection() {
        let publisher = SnapshotPublisher::new();
        publisher.begin_connection_generation();
        assert_eq!(publisher.note_reanchor_attempt().unwrap(), 1);
        assert_eq!(publisher.note_reanchor_attempt().unwrap(), 2);
        assert_eq!(publisher.note_reanchor_attempt().unwrap(), 3);
        assert!(publisher.note_reanchor_attempt().is_err());
        assert_eq!(publisher.state(), OrderbookV1State::Broken);

        publisher.begin_connection_generation();
        assert_eq!(publisher.note_reanchor_attempt().unwrap(), 1);
    }

    #[test]
    fn source_and_observation_exact_freshness_boundaries_are_accepted() {
        let mut diffs = valid_updates();
        diffs[1].event_time = NOW - MAX_BOOK_AGE_MS;
        let candidate = build_snapshot(&snapshot(), &diffs, 1, NOW - MAX_BOOK_AGE_MS, NOW).unwrap();
        assert_eq!(candidate.freshness, "FRESH");

        let mut future = valid_updates();
        future[1].event_time = NOW + MAX_FUTURE_SKEW_MS;
        assert!(build_snapshot(&snapshot(), &future, 1, NOW, NOW).is_ok());
    }

    #[test]
    fn publisher_read_rechecks_freshness_at_decision_time() {
        let publisher = SnapshotPublisher::new();
        let generation = publisher.begin_connection_generation();
        publisher.note_reanchor_attempt().unwrap();
        let candidate =
            build_snapshot(&snapshot(), &valid_updates(), generation, NOW - 10, NOW).unwrap();
        publisher.publish_if_current(candidate).unwrap();

        assert!(publisher.read_fresh(NOW).is_ok());
        assert!(publisher.read_fresh(NOW + MAX_BOOK_AGE_MS).is_err());
    }

    #[test]
    fn rest_anchor_without_exchange_timestamped_bridge_is_not_publishable() {
        assert!(build_snapshot(&snapshot(), &[], 1, NOW, NOW).is_err());
    }

    #[test]
    fn classified_live_diff_maps_source_observation_and_future_skew_to_freshness() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();

        let mut stale_source = update(104, 104, 103, NOW - MAX_BOOK_AGE_MS - 1);
        assert!(matches!(
            apply_live_diff_classified(&current, &stale_source, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Freshness(_))
        ));

        stale_source.event_time = NOW - 20;
        assert!(matches!(
            apply_live_diff_classified(&current, &stale_source, 20, NOW - MAX_BOOK_AGE_MS - 1, NOW),
            Err(LiveDiffRejection::Freshness(_))
        ));

        stale_source.event_time = NOW + MAX_FUTURE_SKEW_MS + 1;
        assert!(matches!(
            apply_live_diff_classified(&current, &stale_source, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Freshness(_))
        ));
        assert_eq!(current.last_applied_u, 103);
    }

    #[test]
    fn classified_live_diff_maps_watermark_and_pu_mismatch_to_sequence() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();
        let mut invalid = update(104, 104, 102, NOW - 20);
        assert!(matches!(
            apply_live_diff_classified(&current, &invalid, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Sequence(_))
        ));

        invalid = update(102, 102, 101, NOW - 20);
        assert!(matches!(
            apply_live_diff_classified(&current, &invalid, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Sequence(_))
        ));
        assert_eq!(current.last_applied_u, 103);
    }

    #[test]
    fn classified_live_diff_maps_header_levels_depth_and_crossed_book_to_shape() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();

        let mut malformed_header = update(104, 104, 103, NOW - 20);
        malformed_header.first_update_id = 0;
        assert!(matches!(
            apply_live_diff_classified(&current, &malformed_header, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Shape(_))
        ));

        let mut invalid_level = update(104, 104, 103, NOW - 20);
        invalid_level.bids.push(["not-a-price".into(), "1".into()]);
        assert!(matches!(
            apply_live_diff_classified(&current, &invalid_level, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Shape(_))
        ));

        assert!(matches!(
            apply_live_diff_classified(
                &current,
                &update(104, 104, 103, NOW - 20),
                19,
                NOW - 5,
                NOW
            ),
            Err(LiveDiffRejection::Shape(_))
        ));

        let mut crossed = update(104, 104, 103, NOW - 20);
        crossed.asks.push(["9999".into(), "1".into()]);
        assert!(matches!(
            apply_live_diff_classified(&current, &crossed, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Shape(_))
        ));
    }

    #[test]
    fn classified_live_diff_preserves_existing_validation_precedence() {
        let current = build_snapshot(&snapshot(), &valid_updates(), 4, NOW - 10, NOW).unwrap();

        // Header validation precedes sequence validation, as in the existing path.
        let mut malformed_and_discontinuous = update(104, 104, 102, NOW - 20);
        malformed_and_discontinuous.first_update_id = 0;
        assert!(matches!(
            apply_live_diff_classified(&current, &malformed_and_discontinuous, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Shape(_))
        ));

        // Sequence validation precedes level and timestamp validation.
        let mut stale_and_discontinuous = update(104, 104, 102, NOW - MAX_BOOK_AGE_MS - 1);
        stale_and_discontinuous
            .bids
            .push(["not-a-price".into(), "1".into()]);
        assert!(matches!(
            apply_live_diff_classified(&current, &stale_and_discontinuous, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Sequence(_))
        ));

        // Shape validation precedes freshness validation after sequence passes.
        let mut stale_and_malformed = update(104, 104, 103, NOW - MAX_BOOK_AGE_MS - 1);
        stale_and_malformed
            .bids
            .push(["not-a-price".into(), "1".into()]);
        assert!(matches!(
            apply_live_diff_classified(&current, &stale_and_malformed, 20, NOW - 5, NOW),
            Err(LiveDiffRejection::Shape(_))
        ));
        assert_eq!(current.last_applied_u, 103);
    }
}
