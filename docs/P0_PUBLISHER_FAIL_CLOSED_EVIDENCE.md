# T1 P0 Publisher Fail-Closed Evidence

Status: **P0 publisher tests PASS; exact-head CI/security/Docker gate PASS on 2026-11-16 SHA `49606c1f74fe4b1727c5c6b80ec549e9104acd3c`; runtime and integration remain NOT VERIFIED.**

## Scope

- Repository: `kenznguyen/binance-futures-collector`
- Branch: `feature/t1-connection-sidecar-integration-20261107`
- PR: #1 remains OPEN / DRAFT / NOT MERGED.
- Scope: publisher lifecycle fail-closed behavior, HTTP listener opt-in/disabled configuration, and dependency advisory reachability.
- No T0/T2 source changes, no VPS listener activation, no T1→T2 loopback, and no real orders.

## Source and workflow evidence

### Exact push run

- Source SHA: `bf0115e04659f1be0c8b10e4ae8840d29c23419d`
- Workflow run: [43717240118](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43717240118)
- Result: all non-security jobs PASS; security job FAILS at `cargo-audit`.
- Collector x86_64 job: fmt, Clippy, release build, workspace tests, sidecar formatting/Clippy/offline tests PASS.
- Collector library suite: 116 passed, 0 failed.
- HTTP contract integration suite: 12 passed, 0 failed.
- Main binary suite: 5 passed, 0 failed.
- Sidecar writer suite: 12 unit tests and 27 integration tests PASS.
- Native ARM64 build, tests, ELF identity verification and artifact upload PASS.
- Docker build, dashboard backend pytest, dashboard frontend type-check/build/Vitest PASS.

Artifacts from run 43717240118 (7-day retention):
- x86_64 artifact ID `14163405321`, SHA-256 `ec72637b4302eee854bba6f03bf8dee2f8d7422a5372624214c735093ba56218`.
- ARM64 artifact ID `14163275690`, SHA-256 `02c8e321e799bb07b37469d71db23b0923970705f650ee7648455ee9d8af1cac`.

### Latest PR-head check

- PR head at inspection: `7d77bc1ded206eddae55db5c3f40889fc6d189f4`.
- Commit is a rustfmt-only adjustment to an HTTP re-anchoring assertion.
- PR workflow run: [43717540362](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43717540362).
- Collector, ARM64, Docker and both dashboard jobs PASS.
- Security job FAILS at `cargo-audit`.
- This PR-triggered run is not treated as an exact-head push build or artifact build.

## P0 publisher lifecycle

**PASS** on source SHA `bf0115e04659f1be0c8b10e4ae8840d29c23419d`.

Verified tests include:
- `fetching_or_validating_publisher_never_serves_cached_snapshot`
- `broken_publisher_never_serves_a_cached_last_good_snapshot`
- `no_snapshot_and_stopped_publishers_fail_closed_over_http`
- `publisher_does_not_serve_last_good_snapshot_after_reanchor_attempt_begins`
- `publisher_never_returns_a_snapshot_outside_ready_state`
- `publisher_invalidates_old_snapshot_and_fences_late_generation`
- `publisher_read_fresh_rejects_a_populated_snapshot_outside_ready_state`
- Re-anchoring HTTP request returns 503 and does not disclose the previous snapshot.

The HTTP handler calls `read_fresh`; that method rejects every lifecycle state other than `READY`. The publisher also clears the previous snapshot when a new connection generation/re-anchor starts, and generation fencing prevents a late prior-generation publication.

## HTTP disabled

**PASS for source control-flow and configuration unit tests; runtime socket-level proof NOT VERIFIED.**

- Missing or empty `T1_ORDERBOOK_HTTP_TOKEN` maps to `None`.
- In the `None` branch, startup logs that the HTTP endpoint is disabled and does not call `bind_loopback()`.
- A non-empty token is required before binding the documented loopback endpoint `127.0.0.1:8787`.
- Tests: `orderbook_http_listener_is_disabled_when_token_is_missing_or_empty` and `orderbook_http_listener_requires_a_nonempty_configured_token`.

No VPS or runtime socket inspection was performed.

## Dependency advisory reachability

The security job's diagnostics ran both default-feature and `--all-features --all-targets` reverse-tree checks.

- `quinn-proto 0.11.18`: no reverse dependency path emitted in either graph. It remains in Cargo.lock through `quinn`, but no active collector dependency path to `quinn` was found. The collector's reqwest features are `json`, `rustls`, `webpki-roots`, and `http2`; HTTP/3 is not enabled.
  - RUSTSEC-2026-0524 — patched in `quinn-proto >=0.11.19`.
  - RUSTSEC-2026-0523 — patched in `quinn-proto >=0.11.19`.
- `quinn-udp 0.5.15`: no reverse dependency path emitted in either graph; it is present in Cargo.lock via the unreferenced optional `quinn` package.
  - RUSTSEC-2026-0538 — patched in `quinn-udp >=0.5.16, <0.6.0` or `>=0.6.3`.
- `rustix 1.1.4`: reachable only through `tempfile 3.27.0`, which is a root `dev-dependency`; the all-target reverse tree explicitly shows `rustix -> tempfile -> binance-futures-collector [dev-dependencies]`.
  - RUSTSEC-2026-0486 is reported by cargo-audit as an allowed unsoundness warning, not one of the three blocking vulnerabilities.
  - Advisory patch version: `rustix >=1.1.6`.

**Security gate: BLOCKED.** `cargo-audit` scans all 248 locked crate entries and fails on the three QUIC advisories even though the all-features/all-targets reverse graph emitted no active path for those two packages. This is not evidence that the locked vulnerable versions are safe: they remain in Cargo.lock and could become reachable if the dependency feature graph changes. Rustix is test-only reachable and still warrants an update. No advisory ignore/suppression was added.

## Acceptance and next step

- P0 publisher lifecycle: PASS.
- HTTP disabled configuration path: PASS; socket-level runtime proof NOT VERIFIED.
- Dependency reachability analysis: PARTIAL; QUIC packages have no active graph path; rustix is dev-only.
- Full security gate: BLOCKED by cargo-audit.
- Exact-head push build for the latest PR head `7d77bc1...`: NOT VERIFIED.
- T1→T2 loopback: BLOCKED until security gate disposition and exact-head push workflow are verified.

Next: update the locked dependency versions to patched releases (or establish a narrowly scoped, continuously enforced unreachable-dependency policy), rerun cargo-audit and all CI on the exact branch head, then refresh this evidence. Do not enable the listener on VPS or run loopback before a separate approval.


## Follow-up — dependency lockfile remediation attempt (2026-11-15)

Status: **BLOCKED — no safe lockfile change made.**

- Confirmed the current PR head before this note was `b95d8e8ab6241b714aac7befa3afa343f95d95f2`.
- Re-read the current `Cargo.toml`, `Cargo.lock`, and CI workflow from the existing branch.
- The available execution container has no `cargo` or `rustc` binary, and HTTPS access to public GitHub failed at DNS resolution. Therefore a real `cargo update`, lockfile regeneration, and local test/audit run could not be performed.
- The GitHub repository interface available for this task supports file reads/writes and workflow result inspection, but no command execution or workflow-dispatch action. Manually editing Cargo.lock checksums/transitive package graph would be unsafe and is explicitly not done.
- No source or dependency files were changed in this follow-up. No CI result is claimed for a regenerated lockfile.

Next required execution: run Cargo's resolver in a network-enabled Rust environment against this exact branch, update the lockfile for patched QUIC packages and test-only `rustix`, inspect the complete lockfile diff, then commit the generated lockfile to this same branch and rerun the full push workflow. Until that evidence exists, security remains **BLOCKED**, and T1→T2 loopback remains **BLOCKED**.


## Follow-up — latest PR workflow inspection (2026-11-15)

- Commit inspected: `d8fbc4547815bd15250b6097059ba1c04020e038` (`docs(t1): record dependency remediation blocker`).
- PR-triggered workflow: [43718687423](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43718687423), run number 710.
- Collector, native ARM64, dashboard frontend, Docker, and dashboard backend jobs: **PASS**.
- Security job: **FAIL** at `cargo-audit`; npm audit was not the failing step.
- The run still reports 3 vulnerabilities across locked packages: `quinn-proto 0.11.18` (RUSTSEC-2026-0524 and RUSTSEC-2026-0523; patched >=0.11.19) and `quinn-udp 0.5.15` (RUSTSEC-2026-0538; patched >=0.5.16, <0.6.0 or >=0.6.3).
- `rustix 1.1.4` remains an allowed unsoundness warning (RUSTSEC-2026-0486; patched >=1.1.6).
- This is a PR-triggered run, not an exact-head push run or a regenerated-lockfile verification.
- No source, dependency manifest, or lockfile changes were made. Security remains **BLOCKED**; exact-head push build remains **NOT VERIFIED**; T1→T2 loopback remains **BLOCKED**.


## Follow-up — exact-head CI/security/Docker gate PASS (2026-11-16)

### Source and workflow identity

- Repository: `kenznguyen/binance-futures-collector`.
- Existing branch: `feature/t1-connection-sidecar-integration-20261107`.
- Exact source SHA: `49606c1f74fe4b1727c5c6b80ec549e9104acd3c`.
- Push workflow [#736](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43827738059), run ID `43827738059`, completed with conclusion **SUCCESS**.
- All 6/6 jobs succeeded: Rust collector fmt/Clippy/build/tests and sidecar writer checks; dashboard frontend TypeScript/Vitest; native ARM64 build/tests; Docker collector + dashboard image build; cargo-audit + npm audit; dashboard backend pytest.
- The Docker job's Rust builder diagnostic, Cargo lockfile delta diagnostic, and both image builds completed successfully. The lockfile remediation removed the stale `ryu 1.0.23` entry; the diagnostic cleanup safely restored ownership before removing its temporary workspace.

### Artifacts published by exact-head push run

| Target | Artifact ID | Artifact name | Archive SHA-256 |
|---|---:|---|---|
| Linux x86_64 | `14206954650` | `t1-collector-linux-x86_64-49606c1f74fe4b1727c5c6b80ec549e9104acd3c` | `767378d3d6295914abb749be5c9211e531c8a6c81acf2472f3f97e22515d9b77` |
| Linux aarch64 | `14206949330` | `t1-collector-linux-aarch64-49606c1f74fe4b1727c5c6b80ec549e9104acd3c` | `580a3d2c409e3f91b5d8fa7eb56dce497244724314fbcafed4c4b279bea8afe8` |

Both artifacts were created on 2026-11-16 and expire 2026-11-23. Digests are GitHub Actions artifact archive metadata, not independently computed checksums of extracted executables. These artifacts prove CI publication only, not deployment or runtime operation.

### Updated disposition

- P0 publisher fail-closed source tests: **PASS** (prior exact test evidence remains in this document).
- Exact-head source CI, security audit, native ARM64 and Docker image build: **PASS** on `49606c1f74fe4b1727c5c6b80ec549e9104acd3c`.
- Previous Cargo audit findings are no longer present in this exact-head run; no advisory suppression was introduced as part of this increment.
- T1 HTTP endpoint socket-level runtime behavior: **NOT VERIFIED**.
- T1 live API / VPS operation: **NOT VERIFIED**.
- T0→T1→T2 live integration: **NOT VERIFIED / NOT AUTHORIZED**.
- PR #1: **OPEN / DRAFT / NOT MERGED**. Existing review finding on the Sidecar post-hard-link manifest publication failure path remains a merge blocker until explicitly resolved or the merge gate is changed by the owner.
- No runtime, Binance capture, database, T0/T2/Core First source, VPS, deployment, or live order was activated or changed. `ai_execution_enabled=false` remains required.

### Next permitted work

1. Conduct independent review of the complete accumulated PR #1 diff, including the existing unresolved Sidecar review finding and the newly added P0 publisher/HTTP changes.
2. Reconcile and document any remaining review findings on this same branch; do not create a parallel branch/PR.
3. Keep runtime and T1→T2 live integration disabled until separately approved and backed by explicit readiness evidence.
