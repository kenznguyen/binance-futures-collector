use anyhow::Result;
use dashmap::{DashMap, DashSet};
use futures_util::future::join_all;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};

use crate::config::Config;
use crate::models::*;
use crate::rest::RestClient;
use crate::utils::{multiply_decimal_strings, time_utils, CsvWriter};

/// Extract the timestamp and four source decimal strings from a REST kline array.
/// Invalid/missing fields are rejected instead of being fabricated as numeric zero.
fn extract_rest_kline(data: &[serde_json::Value]) -> Option<(i64, &str, &str, &str, &str)> {
    if data.len() < 5 {
        return None;
    }
    Some((
        data[0].as_i64()?,
        data[1].as_str()?,
        data[2].as_str()?,
        data[3].as_str()?,
        data[4].as_str()?,
    ))
}

/// Buckets to fetch per `/futures/data` poll. These endpoints are limiter-paced
/// (~20-min effective cadence for ~500 symbols on one IP); fetching 8× 5-min
/// buckets (40 min) comfortably bridges that gap so nothing is missed. This is
/// FREE: the limiter counts requests, not buckets, so a bigger `limit` returns
/// more history at no rate-limit cost. Overlap is de-duplicated.
const DATA_BUCKET_LIMIT: u32 = 8;
/// Buckets to fetch per klines poll (1-min data). Writes the closed minutes with
/// margin (skips the in-progress final bucket); 4 covers the ~90s+jitter poll gap.
const KLINE_BUCKET_LIMIT: u32 = 4;

/// Collects REST-based data (funding rates, OI, ratios, etc.)
#[derive(Clone, Default)]
struct RestTaskGroup {
    handles: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl RestTaskGroup {
    async fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        self.handles.lock().await.push(handle);
    }

    async fn shutdown(&self) {
        let handles = {
            let mut guard = self.handles.lock().await;
            std::mem::take(&mut *guard)
        };
        for handle in &handles {
            handle.abort();
        }
        for handle in handles {
            if let Err(error) = handle.await {
                if !error.is_cancelled() {
                    log::error!("REST collector task join failed: {}", error);
                }
            }
        }
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.handles.lock().await.len()
    }
}

pub struct RestCollector {
    config: Arc<Config>,
    rest_client: Arc<RestClient>,
    csv_writer: Arc<CsvWriter>,
    /// Last-written bucket timestamp per "{datatype}:{symbol}", which lets us poll with
    /// `limit>1` (to capture every native bucket at a slow, ban-safe poll rate)
    /// without double-writing buckets that overlapping polls both return. `Arc` so
    /// all clones of the collector share one dedup map.
    last_bucket: Arc<DashMap<String, i64>>,
    /// Symbols confirmed delisted by periodic discovery (shared with main and
    /// the order-book manager); every per-symbol poll skips members so a dead
    /// market stops consuming retries and rate-limit budget.
    delisted: Arc<DashSet<String>>,
    tasks: RestTaskGroup,
}

impl RestCollector {
    pub fn new(
        config: Arc<Config>,
        rest_client: Arc<RestClient>,
        csv_writer: Arc<CsvWriter>,
        delisted: Arc<DashSet<String>>,
    ) -> Self {
        Self {
            config,
            rest_client,
            csv_writer,
            last_bucket: Arc::new(DashMap::new()),
            delisted,
            tasks: RestTaskGroup::default(),
        }
    }

    /// Returns true (and records) only if `ts` is newer than the last bucket
    /// already written for `key`. Each (datatype, symbol) key is touched by one
    /// task at a time, so the get-then-insert is race-free here.
    fn is_new_bucket(&self, key: &str, ts: i64) -> bool {
        if let Some(prev) = self.last_bucket.get(key) {
            if *prev >= ts {
                return false;
            }
        }
        self.last_bucket.insert(key.to_string(), ts);
        true
    }

    pub async fn start(&self, symbols: Vec<String>) -> Result<()> {
        info!("Starting REST collectors for {} symbols", symbols.len());

        // Stagger collector startup to prevent a rate-limit burst. Without delays,
        // all 8 collectors fire their first batch simultaneously (~96 requests in 2s);
        // spreading over 70 seconds keeps us under the 2400 req/min limit at startup.
        info!("Staggering collector startup over 70 seconds to prevent rate limiting");

        // Start funding rate collector (300s interval) - 0s delay
        // This makes 1 bulk request for all symbols, so start immediately
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            if let Err(e) = collector.collect_funding_rates(symbols_clone).await {
                error!("Funding rate collector error: {}", e);
            }
        }).await;

        // Start mark price klines collector (90s interval) - 10s delay
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(10)).await;
            if let Err(e) = collector.collect_mark_price_klines(symbols_clone).await {
                error!("Mark price klines collector error: {}", e);
            }
        }).await;

        // Start index price klines collector (90s interval) - 20s delay
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(20)).await;
            if let Err(e) = collector.collect_index_price_klines(symbols_clone).await {
                error!("Index price klines collector error: {}", e);
            }
        }).await;

        // Start premium index klines collector (60s interval) - 30s delay
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(30)).await;
            if let Err(e) = collector.collect_premium_index_klines(symbols_clone).await {
                error!("Premium index klines collector error: {}", e);
            }
        }).await;

        // Start open interest collector (300s interval) - 40s delay
        // Makes 2 requests per symbol (OI + premium index for price)
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(40)).await;
            if let Err(e) = collector.collect_open_interest(symbols_clone).await {
                error!("Open interest collector error: {}", e);
            }
        }).await;

        // Start taker buy/sell ratio collector (300s interval) - 50s delay
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(50)).await;
            if let Err(e) = collector.collect_taker_buy_sell_ratios(symbols_clone).await {
                error!("Taker buy/sell ratio collector error: {}", e);
            }
        }).await;

        // Start long/short ratio collector (300s interval) - 60s delay
        // Makes 3 requests per symbol (global + 2 trader ratios), heaviest load
        let collector = self.clone();
        let symbols_clone = symbols.clone();
        self.tasks.spawn(async move {
            sleep(Duration::from_secs(60)).await;
            if let Err(e) = collector.collect_long_short_ratios(symbols_clone).await {
                error!("Long/short ratio collector error: {}", e);
            }
        }).await;

        Ok(())
    }

    /// Stop every background REST collector and wait until no task can write.
    pub async fn shutdown(&self) {
        self.tasks.shutdown().await;
    }

    /// Collect funding rates (includes mark/index prices)
    async fn collect_funding_rates(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.funding_rates_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        debug!(
            "Funding rate collector initialized with {}s interval",
            interval_secs
        );

        loop {
            debug!("Funding rate collector: waiting for tick...");
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "Funding rate: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            debug!("Funding rate collector: tick received!");
            let start = std::time::Instant::now();

            info!("Collecting funding rates for {} symbols", symbols.len());

            match self.rest_client.get_premium_index(None).await {
                Ok(all_rates) => {
                    let timestamp = time_utils::get_current_timestamp_ms();

                    debug!(
                        "API returned {} funding rates. First 3 symbols from API: {:?}",
                        all_rates.len(),
                        all_rates
                            .iter()
                            .take(3)
                            .map(|r| &r.symbol)
                            .collect::<Vec<_>>()
                    );
                    debug!(
                        "Symbol filter list has {} symbols. First 3: {:?}",
                        symbols.len(),
                        symbols.iter().take(3).collect::<Vec<_>>()
                    );

                    let mut matched_count = 0;
                    for rate in &all_rates {
                        // symbols list is lowercase (from discovery), API returns uppercase - convert for comparison
                        let sym = rate.symbol.to_lowercase();
                        // Skip symbols torn down mid-run, matching the per-symbol
                        // REST lanes. Note bulk premiumIndex can still return a
                        // pair while it is halted (non-TRADING), so this check,
                        // not the response contents, is what actually gates it.
                        if symbols.contains(&sym) && !self.delisted.contains(&sym) {
                            matched_count += 1;
                            let csv = FundingRateCsv {
                                exchange: "binance".to_string(),
                                market: "futures".to_string(),
                                datatype: "funding_rates".to_string(),
                                timestamp_ms: timestamp,
                                symbol: rate.symbol.to_lowercase(), // CSV uses lowercase
                                funding_rate: format_decimal_str(&rate.last_funding_rate),
                                next_funding_time: rate.next_funding_time,
                            };

                            if let Err(e) = self.csv_writer.write_funding_rate(&csv).await {
                                error!("Failed to write funding rate for {}: {}", csv.symbol, e);
                            }
                        }
                    }

                    debug!(
                        "Matched {} symbols out of {} returned from API",
                        matched_count,
                        all_rates.len()
                    );
                    info!("Funding rates collected in {:?}", start.elapsed());
                }
                Err(e) => {
                    error!("Failed to fetch funding rates: {}", e);
                }
            }
        }
    }

    async fn collect_open_interest(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.open_interest_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!("OI: applying {}s jitter to prevent burst", jitter_secs);
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!("Collecting open interest for {} symbols", symbols.len());

            // Process in batches of 5 to prevent rate limiting.
            // Each symbol makes 2 API calls (OI + premium), so 5 symbols = 10 parallel requests
            const BATCH_SIZE: usize = 5;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing OI batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_open_interest_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                // 3 second delay between batches to prevent rate limiting
                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(3)).await;
                }
            }

            info!(
                "Open interest collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_open_interest_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        match self.rest_client.get_open_interest(&symbol).await {
            Ok(oi_data) => {
                // Mark price comes from premiumIndex and is needed for the notional
                // (oi_value). If it's unavailable, skip this row rather than write a
                // fabricated 0 price/value; a consumer can't tell a fabricated zero
                // from a genuine one.
                let mark_price = match self.rest_client.get_premium_index(Some(&symbol)).await {
                    Ok(rates) => match rates.first() {
                        Some(r) => r.mark_price.clone(),
                        None => {
                            warn!(
                                "Skipping open interest for {}: premiumIndex returned no data",
                                symbol
                            );
                            return;
                        }
                    },
                    Err(e) => {
                        warn!(
                            "Skipping open interest for {}: mark price unavailable: {}",
                            symbol, e
                        );
                        return;
                    }
                };

                let value = match multiply_decimal_strings(&oi_data.open_interest, &mark_price) {
                    Ok(value) => value,
                    Err(e) => {
                        warn!(
                            "Skipping open interest for {}: invalid source decimal: {}",
                            symbol, e
                        );
                        return;
                    }
                };

                let csv = OpenInterestCsv {
                    exchange: "binance".to_string(),
                    market: "futures".to_string(),
                    datatype: "open_interest".to_string(),
                    timestamp_ms: oi_data.time,
                    symbol: oi_data.symbol.to_lowercase(),
                    price: format_decimal_str(&mark_price),
                    open_interest: format_decimal_str(&oi_data.open_interest),
                    oi_value: value,
                };

                if let Err(e) = self.csv_writer.write_open_interest(&csv).await {
                    error!("Failed to write open interest for {}: {}", csv.symbol, e);
                }
            }
            Err(e) => {
                warn!("Failed to fetch open interest for {}: {}", symbol, e);
            }
        }
    }

    async fn collect_long_short_ratios(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.long_short_ratio_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "L/S ratio: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!("Collecting long/short ratios for {} symbols", symbols.len());

            const BATCH_SIZE: usize = 10;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing L/S ratio batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_long_short_ratios_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(2)).await;
                }
            }

            info!(
                "Long/short ratios collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_long_short_ratios_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        let global_fut =
            self.rest_client
                .get_global_long_short_ratio(&symbol, "5m", DATA_BUCKET_LIMIT);
        let account_fut =
            self.rest_client
                .get_top_trader_account_ratio(&symbol, "5m", DATA_BUCKET_LIMIT);
        let position_fut =
            self.rest_client
                .get_top_trader_position_ratio(&symbol, "5m", DATA_BUCKET_LIMIT);

        let (global_result, account_result, position_result) =
            tokio::join!(global_fut, account_fut, position_fut);

        let ratio_futures = vec![
            ("global", global_result),
            ("top_trader_account", account_result),
            ("top_trader_position", position_result),
        ];

        for (ratio_type, result) in ratio_futures {
            match result {
                Ok(ratios) => {
                    let key = format!("ls:{}:{}", ratio_type, symbol);
                    for ratio in &ratios {
                        if !self.is_new_bucket(&key, ratio.timestamp) {
                            continue;
                        }
                        let (long_ratio, short_ratio) = match (
                            ratio.long_account.as_ref(),
                            ratio.short_account.as_ref(),
                        ) {
                            (Some(long), Some(short)) => (long.clone(), short.clone()),
                            _ => {
                                warn!(
                                    "Skipping {} ratio for {} at {}: source long/short account values are missing",
                                    ratio_type, symbol, ratio.timestamp
                                );
                                continue;
                            }
                        };

                        let csv = LongShortRatioCsv {
                            exchange: "binance".to_string(),
                            market: "futures".to_string(),
                            datatype: "long_short_ratio".to_string(),
                            timestamp_ms: ratio.timestamp,
                            symbol: symbol.clone(),
                            period: "5m".to_string(),
                            ratio_type: ratio_type.to_string(),
                            long_ratio: format_decimal_str(&long_ratio),
                            short_ratio: format_decimal_str(&short_ratio),
                            long_short_ratio: format_decimal_str(&ratio.long_short_ratio),
                        };

                        if let Err(e) = self.csv_writer.write_long_short_ratio(&csv).await {
                            error!("Failed to write {} ratio for {}: {}", ratio_type, symbol, e);
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to fetch {} ratio for {}: {}", ratio_type, symbol, e);
                }
            }
        }
    }

    async fn collect_mark_price_klines(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.mark_price_klines_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "Mark klines: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!("Collecting mark price klines for {} symbols", symbols.len());

            const BATCH_SIZE: usize = 10;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing mark klines batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_mark_price_klines_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(2)).await;
                }
            }

            info!(
                "Mark price klines collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_mark_price_klines_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        match self
            .rest_client
            .get_mark_price_klines(&symbol, "1m", KLINE_BUCKET_LIMIT)
            .await
        {
            Ok(klines) => {
                let key = format!("mark_price_klines:{}", symbol);
                // Skip the final (in-progress) bucket; write each newly-closed one.
                let closed = klines.len().saturating_sub(1);
                for kline_data in klines.iter().take(closed) {
                    let Some((timestamp, open, high, low, close)) = extract_rest_kline(kline_data)
                    else {
                        warn!("Skipping malformed mark price kline for {}", symbol);
                        continue;
                    };
                    if !self.is_new_bucket(&key, timestamp) {
                        continue;
                    }
                    let csv = MarkPriceKlineCsv {
                        exchange: "binance".to_string(),
                        market: "futures".to_string(),
                        datatype: "mark_price_klines".to_string(),
                        timestamp_ms: timestamp,
                        symbol: symbol.clone(),
                        open: format_decimal_str(open),
                        high: format_decimal_str(high),
                        low: format_decimal_str(low),
                        close: format_decimal_str(close),
                    };
                    if let Err(e) = self.csv_writer.write_mark_price_kline(&csv).await {
                        error!("Failed to write mark price kline for {}: {}", symbol, e);
                    }
                }
            }
            Err(e) => {
                debug!("Failed to fetch mark price klines for {}: {}", symbol, e);
            }
        }
    }

    async fn collect_index_price_klines(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.index_price_klines_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "Index klines: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!(
                "Collecting index price klines for {} symbols",
                symbols.len()
            );

            const BATCH_SIZE: usize = 10;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing index klines batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_index_price_klines_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(2)).await;
                }
            }

            info!(
                "Index price klines collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_index_price_klines_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        match self
            .rest_client
            .get_index_price_klines(&symbol, "1m", KLINE_BUCKET_LIMIT)
            .await
        {
            Ok(klines) => {
                let key = format!("index_price_klines:{}", symbol);
                let closed = klines.len().saturating_sub(1);
                for kline_data in klines.iter().take(closed) {
                    let Some((timestamp, open, high, low, close)) = extract_rest_kline(kline_data)
                    else {
                        warn!("Skipping malformed index price kline for {}", symbol);
                        continue;
                    };
                    if !self.is_new_bucket(&key, timestamp) {
                        continue;
                    }
                    let csv = IndexPriceKlineCsv {
                        exchange: "binance".to_string(),
                        market: "futures".to_string(),
                        datatype: "index_price_klines".to_string(),
                        timestamp_ms: timestamp,
                        symbol: symbol.clone(),
                        open: format_decimal_str(open),
                        high: format_decimal_str(high),
                        low: format_decimal_str(low),
                        close: format_decimal_str(close),
                    };
                    if let Err(e) = self.csv_writer.write_index_price_kline(&csv).await {
                        error!("Failed to write index price kline for {}: {}", symbol, e);
                    }
                }
            }
            Err(e) => {
                debug!("Failed to fetch index price klines for {}: {}", symbol, e);
            }
        }
    }

    async fn collect_taker_buy_sell_ratios(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.taker_buy_sell_ratio_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "Taker ratio: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!(
                "Collecting taker buy/sell ratios for {} symbols",
                symbols.len()
            );

            // Process in batches of 10 (like other ratios)
            const BATCH_SIZE: usize = 10;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing taker ratio batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_taker_buy_sell_ratio_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(2)).await;
                }
            }

            info!(
                "Taker buy/sell ratios collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_taker_buy_sell_ratio_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        match self
            .rest_client
            .get_taker_buy_sell_ratio(&symbol, "5m", DATA_BUCKET_LIMIT)
            .await
        {
            Ok(ratios) => {
                let key = format!("taker:{}", symbol);
                // Buckets are ascending (oldest→newest); write only the new ones.
                for ratio in &ratios {
                    if !self.is_new_bucket(&key, ratio.timestamp) {
                        continue;
                    }
                    let csv = TakerBuySellVolumeCsv {
                        exchange: "binance".to_string(),
                        market: "futures".to_string(),
                        datatype: "taker_buy_sell_ratio".to_string(),
                        timestamp_ms: ratio.timestamp,
                        symbol: symbol.clone(),
                        period: "5m".to_string(),
                        buy_volume: format_decimal_str(&ratio.buy_vol),
                        sell_volume: format_decimal_str(&ratio.sell_vol),
                        buy_sell_ratio: format_decimal_str(&ratio.buy_sell_ratio),
                    };
                    if let Err(e) = self.csv_writer.write_taker_buy_sell_ratio(&csv).await {
                        error!("Failed to write taker buy/sell ratio for {}: {}", symbol, e);
                    }
                }
            }
            Err(e) => {
                debug!("Failed to fetch taker buy/sell ratio for {}: {}", symbol, e);
            }
        }
    }

    async fn collect_premium_index_klines(&self, symbols: Vec<String>) -> Result<()> {
        let interval_secs = self.config.rest_intervals.premium_index_klines_seconds;
        let mut interval = interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // Add random jitter (0-30s) to prevent interval re-alignment
            let jitter_secs = rand::random::<u64>() % 31;
            if jitter_secs > 0 {
                debug!(
                    "Premium klines: applying {}s jitter to prevent burst",
                    jitter_secs
                );
                sleep(Duration::from_secs(jitter_secs)).await;
            }

            let start = std::time::Instant::now();

            info!(
                "Collecting premium index klines for {} symbols",
                symbols.len()
            );

            const BATCH_SIZE: usize = 10;
            let total_batches = symbols.len().div_ceil(BATCH_SIZE);

            for (batch_idx, batch) in symbols.chunks(BATCH_SIZE).enumerate() {
                info!(
                    "Processing premium klines batch {}/{} ({} symbols)",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let futures: Vec<_> = batch
                    .iter()
                    .map(|symbol| self.collect_premium_index_klines_single(symbol.clone()))
                    .collect();

                join_all(futures).await;

                if batch_idx < total_batches - 1 {
                    sleep(Duration::from_secs(2)).await;
                }
            }

            info!(
                "Premium index klines collected for all {} symbols in {:?}",
                symbols.len(),
                start.elapsed()
            );
        }
    }

    async fn collect_premium_index_klines_single(&self, symbol: String) {
        if self.delisted.contains(&symbol) {
            return; // delisted mid-run: skip rather than burn budget on a dead market
        }
        match self
            .rest_client
            .get_premium_index_klines(&symbol, "1m", KLINE_BUCKET_LIMIT)
            .await
        {
            Ok(klines) => {
                let key = format!("premium_index_klines:{}", symbol);
                let closed = klines.len().saturating_sub(1);
                for kline_data in klines.iter().take(closed) {
                    let Some((timestamp, open, high, low, close)) = extract_rest_kline(kline_data)
                    else {
                        warn!("Skipping malformed premium index kline for {}", symbol);
                        continue;
                    };
                    if !self.is_new_bucket(&key, timestamp) {
                        continue;
                    }
                    let csv = PremiumIndexKlineCsv {
                        exchange: "binance".to_string(),
                        market: "futures".to_string(),
                        datatype: "premium_index_klines".to_string(),
                        timestamp_ms: timestamp,
                        symbol: symbol.clone(),
                        open: format_decimal_str(open),
                        high: format_decimal_str(high),
                        low: format_decimal_str(low),
                        close: format_decimal_str(close),
                    };
                    if let Err(e) = self.csv_writer.write_premium_index_kline(&csv).await {
                        error!("Failed to write premium index kline for {}: {}", symbol, e);
                    }
                }
            }
            Err(e) => {
                debug!("Failed to fetch premium index klines for {}: {}", symbol, e);
            }
        }
    }
}

impl Clone for RestCollector {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            rest_client: self.rest_client.clone(),
            csv_writer: self.csv_writer.clone(),
            last_bucket: self.last_bucket.clone(),
            delisted: self.delisted.clone(),
            tasks: self.tasks.clone(),
        }
    }
}

#[cfg(test)]
mod rest_decimal_tests {
    use super::extract_rest_kline;
    use serde_json::json;

    #[test]
    fn rest_kline_extract_preserves_source_decimal_strings() {
        let data = vec![
            json!(1_700_000_000_000i64),
            json!("123456789.123456789123456789"),
            json!("123456790.000000000000000001"),
            json!("123456788.000000000000000001"),
            json!("123456789.500000000000000001"),
        ];
        let (timestamp, open, high, low, close) = extract_rest_kline(&data).unwrap();
        assert_eq!(timestamp, 1_700_000_000_000);
        assert_eq!(open, "123456789.123456789123456789");
        assert_eq!(high, "123456790.000000000000000001");
        assert_eq!(low, "123456788.000000000000000001");
        assert_eq!(close, "123456789.500000000000000001");
    }

    #[test]
    fn rest_kline_extract_rejects_missing_or_wrongly_typed_fields() {
        assert!(extract_rest_kline(&[json!(1), json!("1"), json!("2")]).is_none());
        assert!(
            extract_rest_kline(&[json!(1), json!(1.25), json!("2"), json!("1"), json!("1.5")])
                .is_none()
        );
        assert!(extract_rest_kline(&[
            json!("bad-ts"),
            json!("1"),
            json!("2"),
            json!("1"),
            json!("1.5")
        ])
        .is_none());
    }
}

#[cfg(test)]
mod rest_task_group_tests {
    use super::RestTaskGroup;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn shutdown_aborts_and_joins_every_registered_task() {
        let tasks = RestTaskGroup::default();
        let (started_tx, started_rx) = oneshot::channel();
        tasks
            .spawn(async move {
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .await;

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("task should start")
            .expect("start signal should be delivered");

        tasks.shutdown().await;
        assert_eq!(tasks.len().await, 0);
    }

    #[tokio::test]
    async fn shutdown_waits_for_task_cancellation_to_finish() {
        let tasks = RestTaskGroup::default();
        let (started_tx, started_rx) = oneshot::channel();
        tasks
            .spawn(async move {
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .await;

        started_rx.await.expect("task should start");
        tasks.shutdown().await;
        assert_eq!(tasks.len().await, 0);
    }
}
