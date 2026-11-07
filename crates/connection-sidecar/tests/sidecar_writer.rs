use collector_connection_sidecar::{
    ConnectFailureReason, ConnectionIdentity, DisconnectReason, Route, SidecarWriter,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "bc4f0ef633fbeee4a49defd72827f17162693805";
const SESSION: &str = "offline-session-001";
const START: u64 = 1_800_000_000_000;

fn temp_path(label: &str) -> PathBuf {
    let id = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "t1-sidecar-{label}-{}-{id}.jsonl",
        std::process::id()
    ))
}

fn identity(route: Route) -> ConnectionIdentity {
    ConnectionIdentity {
        connection_id: "socket-public-001".to_string(),
        batch_id: 0,
        route,
        subscriptions: vec!["btcusdt@depth@100ms".to_string()],
    }
}

#[test]
fn writes_python_contract_jsonl_and_fsyncs_each_event_before_returning() {
    let path = temp_path("durable");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();

    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    assert_eq!(first.lines().count(), 2);
    let attempted: serde_json::Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    assert_eq!(attempted["eventType"], "CONNECT_ATTEMPTED");
    let connected: serde_json::Value = serde_json::from_str(first.lines().nth(1).unwrap()).unwrap();
    assert_eq!(connected["schemaVersion"], 1);
    assert_eq!(connected["sourceCommit"], SOURCE);
    assert_eq!(connected["sessionId"], SESSION);
    assert_eq!(connected["connectionId"], "socket-public-001");
    assert_eq!(connected["batchId"], 0);
    assert_eq!(connected["route"], "public");
    assert_eq!(connected["eventType"], "CONNECTED");
    assert_eq!(connected["eventAtMs"], START + 1);
    assert_eq!(connected["subscriptionCount"], 1);
    assert_eq!(
        connected["subscriptionSetSha256"].as_str().unwrap().len(),
        64
    );
    assert!(connected["eventId"].as_str().unwrap().contains(SESSION));

    writer
        .ping_sent(&connection, "hb-001", START + 10_000)
        .unwrap();
    writer
        .pong_received(&connection, "hb-001", START + 10_050)
        .unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents.lines().count(), 4);
    assert!(contents.contains("\"heartbeatId\":\"hb-001\""));

    drop(writer);
    let reopened = fs::read_to_string(&path).unwrap();
    assert_eq!(reopened, contents);
    fs::remove_file(path).unwrap();
}

#[test]
fn enforces_connection_lifecycle_and_ping_pong_pairing() {
    let path = temp_path("lifecycle");
    let connection = identity(Route::Market);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();

    assert!(writer
        .ping_sent(&connection, "hb-before-open", START)
        .is_err());
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer.ping_sent(&connection, "hb-1", START + 1).unwrap();
    assert!(writer.ping_sent(&connection, "hb-1", START + 2).is_err());
    assert!(writer
        .pong_received(&connection, "unknown", START + 3)
        .is_err());
    writer
        .pong_received(&connection, "hb-1", START + 4)
        .unwrap();
    writer
        .disconnected(&connection, DisconnectReason::RemoteClose, START + 5)
        .unwrap();
    writer
        .reconnect_scheduled(&connection, 1, 2_000, START + 6)
        .unwrap();
    assert!(writer
        .ping_sent(&connection, "hb-after-close", START + 7)
        .is_err());

    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents.lines().count(), 6);
    let last: serde_json::Value = serde_json::from_str(contents.lines().last().unwrap()).unwrap();
    assert_eq!(last["eventType"], "RECONNECT_SCHEDULED");
    assert_eq!(last["attempt"], 1);
    assert_eq!(last["delayMs"], 2_000);
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_non_monotonic_timestamps_without_appending() {
    let path = temp_path("ordering");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START + 9).unwrap();
    writer.connected(&connection, START + 10).unwrap();

    assert!(writer.ping_sent(&connection, "hb-1", START + 9).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn finalizes_manifest_with_exact_file_size_and_sha256() {
    let path = temp_path("manifest");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer.ping_sent(&connection, "hb-1", START + 10).unwrap();
    writer
        .pong_received(&connection, "hb-1", START + 20)
        .unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 30)
        .unwrap();

    let manifest = writer.finalize(START + 30, START + 40).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.source_commit, SOURCE);
    assert_eq!(manifest.session_id, SESSION);
    assert_eq!(manifest.sidecar_bytes, bytes.len() as u64);
    assert_eq!(manifest.sidecar_sha256.len(), 64);
    assert!(manifest.capture_finalized);
    assert_eq!(manifest.capture_complete_through_ms, START + 30);
    assert_eq!(manifest.event_count, 5);

    let manifest_path = SidecarWriter::manifest_path(&path);
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(saved["sidecarSha256"], manifest.sidecar_sha256);
    assert!(saved["captureFinalized"].as_bool().unwrap());
    assert_eq!(saved["captureCompleteThroughMs"], START + 30);
    assert_eq!(
        saved["connectionSubscriptions"][0]["subscriptions"][0],
        "btcusdt@depth@100ms"
    );
    let digest = Sha256::digest(&bytes);
    let expected = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(manifest.sidecar_sha256, expected);

    fs::remove_file(path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}

#[test]
fn refuses_incomplete_or_time_reversed_finalization() {
    let path = temp_path("bad-finalize");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();

    assert!(writer.finalize(START - 1, START).is_err());
    // Failed finalization must not leave a manifest that could be mistaken for sealed evidence.
    assert!(!SidecarWriter::manifest_path(&path).exists());
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_finalize_while_a_socket_is_still_open() {
    let path = temp_path("open-finalize");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();

    assert!(writer.finalize(START + 1, START + 2).is_err());
    assert!(!SidecarWriter::manifest_path(&path).exists());
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_empty_or_duplicate_subscription_sets() {
    let path = temp_path("subscriptions");
    let mut empty = identity(Route::Public);
    empty.subscriptions.clear();
    let mut empty_writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    assert!(empty_writer.connect_attempted(&empty, START).is_err());
    drop(empty_writer);
    fs::remove_file(&path).unwrap();

    let path = temp_path("duplicate-subscriptions");
    let mut duplicate = identity(Route::Public);
    duplicate
        .subscriptions
        .push("btcusdt@depth@100ms".to_string());
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    assert!(writer.connect_attempted(&duplicate, START).is_err());
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_overwrite_an_existing_sidecar() {
    let path = temp_path("exclusive");
    fs::write(&path, b"existing capture").unwrap();
    assert!(SidecarWriter::create(&path, SOURCE, SESSION).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing capture");
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_invalid_source_identity_and_reconnect_bounds() {
    let path = temp_path("validation");
    assert!(SidecarWriter::create(&path, "not-a-commit", SESSION).is_err());
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::TransportError, START + 1)
        .unwrap();
    assert!(writer
        .reconnect_scheduled(&connection, 0, 2_000, START + 2)
        .is_err());
    assert!(writer
        .reconnect_scheduled(&connection, 1, 60_001, START + 2)
        .is_err());
    writer
        .reconnect_scheduled(&connection, 1, 60_000, START + 2)
        .unwrap();
    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn records_pre_handshake_failures_and_allows_bounded_reconnect() {
    let path = temp_path("pre-handshake-failure");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();

    writer.connect_attempted(&connection, START).unwrap();
    writer
        .connect_failed(&connection, ConnectFailureReason::Timeout, START + 30_000)
        .unwrap();
    writer
        .reconnect_scheduled(&connection, 1, 2_000, START + 30_001)
        .unwrap();

    let lines = fs::read_to_string(&path).unwrap();
    let events: Vec<serde_json::Value> = lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["eventType"], "CONNECT_ATTEMPTED");
    assert_eq!(events[1]["eventType"], "CONNECT_FAILED");
    assert_eq!(events[1]["reason"], "TIMEOUT");
    assert_eq!(events[2]["eventType"], "RECONNECT_SCHEDULED");

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_connected_without_a_prior_attempt_and_rejects_finalizing_pending_attempt() {
    let path = temp_path("missing-attempt");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();

    assert!(writer.connected(&connection, START).is_err());
    writer.connect_attempted(&connection, START).unwrap();
    assert!(writer.finalize(START + 1, START + 2).is_err());
    assert!(!SidecarWriter::manifest_path(&path).exists());

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn poisons_writer_when_manifest_publication_fails_closed() {
    let path = temp_path("manifest-publication-failure");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();

    // Occupy the manifest destination so publication fails deterministically.
    let manifest_path = SidecarWriter::manifest_path(&path);
    fs::write(&manifest_path, b"pre-existing manifest").unwrap();
    let before = fs::read(&path).unwrap();

    assert!(writer.finalize(START + 2, START + 3).is_err());

    // A failed seal must never leave the writer available for additional events.
    assert!(writer
        .reconnect_scheduled(&connection, 1, 2_000, START + 4)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&manifest_path).unwrap(), b"pre-existing manifest");

    drop(writer);
    fs::remove_file(path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}

#[test]
fn poisons_writer_when_sidecar_readback_fails_during_finalization() {
    let path = temp_path("finalize-readback-failure");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();

    // Keep the writer's file descriptor open but remove its pathname. The
    // flush/sync calls succeed; opening the path for digest readback must fail.
    fs::remove_file(&path).unwrap();
    assert!(writer.finalize(START + 2, START + 3).is_err());

    // The failed seal is terminal: no further event may be appended.
    assert!(writer
        .reconnect_scheduled(&connection, 1, 2_000, START + 4)
        .is_err());
    assert!(!SidecarWriter::manifest_path(&path).exists());
    drop(writer);
}
