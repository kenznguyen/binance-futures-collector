# T1 Main Tests, Fixtures and Release Inventory — 2026-11-18

## Scope and provenance
- Repository: `kenznguyen/binance-futures-collector`
- Branch: `main`
- Exact inspected source HEAD: `e0142a372f3145d9eb42e23327ebcc82c3ee2ec3`
- Inspection mode: GitHub tree, file and workflow source review only. No checkout, test execution, release dispatch, tag push, runtime, exchange, data or credential access.
- Current branch HEAD was checked at the beginning of this inventory.

## Files and identities inspected
- `tests/orderbook_v1_red.rs`: `2bdb6386f33a589fee229b0c2790c051c7bbde44`
- `dashboard/backend/tests/conftest.py`: `b1476c267867ce29ca8e5caa8df5a80bcf1659c5`
- `dashboard/backend/tests/test_api.py`: `550c67303d078c6558378294f7cf55de985febe6`
- `dashboard/backend/tests/test_orderbook.py`: `6dc9fef489c0f4f0d6cd75cf80a25ef36e3daf6d`
- `dashboard/backend/tests/test_replay.py`: `a0973a4a773c29e771b34aa6086f1971a1261e1a`
- `dashboard/backend/tests/test_security.py`: `885068192ba61f2b18fc6ae08d2f9448352740b1`
- `dashboard/backend/tests/test_validators.py`: `b4b5ceeb193c565c0bbb75c45be6fc01b8838354`
- `dashboard/frontend/src/lib/format.test.ts`: `a8aca9334473d2b84aa1e3e0d6cebdb5747356b7`
- `dashboard/frontend/package.json`: `17be65fe597ae062a9136354597666db032c70b8`
- `dashboard/backend/requirements-dev.txt`: `9c86f031efc69c8b596562c260bb243918c73d58`
- `.github/workflows/ci.yml`: `c88ae84414e741451321d1d04725249db0b20b78`
- `.github/workflows/release.yml`: `dd09261d89061a2fa3602322a3c582216241dc3e`
- `Cargo.toml`: `8b9140dab2fb24ba3389bb4f033ceb34ff02b075`
- `CHANGELOG.md`: `bce31d20b3da03e2fc20b77306541e96b307a6e1`
- `CONTRIBUTING.md`: `9c6641719774bb2f32d57d6c91fb8c6b5eb2a81a`
- `README.md`: `5e03d3e00bb4b1fe6ed4ed389006e50d5683c5e3`

## Findings
1. Rust integration test inventory contains `tests/orderbook_v1_red.rs`. Its name retains the historical RED-phase label, but its current test bodies assert positive behavior and fail-closed rejection cases for the Orderbook V1 publisher and continuity logic. It imports the public module and is included by the CI command `cargo test --all`. The filename alone is not proof of obsolescence; retain it.
2. Backend test fixtures are not a stale external fixture directory. `conftest.py` creates a synthetic CSV tree under a temporary directory, copies `config.example.toml`, sets `DATA_DIR`, `LOG_DIR`, and `COLLECTOR_CONFIG` before importing the server, and exposes builders for clean/corrupt rows. This matches the tests' need for isolated sample data.
3. Backend tests cover API, order-book reconstruction/continuity, replay, security and data validators. Frontend currently has a formatting-focused Vitest file; package scripts and CI invoke `npm test`.
4. CI explicitly invokes Rust format/clippy/build/test, backend pytest, frontend build/test and Docker image build. The CONTRIBUTING instructions list the corresponding local commands and security checks. No test file was proven orphaned from the current test runner.
5. Release workflow triggers on `v*` tags or manual dispatch, builds Linux x86_64 and macOS Apple Silicon binaries, checks `--version`, packages the binary with `config.example.toml`, `exclusions.toml`, README and LICENSE, creates SHA-256 checksums, uploads artifacts and publishes a GitHub Release.
6. The fork's GitHub Releases endpoint currently returns an empty list. This does not prove the release workflow is obsolete: it remains a valid future tag-based publication path. README/CHANGELOG release links point to the upstream `waynemason1/binance-futures-collector`, consistent with this repository being a fork and with its README clone/download instructions; do not rewrite those links based on fork-local release absence.
7. `Cargo.toml` version remains `1.0.0`, consistent with the current `CHANGELOG.md` version entry. No version mismatch was established.

## Disposition
- Existing files changed/deleted: none.
- Tests/fixtures/release workflow proven obsolete: none.
- `orderbook_v1_red.rs`: retain; historical filename does not invalidate active assertions.
- Fork has no published releases at inspection time; release workflow retained.
- Tests: NOT RUN (static inspection only).
- Overall: PASS for bounded static inventory; test runtime and release publication NOT VERIFIED.

## CI and release evidence
- The previous CI run `44320162432` on source SHA `7163f3f4f0df6c221a4b9fa1d8f372cbe9ff99b8` failed only in the `cargo-audit` security job; collector, Docker, frontend and backend jobs passed. See [workflow run](https://github.com/kenznguyen/binance-futures-collector/actions/runs/44320162432).
- The prior evidence commit `e0142a372f3145d9eb42e23327ebcc82c3ee2ec3` had no associated workflow run visible when checked.
- Release list: empty at inspection time. No release was created or dispatched. No artifact was produced by this inventory.

## Next step and boundaries
- Next read-only scope can cover repository-wide source/config references for unreferenced modules, generated files and historical provider artifacts, without editing them.
- A correctness review of `scripts/verify_data.py` remains a separate candidate scope and should not be mixed into cleanup.
- Do not rename the Rust integration test, alter CI/release workflows, publish tags/releases, remediate dependencies, or change source/runtime/T0/VPS/database without separate explicit authorization.
- Canonical session handoff remains solely in Core repository `kenznguyen/btcusdt-telegram-signals2`, branch `feature/core-operational-proof-20261112`, path `docs/PROJECT_SESSION_HANDOFF.md`. This T1 evidence report is not a handoff.
