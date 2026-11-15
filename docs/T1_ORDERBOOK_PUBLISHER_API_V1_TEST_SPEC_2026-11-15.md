# T1 Order Book Publisher / API V1 — Test Specification

Status: SPECIFICATION ONLY — tests described here have not been added or run by this document.
Date: 2026-11-15
Contract: [T1 Order Book Publisher / API V1 Contract](T1_ORDERBOOK_PUBLISHER_API_V1_CONTRACT_2026-11-15.md)

## 1. Test layers and commands

Repository stack: Rust collector + Tokio; existing CI uses:
- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release --locked`
- `cargo test --all`

Dashboard backend is a separate FastAPI service and is not the T1 live Order Book V1 transport. Do not treat its existing unauthenticated/local dashboard API as the order-book Publisher API.

All new tests must be deterministic, offline, loopback-only, and use disposable in-memory fixtures. No Binance network access, credentials, VPS, T0, or historical collector files are needed.

## 2. Existing test coverage observed

The inspected source already contains:
- Order-book reconstruction tests for exact REST/WS bridge, sequence chain, invalid levels, depth, crossed book, and freshness boundaries.
- Publisher tests for generation invalidation/fencing, immutable live-diff application, re-anchor budget, and read-time freshness.
- HTTP tests for successful serialization, invalid token, broken-publisher fail-closed, unsupported symbol, and unsupported method.

These are source-level observations, not a fresh test run in this task.

## 3. Required test matrix

### A. Wire contract and serialization

| ID | Scenario | Expected |
|---|---|---|
| CT-01 | Valid publisher snapshot, authenticated GET | 200; exact V1 field set and types |
| CT-02 | Contract version mismatch in candidate | Publication rejected / no 200 |
| CT-03 | Symbol differs from BTCUSDT | No snapshot disclosure; 404 or 503 per route/state |
| CT-04 | Market identifier | Exactly `BINANCE_USDM_PERPETUAL` |
| CT-05 | Price and quantity serialization | Both remain decimal strings; trailing zeros preserved |
| CT-06 | Bid ordering | Strict descending prices |
| CT-07 | Ask ordering | Strict ascending prices |
| CT-08 | Depth metadata | Counts equal array lengths; minimum 20 each side for a built production snapshot |
| CT-09 | Watermarks | decisionWatermark equals lastAppliedU; firstAppliedU and lastAppliedU are valid |
| CT-10 | Unknown/missing/extra required field policy | Required fields cannot disappear; additive compatibility policy documented |

### B. Continuity and generation fencing

| ID | Scenario | Expected |
|---|---|---|
| SQ-01 | Exact snapshot bridge: U <= lastUpdateId+1 <= u | Candidate accepted |
| SQ-02 | No bridge after REST snapshot | Candidate rejected |
| SQ-03 | Next diff pu equals previous u | Chain accepted |
| SQ-04 | pu mismatch / sequence gap | Candidate rejected and current publication invalidated as designed |
| SQ-05 | Duplicate or regressing final update ID | Rejected |
| SQ-06 | Re-anchor starts new generation | Previous snapshot immediately unavailable |
| SQ-07 | Late publication from old generation | Rejected; cannot replace current generation |
| SQ-08 | Third re-anchor attempt followed by exhaustion | Publisher Broken; no snapshot served |
| SQ-09 | New connection generation after broken state | Attempt budget resets; still unavailable until revalidated |
| SQ-10 | Live diff | New immutable snapshot; old Arc snapshot unchanged |
| SQ-11 | Watermark equal to current watermark | Rejected |
| SQ-12 | Crossed/locked book | Rejected |

### C. Freshness

| ID | Scenario | Expected |
|---|---|---|
| FR-01 | Source age exactly 2,000 ms | Accepted |
| FR-02 | Source age 2,001 ms | Rejected; API 503 |
| FR-03 | Observation age exactly 2,000 ms | Accepted |
| FR-04 | Observation age 2,001 ms | Rejected; API 503 |
| FR-05 | Future skew exactly 250 ms | Accepted |
| FR-06 | Future skew 251 ms | Rejected; API 503 |
| FR-07 | Fresh at publication, stale at request | 503; no old snapshot body |
| FR-08 | Non-positive or malformed source timestamp | Rejected before publication |

### D. Authentication and transport boundary

| ID | Scenario | Expected |
|---|---|---|
| AU-01 | Correct bearer token | Authorized |
| AU-02 | Missing Authorization header | 401 |
| AU-03 | Wrong token | 401; no bid/ask disclosure |
| AU-04 | Token comparison | Constant-time comparison retained |
| AU-05 | Empty token passed to serve | Serve rejects startup |
| AU-06 | Runtime token env unset/empty | HTTP listener remains disabled |
| AU-07 | Production bind helper | Binds only 127.0.0.1:8787 |
| AU-08 | Non-loopback listener passed to serve | PermissionDenied |
| AU-09 | Non-loopback peer | Connection dropped; no data |
| AU-10 | POST/PUT/DELETE | 405 |
| AU-11 | Unknown route/symbol | 404 |
| AU-12 | Request over 4 KiB | 413 |
| AU-13 | Request body or Transfer-Encoding | 400 |
| AU-14 | More than 8 concurrent clients | Excess request receives 503; bounded resource use |
| AU-15 | I/O deadline exceeded | Connection closes within bounded deadline |
| AU-16 | Successful response headers | application/json, no-store, nosniff, connection close |

### E. Fail-closed behavior

| ID | Scenario | Expected |
|---|---|---|
| FC-01 | Publisher has no snapshot | 503 snapshot_unavailable |
| FC-02 | Publisher state Broken | 503; no cached snapshot |
| FC-03 | Publisher state Stopped | 503; no cached snapshot |
| FC-04 | Re-anchor in progress | 503; no previous generation snapshot |
| FC-05 | Stale source timestamp | 503; no body containing old levels |
| FC-06 | Stale observation timestamp | 503 |
| FC-07 | Oversized serialized snapshot | 503 snapshot_exceeds_response_limit; no truncation |
| FC-08 | Capacity exhausted | 503 capacity_unavailable |
| FC-09 | Publisher read lock poisoned / internal read failure | No 200; bounded failure |
| FC-10 | HTTP task stops or listener unavailable | Consumer sees transport failure and must not use last cached response as current |

### F. Non-regression / isolation

| ID | Scenario | Expected |
|---|---|---|
| NR-01 | Build and test Publisher/API | No writes to collector CSV tree |
| NR-02 | HTTP GET repeated | No collector state mutation |
| NR-03 | HTTP error paths | No collector state mutation |
| NR-04 | Existing ingestion unit/integration tests | Unchanged results |
| NR-05 | Diff review | No changes to collector ingestion, CSV writer, snapshot/diff persistence, gap registry, or historical data |
| NR-06 | CI checkout | Exact intended T1 branch commit; no cross-repository SHA substitution |

## 4. Priority and gate

P0 — must pass before API V1 implementation acceptance:
- CT-01, CT-04, CT-05, CT-08, CT-09
- SQ-01 through SQ-08, SQ-10 through SQ-12
- FR-01 through FR-07
- AU-01 through AU-08, AU-10 through AU-16
- FC-01 through FC-08
- NR-01 through NR-06

P1 — hardening:
- AU-09, FC-09, FC-10, CT-10

A test that has only been specified is NOT RUN. Existing source tests are not counted as passing until a workflow run on the exact source SHA proves them.

## 5. Acceptance evidence

Record:
- Repository and branch.
- Exact commit SHA checked out by CI.
- Workflow run URL and run ID.
- Each job conclusion and test count.
- Test report/artifact name, ID, digest, and expiry where uploaded.
- Explicit diff-based proof that ingestion and historical-data paths were not changed.

No runtime/VPS acceptance is implied by CI. No T1 activation or T2 integration is authorized by this specification.
