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
    let mut value = serde_json::to_value(snapshot).map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
    let object = value
        .as_object_mut()
        .ok_or(SnapshotUnavailable::InvalidSnapshot)?;
    object.remove("snapshotId");
    let canonical = serde_json::to_vec(&value).map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
    let hash = digest(&SHA256, &canonical);
    let hex = hash.as_ref().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
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

    let best_bid = snapshot.bids.first().and_then(|level| level[0].parse::<f64>().ok());
    let best_ask = snapshot.asks.first().and_then(|level| level[0].parse::<f64>().ok());
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) if bid.is_finite() && ask.is_finite() && bid < ask => {}
        _ => return Err(SnapshotUnavailable::InvalidSnapshot),
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
    let mut previous: Option<f64> = None;
    for [price_text, quantity_text] in levels {
        if !is_canonical_decimal(price_text) || !is_canonical_decimal(quantity_text) {
            return Err(SnapshotUnavailable::InvalidSnapshot);
        }
        let price = price_text.parse::<f64>().map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
        let quantity = quantity_text.parse::<f64>().map_err(|_| SnapshotUnavailable::InvalidSnapshot)?;
        if !price.is_finite() || !quantity.is_finite() || price <= 0.0 || quantity <= 0.0 {
            return Err(SnapshotUnavailable::InvalidSnapshot);
        }
        if let Some(prior) = previous {
            if (descending && prior <= price) || (!descending && prior >= price) {
                return Err(SnapshotUnavailable::InvalidSnapshot);
            }
        }
        previous = Some(price);
    }
    Ok(())
}

fn is_canonical_decimal(value: &str) -> bool {
    if value.is_empty() || value.starts_with('+') || value.starts_with('-') || (value.contains('e') || value.contains('E')) {
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
