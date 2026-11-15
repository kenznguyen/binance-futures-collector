# T1 Order Book Publisher / API V1 — P3 Implementation Evidence

Date: 2026-11-15  
Repository: `kenznguyen/binance-futures-collector`  
Branch: `feature/t1-connection-sidecar-integration-20261107`  
Source SHA: `b556a38189db2fef2fd0c1dd0b3a54d8c9d1d8d9`  
Workflow: [CI run #658](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43705302691)  
Workflow run ID: `43705302691`  
Event: `pull_request`

## 1. Scope recorded

- Align the existing HTTP test fixture with the frozen V1 canonical market and Publisher depth requirements.
- Keep changes within `src/orderbook_http.rs`; no T0, T2, collector ingestion, historical CSV, gap registry, or VPS changes in this increment.
- The preceding P3 implementation at `3ea3be9c619467db9a79281d827bc2c806848b30` added Publisher-side canonical contract, depth, and metadata validation plus dedicated contract tests.
- This increment corrected the legacy HTTP fixture: canonical market `BINANCE_USDM_PERPETUAL`, 20 bid levels, 20 ask levels, matching depth metadata, and corresponding response assertions.

## 2. CI evidence on exact SHA

Run #658 completed at 2026-11-15T10:44:29Z with overall conclusion **FAILURE**.

| Job | Result | Evidence |
|---|---|---|
| collector · fmt · clippy · test | PASS | 103 + 3 + 11 + 12 + 27 tests passed; zero failures |
| collector · native ARM64 build · test · artifact | PASS for build/tests | 103 + 3 + 11 + 12 + 27 tests passed; zero failures |
| dashboard frontend · tsc · vitest | PASS | Job conclusion success |
| dashboard backend · pytest | PASS | Job conclusion success |
| docker · build images | PASS | Job conclusion success |
| security · cargo-audit · npm audit | FAIL | cargo-audit failed; npm audit steps were skipped |

The ARM64 and x86_64 artifact upload steps are conditional on `push` or `workflow_dispatch`. This run was a `pull_request` event, so artifact upload was not performed. The workflow artifact API returned an empty artifact list. Therefore no binary artifact, checksum, or artifact digest is claimed for this run.

## 3. Security gate — BLOCKED

The cargo-audit step reported:

- `quinn-proto 0.11.18`: RUSTSEC-2026-0524 and RUSTSEC-2026-0523; patched version >= 0.11.19.
- `quinn-udp 0.5.15`: RUSTSEC-2026-0538; patched version >= 0.5.16 and < 0.6.0, or >= 0.6.3.
- `rustix 1.1.4`: RUSTSEC-2026-0486 (unsound warning); patched version >= 1.1.6.

The same advisories and versions were present in the preceding CI run #655 at source SHA `3ea3be9c619467db9a79281d827bc2c806848b30`, before the HTTP fixture-only correction. This is therefore a pre-existing repository dependency audit finding, not introduced by this P3 fixture change.

The security diagnostics reported no reverse dependency path for `quinn-proto 0.11.18` in either default-feature or all-target `cargo tree` output. The diagnostics did not report a reachability result for `quinn-udp` or `rustix`; their runtime/target reachability is NOT VERIFIED. Do not suppress advisories or modify dependency versions as part of the narrow P3 HTTP fixture task. Track and resolve the dependency audit separately with dependency-path evidence and full regression tests.

## 4. P3 status

- Publisher canonical metadata validation: IMPLEMENTED; CI Rust checks PASS.
- Publisher minimum depth and depth metadata validation: IMPLEMENTED; CI Rust checks PASS.
- HTTP fixture aligned with V1 contract: IMPLEMENTED; CI Rust checks PASS.
- CI overall: **BLOCKED** by the security audit job.
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**. The CI security gate is not green, and the complete contract matrix in the test specification has not all been demonstrated by this run.
- T1 runtime: NOT VERIFIED.
- T1 → T2 integration: BLOCKED.

## 5. Boundaries retained

No T0 or T2 changes, no collector ingestion or historical data changes, no runtime secret configuration, no listener activation, no VPS deployment, no T1 runtime operation, no T1 → T2 integration, no PR merge, and no branch rebase.

## 6. Next step

1. Separately assess and remediate the pre-existing Rust dependency audit findings, first establishing exact reverse dependency paths and whether the affected crates are included in shipped targets.
2. Re-run CI on the exact updated source SHA and retain the job evidence.
3. Complete the remaining P3 contract matrix and diff-based isolation proof before considering P3 accepted.
4. Keep runtime and T1 → T2 integration blocked until separately authorized and verified.


## 7. Follow-up CI evidence — run #659

Evidence update date: 2026-11-15 10:49:12Z  
Source SHA: `ed7d871aa48c167bd91ddde6edfb609052f7fd41` (documentation-only evidence update)  
Workflow: [CI run #659](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43705640428)  
Workflow run ID: `43705640428`  
Event: `push`  
Conclusion: **FAILURE**

| Job | Result |
|---|---|
| collector · fmt · clippy · test | PASS |
| collector · native ARM64 build · test · artifact | PASS |
| dashboard frontend · tsc · vitest | PASS |
| dashboard backend · pytest | PASS |
| docker · build images | PASS |
| security · cargo-audit · npm audit | FAIL; cargo-audit stopped the job before npm audit |

### 7.1 Artifacts actually published

Unlike run #658, run #659 was a push event and uploaded both collector artifacts. GitHub Actions artifact metadata confirms:

| Artifact | Size | SHA-256 digest |
|---|---:|---|
| `t1-collector-linux-aarch64-ed7d871aa48c167bd91ddde6edfb609052f7fd41` | 4,022,090 bytes | `d92cc65cd38b1a3fa2f35ef3e81e001ef6e3cb2392a8d2d64eb40eefd74bafe0` |
| `t1-collector-linux-x86_64-ed7d871aa48c167bd91ddde6edfb609052f7fd41` | 4,310,901 bytes | `d09f292faed5bfb5b772ca4d4ae80008b3a40501f78274a39cb78c07e20e1586` |

Both artifacts were created on 2026-11-15 and had an expiry of 2026-11-22 at the time evidence was collected. These are CI artifacts only; they are not evidence of deployment or runtime operation.

### 7.2 Security diagnostics and gate

Run #659 reproduced the same cargo-audit findings and versions recorded above. The diagnostic commands explicitly queried `quinn-proto` in default-feature and all-target reverse trees; both emitted no dependency path. The diagnostic package list did not query `quinn-udp` or `rustix`, so reachability for those two advisories remains **NOT VERIFIED**. Do not interpret package download lines as dependency reachability.

### 7.3 Updated disposition

- CI build/test/dashboard/docker jobs: **PASS** on SHA `ed7d871aa48c167bd91ddde6edfb609052f7fd41`.
- Security audit: **BLOCKED** by the same pre-existing Rust advisories.
- x86_64 and ARM64 CI artifacts: **PUBLISHED**, with exact names, sizes and digests above.
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**. The security gate remains red and the remaining contract matrix is not yet fully evidenced.
- T1 runtime and T1 → T2 integration: **NOT VERIFIED / BLOCKED**.

No dependency changes, advisory suppressions, runtime activation, VPS deployment, T0/T2 changes, PR merge, or branch rebase were made in this evidence update.


## 8. Follow-up CI evidence — run #661

Evidence update date: 2026-11-15 10:56:51Z  
Source SHA: `f9671e300d915c14505c6b0cb6b3fd51609164b5` (documentation-only evidence update relative to run #659)  
Workflow: [CI run #661](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43706292850)  
Workflow run ID: `43706292850`  
Event: `push`  
Conclusion: **FAILURE**

| Job | Result |
|---|---|
| collector · fmt · clippy · test | PASS |
| collector · native ARM64 build · test · artifact | PASS |
| dashboard frontend · tsc · vitest | PASS |
| dashboard backend · pytest | PASS |
| docker · build images | PASS |
| security · cargo-audit · npm audit | FAIL; cargo-audit found 3 vulnerabilities and 1 allowed warning; npm audit did not run |

### 8.1 Artifacts actually published

GitHub Actions artifact API confirmed both artifacts for run #661. Artifact digests are the SHA-256 values returned by GitHub's artifact metadata:

| Artifact | Size | SHA-256 digest | Expiry |
|---|---:|---|---|
| `t1-collector-linux-aarch64-f9671e300d915c14505c6b0cb6b3fd51609164b5` | 4,022,090 bytes | `0a95e9c1507d5f1ae7c7cb8eb1f62e941cb8b76aac7bc6500a3d6a08367bb68d` | 2026-11-22 10:56:08Z |
| `t1-collector-linux-x86_64-f9671e300d915c14505c6b0cb6b3fd51609164b5` | 4,310,901 bytes | `7a836d596ada38a4d0e45428f480b3a960c8fa69a957773fc96f09a7b9c1fda9` | 2026-11-22 10:56:17Z |

Artifact records: [ARM64 artifact](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43706292850) · [x86_64 artifact](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43706292850). These are CI build artifacts only; they are not deployed binaries and do not prove runtime behavior.

### 8.2 Security job evidence

The run #661 security log reports the same dependency findings and locked versions:
- `quinn-proto 0.11.18`: RUSTSEC-2026-0524 and RUSTSEC-2026-0523; patched >= 0.11.19.
- `quinn-udp 0.5.15`: RUSTSEC-2026-0538; patched >= 0.5.16 and < 0.6.0, or >= 0.6.3.
- `rustix 1.1.4`: RUSTSEC-2026-0486 (unsound warning); patched >= 1.1.6.

Cargo-audit scanned 248 crate dependencies and exited with code 1. No dependency upgrade or advisory suppression was performed. The reachability qualification in sections 3 and 7.2 remains unchanged: no reverse path was reported for quinn-proto in the prior diagnostics; quinn-udp and rustix reachability remains **NOT VERIFIED**.

### 8.3 Updated disposition

- Rust formatting, lint and test job: **PASS**.
- Native ARM64 build/tests and x86_64/ARM64 artifact publication: **PASS**.
- Frontend, backend and Docker jobs: **PASS**.
- Security audit: **BLOCKED** by the existing Rust advisories.
- CI overall: **FAILURE** on exact source SHA `f9671e300d915c14505c6b0cb6b3fd51609164b5`.
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**; security gate remains red and the remaining contract matrix is not fully evidenced.
- T1 runtime: **NOT VERIFIED**.
- T1 → T2 integration: **BLOCKED**.

No source implementation, dependency, T0/T2, ingestion/history, runtime secret, listener, VPS, deployment, PR merge, or branch rebase changes were made for this evidence update.


## 9. CI validation of evidence commit — push run

Evidence update date: 2026-11-15 11:05Z  
Source SHA tested: `09228764a9dc1f1934c89123a8af900442b529c0`  
Workflow: [CI run #43706961805](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43706961805)  
Workflow run ID: `43706961805`  
Event: `push`  
Conclusion: **FAILURE**

| Job | Result |
|---|---|
| collector · fmt · clippy · test | PASS |
| collector · native ARM64 build · test · artifact | PASS |
| dashboard frontend · tsc · vitest | PASS |
| dashboard backend · pytest | PASS |
| docker · build images | PASS |
| security · cargo-audit · npm audit | FAIL |

### 9.1 Artifacts published for the evidence commit

GitHub Actions artifact metadata confirms both artifacts on the exact tested SHA:

| Artifact | Size | SHA-256 digest | Expiry |
|---|---:|---|---|
| `t1-collector-linux-aarch64-09228764a9dc1f1934c89123a8af900442b529c0` | 4,022,090 bytes | `1275defa55417a69fc0f0a941a37f6b34b566ca78a67f022b9f60a45a7d3fc3d` | 2026-11-22 11:04:41Z |
| `t1-collector-linux-x86_64-09228764a9dc1f1934c89123a8af900442b529c0` | 4,310,901 bytes | `157169ff566bf7535e576618e13d5128c097ed3d6872ca27eb13afc3534bebc6` | 2026-11-22 11:04:35Z |

These are CI artifacts only, not deployment or runtime evidence.

### 9.2 Disposition

The evidence commit itself passed Rust formatting/lint/tests, native ARM64 build/tests, frontend, backend and Docker checks. The security audit remains the sole failing job in this workflow. The existing Rust dependency advisories remain unresolved; no dependency change or advisory suppression was made.

- CI on evidence SHA: **FAILURE** (5 jobs PASS, security audit FAIL).
- Artifact publication: **PASS** (ARM64 and x86_64; exact digests recorded).
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**.
- T1 runtime: **NOT VERIFIED**.
- T1 → T2 integration: **BLOCKED**.

No source implementation, dependency, runtime, T0/T2, ingestion/history, listener, VPS deployment, PR merge, or branch rebase changes were made.



## 10. CI validation of evidence commit — push run #43707256078

Evidence update date: 2026-11-15 11:08:38Z  
Source SHA tested: `20c52e8be458d413d8d2ec8ec3f1ca1684317f32`  
Workflow: [CI run #43707256078](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43707256078)  
Workflow run ID: `43707256078`  
Event: `push`  
Conclusion: **FAILURE**

| Job | Result |
|---|---|
| collector · fmt · clippy · test | PASS |
| collector · native ARM64 build · test · artifact | PASS |
| dashboard frontend · tsc · vitest | PASS |
| dashboard backend · pytest | PASS |
| docker · build images | PASS |
| security · cargo-audit · npm audit | FAIL; cargo-audit stopped the job before npm audit |

### 10.1 Artifacts published for the exact tested SHA

GitHub Actions artifact metadata confirms both artifacts on SHA `20c52e8be458d413d8d2ec8ec3f1ca1684317f32`:

| Artifact | Size | SHA-256 digest | Expiry |
|---|---:|---|---|
| `t1-collector-linux-aarch64-20c52e8be458d413d8d2ec8ec3f1ca1684317f32` | 4,022,090 bytes | `cf1ed43f5f54b0f378a95d3eda18266b507be95f67c65fd0b2fca4feb5147129` | 2026-11-22 11:08:14Z |
| `t1-collector-linux-x86_64-20c52e8be458d413d8d2ec8ec3f1ca1684317f32` | 4,310,901 bytes | `555d7c2ddf1d85f92b6b7d62ce2a8fa1cda8ce4f69e4a887850acef055b4ec85` | 2026-11-22 11:08:02Z |

These are CI artifacts only, not deployment or runtime evidence.

### 10.2 Exact security audit output

The security job scanned 248 crate dependencies and exited with code 1. It reported:
- `quinn-proto 0.11.18`: RUSTSEC-2026-0524 and RUSTSEC-2026-0523; patched >= 0.11.19.
- `quinn-udp 0.5.15`: RUSTSEC-2026-0538; patched >= 0.5.16 and < 0.6.0, or >= 0.6.3.
- `rustix 1.1.4`: RUSTSEC-2026-0486 (unsound warning); patched >= 1.1.6.

The log confirms cargo-audit failed and npm audit did not run. This run did not perform new reverse-dependency/reachability diagnostics; the previous qualification remains: quinn-proto had no reverse path in the earlier diagnostic output, while quinn-udp and rustix reachability remains **NOT VERIFIED**. No dependency upgrade or advisory suppression was made.

### 10.3 Disposition

- Rust formatting, lint and test job: **PASS**.
- Native ARM64 build/tests and ARM64/x86_64 artifact publication: **PASS**.
- Frontend, backend and Docker jobs: **PASS**.
- Security audit: **BLOCKED** by the same Rust dependency advisories.
- CI overall: **FAILURE** on exact source SHA `20c52e8be458d413d8d2ec8ec3f1ca1684317f32`.
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**; security gate remains red and the complete contract matrix is not fully evidenced.
- T1 runtime: **NOT VERIFIED**.
- T1 → T2 integration: **BLOCKED**.

No dependency, runtime, T0/T2, ingestion/history, listener, VPS deployment, PR merge, or branch rebase changes were made.


## 11. Follow-up CI evidence — push run #667 and pull-request run #668

Evidence update date: 2026-11-15 11:15Z  
Repository branch: `feature/t1-connection-sidecar-integration-20261107`  
Exact tested source SHA for both runs: `492ac7a8f012f74cd6f1803e14c944e8b71bfc97`  
PR #1 base SHA: `bc4f0ef633fbeee4a49defd72827f17162693805`

### 11.1 Push run #667

- Workflow: [run 43707751127](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43707751127)
- Event: `push`
- Run conclusion: **FAILURE**.
- Jobs: collector fmt/clippy/test PASS; native ARM64 build/test/artifact PASS; dashboard backend PASS; dashboard frontend PASS; Docker build PASS; security cargo-audit/npm audit FAIL.
- Security job stopped at `cargo-audit`; npm audit steps were SKIPPED.
- The Rust audit scanned 248 crate dependencies and reproduced the same 3 vulnerabilities plus 1 allowed unsound warning documented in sections 3 and 10. No dependency upgrade or advisory suppression was made.

Artifacts published for the exact tested SHA (GitHub artifact metadata):

| Artifact | Size | SHA-256 digest | Expiry |
|---|---:|---|---|
| `t1-collector-linux-x86_64-492ac7a8f012f74cd6f1803e14c944e8b71bfc97` | 4,310,901 bytes | `65d265e450c074db6081650abe192bdc62dc168ca1a57ffbcc87c8cfdee72e56` | 2026-11-22 11:14:14Z |
| `t1-collector-linux-aarch64-492ac7a8f012f74cd6f1803e14c944e8b71bfc97` | 4,022,090 bytes | `df380cc227787334f1f7abae81fef60c6d98db38886eeeca91e1432c7fa77a71` | 2026-11-22 11:14:16Z |

Artifact payload bytes were not downloaded or independently re-hashed; the digests above are GitHub artifact metadata only. These artifacts are CI outputs, not deployment or runtime evidence.

### 11.2 Pull-request run #668

- Workflow: [run 43707756454](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43707756454)
- Event: `pull_request`, PR #1, base branch `main`.
- PR head SHA: `492ac7a8f012f74cd6f1803e14c944e8b71bfc97`; base SHA: `bc4f0ef633fbeee4a49defd72827f17162693805`.
- Run conclusion: **FAILURE**.
- Jobs: collector fmt/clippy/test PASS; native ARM64 build/test PASS; dashboard backend PASS; dashboard frontend PASS; Docker build PASS; security cargo-audit/npm audit FAIL.
- Both artifact upload paths were skipped because this was a `pull_request` event; GitHub artifact API returned no artifacts.
- Security findings and skipped npm audit are identical to push run #667.

### 11.3 Combined disposition

- Build, lint, unit-test, dashboard and Docker jobs: **PASS** for their reported job scopes on exact SHA `492ac7a8f012f74cd6f1803e14c944e8b71bfc97`.
- Security audit: **BLOCKED** by the same Rust advisories; npm audit remains NOT RUN in these runs.
- Push artifact publication: **PASS at metadata level**; payload hash verification NOT VERIFIED.
- Pull-request artifact publication: **NOT APPLICABLE / SKIPPED** by workflow condition.
- CI overall: **FAILURE** for both run IDs `43707751127` and `43707756454`.
- P3 full acceptance: **PARTIAL / NOT ACCEPTED**; security gate is red and the full contract matrix remains incomplete.
- T1 runtime: **NOT VERIFIED**.
- T1 → T2 integration: **BLOCKED**.

No source implementation, dependency, test-spec, T0/T2, collector ingestion, historical data, listener, VPS, runtime, PR merge, branch rebase, or deployment change was made for these CI runs. Next: preserve the security blocker; separately obtain precise reverse-dependency and shipped-target reachability evidence for `quinn-udp` and `rustix`, then address the remaining P3 contract matrix under offline GitHub Actions only.


## 12. T2 snapshot identity compatibility fix — exact-source CI (2026-11-15)

### 12.1 Contract finding and RED evidence

- T2 repository: `kenznguyen/btcusdt-telegram-signals2`, active branch `feature/core-operational-proof-20261112`.
- T2 wire route: `GET /internal/v1/orderbook/btcusdt`; T2's `OrderBookHttpClient` validates the frozen `T1_T2_ORDERBOOK_V1` snapshot directly. No `/status` endpoint is part of the T2 consumer contract.
- T2's bundle validator recomputes `snapshotId` as `sha256:` + lowercase SHA-256 of compact UTF-8 JSON with lexicographically sorted object keys and `snapshotId` omitted.
- Prior T1 implementation used a formatted generation/watermark string (`btcusdt-...`) and therefore did not satisfy the T2 snapshot identity algorithm.
- RED test commit: `d77ea4d89a4e6d0d43613b955cc743a3e38d11ae`.
- Push workflow [#671](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43709712802), run ID `43709712802`, checked out exact SHA `d77ea4d89a4e6d0d43613b955cc743a3e38d11ae`: Collector `cargo test --all` failed exactly at `orderbook_v1::tests::exact_bridge_and_pu_chain_build_a_complete_snapshot`; 102 passed, 1 failed. Actual value was the old `btcusdt-7-103-1799999999950-1799999999990`; expected T2 canonical SHA-256 was `sha256:ab1ec89ab2a05462f908196e6316d26dd650f187529db4c975875b3ad8879c1a`. This is the intended behavioral RED.

### 12.2 Implementation and source changes

- Implementation commit: `900914a82fdb234dccd8f7fdbc22f65669f8e879`.
- Follow-up formatting/documentation commit: `36eced798558a1809e780f03aff5347dac6f02b3`.
- Current branch HEAD at this evidence update: `36eced798558a1809e780f03aff5347dac6f02b3`.
- Added deterministic `canonical_snapshot_id()` using SHA-256 and canonical compact JSON; the snapshot builder now emits this identity.
- `SnapshotPublisher::publish_if_current` rejects an identity that does not hash the exact snapshot payload.
- Updated HTTP/P3 fixtures to generate canonical identities and added a tampered-identity rejection regression.
- Added direct `ring 0.17.14` dependency, already present in `Cargo.lock` transitively; no new package version was introduced.
- Updated the V1 contract and test specification with the exact snapshot identity algorithm.
- No T0/T2 source, strategy, risk, ledger, ingestion/history, collector CSV persistence, runtime configuration, or deployment files were changed.

### 12.3 Push workflow #675 — source and artifact evidence

- Workflow: [#675](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43710121719), run ID `43710121719`, event `push`.
- Exact checked-out source SHA: `36eced798558a1809e780f03aff5347dac6f02b3`.
- Collector fmt/Clippy/build/test job ID `132799648997`: **SUCCESS**. `cargo fmt --all --check`, Clippy, release build, `cargo test --all`, Sidecar writer format/Clippy/offline tests all passed. Collector test output included 103 library tests and 27 P3 Order Book HTTP contract integration tests, all passing; all other reported test targets also passed.
- Native ARM64 job ID `132799648999`: **SUCCESS**. Host confirmed `aarch64-unknown-linux-gnu`; ARM64 ELF identity confirmed; test targets passed, including 103 library tests and 27 P3 Order Book HTTP contract integration tests.
- Dashboard frontend job `132799649078`: **SUCCESS**.
- Dashboard backend job `132799649094`: **SUCCESS**.
- Docker job `132799649068`: **SUCCESS**.
- Security job `132799648801`: **FAILURE**; details below.
- x86_64 artifact: ID `14160277053`, name `t1-collector-linux-x86_64-36eced798558a1809e780f03aff5347dac6f02b3`, 4,331,697 bytes, GitHub metadata digest `sha256:4a88cb51eaa03dfc77f64d5fbb09e30332cd899632a5290077d2fd5ffd7874ac`, expires 2026-11-22 11:43:41Z.
- ARM64 artifact: ID `14160167371`, name `t1-collector-linux-aarch64-36eced798558a1809e780f03aff5347dac6f02b3`, 4,035,762 bytes, GitHub metadata digest `sha256:f46fef5a5ad6260ea5105bd3db3f99b9bd31629bc51e2d9d4c972675ce876ec7`, expires 2026-11-22 11:43:47Z.
- Artifact digests are GitHub metadata only; payload bytes were not downloaded or independently re-hashed. Artifacts are CI outputs, not deployment/runtime evidence.

### 12.4 Pull-request workflow #676

- Workflow: [#676](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43710125366), run ID `43710125366`, event `pull_request`, PR #1.
- PR head SHA: `36eced798558a1809e780f03aff5347dac6f02b3`; base SHA: `bc4f0ef633fbeee4a49defd72827f17162693805`.
- Collector job checked out synthetic merge SHA `ecb3f928474627792225ef64385c032c873be379`, merging the PR head into base `45218c694af01602e39966d4668630323b4f8333`. Do not describe this as a direct-head checkout.
- Collector, ARM64, dashboard frontend/backend and Docker jobs: **SUCCESS**. Security job: **FAILURE**. PR workflow produced no artifacts.

### 12.5 Remaining security blocker and disposition

- The security workflow scanned 248 crate dependencies and found the same three vulnerabilities plus one allowed unsound warning:
  - `quinn-proto 0.11.18`: RUSTSEC-2026-0524 and RUSTSEC-2026-0523.
  - `quinn-udp 0.5.15`: RUSTSEC-2026-0538.
  - `rustix 1.1.4`: RUSTSEC-2026-0486 (unsound warning).
- New reverse-dependency diagnostics classified `quinn-proto` as `NO_PATH_EMITTED` in both default-feature and all-target checks. `quinn-udp` and `rustix` reachability remain **NOT VERIFIED**. The cargo-audit step failed; npm audit was skipped. No advisory was suppressed and no dependency upgrade was made.
- Overall push and PR workflows: **FAILURE** due to the security job; all source/build/test/Docker jobs listed above passed.
- T1 snapshot identity compatibility with the T2 canonical hash: **PASS at source/CI level** on exact T1 SHA `36eced798558a1809e780f03aff5347dac6f02b3`.
- T2's separate 49-test Order Book contract/HTTP-reader/loopback suite remains **PASS** on the T2 source SHA recorded in the active project handoff; no T2 source was changed here.
- T1 P3 full acceptance: **PARTIAL / NOT ACCEPTED** while the security gate remains red and the remaining P0 contract matrix is not fully evidenced.
- T1 runtime/listener activation: **NOT VERIFIED / NOT AUTHORIZED**. T1→T2 live runtime integration: **BLOCKED / NOT RUN**.
- PR #1 remains OPEN / DRAFT / NOT MERGED. No VPS, exchange, T0, persistent runtime, live order, or deployment action occurred.

### 12.6 Next permitted action

- Keep the current branch and PR; do not create a parallel branch/PR or merge.
- Preserve the T1 snapshot identity fix.
- Obtain explicit scope approval before changing dependencies to remediate the Rust advisories. First complete the missing reverse-dependency/reachability analysis for `quinn-udp` and `rustix`; do not suppress advisories.
- After the security disposition is resolved, finish the remaining P0 HTTP transport/fail-closed contract gaps with offline tests. Any runtime or T1→T2 activation requires separate explicit approval.


## 13. HTTP transport/fail-closed boundary increment — exact-source CI (2026-11-15)

### 13.1 Test-first RED and implementation

- Added focused HTTP transport tests in commit `473aa8fce23c14a60246089d51bb9dd95f6fb519`; corrected the oversized-response fixture ordering in `2c564e5afacd57175ed40b1c159e34008adb6cc5`; formatting-only correction in `7c20bb031bafb540648a5a14922bf6f760465fe3`.
- RED workflow #683, run ID `43710799427`, exact source SHA `7c20bb031bafb540648a5a14922bf6f760465fe3`: 110 passed, 1 failed. The only failure was `serve_rejects_empty_and_overlong_bearer_tokens_before_accepting`; the existing server rejected an empty token but accepted an overlong configured token and entered its accept loop. Other new boundary tests passed.
- Implementation commit: `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`.
- Token startup now rejects empty tokens and tokens longer than the 4 KiB request bound permits.
- Bearer comparison pads both values to a fixed 4 KiB width, folds length mismatch into the accumulator and traverses the fixed bound without early length return.
- Active request handlers remain capped at 8. Overflow responses now use a separate 8-permit semaphore and the same I/O deadline; further overflow sockets are dropped instead of spawning unbounded response tasks.
- Added/extended tests for content and length mismatch, empty/overlong token startup, non-loopback listener rejection, oversized request headers (413), request body/Transfer-Encoding rejection (400), incomplete-header I/O deadline, concurrent capacity rejection (503), oversized snapshot rejection without truncation (503), and JSON/security response headers.

### 13.2 Push workflow #685 — exact-source results

- Workflow: [#685](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43711013002), run ID `43711013002`, event `push`.
- Exact checked-out source SHA: `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`.
- Collector job ID `132802337835`: **SUCCESS**.
  - `cargo fmt --all --check`: PASS.
  - Clippy: PASS.
  - Release build: PASS.
  - Library tests: 111 PASS, 0 FAIL.
  - Binary tests: 3 PASS.
  - Additional integration targets: 12 PASS, 0 tests, 12 PASS, and P3 Order Book HTTP integration: 27 PASS.
  - Sidecar writer format, Clippy and offline tests: PASS.
- Native ARM64 job ID `132802337774`: **SUCCESS**; host `aarch64-unknown-linux-gnu`, AArch64 ELF verified; same 111 library tests and 27 P3 Order Book HTTP integration tests passed, plus all other targets.
- Dashboard frontend job: SUCCESS. Dashboard backend job: SUCCESS. Docker build job: SUCCESS.
- Security job ID `132802337819`: **FAILURE**. cargo-audit findings remain:
  - `quinn-proto 0.11.18`: RUSTSEC-2026-0524 and RUSTSEC-2026-0523.
  - `quinn-udp 0.5.15`: RUSTSEC-2026-0538.
  - `rustix 1.1.4`: RUSTSEC-2026-0486 (unsound warning).
- Default-feature and all-target diagnostics again emitted `NO_PATH_EMITTED` for `quinn-proto`. `quinn-udp` and `rustix` still have no reachability classification and remain NOT VERIFIED. npm audit steps were skipped after cargo-audit failed. No dependency upgrade or advisory suppression was made.
- x86_64 artifact: ID `14160770490`, name `t1-collector-linux-x86_64-23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`, 4,331,413 bytes, metadata digest `sha256:b6e3bd1f8d08d5f3d597bf44bd3d3968106b9eb436bd07b551b3793648ba9f0a`, expires 2026-11-22 11:54:26Z.
- ARM64 artifact: ID `14160044298`, name `t1-collector-linux-aarch64-23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`, 4,035,167 bytes, metadata digest `sha256:f3d2c1424d95e6968b53eeef16708b548298c7eb7fead820745748edc35f66d2`, expires 2026-11-22 11:54:34Z.
- Artifact digests are GitHub metadata only; payload bytes were not independently downloaded/re-hashed.

### 13.3 Pull-request workflow #686

- Workflow [#686](https://github.com/kenznguyen/binance-futures-collector/actions/runs/43711017167), run ID `43711017167`, PR #1, head SHA `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`.
- Collector job checked out synthetic merge SHA `faf2b9410ef7054167946afa4d377e6eb480f01b`, merging the PR head into base `45218c694af01602e39966d4668630323b4f8333`.
- Collector, native ARM64, dashboard backend/frontend and Docker jobs: **SUCCESS**. Security job: **FAILURE**.
- PR #1 remains OPEN / DRAFT / NOT MERGED.

### 13.4 Updated disposition

- T1 V1 wire compatibility and T2 canonical `snapshotId`: **PASS at source/CI contract level** on exact T1 source SHA `23c5bc975f3d495ec5c482a33c9c192f6a8c0c3f`.
- HTTP transport boundary tests added in this increment: **PASS** on x86_64 and native ARM64.
- T1 P3 full acceptance: **PARTIAL / NOT ACCEPTED**. The security job remains red, and not every scenario in the P0 matrix has a dedicated exact-SHA test (including environment-disabled listener behavior and every publisher fail-closed state).
- T1 runtime/listener activation: **NOT VERIFIED / NOT AUTHORIZED**. T1→T2 live runtime integration: **BLOCKED / NOT RUN**.
- T0, T2 source, Core First source, strategy/risk/ledger, collector history/CSV persistence and deployment configuration remain untouched. No VPS/listener activation, exchange connectivity, persistent runtime or real orders occurred.

### 13.5 Next permitted action

- Preserve T1 branch and PR #1; do not merge or create a parallel branch/PR.
- Complete the remaining P0 environment-disabled and publisher fail-closed state tests offline.
- Obtain explicit bounded approval before changing dependencies. First establish reverse-dependency/reachability for `quinn-udp` and `rustix`; do not suppress audit findings.
- A live T1→T2 loopback integration or any runtime activation still requires separate explicit authorization.
