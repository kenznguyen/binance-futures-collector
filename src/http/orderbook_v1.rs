use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};

const MAX_AGE_MS: i64 = 2_000;
const MAX_FUTURE_SKEW_MS: i64 = 250;
const MIN_LEVELS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderbookDepthV1 {
    pub bids: usize,
    pub asks: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderbookSnapshotV1 {
    pub contract_version: String,
    pub symbol: String,
    pub market: String,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
    pub snapshot_last_update_id: u64,
    pub first_applied_u: u64,
    pub last_applied_u: u64,
    pub source_timestamp_ms: i64,
    pub observed_at_ms: i64,
    pub decision_watermark: u64,
    pub generation: u64,
    pub snapshot_id: String,
    pub continuity: String,
    pub freshness: String,
    pub depth: OrderbookDepthV1,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotUnavailable {
    NotPublished,
    WrongSymbol,
    InvalidSnapshot,
    Stale,
    FutureTimestamp,
    GenerationFenced,
}

pub struct OrderbookV1Publisher {
    generation: AtomicU64,
    snapshot: RwLock<Option<Arc<OrderbookSnapshotV1>>>,
}

impl Default for OrderbookV1Publisher {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderbookV1Publisher {
    pub fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            snapshot: RwLock::new(None),
        }
    }

    pub fn begin_generation(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.snapshot.write().expect("publisher lock poisoned") = None;
        generation
    }

    pub fn invalidate(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.snapshot.write().expect("publisher lock poisoned") = None;
        generation
    }

    pub fn publish(&self, candidate: OrderbookSnapshotV1) -> Result<(), SnapshotUnavailable> {
        let current_generation = self.generation.load(Ordering::SeqCst);
        if candidate.generation == 0 || candidate.generation != current_generation {
            return Err(SnapshotUnavailable::GenerationFenced);
        }
        validate_snapshot(&candidate, chrono::Utc::now().timestamp_millis())?;
        let published = Arc::new(candidate);
        let mut slot = self.snapshot.write().expect("publisher lock poisoned");
        if self.generation.load(Ordering::SeqCst) != published.generation {
            return Err(SnapshotUnavailable::GenerationFenced);
        }
        *slot = Some(published);
        Ok(())
    }

    pub fn read_fresh(
        &self,
        symbol: &str,
        now_ms: i64,
    ) -> Result<Arc<OrderbookSnapshotV1>, SnapshotUnavailable> {
        if symbol != "BTCUSDT" {
            return Err(SnapshotUnavailable::WrongSymbol);
        }
        let snapshot = self
            .snapshot
            .read()
            .expect("publisher lock poisoned")
            .clone()
            .ok_or(SnapshotUnavailable::NotPublished)?;
        if snapshot.generation != self.generation.load(Ordering::SeqCst) {
            return Err(SnapshotUnavailable::GenerationFenced);
        }
        validate_snapshot(&snapshot, now_ms)?;
        Ok(snapshot)
    }
}

pub fn canonical_snapshot_id(
    snapshot: &OrderbookSnapshotV1,
) -> Result<String, SnapshotUnavailable> {
    let mut value =
        serde_json::to_value(snapshot).map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
    let object = value
        .as_object_mut()
        .ok_or(SnapshotUnavailable::InvalidSnapshot)?;
    object.remove("snapshotId");
    let canonical = serde_json::to_vec(&value).map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
    let hash = digest(&SHA256, &canonical);
    let hex = hash
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{hex}"))
}

fn validate_snapshot(
    snapshot: &OrderbookSnapshotV1,
    now_ms: i64,
) -> Result<(), SnapshotUnavailable> {
    if snapshot.contract_version != "T1_T2_ORDERBOOK_V1"
        || snapshot.symbol != "BTCUSDT"
        || snapshot.market != "BINANCE_USDM_PERPETUAL"
        || snapshot.continuity != "VERIFIED"
        || snapshot.freshness != "FRESH"
        || !snapshot.complete
        || snapshot.generation == 0
        || snapshot.first_applied_u <= snapshot.snapshot_last_update_id
        || snapshot.last_applied_u < snapshot.first_applied_u
        || snapshot.decision_watermark != snapshot.last_applied_u
        || snapshot.bids.len() < MIN_LEVELS
        || snapshot.asks.len() < MIN_LEVELS
        || snapshot.depth.bids != snapshot.bids.len()
        || snapshot.depth.asks != snapshot.asks.len()
    {
        return Err(SnapshotUnavailable::InvalidSnapshot);
    }

    validate_levels(&snapshot.bids, true)?;
    validate_levels(&snapshot.asks, false)?;

    let best_bid = &snapshot.bids[0][0];
    let best_ask = &snapshot.asks[0][0];
    if compare_canonical_decimal(best_bid, best_ask) != Some(std::cmp::Ordering::Less) {
        return Err(SnapshotUnavailable::InvalidSnapshot);
    }

    for timestamp in [snapshot.source_timestamp_ms, snapshot.observed_at_ms] {
        if timestamp > now_ms.saturating_add(MAX_FUTURE_SKEW_MS) {
            return Err(SnapshotUnavailable::FutureTimestamp);
        }
        if now_ms.saturating_sub(timestamp) > MAX_AGE_MS {
            return Err(SnapshotUnavailable::Stale);
        }
    }

    let expected_id = canonical_snapshot_id(snapshot)?;
    if snapshot.snapshot_id != expected_id {
        return Err(SnapshotUnavailable::InvalidSnapshot);
    }
    Ok(())
}

fn validate_levels(levels: &[[String; 2]], descending: bool) -> Result<(), SnapshotUnavailable> {
    let mut previous: Option<&str> = None;
    for [price_text, quantity_text] in levels {
        if !is_canonical_decimal(price_text)
            || !is_canonical_decimal(quantity_text)
            || compare_canonical_decimal(quantity_text, "0") != Some(std::cmp::Ordering::Greater)
        {
            return Err(SnapshotUnavailable::InvalidSnapshot);
        }
        if let Some(prior) = previous {
            let ordering = compare_canonical_decimal(prior, price_text)
                .ok_or(SnapshotUnavailable::InvalidSnapshot)?;
            if (descending && ordering != std::cmp::Ordering::Greater)
                || (!descending && ordering != std::cmp::Ordering::Less)
            {
                return Err(SnapshotUnavailable::InvalidSnapshot);
            }
        }
        previous = Some(price_text);
    }
    Ok(())
}

fn compare_canonical_decimal(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    if !is_canonical_decimal(left) || !is_canonical_decimal(right) {
        return None;
    }

    let (left_whole, left_fraction) = left.split_once('.').unwrap_or((left, ""));
    let (right_whole, right_fraction) = right.split_once('.').unwrap_or((right, ""));

    let whole_order = left_whole
        .len()
        .cmp(&right_whole.len())
        .then_with(|| left_whole.cmp(right_whole));
    if whole_order != std::cmp::Ordering::Equal {
        return Some(whole_order);
    }

    let fraction_len = left_fraction.len().max(right_fraction.len());
    for index in 0..fraction_len {
        let left_digit = left_fraction.as_bytes().get(index).copied().unwrap_or(b'0');
        let right_digit = right_fraction
            .as_bytes()
            .get(index)
            .copied()
            .unwrap_or(b'0');
        match left_digit.cmp(&right_digit) {
            std::cmp::Ordering::Equal => {}
            ordering => return Some(ordering),
        }
    }

    Some(std::cmp::Ordering::Equal)
}

fn is_canonical_decimal(value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('+')
        || value.starts_with('-')
        || (value.contains('e') || value.contains('E'))
    {
        return false;
    }
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || (whole.len() > 1 && whole.starts_with('0'))
    {
        return false;
    }
    match fraction {
        Some(digits) => {
            !digits.is_empty()
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && !digits.ends_with('0')
        }
        None => true,
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderbookV1ContinuityError {
    InvalidRange,
    SnapshotAnchorOverflow,
    UpdateIdOverflow,
    InitialBridgeMissing,
    PreviousUpdateMismatch,
    SequenceGap,
    Invalidated,
}

#[derive(Debug, Clone)]
pub struct OrderbookV1Continuity {
    snapshot_last_update_id: u64,
    last_update_id: Option<u64>,
    invalidated: bool,
}

impl OrderbookV1Continuity {
    pub fn new(snapshot_last_update_id: u64) -> Self {
        Self {
            snapshot_last_update_id,
            last_update_id: None,
            invalidated: false,
        }
    }

    pub fn last_update_id(&self) -> Option<u64> {
        self.last_update_id
    }

    pub fn apply(
        &mut self,
        first_u: u64,
        final_u: u64,
        previous_u: u64,
    ) -> Result<(), OrderbookV1ContinuityError> {
        if self.invalidated {
            return Err(OrderbookV1ContinuityError::Invalidated);
        }

        if first_u == 0 || final_u == 0 || first_u > final_u {
            return self.invalidate(OrderbookV1ContinuityError::InvalidRange);
        }

        match self.last_update_id {
            None => {
                let bridge_id = match self.snapshot_last_update_id.checked_add(1) {
                    Some(value) => value,
                    None => {
                        return self
                            .invalidate(OrderbookV1ContinuityError::SnapshotAnchorOverflow);
                    }
                };

                if first_u > bridge_id || final_u < bridge_id {
                    return self
                        .invalidate(OrderbookV1ContinuityError::InitialBridgeMissing);
                }
            }
            Some(last_id) => {
                if previous_u != last_id {
                    return self
                        .invalidate(OrderbookV1ContinuityError::PreviousUpdateMismatch);
                }

                let next_id = match last_id.checked_add(1) {
                    Some(value) => value,
                    None => {
                        return self.invalidate(OrderbookV1ContinuityError::UpdateIdOverflow);
                    }
                };

                if first_u > next_id || final_u < next_id {
                    return self.invalidate(OrderbookV1ContinuityError::SequenceGap);
                }
            }
        }

        self.last_update_id = Some(final_u);
        Ok(())
    }

    fn invalidate<T>(
        &mut self,
        error: OrderbookV1ContinuityError,
    ) -> Result<T, OrderbookV1ContinuityError> {
        self.last_update_id = None;
        self.invalidated = true;
        Err(error)
    }
}
