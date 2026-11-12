use anyhow::{Context, Result};
use chrono::{DateTime, Timelike, Utc};
use dashmap::DashMap;
use log::debug;
use parking_lot::Mutex;
use std::collections::HashMap; // Keep HashMap for CSV file grouping
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{watch, Mutex as AsyncMutex};
use tokio::time::{interval, Duration};

use crate::config::Config;
use crate::constants;
use crate::models::*;

/// Validate a plain decimal lexical form without converting or normalizing it.
/// Optional sign and exponent are accepted; the original bytes are retained for CSV.
fn validate_decimal_string(value: &str, field: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let mut index = 0;

    if bytes.is_empty() {
        anyhow::bail!("invalid empty decimal value for {field}");
    }

    if matches!(bytes[index], b'+' | b'-') {
        index += 1;
    }

    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let integer_digits = index - integer_start;

    let mut fractional_digits = 0;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        fractional_digits = index - fraction_start;
    }

    if integer_digits == 0 && fractional_digits == 0 {
        anyhow::bail!("invalid decimal syntax for {field}");
    }

    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            anyhow::bail!("invalid decimal exponent for {field}");
        }
    }

    if index != bytes.len() {
        anyhow::bail!("invalid decimal syntax for {field}");
    }

    Ok(())
}

fn validate_decimal_fields(fields: &[(&str, &str)]) -> Result<()> {
    for (field, value) in fields {
        validate_decimal_string(value, field)?;
    }
    Ok(())
}

/// Row with sortable metadata for depth updates
#[derive(Clone)]
struct DepthUpdateRow {
    last_update_id: u64, // For sorting (column 8 in HEADER_DEPTH_UPDATES)
    csv_line: String,    // Pre-formatted CSV row
}

/// Buffer for accumulating CSV rows before writing
struct FileBuffer {
    writer: BufWriter<File>,
    row_count: usize,
    last_flush: std::time::Instant,
    #[allow(dead_code)] // Used for logging/debugging in error paths
    path: PathBuf,
    // In-memory buffer for depth_updates (sorted by last_update_id at flush time)
    depth_update_buffer: Option<Vec<DepthUpdateRow>>,
}

impl FileBuffer {
    /// Sort + write any buffered depth rows, then flush the writer. On a write
    /// failure the un-written rows are restored to the buffer so the next flush
    /// retries them instead of silently dropping data.
    fn flush_locked(&mut self) -> Result<()> {
        let mut rows: Vec<DepthUpdateRow> = Vec::new();
        if let Some(depth_buffer) = self.depth_update_buffer.as_mut() {
            if !depth_buffer.is_empty() {
                depth_buffer.sort_by_key(|r| r.last_update_id);
                rows = std::mem::take(depth_buffer);
            }
        }
        for i in 0..rows.len() {
            if let Err(e) = writeln!(self.writer, "{}", rows[i].csv_line) {
                if let Some(db) = self.depth_update_buffer.as_mut() {
                    db.extend(rows.drain(i..));
                }
                return Err(e.into());
            }
        }
        self.writer.flush()?;
        self.last_flush = std::time::Instant::now();
        Ok(())
    }
}

impl Drop for FileBuffer {
    fn drop(&mut self) {
        // Best-effort final drain so buffered depth rows survive an hourly rotation
        // or stale-buffer cleanup instead of being dropped with the Vec.
        let _ = self.flush_locked();
    }
}

fn collect_flush_results(results: impl IntoIterator<Item = Result<()>>) -> Result<()> {
    let mut first_error = None;
    for result in results {
        if let Err(error) = result {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error.context("one or more CSV buffers failed to flush")),
        None => Ok(()),
    }
}

struct BlockingOperationState {
    closed: bool,
    in_flight: usize,
}

struct BlockingOperationTracker {
    state: Mutex<BlockingOperationState>,
    updates: watch::Sender<usize>,
}

struct BlockingOperationGuard {
    tracker: Arc<BlockingOperationTracker>,
}

impl BlockingOperationTracker {
    fn new() -> Self {
        let (updates, _) = watch::channel(0);
        Self {
            state: Mutex::new(BlockingOperationState {
                closed: false,
                in_flight: 0,
            }),
            updates,
        }
    }

    fn begin(self: &Arc<Self>) -> Result<BlockingOperationGuard> {
        let mut state = self.state.lock();
        if state.closed {
            anyhow::bail!("CSV_BLOCKING_OPERATION_ADMISSION_CLOSED");
        }
        state.in_flight += 1;
        self.updates.send_replace(state.in_flight);
        Ok(BlockingOperationGuard {
            tracker: self.clone(),
        })
    }

    #[cfg(test)]
    fn is_closed(&self) -> bool {
        self.state.lock().closed
    }

    fn is_closed_and_drained(&self) -> bool {
        let state = self.state.lock();
        state.closed && state.in_flight == 0
    }

    async fn close_and_drain(&self) -> Result<()> {
        let mut updates = self.updates.subscribe();
        {
            let mut state = self.state.lock();
            state.closed = true;
            self.updates.send_replace(state.in_flight);
        }

        loop {
            if *updates.borrow_and_update() == 0 {
                return Ok(());
            }
            updates
                .changed()
                .await
                .context("CSV blocking-operation drain channel closed")?;
        }
    }
}

impl Drop for BlockingOperationGuard {
    fn drop(&mut self) {
        let mut state = self.tracker.state.lock();
        debug_assert!(state.in_flight > 0);
        state.in_flight -= 1;
        self.tracker.updates.send_replace(state.in_flight);
    }
}

/// Manages CSV file writing with buffering and atomic operations
#[derive(Clone)]
pub struct CsvWriter {
    #[allow(dead_code)] // Used for potential future config-driven features
    config: Arc<Config>,
    base_dir: PathBuf,
    buffers: Arc<DashMap<String, Arc<Mutex<FileBuffer>>>>,
    flush_interval: std::time::Duration,
    buffer_size_bytes: usize,
    max_depth_buffer_rows: usize,
    shutdown_tx: watch::Sender<bool>,
    background_tasks: Arc<AsyncMutex<Vec<tokio::task::JoinHandle<()>>>>,
    blocking_operations: Arc<BlockingOperationTracker>,
}

impl CsvWriter {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let base_dir = PathBuf::from(&config.output.base_dir);

        // Create base directory if it doesn't exist
        std::fs::create_dir_all(&base_dir)?;

        let (shutdown_tx, _) = watch::channel(false);

        Ok(Self {
            config: config.clone(),
            base_dir,
            buffers: Arc::new(DashMap::new()),
            flush_interval: std::time::Duration::from_secs(config.output.flush_interval_seconds),
            buffer_size_bytes: config.output.buffer_size_kb * 1024,
            max_depth_buffer_rows: config.output.max_depth_buffer_rows,
            shutdown_tx,
            background_tasks: Arc::new(AsyncMutex::new(Vec::new())),
            blocking_operations: Arc::new(BlockingOperationTracker::new()),
        })
    }

    /// Get or create file buffer for a specific file path (uses spawn_blocking)
    async fn get_or_create_buffer(&self, file_path: &Path) -> Result<Arc<Mutex<FileBuffer>>> {
        let path_str = file_path.to_string_lossy().into_owned();

        if let Some(buffer) = self.buffers.get(&path_str) {
            debug!(
                "get_or_create_buffer: Using CACHED buffer for {:?}",
                path_str
            );
            return Ok(buffer.clone());
        }

        debug!(
            "get_or_create_buffer: Creating NEW buffer for {:?}",
            path_str
        );
        let file_path_owned = file_path.to_path_buf();
        let buffer_size = self.buffer_size_bytes;
        let header = self.get_header_for_file(file_path).to_string();

        let operation = self.blocking_operations.begin()?;
        let buffer = tokio::task::spawn_blocking(move || -> Result<FileBuffer> {
            let _operation = operation;
            if let Some(parent) = file_path_owned.parent() {
                debug!("spawn_blocking: Creating parent directory {:?}", parent);
                std::fs::create_dir_all(parent)?;
            }

            debug!("spawn_blocking: Opening file {:?}", file_path_owned);
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path_owned)
                .with_context(|| format!("Failed to open file: {:?}", file_path_owned))?;
            debug!(
                "spawn_blocking: File opened successfully: {:?}",
                file_path_owned
            );

            // Check if file is empty to write header
            let metadata = file.metadata()?;
            let needs_header = metadata.len() == 0;

            let mut writer = BufWriter::with_capacity(buffer_size, file);

            if needs_header {
                writeln!(writer, "{}", header)?;
                // Flush immediately to prevent duplicate headers on crash/restart.
                // Without this, the header stays buffered; if the collector crashes before
                // flush the file is empty and the header is written again on restart.
                writer.flush()?;
            }

            // Initialize depth_update_buffer only for depth_updates files
            let is_depth_updates = file_path_owned.to_string_lossy().contains("depth_updates");

            Ok(FileBuffer {
                writer,
                row_count: 0,
                last_flush: std::time::Instant::now(),
                path: file_path_owned,
                depth_update_buffer: if is_depth_updates {
                    Some(Vec::new())
                } else {
                    None
                },
            })
        })
        .await??;

        // Two tasks can both miss the cache for the same brand-new path and both
        // open it (e.g. the periodic snapshot and the one-shot bridge snapshot for
        // one symbol resolve to the same daily file). entry() makes the
        // check-and-insert atomic: if another task cached this path while we were
        // in spawn_blocking, use its buffer and let ours drop, so exactly one
        // FileBuffer (one fd) ever writes the file, instead of two interleaving.
        use dashmap::mapref::entry::Entry;
        match self.buffers.entry(path_str) {
            Entry::Occupied(existing) => Ok(existing.get().clone()),
            Entry::Vacant(slot) => {
                let buffer_arc = Arc::new(Mutex::new(buffer));
                slot.insert(buffer_arc.clone());
                Ok(buffer_arc)
            }
        }
    }

    /// Get appropriate header based on file type (returns &'static str)
    fn get_header_for_file(&self, file_path: &Path) -> &'static str {
        let filename = file_path.file_name().unwrap_or_default().to_string_lossy();

        if filename.contains("depth_updates") {
            constants::HEADER_DEPTH_UPDATES
        } else if filename.contains("depth_snapshot") {
            constants::HEADER_DEPTH_SNAPSHOT
        } else if filename.contains("trades") {
            constants::HEADER_TRADES
        } else if filename.contains("klines")
            && !filename.contains("mark_price")
            && !filename.contains("index_price")
            && !filename.contains("premium_index")
        {
            constants::HEADER_KLINES
        } else if filename.contains("liquidations") {
            constants::HEADER_LIQUIDATIONS
        } else if filename.contains("funding_rates") {
            constants::HEADER_FUNDING_RATES
        } else if filename.contains("open_interest") {
            constants::HEADER_OPEN_INTEREST
        } else if filename.contains("taker_buy_sell_ratio") {
            constants::HEADER_TAKER_BUY_SELL_RATIO
        } else if filename.contains("long_short_ratio") {
            constants::HEADER_LONG_SHORT_RATIO
        } else if filename.contains("premium_index_klines") {
            constants::HEADER_PREMIUM_INDEX_KLINES
        } else if filename.contains("mark_price_klines") || filename.contains("index_price_klines")
        {
            constants::HEADER_MARK_INDEX_KLINES
        } else {
            "exchange,market,datatype,timestamp_ms,datetime_utc,symbol"
        }
    }

    /// Build file path for depth updates (hourly files)
    pub fn get_depth_update_path(&self, symbol: &str, timestamp: i64) -> PathBuf {
        let dt = DateTime::<Utc>::from_timestamp_millis(timestamp).unwrap_or_default();
        let date_str = dt.format("%Y-%m-%d").to_string();
        let hour = dt.hour();
        let next_hour = (hour + 1) % 24;

        let filename = format!(
            "{}_depth_updates_{}_{:02}00-to-{:02}00.csv",
            symbol, // Symbol already lowercase
            date_str,
            hour,
            next_hour
        );

        self.base_dir
            .join("orderbooks")
            .join(symbol) // Symbol already lowercase
            .join(&date_str)
            .join(filename)
    }

    /// Build file path for daily files
    pub fn get_daily_file_path(&self, data_type: &str, symbol: &str, timestamp: i64) -> PathBuf {
        let dt = DateTime::<Utc>::from_timestamp_millis(timestamp).unwrap_or_default();
        let date_str = dt.format("%Y-%m-%d").to_string();

        let filename = match data_type {
            "trades" => format!("{}_trades_{}.csv", symbol, date_str),
            "klines" => format!("{}_klines_{}.csv", symbol, date_str),
            "liquidations" => format!("{}_liquidations_{}.csv", symbol, date_str),
            "funding_rates" => format!("{}_funding_rates_{}.csv", symbol, date_str),
            "open_interest" => format!("{}_open_interest_{}.csv", symbol, date_str),
            "long_short_ratio" => format!("{}_long_short_ratio_{}.csv", symbol, date_str),
            "mark_price_klines" => format!("{}_mark_price_klines_{}.csv", symbol, date_str),
            "index_price_klines" => format!("{}_index_price_klines_{}.csv", symbol, date_str),
            "taker_buy_sell_ratio" => format!("{}_taker_buy_sell_ratio_{}.csv", symbol, date_str),
            "premium_index_klines" => format!("{}_premium_index_klines_{}.csv", symbol, date_str),
            _ => format!("{}_{}.csv", symbol, date_str),
        };

        self.base_dir
            .join(data_type)
            .join(symbol) // Symbol already lowercase
            .join(&date_str)
            .join(filename)
    }

    /// Build file path for depth snapshots
    pub fn get_snapshot_path(&self, symbol: &str, event_type: &str, timestamp: i64) -> PathBuf {
        let dt = DateTime::<Utc>::from_timestamp_millis(timestamp).unwrap_or_default();
        let date_str = dt.format("%Y-%m-%d").to_string();

        let filename = if event_type == "depth_snapshot_initial" {
            // Initial snapshots have timestamp in filename
            let ts_str = dt.format("%Y_%m_%d_%H%M%S").to_string();
            format!(
                "{}_depth_snapshot_initial_{}_{}.csv",
                symbol, // Symbol already lowercase
                ts_str,
                timestamp
            )
        } else {
            // Regular snapshots append to daily file
            format!("{}_depth_snapshot_{}.csv", symbol, date_str)
        };

        self.base_dir
            .join("orderbooks")
            .join(symbol) // Symbol already lowercase
            .join(&date_str)
            .join(filename)
    }

    /// Write depth update to CSV (buffered and sorted by last_update_id)
    /// ISO-8601 UTC (millisecond precision) for a unix-ms timestamp.
    fn iso_utc(ms: i64) -> String {
        DateTime::<Utc>::from_timestamp_millis(ms)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    }

    pub async fn write_depth_update(&self, update: &DepthUpdateCsv) -> Result<()> {
        // Optionally skip raw disk persistence for symbols configured in [storage]
        // (klines + REST metrics are still collected for them).
        if !self.config.storage.should_persist_orderbook(&update.symbol) {
            return Ok(());
        }

        let file_path = self.get_depth_update_path(&update.symbol, update.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},\"{}\",\"{}\"",
            update.exchange,
            update.market,
            update.datatype,
            update.timestamp_ms,
            Self::iso_utc(update.timestamp_ms),
            update.symbol,
            update.first_update_id,
            update.last_update_id,
            update.prev_update_id,
            update.bids_json.replace("\"", "\"\""), // Escape quotes for CSV
            update.asks_json.replace("\"", "\"\"")  // Escape quotes for CSV
        );

        self.write_depth_update_buffered(buffer, update.last_update_id, &row)
            .await
    }

    /// Write depth snapshot to CSV (one row per price level)
    pub async fn write_depth_snapshot(&self, snapshots: Vec<DepthSnapshotCsv>) -> Result<()> {
        if snapshots.is_empty() {
            return Ok(());
        }

        let mut file_groups: HashMap<PathBuf, Vec<String>> = HashMap::new();

        for snapshot in snapshots {
            let file_path = self.get_snapshot_path(
                &snapshot.symbol,
                &snapshot.event_type,
                snapshot.timestamp_ms,
            );

            validate_decimal_fields(&[
                ("price", &snapshot.price),
                ("quantity", &snapshot.quantity),
            ])?;

            let row = format!(
                "{},{},{},{},{},{},{},{},{},{},{}",
                snapshot.exchange,
                snapshot.market,
                snapshot.datatype,
                snapshot.timestamp_ms,
                Self::iso_utc(snapshot.timestamp_ms),
                snapshot.symbol,
                snapshot.event_type,
                snapshot.side,
                snapshot.price,
                snapshot.quantity,
                snapshot.last_update_id
            );

            file_groups.entry(file_path).or_default().push(row);
        }

        for (file_path, rows) in file_groups {
            let buffer = self.get_or_create_buffer(&file_path).await?;
            for row in rows {
                self.write_row(buffer.clone(), &row).await?;
            }
        }

        Ok(())
    }

    pub async fn write_trade(&self, trade: &TradeCsv) -> Result<()> {
        // Optionally skip raw disk persistence for symbols configured in [storage]
        // (klines + REST metrics are still collected for them).
        if !self.config.storage.should_persist_trades(&trade.symbol) {
            return Ok(());
        }

        validate_decimal_fields(&[("price", &trade.price), ("quantity", &trade.quantity)])?;

        let file_path = self.get_daily_file_path("trades", &trade.symbol, trade.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            trade.exchange,
            trade.market,
            trade.datatype,
            trade.timestamp_ms,
            Self::iso_utc(trade.timestamp_ms),
            trade.symbol,
            trade.trade_id,
            trade.side,
            trade.price,
            trade.quantity,
            trade.is_buyer_maker,
            trade.first_update_id,
            trade.last_update_id
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_kline(&self, kline: &KlineCsv) -> Result<()> {
        // Validate every source decimal before opening/creating the destination file.
        // Keep the original lexical representation; never round-trip through f64.
        validate_decimal_string(&kline.open, "open")?;
        validate_decimal_string(&kline.high, "high")?;
        validate_decimal_string(&kline.low, "low")?;
        validate_decimal_string(&kline.close, "close")?;
        validate_decimal_string(&kline.volume, "volume")?;
        validate_decimal_string(&kline.quote_volume, "quote_volume")?;
        validate_decimal_string(&kline.taker_buy_base, "taker_buy_base")?;
        validate_decimal_string(&kline.taker_buy_quote, "taker_buy_quote")?;

        let file_path = self.get_daily_file_path("klines", &kline.symbol, kline.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            kline.exchange,
            kline.market,
            kline.datatype,
            kline.timestamp_ms,
            Self::iso_utc(kline.timestamp_ms),
            kline.symbol,
            kline.open,
            kline.high,
            kline.low,
            kline.close,
            kline.volume,
            kline.quote_volume,
            kline.trade_count,
            kline.taker_buy_base,
            kline.taker_buy_quote
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_liquidation(&self, liquidation: &LiquidationCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("price", &liquidation.price),
            ("quantity", &liquidation.quantity),
            ("value", &liquidation.value),
        ])?;

        let file_path = self.get_daily_file_path(
            "liquidations",
            &liquidation.symbol,
            liquidation.timestamp_ms,
        );
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            liquidation.exchange,
            liquidation.market,
            liquidation.datatype,
            liquidation.timestamp_ms,
            Self::iso_utc(liquidation.timestamp_ms),
            liquidation.symbol,
            liquidation.side,
            liquidation.price,
            liquidation.quantity,
            liquidation.value
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_funding_rate(&self, funding_rate: &FundingRateCsv) -> Result<()> {
        debug!(
            "write_funding_rate() called for symbol={} at ts={}",
            funding_rate.symbol, funding_rate.timestamp_ms
        );
        validate_decimal_fields(&[("funding_rate", &funding_rate.funding_rate)])?;

        let file_path = self.get_daily_file_path(
            "funding_rates",
            &funding_rate.symbol,
            funding_rate.timestamp_ms,
        );
        debug!("write_funding_rate() file_path={:?}", file_path);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{}",
            funding_rate.exchange,
            funding_rate.market,
            funding_rate.datatype,
            funding_rate.timestamp_ms,
            Self::iso_utc(funding_rate.timestamp_ms),
            funding_rate.symbol,
            funding_rate.funding_rate,
            funding_rate.next_funding_time
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_open_interest(&self, oi: &OpenInterestCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("price", &oi.price),
            ("open_interest", &oi.open_interest),
            ("oi_value", &oi.oi_value),
        ])?;

        let file_path = self.get_daily_file_path("open_interest", &oi.symbol, oi.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{}",
            oi.exchange,
            oi.market,
            oi.datatype,
            oi.timestamp_ms,
            Self::iso_utc(oi.timestamp_ms),
            oi.symbol,
            oi.price,
            oi.open_interest,
            oi.oi_value
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_long_short_ratio(&self, ratio: &LongShortRatioCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("long_ratio", &ratio.long_ratio),
            ("short_ratio", &ratio.short_ratio),
            ("long_short_ratio", &ratio.long_short_ratio),
        ])?;

        let file_path =
            self.get_daily_file_path("long_short_ratio", &ratio.symbol, ratio.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            ratio.exchange,
            ratio.market,
            ratio.datatype,
            ratio.timestamp_ms,
            Self::iso_utc(ratio.timestamp_ms),
            ratio.symbol,
            ratio.period,
            ratio.ratio_type,
            ratio.long_ratio,
            ratio.short_ratio,
            ratio.long_short_ratio
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_mark_price_kline(&self, kline: &MarkPriceKlineCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("open", &kline.open),
            ("high", &kline.high),
            ("low", &kline.low),
            ("close", &kline.close),
        ])?;

        let file_path =
            self.get_daily_file_path("mark_price_klines", &kline.symbol, kline.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            kline.exchange,
            kline.market,
            kline.datatype,
            kline.timestamp_ms,
            Self::iso_utc(kline.timestamp_ms),
            kline.symbol,
            kline.open,
            kline.high,
            kline.low,
            kline.close
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_index_price_kline(&self, kline: &IndexPriceKlineCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("open", &kline.open),
            ("high", &kline.high),
            ("low", &kline.low),
            ("close", &kline.close),
        ])?;

        let file_path =
            self.get_daily_file_path("index_price_klines", &kline.symbol, kline.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            kline.exchange,
            kline.market,
            kline.datatype,
            kline.timestamp_ms,
            Self::iso_utc(kline.timestamp_ms),
            kline.symbol,
            kline.open,
            kline.high,
            kline.low,
            kline.close
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_taker_buy_sell_ratio(&self, ratio: &TakerBuySellVolumeCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("buy_volume", &ratio.buy_volume),
            ("sell_volume", &ratio.sell_volume),
            ("buy_sell_ratio", &ratio.buy_sell_ratio),
        ])?;

        let file_path =
            self.get_daily_file_path("taker_buy_sell_ratio", &ratio.symbol, ratio.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            ratio.exchange,
            ratio.market,
            ratio.datatype,
            ratio.timestamp_ms,
            Self::iso_utc(ratio.timestamp_ms),
            ratio.symbol,
            ratio.period,
            ratio.buy_volume,
            ratio.sell_volume,
            ratio.buy_sell_ratio
        );

        self.write_row(buffer, &row).await
    }

    pub async fn write_premium_index_kline(&self, kline: &PremiumIndexKlineCsv) -> Result<()> {
        validate_decimal_fields(&[
            ("open", &kline.open),
            ("high", &kline.high),
            ("low", &kline.low),
            ("close", &kline.close),
        ])?;

        let file_path =
            self.get_daily_file_path("premium_index_klines", &kline.symbol, kline.timestamp_ms);
        let buffer = self.get_or_create_buffer(&file_path).await?;

        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            kline.exchange,
            kline.market,
            kline.datatype,
            kline.timestamp_ms,
            Self::iso_utc(kline.timestamp_ms),
            kline.symbol,
            kline.open,
            kline.high,
            kline.low,
            kline.close
        );

        self.write_row(buffer, &row).await
    }

    /// Write depth update to in-memory buffer (sorted at flush time)
    /// Emergency flush triggered if buffer exceeds max_depth_buffer_rows
    async fn write_depth_update_buffered(
        &self,
        buffer: Arc<Mutex<FileBuffer>>,
        last_update_id: u64,
        row: &str,
    ) -> Result<()> {
        let row_owned = row.to_string();
        let max_rows = self.max_depth_buffer_rows;
        let buffer_clone = buffer.clone();

        let operation = self.blocking_operations.begin()?;
        let needs_emergency_flush = tokio::task::spawn_blocking(move || -> Result<bool> {
            let _operation = operation;
            let mut buf = buffer_clone.lock();

            // Add to in-memory buffer (unsorted for O(1) append)
            if let Some(ref mut depth_buffer) = buf.depth_update_buffer {
                depth_buffer.push(DepthUpdateRow {
                    last_update_id,
                    csv_line: row_owned,
                });

                // Check if we need emergency flush to prevent unbounded memory growth
                Ok(depth_buffer.len() >= max_rows)
            } else {
                Ok(false)
            }
        })
        .await??;

        // Emergency flush if buffer is too large (prevents memory leak)
        if needs_emergency_flush {
            log::warn!(
                "Emergency flush triggered: depth buffer exceeded {} rows",
                max_rows
            );
            self.flush_buffer(buffer).await?;
        }

        Ok(())
    }

    /// Flush a single buffer (extracted for reuse)
    async fn flush_buffer(&self, buffer: Arc<Mutex<FileBuffer>>) -> Result<()> {
        let operation = self.blocking_operations.begin()?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let _operation = operation;
            buffer.lock().flush_locked()
        })
        .await??;

        Ok(())
    }

    /// Write a row to the buffer (uses spawn_blocking) - for non-depth_update data
    async fn write_row(&self, buffer: Arc<Mutex<FileBuffer>>, row: &str) -> Result<()> {
        let row_owned = row.to_string();
        let flush_interval = self.flush_interval;
        let operation = self.blocking_operations.begin()?;

        tokio::task::spawn_blocking(move || -> Result<()> {
            let _operation = operation;
            let mut buf = buffer.lock();
            writeln!(buf.writer, "{}", row_owned)?;
            buf.row_count += 1;

            let elapsed = buf.last_flush.elapsed();
            if elapsed >= flush_interval || buf.row_count >= 1000 {
                buf.writer.flush()?;
                buf.last_flush = std::time::Instant::now();
                buf.row_count = 0;
            }
            Ok(())
        })
        .await??;

        Ok(())
    }

    /// Force flush all buffers (uses spawn_blocking)
    pub async fn flush_all(&self) -> Result<()> {
        let buffers: Vec<_> = self.buffers.iter().map(|e| e.value().clone()).collect();

        // Attempt every buffer, but fail closed if any flush failed. This is a
        // publication/sealing prerequisite: callers must never infer durability
        // from a successful return while one file still has buffered rows.
        let mut results = Vec::with_capacity(buffers.len());
        for buffer in buffers {
            results.push(self.flush_buffer(buffer).await);
        }
        collect_flush_results(results)
    }

    /// Final flush after shutdown has closed admission and drained all blocking operations.
    /// This is the only post-barrier path allowed to start new filesystem work.
    pub async fn flush_all_after_shutdown(&self) -> Result<()> {
        if !self.blocking_operations.is_closed_and_drained() {
            anyhow::bail!("CSV_BLOCKING_OPERATIONS_NOT_DRAINED");
        }

        let buffers: Vec<_> = self
            .buffers
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let mut results = Vec::with_capacity(buffers.len());
        for buffer in buffers {
            let result =
                tokio::task::spawn_blocking(move || -> Result<()> { buffer.lock().flush_locked() })
                    .await?;
            results.push(result);
        }
        collect_flush_results(results)
    }

    /// Flush specific symbol's buffers (uses spawn_blocking)
    pub async fn flush_symbol(&self, symbol: &str) -> Result<()> {
        let buffers: Vec<_> = self
            .buffers
            .iter()
            .filter(|e| e.key().contains(symbol)) // Symbol already lowercase
            .map(|e| e.value().clone())
            .collect();

        for buffer in buffers {
            self.flush_buffer(buffer).await?;
        }

        Ok(())
    }

    /// Close and finalize a file (used for hourly rotation) (uses spawn_blocking)
    pub async fn finalize_file(&self, file_path: &Path) -> Result<()> {
        let path_str = file_path.to_string_lossy().to_string();

        if let Some((_, buffer)) = self.buffers.remove(&path_str) {
            self.flush_buffer(buffer).await?;
        }

        Ok(())
    }

    /// Cleanup stale file buffers
    /// Removes buffers for:
    /// 1. Files from previous days (should be rotated)
    /// 2. Symbols not written to in last 30 minutes (delisted/inactive)
    pub async fn cleanup_stale_buffers(&self) -> Result<()> {
        let now = chrono::Utc::now();
        let current_date = now.format("%Y-%m-%d").to_string();

        let stale_paths: Vec<String> = self
            .buffers
            .iter()
            .filter_map(|entry| {
                let path_str = entry.key().clone();
                let buffer = entry.value();

                // Check 1: Is this from a previous day? Path layout is
                // <base>/<datatype>/<symbol>/<date>/<file>.csv, so the date is the
                // file's parent directory, so read it positionally and confirm it
                // parses as a date, rather than pattern-matching the first 10-char
                // "20…" segment (a symbol directory could match that first).
                if !path_str.contains(&current_date) {
                    if let Some(date_dir) = std::path::Path::new(&path_str)
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                    {
                        if date_dir != current_date
                            && chrono::NaiveDate::parse_from_str(date_dir, "%Y-%m-%d").is_ok()
                        {
                            log::info!("Marking old date buffer for cleanup: {}", path_str);
                            return Some(path_str);
                        }
                    }
                }

                // Check 2: Has this buffer been idle for 30+ minutes?
                if let Some(buf_guard) = buffer.try_lock() {
                    let elapsed = buf_guard.last_flush.elapsed();
                    if elapsed > std::time::Duration::from_secs(1800) {
                        // 30 min
                        log::info!(
                            "Marking idle buffer for cleanup (idle for {}s): {}",
                            elapsed.as_secs(),
                            path_str
                        );
                        return Some(path_str);
                    }
                }

                None
            })
            .collect();

        if stale_paths.is_empty() {
            log::debug!("No stale CSV buffers to clean up");
            return Ok(());
        }

        log::info!("Cleaning up {} stale CSV file buffers", stale_paths.len());

        for path_str in &stale_paths {
            if let Some((_, buffer)) = self.buffers.remove(path_str) {
                if let Err(e) = self.flush_buffer(buffer).await {
                    log::warn!(
                        "Failed to flush buffer before cleanup: {} - {:?}",
                        path_str,
                        e
                    );
                }
            }
        }

        log::info!("Cleaned up {} stale CSV buffers", stale_paths.len());

        // The pages freed by a mass eviction go back to the OS via the global
        // allocator: on Linux this is jemalloc with a background purge thread
        // (see main.rs and .cargo/config.toml), so no explicit trim is needed.

        Ok(())
    }

    async fn retain_background_task(&self, task: tokio::task::JoinHandle<()>) {
        self.background_tasks.lock().await.push(task);
    }

    /// Stop and join every periodic writer/rotation task before final flush or sealing.
    pub async fn shutdown_background_tasks(&self) -> Result<()> {
        self.shutdown_tx.send_replace(true);

        let tasks = {
            let mut tasks = self.background_tasks.lock().await;
            std::mem::take(&mut *tasks)
        };

        let mut join_errors = Vec::new();
        for task in tasks {
            if let Err(error) = task.await {
                join_errors.push(error.to_string());
            }
        }

        self.blocking_operations.close_and_drain().await?;

        if !join_errors.is_empty() {
            anyhow::bail!(
                "one or more CSV background tasks failed to join: {}",
                join_errors.join("; ")
            );
        }

        Ok(())
    }

    pub async fn start_periodic_flush(&self) -> Result<()> {
        if *self.shutdown_tx.borrow() {
            anyhow::bail!("CSV writer background tasks have already been shut down");
        }

        let writer = self.clone();
        let flush_interval = self.flush_interval;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let task = tokio::spawn(async move {
            let mut interval_timer = interval(flush_interval);

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = interval_timer.tick() => {
                        if let Err(e) = writer.flush_all().await {
                            log::error!("Periodic flush failed: {}", e);
                        }
                    }
                }
            }
        });
        self.retain_background_task(task).await;

        // Periodic cleanup task (every 2 minutes)
        let writer_cleanup = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let cleanup_task = tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(tokio::time::Duration::from_secs(120));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = cleanup_interval.tick() => {
                        if let Err(e) = writer_cleanup.cleanup_stale_buffers().await {
                            log::error!("CSV buffer cleanup failed: {:?}", e);
                        }
                    }
                }
            }
        });
        self.retain_background_task(cleanup_task).await;

        // Rotation-aware cleanup runs seconds after each UTC midnight.
        let writer_midnight = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let midnight_task = tokio::spawn(async move {
            loop {
                let now = Utc::now();
                let next = (now + chrono::Duration::days(1))
                    .date_naive()
                    .and_hms_opt(0, 0, 5)
                    .expect("valid time")
                    .and_utc();
                let wait = (next - now).to_std().unwrap_or(Duration::from_secs(86_400));

                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = tokio::time::sleep(wait) => {
                        log::info!("Midnight rotation: running immediate stale-buffer cleanup");
                        if let Err(e) = writer_midnight.cleanup_stale_buffers().await {
                            log::error!("Midnight buffer cleanup failed: {:?}", e);
                        }
                    }
                }
            }
        });
        self.retain_background_task(midnight_task).await;

        Ok(())
    }

    /// Start hourly rotation task for depth update files.
    pub async fn start_hourly_rotation(&self) -> Result<()> {
        if *self.shutdown_tx.borrow() {
            anyhow::bail!("CSV writer background tasks have already been shut down");
        }

        let writer = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let task = tokio::spawn(async move {
            let now = Utc::now();
            let next_hour = (now + chrono::Duration::hours(1))
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();

            let until_next_hour = (next_hour - now)
                .to_std()
                .unwrap_or(Duration::from_secs(3600));

            tokio::select! {
                _ = shutdown_rx.changed() => return,
                _ = tokio::time::sleep(until_next_hour) => {}
            }

            let mut interval = interval(Duration::from_secs(3600));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = interval.tick() => {
                        let current_hour = Utc::now().hour();
                        let prev_hour = if current_hour == 0 { 23 } else { current_hour - 1 };

                        let mut files_to_finalize = Vec::new();
                        for entry in writer.buffers.iter() {
                            let path_str = entry.key();
                            if path_str.contains("depth_updates")
                                && path_str.contains(&format!("{:02}00-to-{:02}00", prev_hour, current_hour))
                            {
                                files_to_finalize.push(PathBuf::from(path_str.clone()));
                            }
                        }

                        for file_path in files_to_finalize {
                            if let Err(e) = writer.finalize_file(&file_path).await {
                                log::error!("Failed to finalize file {:?}: {}", file_path, e);
                            } else {
                                log::info!("Finalized hourly file: {:?}", file_path);
                            }
                        }
                    }
                }
            }
        });

        self.retain_background_task(task).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AggTradeWs, KlineCsv, OpenInterestCsv};

    /// 2023-11-14T22:13:20.000Z, a fixed, human-checkable instant.
    const TS: i64 = 1_700_000_000_000;

    fn test_writer(dir: &Path) -> CsvWriter {
        let mut config = Config::from_file("config.example.toml").unwrap();
        config.output.base_dir = dir.to_string_lossy().into_owned();
        CsvWriter::new(Arc::new(config)).unwrap()
    }

    fn agg_trade(is_buyer_maker: bool) -> AggTradeWs {
        AggTradeWs {
            event_type: "aggTrade".to_string(),
            event_time: TS,
            symbol: "BTCUSDT".to_string(),
            aggregate_trade_id: 42,
            price: "62456.60".to_string(),
            quantity: "0.017".to_string(),
            first_trade_id: 100,
            last_trade_id: 101,
            trade_time: TS,
            is_buyer_maker,
        }
    }

    #[test]
    fn side_derivation_buyer_maker_means_sell() {
        // is_buyer_maker=true → the taker sold; side must be lowercase.
        let sell = agg_trade(true).to_csv();
        assert_eq!(sell.side, "sell");
        assert_eq!(sell.is_buyer_maker, 1);
        let buy = agg_trade(false).to_csv();
        assert_eq!(buy.side, "buy");
        assert_eq!(buy.is_buyer_maker, 0);
        // Binance's UPPERCASE symbol is normalized on conversion.
        assert_eq!(buy.symbol, "btcusdt");
    }

    #[tokio::test]
    async fn trade_roundtrip_matches_schema_v2() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());

        w.write_trade(&agg_trade(true).to_csv()).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("trades", "btcusdt", TS);
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();

        // Header is the schema constant, verbatim.
        assert_eq!(lines.next().unwrap(), constants::HEADER_TRADES);

        // Row aligns column-for-column with the header.
        let row: Vec<&str> = lines.next().unwrap().split(',').collect();
        assert_eq!(row.len(), constants::HEADER_TRADES.split(',').count());
        assert_eq!(
            &row[..6],
            &[
                "binance",
                "futures",
                "trades",
                "1700000000000",
                "2023-11-14T22:13:20.000Z",
                "btcusdt"
            ]
        );
        assert_eq!(row[6], "42"); // trade_id
        assert_eq!(row[7], "sell"); // side sits BEFORE price/quantity
        assert_eq!(row[8], "62456.60");
        assert_eq!(row[9], "0.017");
    }

    #[tokio::test]
    async fn kline_roundtrip_matches_schema_v2() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());

        let k = KlineCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "klines".to_string(),
            timestamp_ms: TS,
            symbol: "ethusdt".to_string(),
            open: "3000.1".to_string(),
            high: "3001.2".to_string(),
            low: "2999.3".to_string(),
            close: "3000.4".to_string(),
            volume: "12.5".to_string(),
            quote_volume: "37505.0".to_string(),
            trade_count: 87,
            taker_buy_base: "6.0".to_string(),
            taker_buy_quote: "18002.4".to_string(),
        };
        w.write_kline(&k).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("klines", "ethusdt", TS);
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), constants::HEADER_KLINES);
        let row: Vec<&str> = lines.next().unwrap().split(',').collect();
        assert_eq!(row.len(), constants::HEADER_KLINES.split(',').count());
        assert_eq!(row[4], "2023-11-14T22:13:20.000Z");
        // OHLC land in header order.
        assert_eq!(row[6], "3000.1");
        assert_eq!(row[9], "3000.4");
    }

    #[tokio::test]
    async fn open_interest_roundtrip_has_mark_price_column() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());

        let oi = OpenInterestCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "open_interest".to_string(),
            timestamp_ms: TS,
            symbol: "solusdt".to_string(),
            price: "150.0".to_string(),
            open_interest: "1000.0".to_string(),
            oi_value: "150000.0".to_string(),
        };
        w.write_open_interest(&oi).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("open_interest", "solusdt", TS);
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();

        // The schema renamed `price` → `mark_price`; the file must say so.
        let header = lines.next().unwrap();
        assert_eq!(header, constants::HEADER_OPEN_INTEREST);
        assert!(header.contains(",mark_price,"));

        let row: Vec<&str> = lines.next().unwrap().split(',').collect();
        assert_eq!(row[6], "150.0");
        assert_eq!(row[7], "1000.0");
        assert_eq!(row[8], "150000.0");
    }

    #[tokio::test]
    async fn appending_to_existing_file_writes_no_second_header() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());

        w.write_trade(&agg_trade(true).to_csv()).await.unwrap();
        w.write_trade(&agg_trade(false).to_csv()).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("trades", "btcusdt", TS);
        let content = std::fs::read_to_string(&path).unwrap();
        let headers = content
            .lines()
            .filter(|l| *l == constants::HEADER_TRADES)
            .count();
        assert_eq!(headers, 1);
        assert_eq!(content.lines().count(), 3); // header + 2 rows
    }

    #[tokio::test]
    async fn cleanup_evicts_previous_day_buffers_and_flushes_them() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());

        // TS is a 2023 date, so this buffer belongs to a "previous day".
        w.write_trade(&agg_trade(true).to_csv()).await.unwrap();
        assert_eq!(w.buffers.len(), 1);

        w.cleanup_stale_buffers().await.unwrap();
        assert_eq!(w.buffers.len(), 0, "old-date buffer must be evicted");

        // The buffered row was flushed on the way out, not dropped.
        let content =
            std::fs::read_to_string(w.get_daily_file_path("trades", "btcusdt", TS)).unwrap();
        assert_eq!(content.lines().count(), 2); // header + row
    }

    #[test]
    fn iso_utc_formats_utc_millis() {
        assert_eq!(CsvWriter::iso_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            CsvWriter::iso_utc(1_700_000_000_123),
            "2023-11-14T22:13:20.123Z"
        );
    }

    #[test]
    fn daily_paths_partition_by_datatype_symbol_date() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());
        let p = w.get_daily_file_path("trades", "btcusdt", TS);
        let s = p.to_string_lossy();
        assert!(s.ends_with("trades/btcusdt/2023-11-14/btcusdt_trades_2023-11-14.csv"));
    }
    #[tokio::test]
    async fn kline_writer_preserves_exact_source_decimal_lexemes() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());
        let k = KlineCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "klines".to_string(),
            timestamp_ms: TS,
            symbol: "btcusdt".to_string(),
            open: "123456789.123456789123456789".to_string(),
            high: "123456789.123456789123456790".to_string(),
            low: "0.000000000000000000000000000123".to_string(),
            close: "1.230000000000000000".to_string(),
            volume: "0.000000000000000000000000000001".to_string(),
            quote_volume: "987654321.000000000000000001".to_string(),
            trade_count: 7,
            taker_buy_base: "0.000000000000000000000000000009".to_string(),
            taker_buy_quote: "999999999999.000000000000000001".to_string(),
        };

        w.write_kline(&k).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("klines", "btcusdt", TS);
        let content = std::fs::read_to_string(path).unwrap();
        let row: Vec<&str> = content.lines().nth(1).unwrap().split(',').collect();

        assert_eq!(row[6], "123456789.123456789123456789");
        assert_eq!(row[7], "123456789.123456789123456790");
        assert_eq!(row[8], "0.000000000000000000000000000123");
        assert_eq!(row[9], "1.230000000000000000");
        assert_eq!(row[10], "0.000000000000000000000000000001");
        assert_eq!(row[11], "987654321.000000000000000001");
        assert_eq!(row[13], "0.000000000000000000000000000009");
        assert_eq!(row[14], "999999999999.000000000000000001");
    }

    #[tokio::test]
    async fn trade_writer_preserves_exact_source_decimal_precision() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());
        let trade = TradeCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "trades".to_string(),
            timestamp_ms: TS,
            symbol: "btcusdt".to_string(),
            trade_id: 43,
            price: "62456.601234567891234567".to_string(),
            quantity: "0.000000000000000000123456789".to_string(),
            side: "buy".to_string(),
            is_buyer_maker: 0,
            first_update_id: 102,
            last_update_id: 103,
        };

        w.write_trade(&trade).await.unwrap();
        w.flush_all().await.unwrap();

        let path = w.get_daily_file_path("trades", "btcusdt", TS);
        let content = std::fs::read_to_string(path).unwrap();
        let row: Vec<&str> = content.lines().nth(1).unwrap().split(',').collect();
        assert_eq!(row[8], "62456.601234567891234567");
        assert_eq!(row[9], "0.000000000000000000123456789");
    }

    #[tokio::test]
    async fn trade_writer_rejects_malformed_decimal_without_writing_zero() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());
        let trade = TradeCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "trades".to_string(),
            timestamp_ms: TS,
            symbol: "btcusdt".to_string(),
            trade_id: 44,
            price: "not-a-number".to_string(),
            quantity: "0.01".to_string(),
            side: "buy".to_string(),
            is_buyer_maker: 0,
            first_update_id: 104,
            last_update_id: 105,
        };

        let path = w.get_daily_file_path("trades", "btcusdt", TS);
        assert!(w.write_trade(&trade).await.is_err());
        assert!(
            !path.exists(),
            "invalid source decimals must fail before file creation"
        );
    }

    #[test]
    fn decimal_lexical_validation_accepts_exact_decimal_and_exponent_forms() {
        for value in ["0", "-1.2", "+0.3", "1e-8", "1.2300E+4", ".25", "5."] {
            assert!(validate_decimal_string(value, "fixture").is_ok(), "{value}");
        }
    }

    #[test]
    fn decimal_lexical_validation_rejects_non_finite_and_malformed_forms() {
        for value in [
            "NaN",
            "inf",
            "-Infinity",
            "not-a-number",
            "1,2",
            "1\n0",
            "",
            ".",
        ] {
            assert!(
                validate_decimal_string(value, "fixture").is_err(),
                "{value}"
            );
        }
    }

    #[tokio::test]
    async fn kline_writer_rejects_non_decimal_input_without_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let w = test_writer(dir.path());
        let k = KlineCsv {
            exchange: "binance".to_string(),
            market: "futures".to_string(),
            datatype: "klines".to_string(),
            timestamp_ms: TS,
            symbol: "btcusdt".to_string(),
            open: "not-a-number".to_string(),
            high: "3001.2".to_string(),
            low: "2999.3".to_string(),
            close: "3000.4".to_string(),
            volume: "12.5".to_string(),
            quote_volume: "37505.0".to_string(),
            trade_count: 87,
            taker_buy_base: "6.0".to_string(),
            taker_buy_quote: "18002.4".to_string(),
        };

        let path = w.get_daily_file_path("klines", "btcusdt", TS);
        assert!(w.write_kline(&k).await.is_err());
        assert!(
            !path.exists(),
            "invalid numeric input must not create a CSV file"
        );
    }
}

#[cfg(test)]
mod flush_all_error_tests {
    use super::{collect_flush_results, CsvWriter};
    use crate::config::Config;
    use std::path::Path;
    use std::sync::Arc;

    fn test_writer(dir: &Path) -> CsvWriter {
        let mut config = Config::from_file("config.example.toml").unwrap();
        config.output.base_dir = dir.to_string_lossy().into_owned();
        CsvWriter::new(Arc::new(config)).unwrap()
    }

    #[test]
    fn flush_results_propagates_an_error_after_checking_all_buffers() {
        let result = collect_flush_results(vec![Ok(()), Err(anyhow::anyhow!("disk full")), Ok(())]);
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("disk full"));
    }

    #[test]
    fn flush_results_succeeds_when_every_buffer_flushes() {
        assert!(collect_flush_results(vec![Ok(()), Ok(())]).is_ok());
    }

    #[test]
    fn flush_results_succeeds_for_no_buffers() {
        assert!(collect_flush_results(Vec::<anyhow::Result<()>>::new()).is_ok());
    }

    #[tokio::test]
    async fn background_writer_tasks_are_joined_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let writer = test_writer(dir.path());

        writer.start_periodic_flush().await.unwrap();
        writer.start_hourly_rotation().await.unwrap();
        writer.shutdown_background_tasks().await.unwrap();

        let tasks = writer.background_tasks.lock().await;
        assert!(tasks.is_empty(), "shutdown must take and join every task");
    }
}

#[cfg(test)]
mod blocking_operation_barrier_tests {
    use super::CsvWriter;
    use crate::config::Config;
    use std::path::Path;
    use std::sync::Arc;

    fn test_writer(dir: &Path) -> CsvWriter {
        let mut config = Config::from_file("config.example.toml").unwrap();
        config.output.base_dir = dir.to_string_lossy().into_owned();
        CsvWriter::new(Arc::new(config)).unwrap()
    }

    #[tokio::test]
    async fn shutdown_closes_admission_waits_for_in_flight_and_gates_final_flush() {
        let dir = tempfile::tempdir().unwrap();
        let writer = test_writer(dir.path());
        let file_path = writer.get_daily_file_path("trades", "btcusdt", 1_800_000_000_000);
        let buffer = writer.get_or_create_buffer(&file_path).await.unwrap();
        let in_flight = writer.blocking_operations.begin().unwrap();

        let shutdown_writer = writer.clone();
        let shutdown =
            tokio::spawn(async move { shutdown_writer.shutdown_background_tasks().await });

        while !writer.blocking_operations.is_closed() {
            tokio::task::yield_now().await;
        }

        assert!(
            writer.blocking_operations.begin().is_err(),
            "shutdown must reject new blocking operations"
        );
        assert!(
            !shutdown.is_finished(),
            "shutdown must wait for an already-admitted blocking operation"
        );
        assert!(
            writer.flush_all_after_shutdown().await.is_err(),
            "final flush must not begin before the in-flight barrier drains"
        );

        drop(in_flight);
        shutdown.await.unwrap().unwrap();
        assert!(
            writer.write_row(buffer, "late row").await.is_err(),
            "writes must be rejected after shutdown closes blocking-operation admission"
        );
        writer.flush_all_after_shutdown().await.unwrap();
    }
}
