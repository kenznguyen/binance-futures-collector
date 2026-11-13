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
