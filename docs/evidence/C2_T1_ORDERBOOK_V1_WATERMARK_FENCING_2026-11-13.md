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
