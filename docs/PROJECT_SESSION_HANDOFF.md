# T1 Project Session Handoff

Updated: 2026-11-17
Repository: `kenznguyen/binance-futures-collector`
Active branch: `feature/t1-connection-sidecar-integration-20261107`
Pull request: [#1 — Add durable pre-handshake connection failure evidence](https://github.com/kenznguyen/binance-futures-collector/pull/1)
Reviewed source SHA: `2c44f3a38ead3dccaf6c37e61a354681a48b243b`

## 1. Current disposition

- PR #1: OPEN / DRAFT / NOT MERGED. Do not merge without explicit approval.
- CI #756: [run 44034675130](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44034675130), exact source SHA `ded4cdef160746a9d683a6792fc7b990ffaf4317` (historical checkpoint).
- CI #761: [run 44042222048](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44042222048), exact source SHA `2c44f3a38ead3dccaf6c37e61a354681a48b243b`; all 6/6 jobs PASS, including x86_64/native ARM64 Collector, Dashboard frontend/backend, Security and Docker.
- Strict HTTP/1.1 framing parser fix: PASS in exact-head CI #761 on both architectures. HTTP/1.1 requires exactly one non-empty Host; duplicate Host and duplicate Content-Length (including duplicate zero values) are rejected with HTTP 400; existing duplicate Authorization and Transfer-Encoding/body rejection remain.
- Boundary test `bounded_response_writer_accepts_exact_limit_then_rejects_next_byte`: PASS on x86_64 and native ARM64.
- Collector, Sidecar, Dashboard and Security jobs: PASS.
- Docker image build: PASS. All 6/6 jobs in CI #756 completed successfully; overall workflow: PASS.
- HTTP contract review: PARTIAL; implementation review completed for the current source, with documentation/strictness discrepancies listed below.
- T1 runtime: NOT VERIFIED.
- T1 to T2 integration: BLOCKED / not authorized.
- No deployment, listener activation, runtime secret, Binance connection, VPS operation, T0/T2 change, or real order execution was performed.

## 2. Exact CI #756 evidence

Workflow run: [44034675130](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44034675130)

Both x86_64 and native ARM64 jobs checked out the exact source SHA above and passed formatting, Clippy, build, Collector tests, Sidecar offline tests, and artifact publication.

Per architecture:
- Collector test suite: 126 passed, 0 failed.
- Additional test suite: 5 passed, 0 failed.
- Publisher/HTTP contract integration: 19 passed, 0 failed.
- Sidecar unit tests: 12 passed, 0 failed.
- Sidecar integration tests: 27 passed, 0 failed.
- New exact-capacity regression test passed: writing exactly the limit succeeds; the next byte is rejected; already-buffered bytes remain unchanged; the exceeded flag is set.

Artifacts published by run #756:
- x86_64: artifact ID `14307161019`, name `t1-collector-linux-x86_64-ded4cdef160746a9d683a6792fc7b990ffaf4317`, size 4,333,905 bytes, GitHub artifact ZIP SHA-256 `2d2f5e6d19c02271cedab839de61d6c1c8966026735b4165bcacdcc4cd84c2d7`.
- ARM64: artifact ID `14306567594`, name `t1-collector-linux-aarch64-ded4cdef160746a9d683a6792fc7b990ffaf4317`, size 4,036,787 bytes, GitHub artifact ZIP SHA-256 `57c9f513fc6ff179a457239cb70399c1ea50aada61652299eb47f99e61814f58`.

These are CI artifacts, not deployed binaries or runtime evidence. Artifact expiry reported by GitHub: 2026-11-24 UTC.

## 2A. Exact CI #761 evidence — strict HTTP/1.1 framing headers

Workflow run: [44042222048](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44042222048)
Exact source SHA: `2c44f3a38ead3dccaf6c37e61a354681a48b243b`

The test-first sequence is preserved:
- Test-only SHA `cffb8717aaa20bb7efe032eddb86ae40897cfb23` produced genuine RED in CI #759: x86_64 and native ARM64 each had 126 passed / 2 failed. The two intended failures were `http_11_requires_exactly_one_nonempty_host_header` and `duplicate_content_length_headers_are_rejected_even_when_both_are_zero`.
- Parser fix SHA `2b9103c61ddcf25433efaad07c1e211a65afa045` implemented unique/non-empty Host and unique Content-Length enforcement. CI #760 stopped at `cargo fmt --check` on both Collector architecture jobs; this was formatting-only and did not establish behavioral GREEN.
- Formatting-only follow-up SHA `2c44f3a38ead3dccaf6c37e61a354681a48b243b` passed exact-head CI #761, all 6/6 jobs.

Per architecture in CI #761:
- Collector test suite: 128 passed, 0 failed.
- Additional test suite: 5 passed, 0 failed.
- Publisher/HTTP contract integration: 19 passed, 0 failed. Includes `production_bind_helper_uses_only_the_documented_loopback_socket`.
- Sidecar unit tests: 12 passed, 0 failed.
- Sidecar integration tests: 27 passed, 0 failed.
- Format, Clippy, build, and sidecar format/Clippy passed.

Artifacts published by CI #761:
- x86_64: artifact ID `14309522571`, name `t1-collector-linux-x86_64-2c44f3a38ead3dccaf6c37e61a354681a48b243b`, size 4,335,201 bytes, GitHub artifact ZIP SHA-256 `4d1422fee72385cadd3a81a36da02ef8a3623c92f48a43736f3ac18271807352`.
- ARM64: artifact ID `14309772626`, name `t1-collector-linux-aarch64-2c44f3a38ead3dccaf6c37e61a354681a48b243b`, size 4,037,368 bytes, GitHub artifact ZIP SHA-256 `20a6aa9448e6dcf917354c385e5f70aae3769790c1f1ace4769d8158e798eaea`. ELF header verification reported `Machine: AArch64`.

These are CI artifacts, not deployed binaries or runtime evidence. GitHub artifact retention is 7 days; do not treat artifacts as permanent backups.

## 3. HTTP V1 implementation review

Reviewed baseline at source SHA `ded4cdef160746a9d683a6792fc7b990ffaf4317`; strict framing and production bind follow-up verified at `2c44f3a38ead3dccaf6c37e61a354681a48b243b`:
- `src/orderbook_http.rs`
- `src/orderbook_v1.rs`
- `src/main.rs`
- `docs/T1_ORDERBOOK_PUBLISHER_API_V1_CONTRACT_2026-11-15.md`
- `docs/T1_ORDERBOOK_PUBLISHER_API_V1_TEST_SPEC_2026-11-15.md`

Confirmed in source:
- Production listener helper binds `127.0.0.1:8787`; startup enables the listener only when non-empty `T1_ORDERBOOK_HTTP_TOKEN` is supplied out-of-band.
- `serve` independently rejects non-loopback listeners.
- Bearer authorization is required; duplicate Authorization headers are rejected with HTTP 400 and `duplicate_authorization`.
- Request size is capped at 4 KiB; I/O deadline is 750 ms.
- At most 8 active request handlers are admitted; a separate bounded pool permits up to 8 short rejection handlers to return capacity 503 responses.
- Only GET for BTCUSDT is served; request bodies and Transfer-Encoding are rejected.
- Publisher read is fail-closed and revalidates freshness at request time; no stale snapshot fallback/cache.
- Publisher enforces canonical V1 contract metadata, minimum 20 bids/asks, matching depth metadata, sorted levels and snapshot invariants.
- Decimal price/quantity values remain strings.
- Response JSON is serialized through a bounded writer capped at 64 KiB. Oversize returns 503 without returning a partial snapshot.

## 4. Contract and test gaps to resolve

These are review findings, not approved source changes:
1. Contract error table does not yet list the implemented `400 duplicate_authorization` response.
2. Contract says at most 8 concurrent connections; implementation allows 8 active handlers plus up to 8 bounded rejection-response handlers. Clarify whether the contract means active request capacity or total accepted sockets.
3. Strict HTTP/1.1 framing policy for Host and Content-Length is now tested and enforced at SHA `2c44f3a38ead3dccaf6c37e61a354681a48b243b`; CI #761 passed both architectures.
4. Production bind helper loopback behavior now has a direct regression test (`production_bind_helper_uses_only_the_documented_loopback_socket`), passing in CI #761.
5. Non-loopback peer rejection and publisher lock-poison failure paths remain additional hardening coverage items unless separately evidenced by later exact-SHA tests.
6. The contract/test matrix is broader than the current test counts. Do not equate CI #761 with complete V1 contract acceptance.

## 5. Prior verified T1 work retained

- Publisher canonical metadata/depth validation and HTTP V1 test fixture alignment are present in the branch history.
- Duplicate Authorization regression: genuine RED was recorded before the fix; the fix rejects the second Authorization header immediately with HTTP 400.
- Bounded response serialization: production writer rejects over-limit writes before retaining them; handler only sends HTTP 200 after complete serialization.
- Independent code review of the bounded writer found no blocking correctness/security issue in that path. The exact-capacity test gap identified by that review is now covered and passes on both architectures.

## 6. Repository / workflow boundaries

- Continue on `feature/t1-connection-sidecar-integration-20261107`; do not create a new branch or rebase/diverge.
- Keep PR #1 open and draft; no merge without explicit approval.
- Do not activate the HTTP listener, inject runtime token secrets, run collector/Binance smoke tests, deploy to VPS, or connect T2 without separate approval.
- No T0 data, T0 source, T2 integration, collector live ingestion, CSV historical output, or execution behavior may be changed as part of this handoff.

## 7. Next actions

1. CI #761 terminal conclusion: SUCCESS, all 6/6 jobs PASS on exact source SHA `2c44f3a38ead3dccaf6c37e61a354681a48b243b`; workflow run: [44042222048](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44042222048).
2. Update the PR checkpoint comment with CI #761 test counts, artifacts and checksums, and link this handoff commit.
3. Resolve remaining HTTP contract discrepancies through tests first, in a separately scoped increment on this branch. In particular, document the 8 active + up to 8 rejection-handler capacity semantics and update the contract error table for `duplicate_authorization`, `missing_host`, `invalid_host`, `duplicate_host`, and `duplicate_content_length`.
4. Add evidence for non-loopback peer rejection and publisher lock-poison failure paths if applicable to the intended V1 contract.
5. Re-run exact-head CI for every subsequent source or contract change and record exact SHA, run ID, test counts and artifacts.
6. Only after the remaining V1 acceptance matrix is evidenced should T1 API acceptance or T1-to-T2 integration be reconsidered.

## 8. Status summary

| Area | Status |
|---|---|
| Exact-capacity boundary test | PASS |
| Strict HTTP/1.1 Host / Content-Length framing | PASS — CI #761, x86_64 + ARM64 |
| Production loopback bind helper regression | PASS — CI #761, x86_64 + ARM64 |
| x86_64 Collector / Sidecar tests | PASS |
| Native ARM64 Collector / Sidecar tests | PASS |
| Dashboard frontend/backend | PASS |
| Security job | PASS |
| Docker / full CI #756 | PASS — 6/6 jobs |
| Docker / full CI #761 | PASS — 6/6 jobs |
| HTTP V1 full contract acceptance | PARTIAL / NOT VERIFIED |
| T1 runtime readiness | NOT VERIFIED |
| T1 to T2 integration | BLOCKED |
