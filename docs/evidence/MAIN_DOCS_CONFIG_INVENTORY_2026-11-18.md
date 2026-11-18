# Main Documentation and Configuration Inventory — 2026-11-18

## Scope and method

- Repository: `kenznguyen/binance-futures-collector`
- Branch: `main`
- Reviewed HEAD before this report: `a99f5d55ad8dc5dc4b89b887559f429d520d7b23`
- Full recursive Git tree: 94 tracked files; GitHub reported `truncated=false`.
- Scope: documentation, root configuration, dashboard package/configuration, and GitHub Actions workflow files.
- Method: read-only GitHub content/tree inspection and static cross-checks against the current source tree. No checkout, build, tests, workflow dispatch, SSH, VPS, collector runtime, exchange, or database access.

## Inventory

### Documentation

| Path | Blob SHA | Disposition |
|---|---|---|
| `README.md` | `5e03d3e00bb4b1fe6ed4ed389006e50d5683c5e3` | Retain. Core product description and setup paths align with the repository. Upstream GitHub links are consistent with repository metadata identifying this repository as a fork of `waynemason1/binance-futures-collector`; not proven stale. |
| `CHANGELOG.md` | `bce31d20b3da03e2fc20b77306541e96b307a6e1` | Retain. Version 1.0.0 corresponds to Cargo package version 1.0.0 and is historical release information. |
| `CONTRIBUTING.md` | `9c6641719774bb2f32d57d6c91fb8c6b5eb2a81a` | Retain. CI commands correspond to the current CI workflow. |
| `SECURITY.md` | `8fdcbd5e2fbceab619e396e84727e5f4de8c7339` | Retain. Dashboard no-auth/local-bind warning agrees with Compose and dashboard configuration. |
| `docs/ARCHITECTURE.md` | `41bd5a67e16395353df4e328b4e648259dc975bb` | Retain pending deeper review. Module map is selective, not explicitly exhaustive; omission of the paused `src/http` module alone does not prove the document is stale. |
| `docs/ORDERBOOK.md` | `d3e8f446258675cc5fb92751db220c9ae19a7b69` | Retain. Describes the current OrderbookManager reconnect/resync behavior; the separate T1 publisher module is not wired into that runtime path. |
| `docs/REPLAY.md` | `f3fdb5dd7befe71af11d216e2dab1c8e444a0f24` | Retain. Describes the dashboard replay API and persisted data model. |
| `docs/SCHEMA.md` | `895f87a8281a3f9b6d37d96b5f68568e4285bee6` | Retain. Defines the persisted CSV schema; no demonstrated conflicting schema change found in this pass. |
| `docs/SOAK_TEST.md` | `159ebc19b0f715b2144dbe2bfbce789aa7c12fde` | Retain. Clearly dated July 2026 historical measurement; age alone does not make historical evidence obsolete. |
| `docs/T1_ORDERBOOK_HTTP_PROVIDER_V1_CONTRACT_AND_RED_PLAN.md` | `fcf45baff1707aa839f6294493e963b1e774c6e8` | Retain. Explicitly records IMPLEMENTED IN PART / PAUSED and the no-route/no-runtime-integration boundary. |

### Root and runtime configuration

| Path | Blob SHA | Disposition |
|---|---|---|
| `.cargo/config.toml` | `edcd6670d5da0b89197a2df225a19e5f96898478` | Retain. jemalloc build configuration is referenced by Cargo.toml, Dockerfile, and soak documentation. |
| `.env.example` | `01f8c101880e2f49774dfe103937e5bbc025e5ab` | Retain. DATA_PATH and DASHBOARD_PORT correspond to docker-compose.yml. |
| `config.example.toml` | `3436a8b75dc827a9299c083e9af783c8a74196c3` | Retain. Reviewed key configuration fields are represented in src/config.rs and consumed by src/main.rs. |
| `docker-compose.yml` | `2dbe59db7eb12f4ef23d2ec20015d8070e1064b6` | Retain. Port 3010 host default maps to dashboard container port 8000; bind is loopback as documented. |
| `exclusions.toml` | `967e868c219b26b9f66abf39b5c81ceefe8b546e` | Retain. Source and documentation explicitly reference the file and its exclusion semantics. |
| `Cargo.toml` | `8b9140dab2fb24ba3389bb4f033ceb34ff02b075` | Retain. Package version, Linux jemalloc dependency, and dependencies align with inspected source/build configuration. |
| `dashboard/frontend/package.json` | `17be65fe597ae062a9136354597666db032c70b8` | Retain. Build/test scripts are referenced by CI and CONTRIBUTING. |
| `dashboard/backend/requirements.txt` | `a4e4cf82efddcf443924a332a405fd6c3f0f6a0c` | Retain. FastAPI runtime dependencies are imported by the dashboard server. |
| `dashboard/frontend/package-lock.json`, `tsconfig.json`, `tsconfig.node.json` | See main tree | Retain. Active frontend install/build configuration; no stale evidence found. |

### GitHub Actions

| Path | Blob SHA | Disposition |
|---|---|---|
| `.github/workflows/ci.yml` | `c88ae84414e741451321d1d04725249db0b20b78` | Retain. Push-to-main and PR CI covers Rust, dashboard backend/frontend, Docker, and security. |
| `.github/workflows/dependency-remediation.yml` | `1f8137ae266b3b28f0c5491da9b041bb844ffb60` | Retain for now. Manual-only; guarded to the existing sidecar branch and explicit confirmation. T1 Provider pause does not prove this separate dependency-remediation path is obsolete. |
| `.github/workflows/release.yml` | `dd09261d89061a2fa3602322a3c582216241dc3e` | Retain. Tag/manual release workflow remains a valid distribution path; no contrary evidence found. |
| `.github/workflows/t0-ssh-preflight.yml` | `738320c24582ea9024401b2665c994529e518f36` | Retain pending owner review. Added 2026-11-10; manual-only, pinned-host-key, read-only SSH preflight. It is T0-specific inside the collector repository, but its location/name mismatch is not proof that the workflow is unused or unsafe. Do not dispatch it in this inventory. |

## Cross-checks

- Repository metadata confirms this is a fork whose parent is `waynemason1/binance-futures-collector`; README/CHANGELOG upstream links are therefore not automatically stale.
- The example configuration fields checked in this pass are represented in `src/config.rs`; `CONFIG_PATH` is consumed by `src/main.rs`.
- `.env.example` values align with Compose substitutions; Compose maps the dashboard to container port 8000 and binds localhost by default.
- The current source tree contains `src/http/mod.rs` and `src/http/orderbook_v1.rs`. The provider document records that the module is not wired to an HTTP route or collector runtime. The selective architecture module map does not claim to enumerate every module, so no correction is made solely for its omission.
- The open PR #1 remains open against `main`, from `feature/t1-connection-sidecar-integration-20261107`. No branch pointer, PR, or history was changed.

## Verification and status

- Inventory result: **PASS — read-only inventory completed for the identified documentation/configuration/workflow set**.
- Confirmed stale files: **NONE**.
- Files modified/deleted as cleanup: **NONE**.
- Tests/builds: **NOT RUN** (static inventory only).
- Workflow dispatch: **NOT RUN**.
- Latest observed CI run triggered by the preceding handoff-file deletion: run `44319541597`, source SHA `a99f5d55ad8dc5dc4b89b887559f429d520d7b23`; it was `in_progress` at inspection. Do not infer a final result from this state.
- No source, T0 data, VPS/runtime, credentials, exchange connection, database, or trading behavior was changed or accessed.

## Next step

Continue a read-only inventory of operational scripts, Docker build/runtime wiring, and dashboard configuration consumers. Only propose a correction/removal when a concrete source/reference/history contradiction proves it is stale. Preserve the single canonical `docs/PROJECT_SESSION_HANDOFF.md` on `kenznguyen/btcusdt-telegram-signals2`, branch `feature/core-operational-proof-20261112`; do not create another handoff in this repository.
