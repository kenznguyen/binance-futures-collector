# T1 Main Scripts, Docker and Dashboard Runtime Inventory — 2026-11-18

## Scope and provenance
- Repository: `kenznguyen/binance-futures-collector`
- Branch: `main`
- Exact inspected source HEAD: `7163f3f4f0df6c221a4b9fa1d8f372cbe9ff99b8`
- Inspection mode: GitHub source/config read-only; no checkout, runtime start, network-to-exchange, data access, or tests.
- Branch HEAD was rechecked immediately before this report and still matched the inspected SHA.

## Files inspected (blob SHA)
- `scripts/soak_monitor.sh`: `24c0e18d6b2996fcbd248a73390b3932b8d87678`
- `scripts/supervise.sh`: `2b995ea1e65662529d44c0df7bf9e446e93661b4`
- `scripts/verify_data.py`: `3ecd3306f46b9a40022cd5afbb2a4035e83cb888`
- `Dockerfile`: `9de2a0fc0f6cb66fcdb120270990372e1aa36c93`
- `dashboard/Dockerfile`: `5eaedc3cad283e28155fb9fe9e1766d78974ba2b`
- `docker-compose.yml`: `2dbe59db7eb12f4ef23d2ec20015d8070e1064b6`
- `build.sh`: `0cd2e69236737d50d327666890820b933c391f12`
- `setup.sh`: `303b784f544d70ec3efab00fadb259ec4daa845c`
- `.dockerignore`: `b0bd55f0a52a2bf66c657c1966fc944ab9f61c43`
- `dashboard/backend/server.py`: `1bfc322d9868f5eda7a2d9152575fb22b547093d`
- `dashboard/backend/requirements.txt`: `a4e4cf82efddcf443924a332a405fd6c3f0f6a0c`
- `dashboard/frontend/vite.config.ts`: `5959a040541e374353fb99b4db6bc4b036f9b576`
- `config.example.toml`: `3436a8b75dc827a9299c083e9af783c8a74196c3`
- `.github/workflows/ci.yml`: `c88ae84414e741451321d1d04725249db0b20b78`
- `README.md`: `5e03d3e00bb4b1fe6ed4ed389006e50d5683c5e3`
- `docs/SOAK_TEST.md`: `159ebc19b0f715b2144dbe2bfbce789aa7c12fde`

## Findings
1. `scripts/supervise.sh` is active in both native setup and the collector Docker image. Its configured binary path, `CONFIG_PATH` handling, data-root derivation and restart sentinel are consistent with the documented dashboard restart model. README also documents the supervisor/Compose integration.
2. `scripts/soak_monitor.sh` is explicitly identified by `docs/SOAK_TEST.md` as the instrument behind the historical soak measurements. This is historical/reproducibility tooling, not evidence of an obsolete file.
3. Native setup passes `DATA_DIR` and `LOG_DIR` to the dashboard and starts the collector under the supervisor. Compose maps the collector and dashboard to the same data volume, shares logs, mounts the same config file for dashboard editing/collector reading, and sets `COLLECTOR_CONFIG`. The backend consumes `DATA_DIR`, `LOG_DIR`, `COLLECTOR_CONFIG` and `SERVE_STATIC`; the frontend Vite config has both dev and preview API proxy blocks.
4. Collector Docker build copies the required Cargo config, source, supervisor and exclusions file. Dashboard Docker build copies backend requirements, backend source, Cargo.toml for version reporting, and the built frontend distribution. No proven missing required path or stale runtime configuration was found in this static cross-check.
5. `scripts/verify_data.py` is not referenced by README, setup, or CI in the inspected files. Its docstring describes order-book update-ID continuity and trade-ID uniqueness, but its current implementation checks CSV shape, selected timestamp uniqueness and 1-minute timestamp gaps; `TRADE_ID_COL` is declared but unused, and no order-book update-ID continuity check is implemented. This is a concrete scope/documentation mismatch, but not proof that the standalone utility is obsolete. Retain unchanged; recommend a separate narrowly scoped correctness review before relying on it as a comprehensive validator.
6. CI workflow exercises Rust fmt/clippy/build/tests, dashboard backend tests, frontend build/tests, and Docker image builds. It does not invoke the standalone `verify_data.py` or `soak_monitor.sh`; no inference of staleness is made from that alone.

## Disposition
- Existing files changed/deleted: none.
- Scripts/config/docs proven obsolete: none.
- `verify_data.py`: retained; limited implementation vs broad docstring recorded as a follow-up review item, not changed.
- Runtime, data, exchange connectivity, credentials, VPS, T0 and database: untouched.
- Tests: NOT RUN (read-only inventory only).
- Overall: PASS for bounded static inventory; runtime behavior and script execution NOT VERIFIED.

## CI provenance
- Workflow run: [44320162432](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44320162432)
- Event: push; branch `main`; exact head/checkout SHA advertised by run: `7163f3f4f0df6c221a4b9fa1d8f372cbe9ff99b8`
- Conclusion: FAILURE. Collector fmt/clippy/test, Docker build, frontend tsc/vitest, and backend pytest jobs succeeded; security `cargo-audit` failed on Rust dependency advisories. Dependency remediation was not authorized and was not performed.
- Artifact: no artifact identity recorded for this inventory run.

## Next step and boundaries
- If continuing, inspect remaining test/fixture and release/package references only as read-only cross-checks, or open a separately approved review of `verify_data.py` correctness.
- Do not repair dependencies, alter source/config/runtime, run soak/data verification against collected data, activate the collector, connect to Binance, or change T0/VPS/database without separate explicit authorization.
- Canonical session handoff remains solely in Core repository `kenznguyen/btcusdt-telegram-signals2`, branch `feature/core-operational-proof-20261112`, path `docs/PROJECT_SESSION_HANDOFF.md`. This T1 evidence report is not a handoff.
