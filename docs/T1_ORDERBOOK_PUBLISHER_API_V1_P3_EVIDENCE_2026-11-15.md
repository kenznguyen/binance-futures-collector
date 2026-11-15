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
