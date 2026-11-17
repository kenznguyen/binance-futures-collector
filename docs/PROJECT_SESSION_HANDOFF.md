# T1 Project Session Handoff

Updated: 2026-11-17
Repository: `kenznguyen/binance-futures-collector`
Active branch: `feature/t1-connection-sidecar-integration-20261107`
Pull request: [#1 — Add durable pre-handshake connection failure evidence](https://github.com/kenznguyen/binance-futures-collector/pull/1)
Reviewed source SHA: `162557e4c53ad7ac6f5a11264f05cab5138e442f`
Latest source SHA: `162557e4c53ad7ac6f5a11264f05cab5138e442f`
Latest evidence/docs commits: FR-08 handoff `98e1444027cf69fbb80ca46c35ee9a78b0125e95`; CT-10 contract `779eb72ff9aefa9a0c55acbdd828c95b970d2799`; test spec `49d1c0849e6f4be59a45aece7e0222f1cac59163`

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

## 6A. Publisher poisoned read-lock regression — PASS

- Test-only commit: `13ab62841a4c28b410c22caa8c9dbe8bf67be4af`, message `test(publisher): cover poisoned read lock failure`.
- Added `publisher_read_fresh_returns_error_when_lock_is_poisoned`; CI #766 confirmed genuine RED on x86_64 and native ARM64: each 128 passed / 1 failed, with the new regression as the only failing test. Logs located the panic at the `expect("snapshot publisher lock poisoned")` in `read_fresh`.
- Minimal fix commit: `cb1ccc0df79584d1b84f257b033a7872b6d409dc`, message `fix(publisher): fail closed on poisoned read lock`. Only `read_fresh` changed: lock poison maps to `anyhow::anyhow!("snapshot publisher lock poisoned")` and returns `Err`; poisoned state is not recovered.
- Exact-head CI [#767](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44049288867) completed SUCCESS, all 6/6 jobs, on exact fix SHA `cb1ccc0df79584d1b84f257b033a7872b6d409dc`.
- Both x86_64 and native ARM64: Collector 129/129; additional tests 5/5; HTTP/P3 integration 19/19; Sidecar unit 12/12; Sidecar integration 27/27. FC-09 regression explicitly passed on both architectures. Format, Clippy, Build, Dashboard frontend/backend, Security, Docker all PASS.
- CI #767 artifacts (7-day retention; CI evidence only, not deployed binaries):
  - x86_64 artifact ID `14313036815`, name `t1-collector-linux-x86_64-cb1ccc0df79584d1b84f257b033a7872b6d409dc`, size 4,335,456 bytes, ZIP SHA-256 `71179ce9cf5310411b0c4830b2f44a6280b1c007c4dcb8f7e8b32898cb1a2242`.
  - ARM64 artifact ID `14313091850`, name `t1-collector-linux-aarch64-cb1ccc0df79584d1b84f257b033a7872b6d409dc`, size 4,037,415 bytes, ZIP SHA-256 `9864f3cd2de65fb5552c5210361127f0a6610170d16769d2c3cf05c2973f440c`. ELF header verified `AArch64`.
- FC-09 is PASS for the tested `read_fresh` failure path. This does not by itself establish full HTTP V1 contract acceptance or runtime readiness.


## 6D. HTTP V1 regression sequence — authentication PASS

- Authentication regression test was added at source commit `bb2931bb4db89a51b6f0016d75de9de43721ec96`, but its first CI run #778 (`44068318739`) failed because the test request literal contained escaped backslashes instead of actual CRLF framing. The handler correctly returned `400 incomplete_request`; this was invalid test input, not evidence of an authentication defect.
- Corrected only the test request literal in commit `4c0dfe7c538de7e97016a670d5731bf8003a24d9`, message `test(http): correct missing authorization request framing`. The request now uses valid HTTP CRLF framing and intentionally omits Authorization. No production code changed.
- Exact-head CI #780: [run 44071625375](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44071625375), SUCCESS, all 6/6 jobs on exact SHA `4c0dfe7c538de7e97016a670d5731bf8003a24d9`.
- x86_64 and native ARM64: Collector 131/131; additional test suites 5/5 and HTTP/P3 integration 19/19; Sidecar unit 12/12 and Sidecar integration 27/27. The `missing_authorization_header_is_rejected_without_snapshot_disclosure` test explicitly passed on both architectures. Format, Clippy, Build, Dashboard frontend/backend, Security and Docker all PASS.
- CI artifacts (7-day retention; CI evidence only, not deployed binaries): x86_64 artifact ID `14321764052`, name `t1-collector-linux-x86_64-4c0dfe7c538de7e97016a670d5731bf8003a24d9`, size 4,335,457 bytes; ARM64 artifact ID `14322602274`, name `t1-collector-linux-aarch64-4c0dfe7c538de7e97016a670d5731bf8003a24d9`, size 4,037,419 bytes; ARM64 ELF header verified AArch64.
- Status: authentication regression PASS. Token/request-size PASS. Freshness, wire-schema and strict-parser regression groups remain pending in that order. HTTP V1 full contract acceptance remains PARTIAL / NOT VERIFIED; T1 runtime NOT VERIFIED; T1-to-T2 integration BLOCKED.
- Boundaries unchanged: remain on this branch; PR #1 OPEN / DRAFT / NOT MERGED. No listener activation, secret injection, Binance smoke test, VPS deployment, T0/T2 changes or real orders.


## 7. Next actions

1. CI #761 terminal conclusion: SUCCESS, all 6/6 jobs PASS on exact source SHA `2c44f3a38ead3dccaf6c37e61a354681a48b243b`; workflow run: [44042222048](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44042222048).
2. Update the PR checkpoint comment with CI #761 test counts, artifacts and checksums, and link this handoff commit.
3. Resolve remaining HTTP contract discrepancies through tests first, in a separately scoped increment on this branch. In particular, document the 8 active + up to 8 rejection-handler capacity semantics and update the contract error table for `duplicate_authorization`, `missing_host`, `invalid_host`, `duplicate_host`, and `duplicate_content_length`.
4. Add evidence for non-loopback peer rejection and publisher lock-poison failure paths if applicable to the intended V1 contract.
5. Continue remaining HTTP V1 acceptance matrix. AU-09 is documented as NOT EXERCISED at TCP level under the required loopback-only harness: a non-loopback peer cannot establish a TCP connection to a listener bound to 127.0.0.1. The source retains the defensive peer-address drop; do not weaken loopback binding or use a fabricated SocketAddr test as network evidence.
6. Re-run exact-head CI for every subsequent source or contract change and record exact SHA, run ID, test counts and artifacts.
6. Only after the remaining V1 acceptance matrix is evidenced should T1 API acceptance or T1-to-T2 integration be reconsidered.

## 8. Status summary

| Area | Status |
|---|---|
| Exact-capacity boundary test | PASS |
| Strict HTTP/1.1 Host / Content-Length framing | PASS — CI #761, x86_64 + ARM64 |
| Contract error table / capacity semantics synchronized | PASS — docs commits `cdc199e` / `aa205aa` |
| Production loopback bind helper regression | PASS — CI #761, x86_64 + ARM64 |
| Publisher poisoned read-lock regression | PASS — CI #767, x86_64 + ARM64 |
| AU-09 non-loopback peer TCP test | NOT EXERCISED — impossible under loopback-only listener/harness; defensive source check retained |
| x86_64 Collector / Sidecar tests | PASS |
| Native ARM64 Collector / Sidecar tests | PASS |
| Dashboard frontend/backend | PASS |
| Security job | PASS |
| Docker / full CI #756 | PASS — 6/6 jobs |
| Docker / full CI #761 | PASS — 6/6 jobs |
| HTTP V1 full contract acceptance | PARTIAL / NOT VERIFIED |
| T1 runtime readiness | NOT VERIFIED |
| T1 to T2 integration | BLOCKED |


## 6B. AU-09 testability boundary and documentation sync

- Source SHA remains `cb1ccc0df79584d1b84f257b033a7872b6d409dc`; no source/runtime behavior changed in this documentation checkpoint.
- Review of `serve` confirms the production listener is required to bind to a loopback address before `accept`; `bind_loopback` binds `127.0.0.1:8787`. The accept loop also defensively drops any peer for which `peer.ip().is_loopback()` is false.
- Under the test specification's loopback-only offline requirement, an actual non-loopback peer cannot complete a TCP connection to a loopback-bound listener. A fabricated `SocketAddr` unit test would only exercise the standard-library address predicate, not the network accept/drop behavior. Therefore AU-09 is documented as NOT EXERCISED at TCP level rather than manufacturing misleading evidence or weakening the bind guard.
- Test spec update commit `3f7f632e47ff497c5aadef6d565b4554889619c9`; contract update commit `ee6ad175784bbaf832ff7ec3b0f05de5d789d460`.
- This is a documentation/testability disposition, not full contract acceptance. AU-09 remains a known unexercised defense-in-depth path; HTTP V1 remains PARTIAL / NOT VERIFIED.


## 6C. HTTP V1 regression sequence — token/request-size PASS

- Regression test added first at commit `214cd300fade9e7228e1bb1a5ae781b7343cd7a6`: `maximum_configured_bearer_token_fits_a_minimal_valid_request`. The first CI attempt stopped at `cargo fmt --check`; formatting-only correction was committed at `c30ed314435c9d3b67403cdbaf804819a0dbcdd2`. Neither run was treated as regression RED evidence.
- Exact RED evidence: CI run `44065945507` on SHA `c30ed314435c9d3b67403cdbaf804819a0dbcdd2`; x86_64 and ARM64 each executed 129 passing collector tests and the new test failed with HTTP 413. This demonstrated that the configured maximum bearer token did not fit the minimum valid HTTP request framing within the 4 KiB request cap.
- Minimal fix commit: `60fc9b3a228222b78fe72d34f4456c1c17d57944`, message `fix(http): reserve request framing in bearer token limit`. `MAX_BEARER_TOKEN_BYTES` now reserves the complete minimum request framing overhead rather than only the `Bearer ` prefix. Request cap remains 4 KiB; no listener/runtime behavior was activated.
- Exact-head CI #776: [run 44066756115](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44066756115), SUCCESS, 6/6 jobs on exact SHA `60fc9b3a228222b78fe72d34f4456c1c17d57944`.
- x86_64 and native ARM64 both passed format, Clippy, build and collector tests; collector test result 130 passed / 0 failed on each architecture. The maximum-token regression explicitly passed on both.
- Dashboard backend, dashboard frontend, Docker image build and security jobs all PASS.
- CI artifacts (CI evidence only; not deployed binaries): x86_64 artifact ID `14320707388`, name `t1-collector-linux-x86_64-60fc9b3a228222b78fe72d34f4456c1c17d57944`, size 4,335,457 bytes; ARM64 artifact ID `14320439477`, name `t1-collector-linux-aarch64-60fc9b3a228222b78fe72d34f4456c1c17d57944`, size 4,037,419 bytes.
- Status: token/request-size regression PASS. Authentication, freshness, wire-schema and strict-parser regression groups remain pending in that order. HTTP V1 full contract acceptance remains PARTIAL / NOT VERIFIED; T1 runtime NOT VERIFIED; T1-to-T2 integration BLOCKED.
- Boundaries unchanged: remain on this branch; PR #1 OPEN / DRAFT / NOT MERGED. No listener activation, secret injection, Binance smoke test, VPS deployment, T0/T2 changes or real orders.


## 6E. HTTP V1 regression sequence — freshness PASS (2026-11-17)

- Freshness test commit: `913ff54eaa43fa511895af794d82f0251c2d3d4a`, message `test(http): cover independent freshness boundaries`. It added three HTTP integration regressions: stale source timestamp, stale observation timestamp, and a snapshot that becomes stale after publication. Each asserts HTTP 503 and no stale bid-price disclosure.
- The first exact-head CI attempt stopped at `cargo fmt --all --check` on both x86_64 and ARM64 because of formatting in the new test. This was a formatting-only failure; it did not establish RED/GREEN behavior.
- Formatting-only correction commit: `ca224427245268365d6a3c79558f006dfb808865`, message `style(test): format freshness boundary regressions`. No production source or test semantics changed in this correction.
- Exact SHA workflow: [CI #782 / run 44082443503](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44082443503), source SHA `ca224427245268365d6a3c79558f006dfb808865`.
- On both x86_64 and native ARM64: Format, Clippy, Build PASS; Collector 131/131; additional suite 5/5; HTTP/P3 contract 22/22; Sidecar unit 12/12; Sidecar integration 27/27. All three new freshness regressions explicitly PASS on both architectures.
- Dashboard backend, Dashboard frontend and Security jobs PASS. Docker image build job was still IN_PROGRESS at this handoff checkpoint; therefore the complete workflow was not yet terminal and must not be represented as 6/6 PASS.
- CI artifacts (7-day retention; CI evidence only, not deployed binaries): x86_64 ID `14327642353`, name `t1-collector-linux-x86_64-ca224427245268365d6a3c79558f006dfb808865`, size 4,335,457 bytes, ZIP SHA-256 `f601845339855bc16c2ed443ffef5494035b38241bce95ed883627ff3aba4c18`; ARM64 ID `14326669712`, name `t1-collector-linux-aarch64-ca224427245268365d6a3c79558f006dfb808865`, size 4,037,419 bytes, ZIP SHA-256 `b4ea716f33d6dd6bbefb70d87af63d88b455d02c94e4943660f96df6ac059107`. ARM64 ELF header verified AArch64.
- Status: freshness regression group PASS on both Collector architectures. Overall HTTP V1 full contract acceptance remains PARTIAL / NOT VERIFIED. Wire-schema and strict-parser groups remain next, in that order. AU-09 non-loopback TCP remains NOT EXERCISED under loopback-only harness.
- Boundaries unchanged: PR #1 OPEN / DRAFT / NOT MERGED; no listener activation, runtime token injection, Binance smoke test, VPS deployment, T0/T2 changes, T1→T2 integration or real orders.


## 6F. HTTP V1 regression sequence — wire schema PASS (2026-11-17)

- Test-only source commit: `e92919f696653140c28879dd79d74cbe6c7d8395`, message `test(http): assert complete V1 wire schema and identity`. It added an exact HTTP response assertion for the required top-level field set, field types, 20 bid/ask levels, two-string level tuples, exact depth metadata and recomputation of `snapshotId` from the response body.
- Formatting-only correction: `6566969632ac5c5ecc04303d6be3fb25dc1c3a30`, message `style(test): format HTTP V1 schema assertions`. No production source or test semantics changed.
- Exact-head CI #785: [run 44085612119](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44085612119), workflow conclusion SUCCESS, all 6/6 jobs, exact SHA `6566969632ac5c5ecc04303d6be3fb25dc1c3a30`.
- x86_64 Collector: 131/131; additional suite 5/5; HTTP/P3 contract 22/22; Sidecar unit 12/12; Sidecar integration 27/27. Native ARM64 passed the same test suites and build/format/Clippy checks. The test `authenticated_response_matches_exact_v1_schema_market_and_depth` explicitly passed on both architectures; it includes snapshot identity recomputation.
- Dashboard backend, Dashboard frontend, Security and Docker image build: PASS. Overall workflow: 6/6 jobs SUCCESS.
- CI artifacts (7-day retention; CI evidence only, not deployed binaries):
  - x86_64 ID `14328309241`, name `t1-collector-linux-x86_64-6566969632ac5c5ecc04303d6be3fb25dc1c3a30`, size 4,335,457 bytes, ZIP SHA-256 `db2ff69e181ecadfce40e498de6018907b9f52faf29bf0cc65a3d2a8d655c7e9`.
  - ARM64 ID `14329536648`, name `t1-collector-linux-aarch64-6566969632ac5c5ecc04303d6be3fb25dc1c3a30`, size 4,037,419 bytes, ZIP SHA-256 `dde327a444d16672e67fa488b76bd1558f1b6f1d1743f90ff8a6f208026b9560`; native ARM64 build and ELF verification passed.
- Regression sequence: token/request-size PASS; authentication PASS; freshness PASS; wire schema PASS. Next group is strict HTTP parser (tab-separated request line and invalid header-name syntax), test-first.
- AU-09 non-loopback peer TCP remains NOT EXERCISED under loopback-only harness. Do not weaken loopback binding or substitute fabricated-address tests as TCP evidence.
- HTTP V1 full contract acceptance remains PARTIAL / NOT VERIFIED. T1 runtime remains NOT VERIFIED; T1→T2 integration remains BLOCKED.
- PR #1 remains OPEN / DRAFT / NOT MERGED. No listener activation, runtime token injection, Binance connection, VPS deployment, T0/T2 changes, T1→T2 integration or real orders.


## 6G. HTTP V1 regression sequence — strict parser PASS (2026-11-17)

- Test-first source commit: `c1c6424a08ef4eaa36582c0e7975e499230db675`, message `test(http): use actual HTTP whitespace and line endings`. It corrected two regression requests to use actual Rust tab/CRLF escapes: tab-separated request-line and invalid header field-name. The earlier test commit `3fb69c7434c76922fcb0cda8c9bc53a9cbe9cf0e` used literal backslash sequences and is explicitly invalid evidence; do not count CI #787 as a valid RED/GREEN run.
- Valid RED: CI #788 / run `44094813998` on exact test SHA `c1c6424a08ef4eaa36582c0e7975e499230db675`. Both x86_64 and ARM64 reproduced both malformed requests as HTTP 200 with snapshot disclosure; each architecture had 131 passing collector tests and 2 failing new regressions. This is the accepted RED evidence.
- Minimal parser fix: source commit `596447d32f1a3ed0b5d778f7959d2ee3e7cf00a0`, message `fix(http): enforce strict request and header token syntax`. Request-line parsing now uses exact ASCII-space separators and rejects empty components / tabs; header names are validated against the HTTP token character set before header processing. Malformed requests are rejected with HTTP 400 before snapshot retrieval. No unrelated production files changed.
- Formatting checkpoint: CI on SHA `596447d...` identified only a missing trailing newline at `cargo fmt --all --check`; newline restored in commit `29667927d944e3e20db4d7e3c1ca6e2b30eb7975`, message `style(http): restore source trailing newline`. No logic/test semantics changed in this final formatting commit.
- Exact-head CI #790: [run 44095989717](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44095989717), SUCCESS, all 6/6 jobs on exact SHA `29667927d944e3e20db4d7e3c1ca6e2b30eb7975`.
- x86_64 and native ARM64 both passed Format, Clippy, Build; Collector 133/133; additional suite 5/5; HTTP/P3 contract 22/22; Sidecar unit 12/12; Sidecar integration 27/27. Both strict parser regression tests explicitly PASS on both architectures. Dashboard backend, Dashboard frontend, Security and Docker jobs PASS.
- CI artifacts (7-day retention; CI evidence only, not deployed binaries):
  - x86_64 ID `14332968779`, name `t1-collector-linux-x86_64-29667927d944e3e20db4d7e3c1ca6e2b30eb7975`, size 4,336,116 bytes, ZIP SHA-256 `78647ecb3061bc6e42082b0e25ed8e82b37ba916f18f0822038911e44bbcb8d8`.
  - ARM64 ID `14334296421`, name `t1-collector-linux-aarch64-29667927d944e3e20db4d7e3c1ca6e2b30eb7975`, size 4,038,067 bytes, ZIP SHA-256 `3a0367851717043793bc6ce7d3b4704f33c4cdea0fc717c45546f230d2acdd0b`; ELF identity verified as 64-bit AArch64.
- Regression sequence complete: token/request-size PASS; authentication PASS; freshness PASS; wire schema PASS; strict parser PASS.
- Remaining limitations: AU-09 non-loopback peer TCP remains NOT EXERCISED under the loopback-only listener/harness. HTTP V1 full acceptance remains PARTIAL / NOT VERIFIED until the remaining contract acceptance matrix and AU-09 disposition are closed. T1 runtime NOT VERIFIED; T1→T2 integration BLOCKED.
- Boundaries unchanged: branch remains `feature/t1-connection-sidecar-integration-20261107`; PR #1 OPEN / DRAFT / NOT MERGED. No listener activation, runtime token injection, Binance smoke test, VPS deployment, T0/T2 changes, T1→T2 integration or real orders.


## 6H. FR-08 non-positive source timestamp rejection — PASS (2026-11-17)

- Test-only source commit: `b005a7c1fe2ba555f5bebebee4c453d07d6f20ac`, message `test(t1): cover non-positive V1 source timestamps`.
- Added `non_positive_exchange_event_timestamps_are_rejected_before_publication` in `src/orderbook_v1.rs`. Cases `event_time = 0` and `event_time = -1` both fail snapshot construction; publisher exposes no snapshot, `read_fresh` errors, and state remains `FetchingSnapshot`.
- Exact-head CI #796: [run 44113367744](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44113367744), SUCCESS, all 6/6 jobs on exact source SHA `b005a7c1fe2ba555f5bebebee4c453d07d6f20ac`.
- x86_64 and native ARM64 Collector format/Clippy/build/tests PASS; Dashboard backend/frontend, Security and Docker PASS.
- FR-08: PASS for the tested offline publication-boundary cases only. Runtime/listener acceptance is not implied.
- HTTP V1 full acceptance remains PARTIAL / NOT VERIFIED. FC-10 NOT VERIFIED; AU-09 NOT EXERCISED; NR-05 FAIL for PR #1 as written; NR-01..03 NOT VERIFIED; NR-04 PARTIAL.
- Boundaries unchanged: branch `feature/t1-connection-sidecar-integration-20261107`; PR #1 OPEN / DRAFT / NOT MERGED; no listener activation, token injection, VPS deployment, T0/T2 changes, T1→T2 integration or real orders.



## 6I. CT-10 closed-world schema policy; FC-10 consumer boundary (2026-11-17)

- Contract policy commit: `779eb72ff9aefa9a0c55acbdd828c95b970d2799`, message `docs(t1): define closed V1 schema compatibility policy`.
- Test-spec reconciliation commit: `49d1c0849e6f4be59a45aece7e0222f1cac59163`, message `docs(t1): record CT-10 schema compatibility policy`.
- CT-10: PASS for the documented V1 compatibility policy and producer-side exact-schema evidence. `T1_T2_ORDERBOOK_V1` is closed-world: required fields/types/invariants are mandatory; unknown top-level/nested fields and unknown versions are rejected; additive or semantic changes require a separately reviewed contract version.
- Producer exact-schema test `authenticated_response_matches_exact_v1_schema_market_and_depth` was verified on source SHA `6566969632ac5c5ecc04303d6be3fb25dc1c3a30`, CI #785, and rerun in CI #790. This does not prove consumer parser enforcement; that remains a T2-side requirement.
- FC-10: NOT VERIFIED / BLOCKED at this repository boundary. The T1 repository contains no V1 HTTP consumer client. Dashboard backend reads local collector CSV/stats and is not the V1 consumer. Do not add a mock client to T1 or claim stale-cache protection. Obtain the actual T2 consumer repository and add a transport-failure/cache-invalidation regression there before integration.
- Latest T1 source SHA remains `b005a7c1fe2ba555f5bebebee4c453d07d6f20ac`; CT-10 documentation commits do not change production source.
- Overall HTTP V1 acceptance remains PARTIAL / NOT VERIFIED. AU-09 NOT EXERCISED; NR-05 FAIL for PR #1 as written; NR-01..03 NOT VERIFIED; NR-04 PARTIAL. Runtime NOT VERIFIED; T1→T2 integration BLOCKED.
- Boundaries unchanged: keep branch `feature/t1-connection-sidecar-integration-20261107`; PR #1 OPEN / DRAFT / NOT MERGED. No listener activation, token injection, VPS deployment, T0/T2 changes, T1→T2 connection or real orders.


## 6J. Documentation CI terminal reconciliation (2026-11-17)

- Contract policy SHA `779eb72ff9aefa9a0c55acbdd828c95b970d2799`: CI run [44115673509](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44115673509), exact SHA, completed SUCCESS, all 6/6 jobs.
- Test-spec SHA `49d1c0849e6f4be59a45aece7e0222f1cac59163`: CI run [44115703070](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44115703070), exact SHA, completed SUCCESS, all 6/6 jobs.
- Handoff SHA `bea1e61c2aca8b7e98835bbfa467031306bcc665`: CI run [44115745388](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44115745388), exact SHA, completed SUCCESS, all 6/6 jobs.
- All three runs passed x86_64 Collector, native ARM64 Collector, Dashboard frontend/backend, Security and Docker jobs. No runtime or deployment evidence is implied.
- Current docs/source branch HEAD before this reconciliation: `bea1e61c2aca8b7e98835bbfa467031306bcc665`. Latest production/source SHA remains `b005a7c1fe2ba555f5bebebee4c453d07d6f20ac`.
- Matrix disposition unchanged: FR-08 PASS only for offline publication-boundary cases; CT-10 PASS for documented closed-world policy and producer exact-schema output; FC-10 NOT VERIFIED / BLOCKED pending actual T2 consumer repo; AU-09 NOT EXERCISED; NR-01..03 NOT VERIFIED; NR-04 PARTIAL; NR-05 FAIL for PR #1 as written; NR-06 PASS.
- Overall HTTP V1 acceptance remains PARTIAL / NOT VERIFIED; T1 runtime NOT VERIFIED; T1→T2 integration BLOCKED.
- Next safe work: inspect NR-01..03 definitions and existing offline tests, then propose narrowly scoped test-only TDD changes on this branch. Do not alter collector ingestion/persistence, waive NR-05, activate listener, deploy, connect T2, or place real orders.


## 6K. NR-02/NR-03 test syntax correction (2026-11-17)

- Initial test commit `6f29adba27fcb67da6ddc2a8ec8ae5025486796f` failed exact-head CI run [44118264186](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44118264186). The Rust parser reported an unexpected closing delimiter at `src/orderbook_http.rs:1074`; root cause: the tests had been appended after the existing test module's closing brace. x86_64 Collector fmt/test, native ARM64 fmt/test, and Docker Collector build failed from this same syntax error; Security and Dashboard jobs passed.
- Corrective commit: `29c50795ccd7500fe7f5352a18b3877ad00cd4c6`, moving the two tests inside the existing test module without production-code changes.
- Exact-source CI run [44120275289](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44120275289) is QUEUED on corrective SHA `29c50795ccd7500fe7f5352a18b3877ad00cd4c6`. NR-02/NR-03 remain NOT VERIFIED pending terminal SUCCESS.
- Handoff documentation commit `7a44e3ea41cc43e40371a1cc608618d481a4112b` CI run [44117997841](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44117997841) completed SUCCESS 6/6 on its exact SHA.
- NR-01 remains NOT VERIFIED. Do not proceed to add further tests until the corrective exact-source CI result is known.


## 6L. Publisher immutability regression tests — NR-02 / NR-03 PASS (2026-11-17)

- Test-only commits: `6f29adba27fcb67da6ddc2a8ec8ae5025486796f` (initial placement), `29c50795ccd7500fe7f5352a18b3877ad00cd4c6` (move tests inside the test module), and formatting-only correction `162557e4c53ad7ac6f5a11264f05cab5138e442f`.
- The first attempt CI #793 / run `44118264186` failed at Rust parsing because the tests were appended outside the module; no assertions ran. The corrective placement then reached CI #794 / run `44120275289`, which failed only `cargo fmt --check` because one tuple was not rustfmt-compliant. The final commit changed formatting only.
- Exact-head CI #795: [run 44120909299](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44120909299), conclusion SUCCESS, all 6/6 jobs on exact SHA `162557e4c53ad7ac6f5a11264f05cab5138e442f`.
- x86_64 and native ARM64 Collector format, Clippy and tests PASS. Dashboard backend pytest, Dashboard frontend tsc/vitest, Security cargo-audit/npm audit, and Docker image build all PASS.
- NR-02 test `repeated_authenticated_gets_do_not_mutate_publisher_state`: five authenticated GETs each return HTTP 200; Publisher state and generation remain unchanged, and the same snapshot Arc remains published.
- NR-03 test `http_error_paths_do_not_mutate_publisher_state`: 401, 405 and 404 paths leave Publisher state/generation and published snapshot Arc unchanged.
- Verdict: NR-02 and NR-03 PASS for the tested offline Publisher state/generation/snapshot-identity invariants. This does not prove filesystem-wide Collector CSV/gap-tree immutability (NR-01), runtime listener behavior, T1 deployment, or T1-to-T2 integration.
- PR #1 remains OPEN / DRAFT / NOT MERGED. No listener activation, token injection, VPS deployment, T0/T2 changes, T1-to-T2 integration or real orders.



## 6M. NR-01 isolated filesystem non-regression — test design (2026-11-17)

- Handoff documentation commit `d24098b73a7cc2b3ee5107edf196963e20bd9d5c` was verified by exact-SHA CI run [44124447353](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44124447353): SUCCESS, all 6/6 jobs.
- NR-01 remains NOT VERIFIED. No source or workflow implementation has been changed for NR-01 in this checkpoint.
- Proposed acceptance method: add a dedicated CI-only isolated test step. Build the Publisher/API test targets, then execute only the Publisher HTTP unit-test group and `orderbook_v1_p3_contract` integration-test binary from a fresh temporary working directory. Run the compiled test executables directly so their process working directory is the sandbox rather than the repository root.
- After each targeted test executable, assert that neither `data/futures` nor `data/gaps` exists in the sandbox. Preserve the temporary directory path and assertion output in CI logs. The sandbox must be newly created and empty before execution; cleanup must run even on failure.
- This approach tests filesystem side effects in the default relative Collector CSV/gap output locations without invoking Collector ingestion, starting a listener, contacting Binance, injecting runtime secrets, or changing production code. It does not establish that arbitrary absolute paths or external mounted volumes are untouched.
- Before implementation, confirm the exact Cargo test-binary discovery command and test filter against the repository's current Cargo targets; fail closed if zero or multiple matching binaries are found. Do not weaken the assertion or silently skip a missing test binary.
- Use TDD/evidence discipline: first demonstrate that the sandbox assertion is effective with a controlled temporary fixture (create a sentinel under each prohibited path and confirm the check fails), then run the real Publisher/API test binaries in a clean sandbox. This fixture must be isolated from the repository and removed by a trap.
- Acceptance is limited to the test process working directory and the two specified relative output trees. NR-01 must not be marked PASS from source inspection or `git status` alone.
- Boundaries unchanged: remain on `feature/t1-connection-sidecar-integration-20261107`; PR #1 stays OPEN / DRAFT / NOT MERGED. No collector ingestion/persistence changes, listener activation, token injection, VPS deployment, T0/T2 changes, T1→T2 integration, or real orders.


## 6N. NR-01 isolated filesystem harness implementation (2026-11-17)

- NR-01 design-gate workflow [#44128119227](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44128119227) completed SUCCESS, all 6/6 jobs, on exact docs commit `bab29be73aaed5122fac238f3a6fc78acd28603b`. x86_64 and native ARM64 Collector, Dashboard backend/frontend, Security and Docker all PASS.
- CI-only harness commit: `ef2a3387519b5850bba163ca04a2cbaae0ac5c16`, message `test(ci): add isolated NR-01 filesystem guard`.
- Only `.github/workflows/ci.yml` changed. Production source SHA remains `162557e4c53ad7ac6f5a11264f05cab5138e442f`; no Collector ingestion, persistence, Publisher, API or runtime source was modified.
- Added a dedicated Collector CI step that:
  - creates a fresh temporary sandbox and a separate sentinel fixture;
  - proves the assertion detects `data/futures` and `data/gaps` independently;
  - builds only the named Publisher binary and `orderbook_v1_p3_contract` test target;
  - fails closed unless exactly one executable for each target is discovered and at least one HTTP unit test matches `orderbook_http::tests::`;
  - executes the selected Publisher HTTP unit tests and complete P3 contract integration binary with the sandbox as process working directory;
  - checks both prohibited relative paths after each test executable and cleans both temporary directories on exit.
- Exact-head CI for `ef2a3387519b5850bba163ca04a2cbaae0ac5c16`: NOT YET VERIFIED at this handoff update; run ID and terminal result must be added after discovery.
- NR-01 verdict remains NOT VERIFIED until exact-head CI proves sentinel detection, unique target discovery, both test executions and post-run filesystem assertions.
- Scope limitation remains: evidence covers only `data/futures` and `data/gaps` relative to the isolated test working directory; it does not prove arbitrary absolute paths or external volumes are untouched.
- Boundaries unchanged: remain on `feature/t1-connection-sidecar-integration-20261107`; PR #1 OPEN / DRAFT / NOT MERGED. No listener activation, token injection, Collector ingestion/persistence change, VPS deployment, T0/T2 change, T1→T2 integration or real orders.


## 6O. NR-01 CI binary-discovery correction and first execution evidence (2026-11-18)

- Exact-head CI run [#44134425424](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44134425424), source SHA `077fe3ecd77ed4efecfe6e84d68b94ce7d391db9`, has not reached terminal run conclusion at this checkpoint; Docker image job remained in progress when checked.
- NR-01 harness reached its isolated step and both sentinel checks worked: `data/futures` and `data/gaps` were each detected. The sandbox started empty.
- NR-01 execution then failed closed because the harness assumed Cargo test executables were under `target/debug/deps`; Cargo actually reported the executables under `target/debug/build/.../out/`. The resulting publisher binary count was zero. This is a harness discovery defect, not a failed filesystem assertion or a production regression. Publisher HTTP tests (136 passed in the ordinary Collector test step) and P3 contract tests (22 passed in the ordinary test step) passed, but those runs were not executed from the isolated sandbox.
- CI-only correction commit: `835b26a49ee7e778bbf39d2be15e9aa2cc42631e`, message `test(ci): resolve NR-01 binaries from Cargo metadata`. It changes only `.github/workflows/ci.yml`; target executable paths are now selected from Cargo's `compiler-artifact` JSON records by exact target name and test profile, rather than a guessed output directory.
- Exact-head workflow run #811: [44135521878](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44135521878), event push, queued at this checkpoint on exact SHA `835b26a49ee7e778bbf39d2be15e9aa2cc42631e`. NR-01 remains NOT VERIFIED pending this run's terminal result and log assertions.
- Security job on run #810 failed at `npm audit --omit=dev --audit-level=high`: 10 high-severity findings in `picomatch` dependency paths (including `anymatch`, `chokidar`, `tinyglobby`, `fast-glob`, and `micromatch`); npm reports that its automatic fix requires a breaking `tailwindcss@4.3.5` upgrade. Do not run `npm audit fix --force` or change frontend dependencies without a separate compatibility assessment. Rust `cargo-audit` completed with one allowed unmaintained-`chrono` warning (RUSTSEC-2026-0543), no fixed upgrade reported.
- Artifact from run #810: `t1-collector-linux-aarch64-077fe3ecd77ed4efecfe6e84d68b94ce7d391db9`, artifact ID `14350713309`, SHA-256 `1419507e1ac8131caebc52011c2e4449d66ab4e29a32ca90e2db48d1389d5548`; expires 2026-11-24T17:14:54Z. Native ARM64 job completed SUCCESS, including build, tests and Sidecar offline tests.
- Other run #810 evidence: Dashboard backend/frontend PASS; Collector format, Clippy, build and ordinary tests PASS; NR-01 step FAIL as described; Security FAIL at npm audit; Docker was still in progress at last status query. Overall run conclusion is not yet terminal/verified.
- PR #1 remains OPEN / DRAFT / NOT MERGED, branch `feature/t1-connection-sidecar-integration-20261107`. No production source changes, no runtime/deployment changes, no listener activation, no T0/T2 changes, no T1→T2 integration and no real orders.
- Next: inspect exact-head run #811. Require both sentinel detections, exactly one Cargo-reported executable for each target, nonzero Publisher HTTP test discovery, both sandbox test commands PASS, and both post-run output-tree assertions before NR-01 can be PASS. Separately assess a non-breaking remediation path for the npm audit findings; do not weaken the audit threshold or force a major upgrade.
