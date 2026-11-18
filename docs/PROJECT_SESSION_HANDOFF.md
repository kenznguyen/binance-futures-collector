# Project Session Handoff — Binance Futures Collector

Last updated: 2026-11-18  
Repository: `kenznguyen/binance-futures-collector`  
Default branch: `main`

## Current repository state

- Reviewed `main` HEAD before this handoff update: `efbd57e7f869bc2298010772deeaf17352acc6f4`.
- T1 HTTP Provider work is present on `main` and is **IMPLEMENTED IN PART / PAUSED**. See `docs/T1_ORDERBOOK_HTTP_PROVIDER_V1_CONTRACT_AND_RED_PLAN.md`.
- Preserve all existing T1 commits on `main`. Do not revert or isolate them without a demonstrated technical conflict, security risk, or explicit Git-history requirement.
- Do not add HTTP endpoints, connect the provider to collector runtime, start the collector, deploy, or dispatch T1 provider workflows while this work is paused.

## Branch divergence inventory (read-only, 2026-11-18)

| Branch | Relation to `main` | Current handling |
|---|---:|---|
| `chore/expose-dependency-remediation-workflow-20261115` | 3 commits ahead / 27 behind when compared against current `main` | Workflow file content matches `main`; branch pointer/history is stale. Do not delete or rewrite without explicit approval. |
| `feature/t1-connection-sidecar-integration-20261107` | 479 commits ahead / 28 behind | PR #1 remains open; contains broad Sidecar, collector, workflow, and runtime changes. Keep frozen and do not merge/rebase. |

The chore branch's three commits are not a reason to re-merge its workflow: the exact workflow blob on that branch and `main` is `1f8137ae266b3b28f0c5491da9b041bb844ffb60`. No branch pointer has been rewritten or deleted.

## Protected boundaries

- T0 is an independent canonical data asset. Do not modify T0, its data, database, feeder, or VPS.
- Do not change Core First / T2, dashboard runtime, collector runtime, deployment, credentials, or trading behavior during repository cleanup.
- No live trading or real orders. No systemd/autostart or production deployment.
- Do not reset, force-push, delete branches, cherry-pick, merge, or rebase as a cleanup shortcut.
- Preserve evidence/artifacts and record exact source SHA, workflow run IDs, artifacts, test outcomes, and PASS/BLOCKED/NOT VERIFIED status.

## Cleanup status and next steps

- Inventory of `main` completed: 94 tracked files; 6 Markdown docs under `docs/`; 4 GitHub workflows.
- Corrected the stale T1 Provider status in the contract/RED-plan document. The source and runtime were not changed.
- This handoff file is the repository's current continuation point.
- Continue with read-only review of genuinely stale documentation/configuration. Do not delete source, workflows, fixtures, evidence, or artifacts without proving they are obsolete and obtaining the required approval.
- For any future source or runtime work, define scope and acceptance tests first; use the existing approved branch/process and record evidence before claiming PASS.
