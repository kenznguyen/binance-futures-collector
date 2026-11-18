# T1 Order Book HTTP Provider V1 — Contract and RED Test Plan

Status: IMPLEMENTED IN PART / PAUSED — no HTTP route or collector runtime integration  
Repository: `kenznguyen/binance-futures-collector`  
Target module: `src/http/orderbook_v1.rs`  
Target runtime: same Rust collector process  
Design baseline: `main` at `d43a673451f7347e16a214612165c63da1b4339c`  
Inspected `src/collectors/orderbook_manager.rs` blob: `d18d047c1f7cdddd03cea60b02de54b656ad4edf`


## Implementation status (2026-11-18)

The following provider-related source work is present on `main` and is intentionally frozen:

- Source HEAD at review: `efbd57e7f869bc2298010772deeaf17352acc6f4`.
- Provider/publisher module: `src/http/orderbook_v1.rs`; module registration: `src/http/mod.rs` and `src/lib.rs`.
- WebSocket depth-diff receipt timestamp buffering and fail-closed continuity work is present in `src/websocket/connection_manager.rs`.
- Related tests: `tests/orderbook_v1_red.rs`.
- The implementation includes snapshot publication/validation and continuity logic. It is not wired to a running HTTP server/route in `src/main.rs`; no collector runtime endpoint is currently integrated.
- T1 HTTP Provider development is paused. Do not add endpoints, connect runtime, dispatch provider workflows, deploy, or alter T0/T2 as part of this paused scope.
- Keep all existing T1 commits on `main`. Do not revert or isolate them unless a demonstrated technical conflict, security risk, or explicit Git-history requirement is established.

This status update records the current repository state; it does not authorize further implementation or runtime activation. The original contract and RED plan below remain design/acceptance reference material, not an active implementation instruction.

## 1. Decision and scope

Implement an authenticated, read-only HTTP provider inside the Rust collector. It must read an immutable, already-verified BTCUSDT snapshot published by `OrderbookManager`; it must not reconstruct a book from CSV, call Binance itself, or use Capture Desk's Python/FastAPI process.

This document defines the contract and tests only. It does not authorize Rust source changes, dependency changes, workflow dispatch, deployment, collector startup, exchange access, or trading.

## 2. Consumer compatibility lock

The consumer is `core_runtime/orderbook_http.py` and validator is `core_runtime/orderbook_contract.py` in `kenznguyen/btcusdt-telegram-signals2`, branch `feature/core-operational-proof-20261112`, inspected HEAD `e6606e28a696374f31cd6db1ec0a1372d942d64d`.

- Method and route: `GET /internal/v1/orderbook/btcusdt`
- Auth: `Authorization: Bearer <token>`
- Listener: loopback only (`127.0.0.1`)
- Consumer sends `Accept: application/json`, `Accept-Encoding: identity`, `Cache-Control: no-store`; it rejects redirects and does one request without retry/fallback.
- Consumer bounds connect/read/total timeout and response body to 65,536 bytes.
- HTTP 200 must have `Content-Type: application/json`; content encoding must be identity.
- HTTP 401 maps to auth rejection. Other non-200 responses fail closed as unavailable.

The JSON object must contain exactly these fields (no missing or additional keys):

| Field | Type | Contract meaning |
|---|---|---|
| contractVersion | string | `T1_T2_ORDERBOOK_V1` |
| symbol | string | `BTCUSDT` |
| market | string | `BINANCE_USDM_PERPETUAL` |
| bids / asks | array of [price, quantity] | Canonical decimal strings; bids descending, asks ascending |
| snapshotLastUpdateId | unsigned integer | REST depth snapshot anchor `lastUpdateId` |
| firstAppliedU | unsigned integer | `U` of first diff applied after anchor bridge |
| lastAppliedU | unsigned integer | `u` of latest diff applied |
| sourceTimestampMs | unsigned integer | Binance event time `E` of latest applied diff |
| observedAtMs | unsigned integer | Local UTC wall-clock receipt time for that same latest diff |
| decisionWatermark | unsigned integer | Must equal `lastAppliedU` |
| generation | unsigned integer | Non-zero generation, fenced on invalidation/re-anchor |
| snapshotId | string | `sha256:<hex>` of canonical snapshot identity excluding snapshotId itself |
| continuity | string | Exactly `VERIFIED` |
| freshness | string | Exactly `FRESH` |
| depth | object | Exactly {`bids`: count, `asks`: count} |
| complete | boolean | Exactly `true` |

The consumer's frozen validation thresholds are: at least 20 levels on each side; source and observation age no more than 2,000 ms at decision time; future skew no more than 250 ms; canonical decimal text; no duplicate price levels; strictly ordered sides; best bid below best ask; and valid sequence/watermark relationships. The provider must not weaken or redefine these consumer rules.

## 3. Required in-process interface

Target public interface (names are proposed; finalize only after source review):

```rust
pub struct PublishedOrderbookSnapshot {
    // Immutable, fully materialized V1 fields.
}

pub enum SnapshotUnavailable {
    NotLive,
    ContinuityUnverified,
    Stale,
    IncompleteDepth,
    InvalidBook,
}

impl OrderbookManager {
    pub fn read_verified_snapshot(
        &self,
        symbol: &str,
        now_ms: i64,
    ) -> Result<Arc<PublishedOrderbookSnapshot>, SnapshotUnavailable>;
}
```

The HTTP module receives an `Arc<OrderbookManager>` (or a narrow read-only trait implemented by it), never mutable book maps. The manager publishes snapshots by building a complete candidate off-lock and atomically swapping an immutable `Arc`. A reader must observe either the previous complete snapshot or the next complete snapshot, never a partially updated book.

The endpoint is BTCUSDT-only in V1. Do not expose generic symbols until a separately versioned contract is approved.

## 4. Source-level blockers that must be resolved before implementation

The inspected collector source does not currently contain enough trustworthy state to emit this contract by simply serializing its existing `Orderbook`:

1. `Orderbook` stores price keys and quantities as `f64`; original decimal strings are discarded on application. Reformatting these floats to decimal text may lose source precision. Preserve exact wire decimal strings in a parallel validated representation (or prove a lossless representation) for the published snapshot.
2. `Orderbook` currently stores `last_update_id` and `Instant`, but not the snapshot anchor, first applied U, source event time E, local receipt wall-clock time, or generation needed by V1.
3. `run_live_depth_update_loop` explicitly does not enforce `pu == last_update_id` after reconnect. It applies diffs and relies on offline detection/self-healing. This is incompatible with publishing `continuity=VERIFIED`. The V1 publication state must be invalidated immediately on a sequence break and remain unavailable until a new REST anchor is bridged and every subsequent diff is verified.
4. `WebSocketManager` buffers `DepthUpdateWs`; the provider needs a trusted local UTC receipt timestamp paired with each buffered diff. `Instant` alone cannot be serialized as epoch milliseconds.
5. The REST `DepthSnapshotRest` has no exchange timestamp. Do not invent one. Set `sourceTimestampMs` from the latest successfully applied Binance diff's E; set `observedAtMs` from the local receive timestamp of that same diff. A book with no applied diff after the REST anchor is not eligible for V1 publication.
6. The current manager's periodic CSV snapshots are persistence outputs, not the provider's authoritative snapshot source. Do not read them from the HTTP handler.

These are explicit design gates, not permission to modify `OrderbookManager` now.

## 5. Publication and fencing semantics

- A symbol is publishable only after the existing REST-anchor bridge succeeds and at least one post-anchor diff has been applied.
- First applied diff records `firstAppliedU = U`; each subsequent diff must satisfy the existing accepted bridge semantics for the first event and then strict `pu == previous u` continuity. Reject malformed ranges (`u < U`) and duplicate/regressing updates unless the existing manager's verified attach rules explicitly classify them as already covered.
- `lastAppliedU` advances only after successful application of the entire diff. `decisionWatermark == lastAppliedU`.
- Any reconnect gap, invalid update, resnapshot, delisting, cleanup, or manager state loss invalidates the published pointer before the book can be treated as verified again.
- Each invalidation/re-anchor advances the per-symbol generation fence. Generation is non-zero and monotonic for the lifetime of the collector process; restart semantics must be documented and tested.
- Snapshot build validates depth, finite positive prices/quantities, no duplicate price keys, side ordering, and non-crossed spread. If any check fails, publish nothing.
- Build `snapshotId` as SHA-256 over deterministic canonical JSON of all V1 fields except `snapshotId` (sorted object keys, compact separators, UTF-8). Hash algorithm and canonicalization must be covered by golden tests.
- Publication uses one atomic pointer replacement after all fields and digest are complete. The HTTP handler clones/serializes the immutable snapshot and never holds a manager lock during socket writes.

## 6. HTTP behavior and security

- Bind only to `127.0.0.1`; no wildcard, LAN, or public listener.
- Bearer token comes from explicit runtime configuration/environment. Missing or malformed provider auth configuration must prevent the route from starting; never log or echo the token.
- Compare credentials without data-dependent early exit where practical. Never accept query-string tokens.
- GET only. No mutation route, no CORS requirement, no redirects, no compression.
- Return 401 for absent/invalid bearer token; return 503 when no currently verified/fresh/complete snapshot exists; return 200 only for a valid immutable snapshot.
- Set `Content-Type: application/json`, `Cache-Control: no-store`, and `Content-Length`; response must remain <=65,536 bytes. No debug traces or internal state in error bodies.
- Avoid introducing a web framework or dependency unless existing dependencies cannot provide a bounded and reviewed implementation. Any new dependency requires a separate dependency review.

## 7. RED test plan (tests first; no implementation yet)

### A. Snapshot manager unit tests

| Test | Expected RED condition before implementation |
|---|---|
| no BTCUSDT book / non-Live state | verified snapshot accessor unavailable |
| REST anchor with no post-anchor diff | unavailable |
| valid anchor bridge + one diff | immutable snapshot returned with exact anchor/U/u |
| first bridge event accepted under manager bridge rule | accepted |
| subsequent diff pu mismatch | publication invalidated; unavailable |
| update range u < U | rejected and publication invalidated |
| duplicate/regressing already-covered event | does not move watermark or corrupt book |
| reconnect gap then later absolute update | remains unavailable until re-anchor; no self-heal promotion |
| resnapshot/re-anchor | generation increases; old generation cannot republish |
| delist/stale cleanup | snapshot pointer invalidated |
| concurrent reader and publisher | readers see only whole old/new snapshots |
| exact decimal preservation | emitted decimal strings preserve source precision without float rounding |
| less than 20 levels either side | unavailable |
| crossed/locked book or non-finite/zero/negative values | unavailable |
| source/observation timestamps missing or future | unavailable |

### B. Serialization and contract tests

- exact key set and exact V1 identity strings;
- canonical decimal text only, no JSON numeric prices/quantities;
- bid descending / ask ascending;
- sequence relationships and watermark equality;
- source/observed timestamps tied to the latest applied event;
- generation non-zero and fenced;
- deterministic snapshot digest golden vector;
- JSON round-trip accepted by the pinned Core First `validate_snapshot` fixture;
- serialized response <=65,536 bytes at configured provider depth.

### C. HTTP handler tests

- exact GET route returns 200 for valid fixture;
- route is not exposed for other symbols or alternate paths;
- missing token / invalid token returns 401;
- query token is ignored/rejected;
- non-GET method is rejected;
- unavailable, stale, incomplete or continuity-invalid snapshot returns non-200 (503);
- correct content type, no-store, identity encoding and content length;
- no redirects, no compression, no token/body leakage in logs/errors;
- oversized response cannot be emitted;
- concurrent requests do not block orderbook updates or hold manager locks across writes.

### D. In-process integration tests

- construct manager with deterministic fake REST anchor and buffered diff sequence; no exchange/network;
- start the HTTP handler on ephemeral loopback only;
- fetch using the same route, Bearer header and strict consumer validator behavior;
- force a sequence gap while requests are concurrent; verify provider stops returning 200 before any unverified state can escape;
- re-anchor and verify publication resumes with a higher generation and a new digest;
- verify all tests use disposable fixtures and never create/open canonical T0 or persistent trading storage.

### E. Regression gates

- `cargo fmt -- --check`
- focused `cargo test` for the new manager/provider modules
- full `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings` if this matches current repository CI policy; inspect workflows before treating it as required.
- existing orderbook manager and collector tests remain green.

RED is valid only when the new behavior-specific assertions fail because the required API/state/handler is absent. Import errors, missing crates, unavailable toolchain, or test harness failures are NOT valid RED evidence.

## 8. Acceptance gates after a separately authorized implementation

1. Record implementation commit SHA and changed paths.
2. Show focused RED evidence from before implementation and GREEN evidence after implementation on the same intended source lineage.
3. Run Rust regression suites and report exact counts/outcomes.
4. Run consumer/provider wire-compatibility tests using the pinned Core First validator; record both source SHAs.
5. Inspect the final diff and obtain independent review; no high/critical unresolved finding.
6. Only then freeze the T0/T1/T2 dependency matrix.
7. Request separate explicit approval before dispatching the existing Core First workflow on the exact current Core First HEAD. Workflow execution is not authorized by this design.

## 9. Non-authorizations

No source edit, Cargo dependency change, CI/workflow change or workflow dispatch; no branch creation, merge, rebase, cherry-pick or main/Core First reconciliation; no collector startup, VPS/deployment, exchange connection, T0 database/Feeder access, credentials change, AI execution, paper execution or live order.

