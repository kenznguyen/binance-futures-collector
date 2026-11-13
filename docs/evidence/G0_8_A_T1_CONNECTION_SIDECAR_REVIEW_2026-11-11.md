# G0.8-A — T1 Connection Sidecar and CI Review (2026-11-11)

## Scope

Owner-approved read-only review of the existing T1 branch: Connection Sidecar contract, tests, collector integration boundary, BTCUSDT configuration implications, and exact-SHA CI evidence. No source, test, workflow, config, runtime, VPS, data, deployment, or exchange changes were made.

## Source and branch identity

- Repository: `kenznguyen/binance-futures-collector`
- Reviewed branch: `feature/t1-connection-sidecar-integration-20261107`
- Exact branch HEAD: `f3bae0cfbc26aa694a4d6c9476965121818d3567`
- Reviewed sidecar source blob: `crates/connection-sidecar/src/lib.rs` — `0a2c127000ee1c9776b3594e1292b481c42c878c`
- Reviewed sidecar tests blob: `crates/connection-sidecar/tests/sidecar_writer.rs` — `65d0376fbd15cac000306a7ce789ff8cefedda73`
- Reviewed sidecar README blob: `crates/connection-sidecar/README.md` — `43806ca013aa1b8b8784fd29805dac677729aa12`
- Reviewed CI workflow blob: `.github/workflows/ci.yml` — `8329039c807d63cf6834ca9c85fbd0cd99e474d4`
- Reviewed example config blob: `config.example.toml` — `3436a8b75dc827a9299c083e9af783c8a74196c3`
- Reviewed root Cargo manifest blob: `Cargo.toml` — `f373e8b32e090a76399905538742e8b34a669419`

## Findings

### 1. Sidecar is a standalone offline library, not integrated into the live Collector

The sidecar README explicitly says “Not integrated into the upstream collector.” The root `Cargo.toml` has no workspace declaration and no dependency on `collector-connection-sidecar`. The inspected collector order-book manager and CSV writer do not call `SidecarWriter`.

Consequences:
- The library tests prove its isolated writer/validator behavior only.
- They do not prove that the actual Collector emits lifecycle events, that event acknowledgements gate connection transitions, or that real heartbeat Ping/Pong payloads are correlated.
- Do not count sidecar support as a live T1 connection-evidence capability yet.

The README correctly specifies the integration shape for a future separately approved change: one dedicated blocking writer worker, bounded queue, and durable acknowledgement before claiming an event was recorded. It also requires a unique heartbeat ID to be echoed in the matching Pong payload.

### 2. Contract and offline tests

The inspected implementation provides:
- Exclusive create + append; no silent truncation/reuse.
- Flush and `sync_data()` before each writer method returns.
- Bounded event/file/session/subscription sizes.
- Connection lifecycle and reconnect-state validation.
- Ping/Pong pairing, timeout and abandoned-Ping distinction.
- Finalization that checks writer-accounted bytes, file identity, JSONL records and SHA-256 before publishing a non-replacing manifest.
- Read-only validation that distinguishes unsealed, malformed/truncated and structurally sealed-consistent captures.

Tests cover durable JSONL writes, lifecycle transitions, Ping/Pong pairing, timestamp ordering, finalization/manifest digest, invalid manifest inventory and filesystem failure cases. These are offline temporary-file tests, not exchange or long-running collector tests.

Important claim boundary: the caller supplies `capture_complete_through_ms`. The writer does not inspect Binance market-data payloads, stream sequence continuity, gaps, REST coverage or downstream ingestion. A valid manifest / `SealedConsistent` result proves sidecar structural consistency only, not market-data completeness or authenticity. The README explicitly documents this limitation.

### 3. T1 CI evidence

- Workflow: `CI`
- Run: [42656715617](https://github.com/kenznguyen/binance-futures-collector/actions/runs/42656715617)
- Exact source SHA: `f3bae0cfbc26aa694a4d6c9476965121818d3567`
- Event: pull request (the workflow's push-only binary-artifact steps were skipped)
- Result: 5/5 jobs SUCCESS:
  - collector format, Clippy, release build and tests; sidecar format, Clippy and offline tests — SUCCESS
  - dashboard backend pytest — SUCCESS
  - dashboard frontend type-check/build and tests — SUCCESS
  - Docker build — SUCCESS
  - Security diagnostics, cargo-audit and npm audit — SUCCESS
- Artifact: none; artifact upload steps were skipped for this event.
- Test counts are not stated here because the retrieved job summary confirms step conclusions but not individual test totals.

### 4. Branch divergence

Comparison of T1 `main` against the reviewed feature branch reports `diverged`: feature branch is 144 commits ahead and 1 commit behind, merge base `bc4f0ef633fbeee4a49defd72827f17162693805`. No merge, rebase, cherry-pick or branch rewrite was performed. Preserve the existing feature branch until a separate reconciliation decision.

### 5. BTCUSDT-only configuration and storage implications

The checked-in `config.example.toml` has `[collection].symbols = []`, which means the entire discovered USDT-M perpetual universe, not BTCUSDT-only. The README estimates roughly 40 GB/day at all ~500 pairs and 500 ms depth, varying with volatility. That estimate must not be used as the BTCUSDT-only disk forecast.

For the intended BTCUSDT-only plan, an explicit whitelist such as `symbols = ["btcusdt"]` is the relevant configuration direction, but it has not been applied, built, or runtime-verified. The README/config also state that a non-empty whitelist skips dynamic discovery/delisting detection. This trade-off must be accepted deliberately. No exact BTCUSDT daily storage estimate is claimed from the all-market figure.

## Review verdict

- Sidecar library contract and isolated CI checks: **PASS** for the exact source SHA and run above.
- Live Collector integration: **NOT IMPLEMENTED / BLOCKED**.
- Market-data completeness from sidecar evidence: **NOT VERIFIED** (explicitly outside the crate contract).
- BTCUSDT-only runtime config and actual disk rate: **NOT VERIFIED**.
- Long-running reconnect/heartbeat behavior against Binance: **NOT RUN**.
- T1 branch reconciliation with main: **NOT STARTED**.
- VPS/runtime/deployment: **NOT AUTHORIZED / NOT RUN**.

## Next step

G0.8-B: read-only contract mapping of T0 → T1 → T2, identifying which data streams and interfaces are actually implemented versus merely documented. Any live sidecar integration, config change, test dispatch, branch reconciliation, VPS provisioning or deployment requires its own bounded approval.

## Safety and non-actions

No source/test/workflow/config files were changed. No workflow was dispatched. No artifact was produced. No VPS/SSH, service, database, market-data payload, exchange connection, T0 canonical data, AI training, trading or real order was accessed or changed. Paper-only posture and `AI_EXECUTION_ENABLED=false` remain unchanged.
