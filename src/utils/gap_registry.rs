use anyhow::Result;
use dashmap::DashMap;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use tokio::sync::{watch, Mutex};
use tokio::time::interval;

/// Resnapshot event, recorded when a detected gap/staleness triggers a fresh
/// order-book snapshot. Gap *backfilling* was removed by design: gaps are
/// recovered by resnapshot, and the raw depth CSV preserves every update's
/// `U`/`u`/`pu` IDs for offline gap analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapEvent {
    #[serde(rename = "type")]
    pub event_type: String, // "resnapshot"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
}

impl GapEvent {
    pub fn new_resnapshot(reason: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let datetime_utc = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "unknown".to_string());

        Self {
            event_type: "resnapshot".to_string(),
            reason: Some(reason),
            timestamp: Some(timestamp),
            datetime_utc: Some(datetime_utc),
        }
    }
}

/// Gap registry for persisting gap events to disk
pub struct GapRegistry {
    gaps_dir: PathBuf,
    registry: Arc<DashMap<String, Vec<GapEvent>>>,
    dirty_symbols: Arc<Mutex<HashSet<String>>>,
    shutdown_tx: watch::Sender<bool>,
    periodic_save_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl GapRegistry {
    pub async fn new(gaps_dir: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&gaps_dir).await?;

        let (shutdown_tx, _) = watch::channel(false);

        Ok(Self {
            gaps_dir,
            registry: Arc::new(DashMap::new()),
            dirty_symbols: Arc::new(Mutex::new(HashSet::new())),
            shutdown_tx,
            periodic_save_task: Arc::new(Mutex::new(None)),
        })
    }

    /// Start periodic save task (every 60 seconds).
    /// The task is retained so shutdown can stop and join it before final persistence.
    pub async fn start_periodic_save(&self) -> Result<()> {
        let mut task_slot = self.periodic_save_task.lock().await;
        if task_slot.is_some() {
            return Ok(());
        }
        if *self.shutdown_tx.borrow() {
            anyhow::bail!("gap registry has already been shut down");
        }

        let registry = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        *task_slot = Some(tokio::spawn(async move {
            let mut interval_timer = interval(std::time::Duration::from_secs(60));

            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = interval_timer.tick() => {
                        if let Err(e) = registry.save_all_dirty().await {
                            error!("Failed to save dirty gap data: {}", e);
                        }
                    }
                }
            }
        }));

        info!("Gap registry periodic save task started");
        Ok(())
    }

    /// Stop and join the periodic saver, then persist every remaining dirty symbol.
    pub async fn shutdown(&self) -> Result<()> {
        self.shutdown_tx.send_replace(true);

        let task = self.periodic_save_task.lock().await.take();
        if let Some(task) = task {
            task.await
                .map_err(|error| anyhow::anyhow!("gap saver task join failed: {error}"))?;
        }

        self.save_all_dirty().await
    }

    pub async fn record_resnapshot(&self, symbol: &str, reason: String) {
        let event = GapEvent::new_resnapshot(reason);
        self.add_event(symbol, event).await;
    }

    async fn add_event(&self, symbol: &str, event: GapEvent) {
        // Load from disk if not in memory
        if !self.registry.contains_key(symbol) {
            if let Err(e) = self.load_from_disk(symbol).await {
                error!("Failed to load gap data for {}: {}", symbol, e);
            }
        }

        // Add event, bounding the per-symbol history so a run of any length can't
        // grow this unboundedly (the one push-only structure in the collector).
        // The raw depth CSV preserves every U/u/pu for full offline gap analysis,
        // so the registry only needs recent events.
        const MAX_EVENTS_PER_SYMBOL: usize = 512;
        {
            let mut entry = self.registry.entry(symbol.to_string()).or_default();
            entry.push(event);
            if entry.len() > MAX_EVENTS_PER_SYMBOL {
                let excess = entry.len() - MAX_EVENTS_PER_SYMBOL;
                entry.drain(0..excess);
            }
        }

        let mut dirty = self.dirty_symbols.lock().await;
        dirty.insert(symbol.to_string());
    }

    async fn load_from_disk(&self, symbol: &str) -> Result<()> {
        let filepath = self.gaps_dir.join(format!("{}_gaps.json", symbol));

        if filepath.exists() {
            let content = fs::read_to_string(&filepath).await?;
            let events: Vec<GapEvent> = serde_json::from_str(&content)?;
            self.registry.insert(symbol.to_string(), events);
        } else {
            self.registry.insert(symbol.to_string(), Vec::new());
        }

        Ok(())
    }

    async fn save_to_disk(&self, symbol: &str) -> Result<()> {
        let filepath = self.gaps_dir.join(format!("{}_gaps.json", symbol));

        // Clone + drop the DashMap shard guard BEFORE awaiting disk I/O. Holding a
        // shard guard across .await pins tokio worker threads (parking_lot lock) and
        // can stall the whole runtime under load.
        let events = match self.registry.get(symbol) {
            Some(e) => e.clone(),
            None => return Ok(()),
        };

        // Write to temp file first, then atomic rename.
        let temp_filepath = filepath.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&events)?;
        fs::write(&temp_filepath, json).await?;
        fs::rename(&temp_filepath, &filepath).await?;

        Ok(())
    }

    pub async fn save_all_dirty(&self) -> Result<()> {
        let dirty_copy = {
            let mut dirty = self.dirty_symbols.lock().await;
            let copy: Vec<String> = dirty.iter().cloned().collect();
            dirty.clear();
            copy
        };

        if dirty_copy.is_empty() {
            return Ok(());
        }

        info!("Saving gap data for {} dirty symbols...", dirty_copy.len());

        let mut save_results = Vec::with_capacity(dirty_copy.len());
        for symbol in &dirty_copy {
            save_results.push((symbol.clone(), self.save_to_disk(symbol).await));
        }

        let (failed_symbols, error) = collect_failed_gap_saves(save_results);
        if !failed_symbols.is_empty() {
            // A failed write must remain dirty so a later periodic/final save can retry it.
            let mut dirty = self.dirty_symbols.lock().await;
            dirty.extend(failed_symbols);
        }

        if let Some(error) = error {
            return Err(error);
        }

        info!("Finished saving dirty gap data");
        Ok(())
    }
}

fn collect_failed_gap_saves(
    results: impl IntoIterator<Item = (String, Result<()>)>,
) -> (Vec<String>, Option<anyhow::Error>) {
    let mut failed_symbols = Vec::new();
    let mut errors = Vec::new();

    for (symbol, result) in results {
        if let Err(error) = result {
            failed_symbols.push(symbol.clone());
            errors.push(format!("{symbol}: {error:#}"));
        }
    }

    let error = if errors.is_empty() {
        None
    } else {
        Some(anyhow::anyhow!(
            "failed to persist gap data for {} symbol(s): {}",
            errors.len(),
            errors.join("; ")
        ))
    };

    (failed_symbols, error)
}

impl Clone for GapRegistry {
    fn clone(&self) -> Self {
        Self {
            gaps_dir: self.gaps_dir.clone(),
            registry: self.registry.clone(),
            dirty_symbols: self.dirty_symbols.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            periodic_save_task: self.periodic_save_task.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_gap_saves_are_reported_and_remain_retryable() {
        let results = vec![
            ("btcusdt".to_string(), Ok(())),
            (
                "ethusdt".to_string(),
                Err(anyhow::anyhow!("simulated disk write failure")),
            ),
        ];

        let (failed_symbols, error) = collect_failed_gap_saves(results);

        assert_eq!(failed_symbols, vec!["ethusdt".to_string()]);
        assert!(error.is_some(), "a failed gap save must not report success");
    }

    #[test]
    fn all_successful_gap_saves_return_no_failures() {
        let results = vec![
            ("btcusdt".to_string(), Ok(())),
            ("ethusdt".to_string(), Ok(())),
        ];

        let (failed_symbols, error) = collect_failed_gap_saves(results);

        assert!(failed_symbols.is_empty());
        assert!(error.is_none());
    }

    #[tokio::test]
    async fn shutdown_joins_periodic_saver_and_persists_remaining_dirty_events() {
        let dir = tempfile::tempdir().unwrap();
        let registry = GapRegistry::new(dir.path().to_path_buf()).await.unwrap();

        registry
            .record_resnapshot("btcusdt", "test shutdown".to_string())
            .await;
        registry.start_periodic_save().await.unwrap();
        registry.shutdown().await.unwrap();

        let path = dir.path().join("btcusdt_gaps.json");
        let content = tokio::fs::read_to_string(path).await.unwrap();
        let events: Vec<GapEvent> = serde_json::from_str(&content).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "resnapshot");
    }
}
