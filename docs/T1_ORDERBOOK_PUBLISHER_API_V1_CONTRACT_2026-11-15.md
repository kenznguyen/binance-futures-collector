# T1 Order Book Publisher / API V1 Contract

Status: DESIGN BASELINE — documentation only; not an implementation or runtime acceptance.
Date: 2026-11-15
Repository: `kenznguyen/binance-futures-collector`
Target branch: `feature/t1-connection-sidecar-integration-20261107`

## 1. Scope and invariants

This contract defines a read-only consumer boundary for the existing BTCUSDT Order Book V1 publisher.

In scope:
- Read one immutable, validated BTCUSDT snapshot from the existing in-memory `SnapshotPublisher`.
- Expose the snapshot over an authenticated, bounded, loopback-only HTTP transport.
- Return no snapshot unless publisher state, continuity, and freshness permit it.
- Keep the collector ingestion and append-only historical CSV path unchanged.

Out of scope:
- Any changes to WebSocket ingestion, REST snapshot acquisition, CSV writers, historical files, replay, or gap tracking.
- Any T0 access or change.
- Any public network bind, reverse-proxy exposure, VPS deployment, service activation, or T2 runtime integration.
- Any order placement, account endpoint, or execution capability.

The Publisher is not a second order-book reconstruction engine. It must reuse `orderbook_v1::SnapshotPublisher`, `build_snapshot`, and `apply_live_diff`.

## 2. Existing source baseline observed

At the inspected branch source:
- `src/orderbook_v1.rs` defines `SnapshotPublisher`, `OrderbookSnapshotV1`, continuity reconstruction, immutable live-diff application, generation fencing, and read-time freshness validation.
- `src/orderbook_http.rs` already implements a private read-only HTTP transport.
- `src/main.rs` creates one BTCUSDT publisher and enables the listener only when non-empty `T1_ORDERBOOK_HTTP_TOKEN` is supplied.
- Listener address is fixed at `127.0.0.1:8787`.
- Existing route is `GET /internal/v1/orderbook/btcusdt`. There is no status endpoint in the inspected implementation.
- Existing transport tests cover successful response, invalid token, broken publisher, wrong symbol, and wrong method.

This document records the existing interface and its required acceptance tests. It does not claim runtime readiness.

## 3. HTTP transport contract

### Listener

| Property | V1 requirement |
|---|---|
| Bind | `127.0.0.1:8787` only |
| Enablement | Enabled only when `T1_ORDERBOOK_HTTP_TOKEN` exists and is non-empty |
| Missing/empty token configuration | Listener remains disabled; startup must not expose an unauthenticated endpoint |
| Authentication | `Authorization: Bearer <token>` |
| Allowed method | GET only |
| Request body | Not supported |
| Cache | `Cache-Control: no-store` |
| Content type | `application/json` |
| Maximum request | 4 KiB |
| Maximum response | 64 KiB |
| Concurrent connections | At most 8 |
| I/O deadline | 750 ms |
| Network exposure | Loopback only; no wildcard or non-loopback bind |

The token must be injected out-of-band. It must not be placed in a URL, query string, response, logs, repository file, or test artifact.

### Resource

`GET /internal/v1/orderbook/btcusdt`

No query parameters and no request body are defined in V1. Other symbols and unknown paths return 404. Non-GET methods return 405.

A separate `/status` endpoint is not part of this V1 contract. Health/status exposure requires a separately versioned and reviewed extension; consumers must treat the order-book resource response itself as the readiness signal.

## 4. Success response schema

HTTP 200 returns exactly one complete immutable snapshot serialized from `OrderbookSnapshotV1`.

| JSON field | Type | Requirement |
|---|---|---|
| `contractVersion` | string | Exactly `T1_T2_ORDERBOOK_V1` |
| `symbol` | string | Exactly `BTCUSDT` |
| `market` | string | Exactly `BINANCE_USDM_PERPETUAL` (canonical builder value; see open discrepancy below) |
| `bids` | array of [string,string] | Price/quantity decimal strings; best bid first |
| `asks` | array of [string,string] | Price/quantity decimal strings; best ask first |
| `snapshotLastUpdateId` | uint64 | REST snapshot anchor update ID |
| `firstAppliedU` | uint64 | First applied diff's U |
| `lastAppliedU` | uint64 | Last applied diff's u |
| `decisionWatermark` | uint64 | Must equal `lastAppliedU` |
| `sourceTimestampMs` | int64 | Exchange event timestamp of latest applied diff, UTC ms |
| `observedAtMs` | int64 | Local observation timestamp, UTC ms |
| `generation` | uint64 | Current connection/re-anchor generation |
| `snapshotId` | string | Stable identifier for this immutable publication |
| `continuity` | string | Exactly `VERIFIED` |
| `freshness` | string | Exactly `FRESH` |
| `complete` | boolean | Exactly `true` |
| `depth.bids` | integer | Must equal `bids.length` |
| `depth.asks` | integer | Must equal `asks.length` |

Decimal values must remain strings; consumers must not parse them through binary floating point before precision-sensitive calculations. Zero-quantity updates delete a level and must never appear as a retained level.

The builder requires at least 20 levels on each side, strictly ordered prices, valid positive prices/non-negative quantities, and a non-crossed book. The published depth counts must reflect the actual arrays.

## 5. Continuity and publication invariants

A snapshot is publishable only when all conditions hold:
1. A REST snapshot anchor exists.
2. At least one exchange-timestamped WebSocket depth diff bridges the REST anchor.
3. The first applied diff satisfies `U <= snapshotLastUpdateId + 1 <= u`.
4. Each subsequent diff satisfies `pu == previous u` and its final update ID strictly advances.
5. The resulting book is structurally valid, sorted, sufficiently deep, and not crossed.
6. The candidate generation equals the publisher's current generation.
7. Publication state is an allowed lifecycle state.
8. `complete=true`, `continuity=VERIFIED`, and `freshness=FRESH`.
9. A publication cannot repeat or regress the current generation's watermark.

Any gap, invalid sequence, malformed level, failed re-anchor, generation change, or publisher break invalidates the current snapshot. A late candidate from an older generation must not restore it.

## 6. Freshness contract

Existing source constants:
- Maximum accepted book age: 2,000 ms.
- Maximum accepted future timestamp skew: 250 ms.

At request time, both `sourceTimestampMs` and `observedAtMs` are revalidated against current UTC milliseconds. If either timestamp is stale or exceeds the permitted future skew, the API returns 503 and no snapshot body.

These values are existing source behavior and are the proposed V1 contract defaults. Changing either threshold requires an explicit contract revision and corresponding boundary tests.

## 7. Error contract

All error bodies are JSON objects with a single machine-readable `error` string. The implementation currently uses these codes:

| HTTP | Error code | Meaning |
|---|---|---|
| 400 | `incomplete_request` | Request ended before complete headers |
| 400 | `request_body_not_supported` | Request body supplied |
| 400 | `transfer_encoding_not_supported` | Transfer-Encoding supplied |
| 400 | `invalid_request_line` | Invalid HTTP/1.1 request line |
| 400 | `invalid_header` | Malformed header |
| 401 | `unauthorized` | Missing or invalid bearer token |
| 404 | `not_found` | Unknown route or unsupported symbol |
| 405 | `method_not_allowed` | Method other than GET |
| 413 | `request_too_large` | Request exceeds 4 KiB |
| 503 | `capacity_unavailable` | All 8 connection permits are occupied |
| 503 | `snapshot_unavailable` | Publisher is absent, broken, stopped, or not fresh/ready |
| 503 | `snapshot_exceeds_response_limit` | Serialized snapshot exceeds 64 KiB |

No error response may include bids, asks, token material, stack traces, internal paths, or other sensitive process details. Responses are not cached.

## 8. Fail-closed rules

- No publisher snapshot: 503, no stale fallback.
- Publisher state not READY or invalidated: 503, no stale fallback.
- Sequence discontinuity or re-anchor: old snapshot unavailable until a new generation is validated and published.
- Stale/future source or observation timestamp: 503.
- Invalid token: 401 before any snapshot data is disclosed.
- Missing token configuration: transport disabled.
- Non-loopback bind: startup/serve fails.
- Capacity or size limit exceeded: bounded error response; never truncate or partially return a snapshot.
- Transport timeout or malformed request: close/reject; never substitute cached data.

## 9. Known source discrepancies to resolve before implementation changes

1. The canonical `build_snapshot` writes `market=BINANCE_USDM_PERPETUAL`, while the HTTP test fixture manually constructs `market=USDT-M-PERPETUAL`. The contract selects the builder's canonical value. A future test must use a shared fixture/builder and assert the exact value; do not silently accept both.
2. The builder enforces minimum depth 20/20, but `SnapshotPublisher::publish_if_current` itself checks completeness/status strings and does not independently verify depth counts against arrays. The HTTP test fixture currently publishes a one-level book directly. Add a negative publisher/transport contract test or make construction of publishable snapshots inaccessible in a separately approved implementation increment.
3. Existing HTTP tests exercise an ephemeral loopback port by passing a test listener to `serve`; production `bind_loopback` must separately be tested to bind only `127.0.0.1:8787`.
4. Existing transport has no status route. Do not document or assume one.
5. The transport task logs and exits if the HTTP server stops; it does not restart the listener. Consumer behavior on connection refusal must remain fail-closed.

## 10. Change control

This contract is a design baseline only. Before implementation:
- Freeze this schema and the test specification against an exact source commit.
- Resolve the discrepancies in section 9 through tests first.
- Keep changes limited to the Publisher/API boundary and its tests.
- Prove the collector ingestion and historical output paths are unchanged.
- Run the repository CI and record exact source SHA, workflow run, job conclusions, test counts, and artifact metadata.
- Do not enable the listener, set runtime secrets, deploy, restart, or connect T2 without separate authorization.
