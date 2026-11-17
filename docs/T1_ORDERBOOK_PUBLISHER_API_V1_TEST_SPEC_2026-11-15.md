# T1 Order Book Publisher / API V1 — Test Specification

Status: PARTIAL IMPLEMENTATION / EVIDENCE — the P0 matrix is not yet fully evidenced. Exact-SHA CI #761 covers strict HTTP/1.1 framing and production bind behavior; see the coverage update below.
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

These were source-level observations when the specification was first written. Exact-SHA CI evidence is recorded in the P3 evidence document; a specified scenario without an exact-SHA test result remains NOT RUN.

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
| CT-11 | Snapshot identity | `snapshotId` matches the T2 SHA-256 canonical JSON algorithm; any payload mutation invalidates the identity |

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
| AU-17 | HTTP/1.1 Host header missing, empty, or duplicated | 400; no snapshot disclosure |
| AU-18 | Duplicate Content-Length headers, including two zero values | 400; no snapshot disclosure |
| AU-19 | Duplicate Authorization headers in either order | 400; no snapshot disclosure |

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
- AU-01 through AU-08, AU-10 through AU-19
- FC-01 through FC-08
- NR-01 through NR-06

P1 — hardening:
- AU-09, FC-10, CT-10
- FC-09 has since been verified on exact-head CI #767 (see the 2026-11-17 update below).

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


## 5. Exact-SHA coverage update — 2026-11-15

- Exact T1 source SHA: `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`.
- Push workflow [#685](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43711013002): collector fmt/Clippy/release build/all tests PASS; 111 library tests and 27 P3 Order Book HTTP contract integration tests PASS. Native ARM64 job also PASS with the same test counts.
- New HTTP test coverage in `src/orderbook_http.rs`:
  - AU-04: equal/content/length mismatch semantics; implementation uses a fixed 4 KiB comparison loop with no early length return.
  - AU-05: empty and overlong bearer token rejected before listener accept loop.
  - AU-08: non-loopback listener rejected.
  - AU-12: oversized request headers return 413.
  - AU-13: request body and Transfer-Encoding return 400.
  - AU-14: concurrent excess client receives 503; overflow response tasks are bounded separately from active request tasks.
  - AU-15: incomplete headers are closed at the configured I/O deadline.
  - AU-16: JSON content type, no-store, nosniff and connection-close headers are asserted.
  - FC-07: oversized snapshot returns 503 and is not truncated or disclosed.
  - FC-08: request capacity exhaustion returns 503.
- TDD RED for AU-05: test commit `7c20bb031bafb540648a5a14922bf6f760465fe3`, workflow #683 / run ID `43710799427`, 110 passed and 1 failed because an overlong configured bearer token was not rejected. Fix commit `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`; the corresponding test passes in run #685.
- Full P0 matrix remains **PARTIAL / NOT ACCEPTED**. In particular, environment-disabled listener behavior and every publisher fail-closed state (no snapshot, stopped, broken, reanchor/validation, independent source/observation staleness) are not all tied to dedicated exact-SHA tests yet.
- The security workflow in #685 remains FAIL due the recorded Rust advisories. This test evidence does not override the security gate or authorize runtime activation.

## 6. Exact-SHA coverage update — 2026-11-17

- Exact source SHA: `2c44f3a38ead3dccaf6c37e61a354681a48b243b`.
- Workflow [#761](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44042222048): all 6/6 jobs PASS (x86_64 Collector, native ARM64 Collector, Dashboard frontend/backend, Security, Docker).
- On both architectures: Collector 128/128, additional 5/5, Publisher/HTTP integration 19/19, Sidecar unit 12/12, Sidecar integration 27/27; Format, Clippy and Build PASS.
- AU-07 production bind helper regression `production_bind_helper_uses_only_the_documented_loopback_socket`: PASS on x86_64 and ARM64.
- AU-17 `http_11_requires_exactly_one_nonempty_host_header`: PASS on x86_64 and ARM64. Covers missing, empty and duplicate Host with HTTP 400 and no snapshot disclosure.
- AU-18 `duplicate_content_length_headers_are_rejected_even_when_both_are_zero`: PASS on x86_64 and ARM64.
- AU-19 `duplicate_authorization_headers_are_rejected_regardless_of_order`: retained and PASS on x86_64 and ARM64.
- The contract now defines 8 active request handlers + up to 8 bounded rejection writers; further sockets are dropped without response. The existing concurrency test verifies an excess client receives bounded 503 while rejection capacity is available.
- CI artifacts: x86_64 ID `14309522571`, ZIP SHA256 `4d1422fee72385cadd3a81a36da02ef8a3623c92f48a43736f3ac18271807352`; ARM64 ID `14309772626`, ZIP SHA256 `20a6aa9448e6dcf917354c385e5f70aae3769790c1f1ace4769d8158e798eaea` (7-day retention).
- AU-09 is NOT EXERCISED at the TCP integration level under the mandated loopback-only offline harness: the production listener is bound to 127.0.0.1, so a non-loopback remote peer cannot establish a TCP connection to it. A fabricated SocketAddr unit test would only retest the standard-library address predicate and would not prove a socket-level drop; do not weaken the loopback bind or claim such a test as network evidence. The accept loop retains a defensive `!peer.ip().is_loopback()` drop check.
- FC-09 was subsequently verified on exact fix SHA `cb1ccc0df79584d1b84f257b033a7872b6d409dc`, CI #767 (see below).
- Other unexecuted rows remain subject to the required matrix. Full V1 acceptance remains PARTIAL / NOT VERIFIED; no runtime acceptance is implied.

## 7. Exact-SHA coverage update — 2026-11-17 FC-09

- Test-only SHA: `13ab62841a4c28b410c22caa8c9dbe8bf67be4af`; exact-head CI [#766](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44048337134) confirmed genuine RED on x86_64 and native ARM64 (128 passed / 1 failed on each; the only failing test was `publisher_read_fresh_returns_error_when_lock_is_poisoned`).
- Fix SHA: `cb1ccc0df79584d1b84f257b033a7872b6d409dc`; `SnapshotPublisher::read_fresh` now maps poisoned read-lock state to an ordinary error and does not recover the poisoned lock.
- Exact-head CI [#767](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44049288867): SUCCESS, all 6/6 jobs. Both x86_64 and native ARM64: Collector 129/129, additional 5/5, Publisher/HTTP integration 19/19, Sidecar unit 12/12, Sidecar integration 27/27. FC-09 test explicitly PASS on both architectures. Format, Clippy, Build, Dashboard frontend/backend, Security and Docker PASS.
- Artifacts (7-day retention; CI evidence only): x86_64 ID `14313036815`, ZIP SHA256 `71179ce9cf5310411b0c4830b2f44a6280b1c007c4dcb8f7e8b32898cb1a2242`; ARM64 ID `14313091850`, ZIP SHA256 `9864f3cd2de65fb5552c5210361127f0a6610170d16769d2c3cf05c2973f440c`. ARM64 ELF machine verified as AArch64.
- FC-09: PASS for the tested publisher `read_fresh` poisoned-lock path. This does not claim every possible internal publisher error or full HTTP V1 acceptance.
- AU-09: NOT EXERCISED at TCP level for the loopback-only reason above. Full V1 acceptance remains PARTIAL / NOT VERIFIED.


## 8. Exact-SHA matrix reconciliation — 2026-11-17

Scope: source/test-to-requirement mapping only; no production code changed.
- Exact HTTP/parser source SHA: `29667927d944e3e20db4d7e3c1ca6e2b30eb7975`.
- Exact-source CI #790: [run 44095989717](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44095989717), SUCCESS 6/6 jobs. x86_64 and native ARM64 each report Collector 133/133, additional suite 5/5, HTTP/P3 contract 22/22, Sidecar unit 12/12 and Sidecar integration 27/27. Format, Clippy, Build, Dashboard backend/frontend, Security and Docker PASS.
- The following mapping is based on test names present in the exact source SHA and the complete test-suite result in #790. Unless explicitly stated, it is not a separately named workflow step per matrix row.

| Matrix IDs | Exact-source test evidence | Disposition |
|---|---|---|
| CT-01, CT-04, CT-05 | `ready_snapshot_is_returned_as_one_exact_v1_document_without_numeric_coercion`; `serialized_snapshot_matches_t2_v1_wire_field_names_and_string_levels` | PASS |
| CT-02 | `publisher_rejects_noncanonical_contract_metadata_and_invalidates_ready_snapshot` | PASS |
| CT-03, AU-10, AU-11 | `only_get_for_the_published_symbol_is_accepted` | PASS for tested method/symbol route cases |
| CT-06, CT-07 | `duplicate_normalized_levels_and_unsorted_snapshot_are_rejected`; live duplicate/crossed-book rejection tests | PASS |
| CT-08 | `insufficient_depth_and_crossed_books_are_rejected`; `publisher_rejects_insufficient_depth_and_invalidates_ready_snapshot` | PASS |
| CT-09 | `live_diff_requires_exact_pu_and_publishes_a_new_immutable_watermark`; exact wire serialization assertion | PASS |
| CT-11 | `publisher_rejects_noncanonical_snapshot_id_and_invalidates_ready_snapshot`; canonical identity is also asserted by exact-schema HTTP test group 4 | PASS |
| SQ-01, SQ-03 | `exact_bridge_and_pu_chain_build_a_complete_snapshot` | PASS |
| SQ-02 | `legacy_pu_attach_bridge_is_rejected`; `rest_anchor_without_exchange_timestamped_bridge_is_not_publishable` | PASS |
| SQ-04, SQ-05 | `sequence_gap_duplicate_or_regression_is_rejected`; `publisher_rejects_same_generation_watermark_regression` | PASS |
| SQ-06, SQ-07 | `publisher_invalidates_old_snapshot_and_fences_late_generation`; `publisher_does_not_serve_last_good_snapshot_after_reanchor_attempt_begins` | PASS |
| SQ-08, SQ-09 | `reanchor_budget_stops_after_three_attempts_and_resets_on_new_connection` | PASS |
| SQ-10 | `live_diff_requires_exact_pu_and_publishes_a_new_immutable_watermark`; `live_zero_quantity_deletes_level_without_mutating_prior_snapshot` | PASS |
| SQ-11 | `publisher_rejects_same_generation_watermark_regression` | PASS |
| SQ-12 | `insufficient_depth_and_crossed_books_are_rejected`; `live_duplicate_normalized_prices_and_crossed_book_are_rejected` | PASS |
| FR-01 through FR-06 | `source_and_observation_exact_freshness_boundaries_are_accepted`; `source_and_observation_freshness_boundaries_are_enforced`; `classified_live_diff_maps_source_observation_and_future_skew_to_freshness` | PASS for source-observation age and future-skew boundaries |
| FR-07 | `publisher_read_rechecks_freshness_at_decision_time`; HTTP stale-source/stale-observation/age-after-publication regressions from group 3 | PASS |
| FR-08 | Header validation rejects event_time <= 0, but no dedicated exact-SHA test was identified for non-positive/malformed source timestamps at the publication boundary | NOT VERIFIED |
| AU-01 through AU-05 | HTTP bearer-auth tests, including constant-time comparison, missing/wrong/duplicate Authorization and empty/overlong configured token tests | PASS |
| AU-06 | `orderbook_http_listener_is_disabled_when_token_is_missing_or_empty` and main startup gate test | PASS in exact-source unit suite |
| AU-07, AU-08 | `production_bind_helper_uses_only_the_documented_loopback_socket`; `serve_rejects_a_non_loopback_listener` | PASS; AU-07 also explicitly passed in exact-head CI #761 |
| AU-09 | No real non-loopback TCP peer can reach the loopback-only listener in the mandated offline harness | NOT EXERCISED |
| AU-12 through AU-16 | Oversize request, body/Transfer-Encoding, I/O deadline, bounded concurrency, and response-header tests | PASS |
| AU-17 through AU-19 | Exact tests for missing/empty/duplicate Host, duplicate Content-Length, duplicate Authorization | PASS; also explicitly reported on both architectures in exact-head CI #761 |
| FC-01 through FC-08 | No-snapshot/stopped, broken, reanchor/fetching/validating, stale-source/observation, oversized response and capacity tests | PASS |
| FC-09 | `publisher_read_fresh_returns_error_when_lock_is_poisoned`; RED #766 and fix GREEN #767 | PASS on exact fix SHA `cb1ccc0df79584d1b84f257b033a7872b6d409dc`; #790 also reruns the suite on later source SHA |
| FC-10 | No HTTP consumer/client integration test proves transport failure prevents stale cached response reuse | NOT VERIFIED |
| NR-01 through NR-03 | No dedicated filesystem/collector-state immutability assertions identified for build, repeated GET and HTTP error paths | NOT VERIFIED |
| NR-04 | Existing `cargo test --all` suite passes on both architectures in #790; no baseline-vs-feature result comparison recorded here | PARTIAL |
| NR-05 | PR #1 changed-file list and patches include `src/collectors/orderbook_manager.rs`, `src/collectors/rest_collector.rs`, `src/utils/csv_writer.rs`, `src/utils/gap_registry.rs`, and `src/websocket/message_handler.rs`; this violates the matrix row's literal no-change requirement | FAIL (scope conflict) |
| NR-06 | Exact source SHA and repository branch were checked for CI #790; exact handoff SHA separately verified by CI #791 | PASS |

### Remaining matrix work and gates

1. Add a deterministic offline regression for FR-08: zero/negative/malformed source timestamp must be rejected before publication and must not expose a snapshot. Use TDD RED/GREEN on the existing branch, then exact-head CI on x86_64 and native ARM64.
2. Resolve CT-10 by explicitly documenting the wire compatibility policy. The current HTTP response exact-key assertion proves the emitted schema, but does not alone define consumer behavior for unknown additive fields or future contract versions.
3. Keep AU-09 NOT EXERCISED unless a genuinely valid isolated TCP topology can test a non-loopback peer without weakening the production loopback bind. Do not substitute a fabricated peer address.
4. Keep FC-10 NOT VERIFIED until a consumer-side test demonstrates transport failure invalidates/ages out cached data and cannot be treated as current.
5. NR-05 is FAIL against the current PR-wide scope: ingestion/order-book manager, REST collector, CSV writer, gap registry and WebSocket message handler are changed in PR #1. This is a pre-existing PR scope conflict, not introduced by this documentation-only checkpoint. Do not claim historical-data isolation or narrow the criterion silently; require an explicit scope decision before treating NR-05 as satisfied. NR-01..03 remain NOT VERIFIED and NR-04 PARTIAL.

Current acceptance: HTTP V1 full P0/P1 matrix **PARTIAL / NOT VERIFIED**. CI #790 is exact-source PASS; CI #791 is handoff/docs SHA PASS, not runtime acceptance. No listener activation, runtime token injection, VPS deployment, T0/T2 changes, T1→T2 connection, or real orders.


### NR-05 scope finding — 2026-11-17

The PR-wide changed-file list and inspected patches confirm modifications to collector/ingestion and historical-data-adjacent components, including `src/collectors/orderbook_manager.rs`, `src/collectors/rest_collector.rs`, `src/utils/csv_writer.rs`, `src/utils/gap_registry.rs`, and `src/websocket/message_handler.rs`. Therefore NR-05, as currently worded ("No changes" to these paths), is **FAIL for PR #1 as a whole**. This does not establish data corruption; it establishes that the strict no-change acceptance criterion is not met. Do not re-label these changes as HTTP-only or silently waive the criterion. A separately approved scope interpretation is required before acceptance.
