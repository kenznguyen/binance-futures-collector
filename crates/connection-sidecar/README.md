# Rust Collector Connection Sidecar Writer

**Status: portable offline reference implementation. Not integrated into the upstream collector.**

This crate writes a separate append-only JSONL event stream for WebSocket connection lifecycle evidence. It is deliberately independent of the CSV market-data importer.

## Contract

Each event contains the v1 identity fields:

- `schemaVersion`, `eventId`, `sourceCommit`, `sessionId`
- `connectionId` (unique for one socket lifetime), `batchId`, `route`
- `eventType`, `eventAtMs`

Supported event types:

- `CONNECT_ATTEMPTED` (durably written before DNS/TCP/WebSocket handshake begins)
- `CONNECT_FAILED` (DNS, transport, timeout, or handshake rejection before CONNECTED)
- `CONNECTED` (only after a successful handshake)
- `PING_SENT` and `PONG_RECEIVED`, paired by a unique `heartbeatId`; `PING_TIMEOUT` records an outstanding PING that exceeded the strict 60,000 ms response threshold
- `DISCONNECTED`, with `REMOTE_CLOSE`, `TRANSPORT_ERROR`, `HEARTBEAT_TIMEOUT`, `SHUTDOWN`, or `UNKNOWN`
- `RECONNECT_SCHEDULED`, with bounded `attempt` and `delayMs`

The `CONNECTED` event stores the number of subscriptions and a deterministic SHA-256 of the sorted subscription set. The manifest stores the exact subscription names and their hash for audit/reconciliation.

## Basic use

```rust
use collector_connection_sidecar::{
    ConnectionIdentity, DisconnectReason, Route, SidecarWriter,
};

let mut writer = SidecarWriter::create(
    "capture/session-001/connection-events.jsonl",
    "bc4f0ef633fbeee4a49defd72827f17162693805",
    "process-session-uuid",
)?;

let socket = ConnectionIdentity {
    connection_id: "socket-uuid-for-this-one-socket".to_string(),
    batch_id: 0,
    route: Route::Public,
    subscriptions: vec!["btcusdt@depth@100ms".to_string()],
};

writer.connect_attempted(&socket, attempt_started_at_ms)?;
writer.connected(&socket, connected_at_ms)?;
writer.ping_sent(&socket, "heartbeat-0001", ping_sent_at_ms)?;
writer.pong_received(&socket, "heartbeat-0001", pong_received_at_ms)?;
writer.disconnected(&socket, DisconnectReason::Shutdown, disconnected_at_ms)?;

let manifest = writer.finalize(capture_complete_through_ms, finalized_at_ms)?;
# Ok::<(), std::io::Error>(())
```

If the handshake fails, call `connect_failed` with a classified reason and then `reconnect_scheduled`; this records failures that happen before a WebSocket socket is established. A connection attempt must be durably recorded before calling the network connector. The caller supplies UTC Unix epoch milliseconds. The caller must generate a fresh process `sessionId` for every collector process and a fresh `connectionId` for every WebSocket socket lifetime. Never reuse the upstream collector's current stable `(batch, route)` connection key as a socket ID.

## Durability and fail-closed rules

- The JSONL file is created with exclusive create + append; an existing path is never truncated or reused.
- Each event is serialized as one line, flushed, then `sync_data()` completes before the writer method returns.
- Initial file creation and manifest publication sync the parent directory on Unix.
- Event timestamps must be globally non-decreasing.
- Ping/Pong must pair exactly; duplicate heartbeat IDs, unmatched Pong, invalid lifecycle transitions, invalid reconnect bounds, and identity reuse are rejected.
- Limits: 16 KiB per event line, 64 MiB per sidecar, 100,000 events, at most 1,024 socket connections per session, 10,000 subscriptions per socket, 256 bytes per subscription name, 1 MiB aggregate subscription-name bytes per session, and 1 MiB serialized manifest. The session-wide aggregate bound is stricter than the per-socket subscription-count maximum; there is no separate 2 MiB-per-socket byte allowance.
- Finalization requires at least one event, all sockets disconnected, a completeness watermark at or beyond the final event, and a finalization timestamp at or beyond the watermark.
- The manifest is written to a temporary file, synced, linked into place without replacing an existing manifest, and followed by a parent-directory sync on Unix. It records sidecar byte size, SHA-256, event count, session/source identity, watermark, and exact subscription sets.

SHA-256 detects a sidecar mismatch only when compared with a separately protected expected digest. The manifest is not digitally signed and does not authenticate its own producer.

## Async collector integration constraints

The writer uses synchronous filesystem operations. Do not call it directly from a high-frequency Tokio task. A future collector integration should use one dedicated blocking writer worker with a bounded queue and wait for a durable acknowledgement for each lifecycle event before claiming it was recorded.

For heartbeat correlation, the collector must put the unique `heartbeatId` in the client Ping payload and read that same payload from the matching Pong frame. A server Ping followed by the collector's Pong response is not a `PONG_RECEIVED` event for the collector's own Ping.

## Offline checks

```bash
cargo fmt --manifest-path crates/connection-sidecar/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/connection-sidecar/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/connection-sidecar/Cargo.toml --all-targets
```

Tests use temporary local files only. They do not start a collector, open a database, or contact an exchange.


## Pre-handshake failure contract

Each unique socket-attempt ID follows one of these paths:

- `CONNECT_ATTEMPTED → CONNECTED → DISCONNECTED`
- `CONNECT_ATTEMPTED → CONNECT_FAILED`

Both terminal events may be followed by one `RECONNECT_SCHEDULED`. A later retry must use a new `connectionId`; the upstream stable logical (batch, route) key is not a socket-attempt ID. A session cannot be finalized while any attempt remains unresolved.


## Interpreting finalization and completeness

The sidecar has three distinct properties. They must not be treated as interchangeable:

1. **Durable event persistence** means a writer method returned after its JSONL line was flushed and `sync_data()` completed. This is evidence that the sidecar event was persisted according to the writer's filesystem contract. It says nothing about market-data messages that were never delivered to, or never submitted to, this writer.
2. **Manifest consistency** means the finalized manifest agrees with the sidecar snapshot's filename, schema version, byte count, SHA-256 and JSONL event count. The offline `validate_capture()` result `SealedConsistent` establishes this structural agreement for the files it read. It is not proof that the exchange stream was complete, that every expected subscription delivered data, or that no market-data event was lost.
3. **Market-data completeness** is a separate claim that requires independent evidence about the actual market-data streams and their expected coverage. This connection sidecar does not collect or validate market-data payloads, trade IDs, kline buckets, order-book update-ID continuity, REST poll coverage, or downstream ingestion.

### The watermark is a caller assertion

`capture_complete_through_ms` is supplied by the caller to `finalize()`. The writer checks only local ordering constraints: the watermark must not precede the final sidecar event, and `finalized_at_ms` must not precede the watermark. The writer does not query Binance, inspect market-data files, reconcile stream sequence IDs, inspect gap registries, or independently establish that collection continued without interruption through that time.

Therefore:

- A watermark records what the caller asserts as its intended completeness boundary; it is not independently verified evidence of market-data coverage.
- `capture_finalized: true` means the sidecar finalization procedure completed and the manifest was published. It does not mean a market-data capture session is complete.
- `SealedConsistent` means the JSONL and manifest agree structurally. It does not certify market-data completeness or correctness.
- A valid JSONL file without a manifest is `UnsealedValid`: it is incomplete from the sidecar-finalization perspective and must not be promoted to a completed capture.
- A missing manifest must never be interpreted as success, and validation must never repair, truncate, append to, or resume the source files.

A downstream system may claim market-data completeness only after it performs and records its own stream-specific coverage checks (for example, order-book update-ID continuity, trade-ID continuity where applicable, expected kline bucket coverage, REST metric sampling windows, reconnect/gap reconciliation, and explicit handling of unavailable or unsupported coverage). Those checks are outside this crate's contract.

**Safe status vocabulary:** report sidecar persistence, manifest consistency, and market-data completeness as separate statuses. If only this crate's evidence is available, report at most `sidecar_persisted` / `manifest_consistent`; report market-data completeness as `unknown` or `not_verified`, never infer it from the watermark or manifest alone.

### Publication error after the final manifest link

A failure can occur after the temporary manifest has been hard-linked to the final manifest path:

- If temporary-name removal fails, both the final manifest and temporary name may remain.
- If temporary-name removal succeeds but parent-directory sync fails, the final manifest may remain while the temporary name is absent.
- In either case, `finalize()` returns an error and the writer remains terminal/poisoned. Do not retry finalization with that writer or append more events.
- A later read-only `validate_capture()` may return `SealedConsistent` if the surviving final manifest agrees with the sidecar. This means only that the files read are structurally consistent. It does **not** retroactively prove that the earlier `finalize()` call returned success or that the parent-directory entry was durably committed at the time of the sync error.
- A leftover temporary manifest is diagnostic residue; `validate_capture()` intentionally does not remove it. Do not remove the final manifest merely because finalization returned an error.

Consumers must preserve the distinction between the finalize call result, manifest/sidecar content consistency, and independently established durability. Until a separate recovery protocol exists, classify these post-publication-error states as requiring recovery review rather than silently promoting them to successful finalization.

### Temporary manifest without a final manifest

If the process stops after creating/writing the temporary manifest but before the hard-link to the final manifest, the sidecar may remain valid while only a temporary manifest file exists. In that state, `validate_capture()` returns `UnsealedValid` because no final manifest is present; it does not scan for, classify, or remove temporary manifest files.

Therefore, `UnsealedValid` means only that the sidecar is valid and no final manifest was found. It does **not** prove that finalization was never attempted. A matching temporary manifest is diagnostic evidence of an interrupted publication attempt and requires recovery review. Do not promote it to a sealed capture, rename it to the final path, or delete it automatically without a separately specified recovery protocol.
