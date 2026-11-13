# C2 — T1 BTCUSDT Order Book V1 watermark fencing (2026-11-13)

## Scope and approval

Owner-approved C2 work: implement and verify the T1 Order Book V1 snapshot publication guard and isolated regression coverage on the existing branch. The bounded change prevents a delayed candidate snapshot from replacing a newer snapshot when both belong to the same connection generation.

No T0 code, database, API, VPS, runtime activation, systemd/autostart, deployment, exchange connection, trading strategy, ledger, Shared Risk Engine, AI execution, or real-order behavior was changed.

## Source identity

- Repository: `kenznguyen/binance-futures-collector`
- Branch: `feature/t1-connection-sidecar-integration-20261107`
- Exact verified branch HEAD: `ac13a943595ec8a73f75d87ccff938b16498da55`
- Changed source: `src/orderbook_v1.rs`
- Source blob SHA: `ee38ef1ed5be4bc11b5e91ac3b85521d7b2a1964`
- Regression test: `publisher_rejects_same_generation_watermark_regression`
- Guard: within the same generation, reject a candidate whose `last_applied_u` is less than or equal to the currently published watermark. A new connection generation retains its existing generation fencing behavior.

## Test and CI evidence

- Workflow: CI, push run #603
- Run: https://github.com/kenznguyen/binance-futures-collector/actions/runs/43346949935
- Run ID: `43346949935`
- Exact tested source SHA: `ac13a943595ec8a73f75d87ccff938b16498da55`
- Conclusion: **SUCCESS** (all six jobs)
- Collector x86_64: rustfmt PASS, Clippy PASS, build PASS, unit tests PASS.
- Collector ARM64: rustfmt PASS, Clippy PASS, unit tests PASS, ELF identity check PASS (`ELF 64-bit LSB pie executable, ARM aarch64`).
- Dashboard backend pytest: PASS.
- Dashboard frontend type-check/Vitest: PASS.
- Docker build: PASS.
- cargo-audit/npm audit: PASS.
- Regression test appears in both x86_64 and ARM64 logs as PASS.
- Unit-test totals in each collector build: 99 + 3 + 12 + 27 tests, all passed (141 test executions per target across the reported test binaries; zero failures).

## Artifacts

- x86_64 artifact ID: `14001009082`
- x86_64 name: `t1-collector-linux-x86_64-ac13a943595ec8a73f75d87ccff938b16498da55`
- x86_64 ZIP SHA-256: `17c34408ac10d507fac332053c16c49c310cb8b8b213cbe09405b7d851186889`
- ARM64 artifact ID: `14001523880`
- ARM64 name: `t1-collector-linux-aarch64-ac13a943595ec8a73f75d87ccff938b16498da55`
- ARM64 ZIP SHA-256: `1803016963974aa232201ba8ce2d4e6168f14fbab67501b18309b22f696e5750`
- Both artifacts were unexpired at evidence capture; expiry: 2026-11-20 UTC.
- Artifacts are CI evidence only. They are not approved for VPS installation or runtime activation.

## TDD evidence qualification

The regression test is present and passes on the exact verified SHA. A separate RED run proving this new test fails against the unfixed implementation was **not captured**: the earlier workflow attempt failed at rustfmt before reaching tests. Therefore TDD RED is **NOT VERIFIED**, while exact-SHA GREEN is **PASS**.

## Status

- Same-generation watermark regression guard: **PASS — implemented and exact-SHA CI verified**.
- x86_64 and ARM64 compile/lint/test: **PASS**.
- Existing collector/dashboard/Docker/security CI: **PASS**.
- Runtime Binance feed behavior and reconnect soak: **NOT VERIFIED / NOT RUN**.
- T0→T1→T2 end-to-end data delivery: **NOT VERIFIED**.
- Core consumer integration and One-shot Core Runner: **NOT IMPLEMENTED**.
- T1 VPS/runtime activation: **NOT AUTHORIZED / NOT RUN**.
- TDD RED proof: **NOT VERIFIED**.

## Next step and limits

Proceed with a read-only C2 contract audit against the intended T2 consumer fields and trust conditions: complete snapshot publication, continuity, freshness/watermark semantics, exact decimal-string preservation, generation fencing and re-anchor behavior. Add only missing isolated tests/guards after identifying concrete gaps. No runtime, deployment, T0, trading or live-order actions without separate approval.


## Follow-up — frozen T1→T2 wire-schema alignment (2026-11-13)

The read-only comparison against T2's frozen `T1_T2_ORDERBOOK_V1` validator found two concrete producer mismatches: the T1 version literal was `T1_BTCUSDT_ORDERBOOK_V1`, and the producer emitted `bidDepth` / `askDepth` instead of the required `depth: {bids, asks}` object.

### TDD and source evidence

- RED test-only commit: `3aa0b08b1398b7008bfdea4ca13ba3fd0b021a1f`.
- RED push run #609: `43347745935`; both x86_64 and ARM64 ran 99 unit tests and failed only the new `serialized_snapshot_matches_t2_v1_wire_field_names_and_string_levels` assertion (98 passed, 1 failed). Format and Clippy passed. This is the expected schema mismatch reproduction.
- GREEN implementation commit: `96cd94f3eb4d2bb5e449537672c145ee055b8076`.
- Changed source blob `src/orderbook_v1.rs`: `a1711d2949fb6c9e807b385a4c9c9b811781141c`.
- `CONTRACT_VERSION` now equals `T1_T2_ORDERBOOK_V1`.
- Replaced separate `bidDepth` / `askDepth` fields with the serialized `depth` object containing `bids` and `asks`. Internal depth updates and assertions use the same structure.
- Serialization regression test also asserts that legacy `bidDepth` and `askDepth` keys are absent.

### Exact-SHA GREEN workflow evidence

- Push run #611: https://github.com/kenznguyen/binance-futures-collector/actions/runs/43347993751
- Run ID: `43347993751`; exact tested SHA: `96cd94f3eb4d2bb5e449537672c145ee055b8076`.
- x86_64 collector job `131631890817`: format, Clippy, build, all tests and artifact upload PASS.
- ARM64 collector job `131631890664`: format, Clippy, all tests, ELF verification and artifact upload PASS.
- Both architectures: 99 + 3 + 12 + 27 tests per target, all passed; the new T2 schema serialization test passed on both.
- Dashboard backend `131631890818`: PASS.
- Dashboard frontend `131631890876`: PASS.
- Security `131631890947`: PASS.
- Docker job `131631890815`: IN PROGRESS at the time this evidence section was written; overall run not yet promoted to SUCCESS.
- x86_64 artifact ID `14001414817`, name `t1-collector-linux-x86_64-96cd94f3eb4d2bb5e449537672c145ee055b8076`, ZIP SHA-256 `c38fde66b29d75144db162fcf50f7c937973bcba2fda79f0789f8ecf749a3125`.
- ARM64 artifact ID `14001894565`, name `t1-collector-linux-aarch64-96cd94f3eb4d2bb5e449537672c145ee055b8076`, ZIP SHA-256 `c93878920b56c1ae81783032ffbddb72607740d7052f2b2639c0688c3c923623`.
- Both artifacts were unexpired at evidence capture; expiry 2026-11-20 UTC. CI evidence only; not authorized for deployment.

### Updated disposition

- T1 V1 publisher watermark fencing regression guard: **PASS**.
- T1→T2 contract version and depth wire-shape alignment: **PASS — exact-SHA x86_64/ARM64 tests**.
- Collector compile/lint/tests and artifacts: **PASS** on both architectures.
- Overall workflow #611: **PARTIAL / NOT FINAL** until Docker job completes.
- T1 runtime reconnect/re-anchor, transport/storage atomicity to T2, T2 live reader and end-to-end delivery: **NOT VERIFIED / NOT IMPLEMENTED**.
- TDD RED for schema alignment: **PASS** (run #609 reproduced the expected failure); GREEN: **PASS** (run #611 tests passed).


### Final workflow closure — run #611

- Run #611 `43347993751` completed at 2026-11-13T01:55:04Z with overall conclusion **SUCCESS** on exact source SHA `96cd94f3eb4d2bb5e449537672c145ee055b8076`.
- All six jobs PASS, including Docker build job `131631890815`; no outstanding CI job remains.
- Therefore the schema-alignment increment's exact-SHA CI status is promoted from PARTIAL to **PASS**. This does not promote runtime, transport, T2 reader, or end-to-end readiness.

## C2.2 — dedicated loopback HTTP snapshot endpoint (2026-11-13)

### Implementation

- Added src/orderbook_http.rs as a read-only transport over the live SnapshotPublisher.
- Route: GET /internal/v1/orderbook/{symbol}; fixed production bind 127.0.0.1:8787.
- Endpoint is opt-in: it starts only when T1_ORDERBOOK_HTTP_TOKEN is present and non-empty. The token is supplied through the process environment, not source control or URL.
- Handler checks loopback peer, requires Bearer authentication, accepts only GET for BTCUSDT, reads one freshness-checked immutable snapshot with one read_fresh(now_ms), serializes that object without numeric coercion, and returns Cache-Control: no-store.
- Request size, response size, concurrent connection count and handler time are bounded. Broken/unavailable snapshots return non-2xx; no last-good cache, write route, subscription, or mutation path was added.
- Integrated into the collector lifecycle and aborts the listener task during graceful shutdown. Default runtime behavior remains disabled when the token is unset.
- Source branch: feature/t1-connection-sidecar-integration-20261107.
- Exact tested branch HEAD/source SHA: 8326ac2075ebd644e5cca0146a77ce1ae110954c.
- Current source blobs: src/orderbook_http.rs bd9aa9b66b40dca2cc6231f7599368d3de900a00; src/lib.rs e57fff3722e7af099c996446fa9ccdc3065fe28d; src/main.rs ba60c7ca8259f3c126eb62851f9fab68f8025e28.

### Test and exact-SHA CI evidence

- The first fixture run exposed a CRLF parser mismatch: the parser compared the header terminator against an escaped literal. The implementation was corrected in commit 8326ac2075ebd644e5cca0146a77ce1ae110954c; the four endpoint contract tests then passed.
- Exact-SHA workflow run #637: 43355193041, source SHA 8326ac2075ebd644e5cca0146a77ce1ae110954c, completed SUCCESS at 2026-11-13T02:56:20Z.
- All six workflow jobs PASS:
  - x86_64 collector job 131655458945: Format, Clippy, Build, Test, sidecar-writer format/Clippy/offline tests and artifact upload PASS.
  - ARM64 collector job 131655458795: native ARM64 assertion, Format, Clippy, Build, Test, sidecar-writer offline tests, ELF verification and artifact upload PASS.
  - Dashboard frontend 131655458874: PASS.
  - Dashboard backend 131655458978: PASS.
  - Security 131655458484: PASS.
  - Docker 131655458759: PASS.
- Per architecture Rust test groups: 103 + 3 + 12 + 27 = 145 passed, 0 failed. Sidecar-writer offline test step also PASS.
- x86_64 artifact ID 14006001176, name t1-collector-linux-x86_64-8326ac2075ebd644e5cca0146a77ce1ae110954c, ZIP SHA-256 54fcea918ed96f4e036f8d483c87bb6376f71d54fc226319a3067c12be1b0112.
- ARM64 artifact ID 14005816766, name t1-collector-linux-aarch64-8326ac2075ebd644e5cca0146a77ce1ae110954c, ZIP SHA-256 ae9b52040fa46caf82c04069679c9b1d9213e737db318c9f52911ab77f40a8e3.
- Both artifacts expire 2026-11-20T02:54–02:55Z; CI evidence only, not approved for deployment or VPS installation.

### C2.2 T1 disposition

- Loopback HTTP endpoint source, x86_64/ARM64 build and exact-SHA CI: PASS.
- Endpoint unit tests: PASS on both architectures.
- Runtime activation, actual inter-process T1→T2 delivery, reconnect/re-anchor soak and VPS behavior: NOT VERIFIED / NOT RUN.
- T2 reader and cross-boundary integration: NOT IMPLEMENTED at this checkpoint.
- No T0 data/API/storage, VPS, systemd/autostart, deployment, strategy, ledger, Shared Risk Engine, AI execution or real-order changes.