use collector_connection_sidecar::{
    validate_capture, CaptureValidation, ConnectFailureReason, ConnectionIdentity,
    DisconnectReason, Route, SidecarWriter,
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

#[test]
fn refuses_to_finalize_when_sidecar_bytes_were_appended_outside_the_writer() {
    let path = temp_path("external-append");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();

    // Simulate an out-of-band modification while the writer is still alive.
    use std::io::Write as _;
    let mut external = fs::OpenOptions::new().append(true).open(&path).unwrap();
    external.write_all(b"{\"unexpected\":true}\n").unwrap();
    external.sync_all().unwrap();
    drop(external);

    assert!(
        writer.finalize(START + 2, START + 3).is_err(),
        "finalize must reject bytes that are not represented by the writer event counter"
    );
    assert!(!SidecarWriter::manifest_path(&path).exists());

    drop(writer);
    fs::remove_file(path).unwrap();
}

fn create_disconnected_sidecar(label: &str) -> PathBuf {
    let path = temp_path(label);
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();
    drop(writer);
    path
}

#[test]
fn validator_classifies_valid_jsonl_without_manifest_as_unsealed() {
    let path = create_disconnected_sidecar("validator-unsealed");
    let before = fs::read(&path).unwrap();

    let result = validate_capture(&path).unwrap();

    assert!(matches!(
        result,
        CaptureValidation::UnsealedValid {
            sidecar_bytes: _,
            event_count: 3
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!SidecarWriter::manifest_path(&path).exists());
    fs::remove_file(path).unwrap();
}

#[test]
fn validator_classifies_missing_newline_or_torn_final_record_as_truncated_tail() {
    let path = create_disconnected_sidecar("validator-torn-tail");
    let mut bytes = fs::read(&path).unwrap();
    bytes.pop();
    fs::write(&path, &bytes).unwrap();
    let before = fs::read(&path).unwrap();

    let result = validate_capture(&path).unwrap();

    assert!(matches!(
        result,
        CaptureValidation::TruncatedFinalLine { line_number: 3 }
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!SidecarWriter::manifest_path(&path).exists());
    fs::remove_file(path).unwrap();
}

#[test]
fn validator_classifies_malformed_middle_jsonl_record() {
    let path = create_disconnected_sidecar("validator-malformed");
    let original = fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = original.lines().collect();
    lines[1] = "{malformed";
    fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    let before = fs::read(&path).unwrap();

    let result = validate_capture(&path).unwrap();

    assert!(matches!(
        result,
        CaptureValidation::MalformedJsonl { line_number: 2 }
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_file(path).unwrap();
}

#[test]
fn validator_classifies_manifest_byte_hash_or_event_count_mismatch() {
    let seal_path = temp_path("validator-sealed-mismatch");
    let connection = identity(Route::Public);
    let mut seal_writer = SidecarWriter::create(&seal_path, SOURCE, SESSION).unwrap();
    seal_writer.connect_attempted(&connection, START).unwrap();
    seal_writer.connected(&connection, START + 1).unwrap();
    seal_writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();
    seal_writer.finalize(START + 2, START + 3).unwrap();
    drop(seal_writer);

    let manifest_path = SidecarWriter::manifest_path(&seal_path);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["eventCount"] = serde_json::json!(99);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let before_sidecar = fs::read(&seal_path).unwrap();
    let before_manifest = fs::read(&manifest_path).unwrap();

    let result = validate_capture(&seal_path).unwrap();

    assert!(matches!(result, CaptureValidation::ManifestMismatch { .. }));
    assert_eq!(fs::read(&seal_path).unwrap(), before_sidecar);
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    fs::remove_file(seal_path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}

#[test]
fn validator_classifies_sealed_manifest_consistent_capture() {
    let path = temp_path("validator-sealed-valid");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();
    writer.finalize(START + 2, START + 3).unwrap();
    drop(writer);
    let before_sidecar = fs::read(&path).unwrap();
    let manifest_path = SidecarWriter::manifest_path(&path);
    let before_manifest = fs::read(&manifest_path).unwrap();

    let result = validate_capture(&path).unwrap();

    assert!(matches!(
        result,
        CaptureValidation::SealedConsistent { event_count: 3, .. }
    ));
    assert_eq!(fs::read(&path).unwrap(), before_sidecar);
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    fs::remove_file(path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}

#[test]
fn refuses_to_disconnect_or_finalize_with_an_unanswered_ping() {
    let path = temp_path("pending-ping-disconnect");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .ping_sent(&connection, "hb-unanswered", START + 2)
        .unwrap();

    let before = fs::read(&path).unwrap();
    assert!(writer
        .disconnected(&connection, DisconnectReason::RemoteClose, START + 3)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(writer.finalize(START + 2, START + 4).is_err());
    assert!(!SidecarWriter::manifest_path(&path).exists());

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn enforces_aggregate_subscription_budget_across_the_session() {
    let path = temp_path("aggregate-subscription-budget");
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();

    let subscriptions = (0..2_400)
        .map(|index| format!("{}-{index:04}", "x".repeat(240)))
        .collect::<Vec<_>>();
    let first = ConnectionIdentity {
        connection_id: "socket-aggregate-001".to_string(),
        batch_id: 1,
        route: Route::Public,
        subscriptions: subscriptions.clone(),
    };
    let second = ConnectionIdentity {
        connection_id: "socket-aggregate-002".to_string(),
        batch_id: 2,
        route: Route::Market,
        subscriptions,
    };

    writer.connect_attempted(&first, START).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(writer.connect_attempted(&second, START + 1).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_finalize_when_sidecar_path_is_replaced_with_same_size_valid_jsonl() {
    let path = temp_path("same-size-path-replacement");
    let backup = path.with_extension("original.jsonl");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .disconnected(&connection, DisconnectReason::Shutdown, START + 2)
        .unwrap();

    let original = fs::read(&path).unwrap();
    let mut replacement = original.clone();
    let source_offset = replacement
        .windows(SOURCE.len())
        .position(|window| window == SOURCE.as_bytes())
        .unwrap();
    replacement[source_offset] = if replacement[source_offset] == b'a' {
        b'b'
    } else {
        b'a'
    };
    assert_eq!(replacement.len(), original.len());
    assert_eq!(replacement.iter().filter(|byte| **byte == b'\n').count(), 3);
    for line in replacement
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        assert!(serde_json::from_slice::<serde_json::Value>(line)
            .unwrap()
            .is_object());
    }

    fs::rename(&path, &backup).unwrap();
    fs::write(&path, &replacement).unwrap();

    assert!(writer.finalize(START + 2, START + 3).is_err());
    assert!(!SidecarWriter::manifest_path(&path).exists());

    drop(writer);
    fs::remove_file(path).unwrap();
    fs::remove_file(backup).unwrap();
}

#[test]
fn records_heartbeat_timeout_and_allows_terminal_disconnect_and_finalize() {
    let path = temp_path("heartbeat-timeout");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .ping_sent(&connection, "hb-timeout", START + 2)
        .unwrap();

    writer
        .heartbeat_timed_out(&connection, "hb-timeout", START + 2 + 60_001)
        .expect("expired heartbeat must be durably resolved");

    writer
        .disconnected(
            &connection,
            DisconnectReason::HeartbeatTimeout,
            START + 2 + 60_001,
        )
        .expect("a timed-out heartbeat must be explicitly resolved before disconnect");

    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("\"eventType\":\"PING_TIMEOUT\""));
    assert!(contents.contains("\"eventType\":\"DISCONNECTED\""));
    assert!(contents.contains("\"reason\":\"HEARTBEAT_TIMEOUT\""));

    writer
        .finalize(START + 2 + 60_001, START + 2 + 60_002)
        .expect("a heartbeat-timeout disconnect must permit sealing");
    drop(writer);

    let manifest_path = SidecarWriter::manifest_path(&path);
    assert!(manifest_path.exists());
    fs::remove_file(path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}

#[test]
fn refuses_heartbeat_timeout_disconnect_before_timeout_threshold() {
    let path = temp_path("heartbeat-timeout-early");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .ping_sent(&connection, "hb-not-yet-timeout", START + 2)
        .unwrap();

    let before = fs::read(&path).unwrap();
    assert!(writer
        .heartbeat_timed_out(&connection, "hb-not-yet-timeout", START + 2 + 60_000)
        .is_err());
    assert!(writer
        .disconnected(
            &connection,
            DisconnectReason::HeartbeatTimeout,
            START + 2 + 60_000,
        )
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_send_new_ping_after_heartbeat_timeout_is_observed() {
    let path = temp_path("heartbeat-timeout-terminal");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .ping_sent(&connection, "hb-expired", START + 2)
        .unwrap();
    writer
        .heartbeat_timed_out(&connection, "hb-expired", START + 2 + 60_001)
        .unwrap();

    let before = fs::read(&path).unwrap();
    assert!(writer
        .ping_sent(&connection, "hb-after-timeout", START + 2 + 60_002)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn requires_heartbeat_timeout_reason_after_timeout_was_observed() {
    let path = temp_path("heartbeat-timeout-reason");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer
        .ping_sent(&connection, "hb-reason", START + 2)
        .unwrap();
    writer
        .heartbeat_timed_out(&connection, "hb-reason", START + 2 + 60_001)
        .unwrap();

    let before = fs::read(&path).unwrap();
    assert!(writer
        .disconnected(
            &connection,
            DisconnectReason::TransportError,
            START + 2 + 60_002,
        )
        .is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a timeout-observed connection must not be recorded with a different disconnect reason"
    );

    drop(writer);
    fs::remove_file(path).unwrap();
}

#[test]
fn resolves_other_pending_ping_after_one_heartbeat_times_out() {
    let path = temp_path("heartbeat-timeout-multiple-pings");
    let connection = identity(Route::Public);
    let mut writer = SidecarWriter::create(&path, SOURCE, SESSION).unwrap();
    writer.connect_attempted(&connection, START).unwrap();
    writer.connected(&connection, START + 1).unwrap();
    writer.ping_sent(&connection, "hb-expired", START + 2).unwrap();
    writer.ping_sent(&connection, "hb-live", START + 3).unwrap();

    let timeout_at = START + 2 + 60_001;
    writer
        .heartbeat_timed_out(&connection, "hb-expired", timeout_at)
        .unwrap();

    writer
        .pong_received(&connection, "hb-live", timeout_at)
        .expect("a different Ping answered within its own 60-second bound");

    let before_rejected_pong = fs::read(&path).unwrap();
    assert!(writer
        .pong_received(&connection, "hb-expired", timeout_at)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before_rejected_pong);

    writer
        .disconnected(
            &connection,
            DisconnectReason::HeartbeatTimeout,
            timeout_at,
        )
        .unwrap();
    writer.finalize(timeout_at, timeout_at + 1).unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("\"eventType\":\"PING_TIMEOUT\""));
    assert!(contents.contains("\"eventType\":\"PONG_RECEIVED\""));
    assert!(contents.contains("\"reason\":\"HEARTBEAT_TIMEOUT\""));

    drop(writer);
    let manifest_path = SidecarWriter::manifest_path(&path);
    fs::remove_file(path).unwrap();
    fs::remove_file(manifest_path).unwrap();
}
