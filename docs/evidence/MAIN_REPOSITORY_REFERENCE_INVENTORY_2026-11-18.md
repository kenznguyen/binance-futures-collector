# T1 Main Repository-Wide Reference Inventory — 2026-11-18

## Scope and provenance
- Repository: `kenznguyen/binance-futures-collector`
- Branch: `main`
- Exact inspected source HEAD: `f848e753c2c2a03e8cba085b9e7548f4ade3e096`
- Git tree: `9f30a48145fc14eebe22c20e583e65af4870898a`; recursive tree returned 118 entries, `truncated=false`.
- Mode: read-only GitHub tree and source/reference inspection. No checkout, build, test, release, workflow dispatch, runtime, exchange, data, credential, T0, VPS, or database access.
- Main HEAD was rechecked immediately before publishing this report and still matched the inspected SHA.

## Inspected source identities
- `src/lib.rs`: `c16d25102a89f37e90db63b094504d615b6f566e`
- `src/main.rs`: `d3cde50a9fb761a1d6ef711a5a04cbbda359f3ed`
- `src/collectors/mod.rs`: `08e11857dfe44e836cadf774958c48cdf8c51833`
- `src/collectors/orderbook_manager.rs`: `d18d047c1f7cdddd03cea60b02de54b656ad4edf`
- `src/collectors/rest_collector.rs`: `1a24b0b70737eee74f271bf39bc21a80d06d5cf5`
- `src/collectors/symbol_discovery.rs`: `34e453cbb96d5964d1f58e204bbfbe147041ced6`
- `src/http/mod.rs`: `8072db7b25198c8b9063e837aee775a5ecaf39a0`
- `src/http/orderbook_v1.rs`: `1a17f60f3b0a960ac5cb67ddee1dafc57dd718b2`
- `src/rest/mod.rs`: `111fe9bb0ff183e273aad355fbefd77b26424c26`
- `src/rest/client.rs`: `3f2068a07fbf6a6e348fb227274f7b0d54703e8d`
- `src/rest/proxy_manager.rs`: `da79c6a3e3c3e60ff278185724ccf03c5f9e4b74`
- `src/rest/proxy_usage_tracker.rs`: `3d725da428df5f2b297bf975fdd009ad35e8f786`
- `src/utils/mod.rs`: `56ea56aa81c3d7a32bc5272cd6900c63e406f784`
- `src/utils/stats_exporter.rs`: `8d39d92d29c6cf72c1931582f98ac2400a5ff55e`
- `src/utils/gap_registry.rs`: `494db80a855efeebf8b0d9084d9179254c525b6a`
- `src/utils/csv_writer.rs`: `2c4538dcef7dfa758a854292031239c15f7c282b`
- `src/utils/time_utils.rs`: `611ce76e7c71990f2fa420fe47b70531c84a773f`
- `src/websocket/mod.rs`: `98ea0969ba6c67ae016c9ef31fd3264008730710`
- `src/websocket/connection_manager.rs`: `bc14a36307a8061ccab22f96e0353d8b0df5c32a`
- `src/websocket/message_handler.rs`: `7d7f221fbf4b31470049baea55584645352f61b7`
- `tests/orderbook_v1_red.rs`: `2bdb6386f33a589fee229b0c2790c051c7bbde44`
- `scripts/verify_data.py`: `3ecd3306f46b9a40022cd5afbb2a4035e83cb888`
- `scripts/soak_monitor.sh`: `24c0e18d6b2996fcbd248a73390b3932b8d87678`
- `dashboard/backend/server.py`: `1bfc322d9868f5eda7a2d9152575fb22b547093d`
- `dashboard/frontend/src/App.tsx`: `4a5d2b8db7a6bb2f73d2373edabf5965b71fba52`
- `dashboard/frontend/src/main.tsx`: `45a416903ece4f76f4ee75db7310f8d545cb0f09`
- `docs/T1_ORDERBOOK_HTTP_PROVIDER_V1_CONTRACT_AND_RED_PLAN.md`: `fcf45baff1707aa839f6294493e963b1e774c6e8`
- `README.md`: `5e03d3e00bb4b1fe6ed4ed389006e50d5683c5e3`

## Findings

### Rust modules and consumers
1. The module graph in `src/lib.rs` and the nested `mod.rs` files registers the collectors, REST, utility, websocket, and HTTP modules. The executable `src/main.rs` constructs the active collector graph using `OrderbookManager`, `RestCollector`, `SymbolDiscovery`, `ProxyManager`, `RestClient`, `CsvWriter`, `GapRegistry`, `StatsExporter`, `MessageHandler`, and `WebSocketManager`. These are not orphan modules.
2. `ProxyUsageTracker` and `ProxyUsageRecord` are consumed by `src/rest/client.rs`; `RestClient::get_proxy_usage_stats` exposes the records, and `StatsExporter` includes them in the collector stats JSON. Retain.
3. `StatsExporter` writes `stats.json`; the dashboard backend reads that same path, and the soak monitor reads the collector heartbeat. This is an active cross-component data contract, not an unused generated file.
4. `GapRegistry` is constructed by `main.rs` and passed to `OrderbookManager`; `CsvWriter` is passed to order-book, REST, and websocket ingestion. Retain.
5. `SymbolSetTracker` is used by periodic symbol discovery in `main.rs`; its add/remove confirmation logic is also exercised by unit tests. Retain.
6. `src/utils/time_utils.rs` is registered as a public module. `get_current_timestamp_ms` has an in-repository consumer in `src/collectors/rest_collector.rs`. No in-repository call to `round_timestamp_down` was found in the inspected source set. Because the module is part of the public library surface and may be consumed externally, this is only an unused-in-repository function candidate, not proof that the file is obsolete. Do not remove it under this inventory.
7. The frontend component files are imported by the application/page graph: `ErrorBoundary` by `App.tsx`; `AnimatedNumber`, `SymbolExplorer`, and `ThroughputChart` by the Console page. The seven dashboard pages are wired from `App.tsx`; no component/page was established as orphaned in the inspected graph.

### Paused T1 HTTP Provider
8. `src/http/orderbook_v1.rs` is registered by `src/http/mod.rs`, and `src/http` is exported by `src/lib.rs`. `tests/orderbook_v1_red.rs` imports the provider publisher and exercises snapshot publication/validation and fail-closed behavior. The continuity implementation is also covered by the test file.
9. The provider module is not started or routed by `src/main.rs`; there is no active HTTP server/route integration. This matches the existing provider contract status: `IMPLEMENTED IN PART / PAUSED`. The module is not unreferenced, and the absence of runtime wiring is intentional under the recorded pause. Preserve all Provider commits and files; do not extend, wire, revert, or isolate them in this cleanup scope.

### Scripts, docs, and generated assets
10. `scripts/soak_monitor.sh` is documented by `docs/SOAK_TEST.md` and consumes the collector stats output. Retain.
11. `scripts/verify_data.py` has no invocation found in the inspected README, setup, Docker, or CI paths. Its standalone/manual nature does not prove obsolescence. Its earlier-noted docstring/implementation scope mismatch remains a separate correctness-review candidate; do not mix it into cleanup.
12. README references `docs/dashboard.png` and `docs/replay.gif`; `docs/SOAK_TEST.md` references `docs/soak_glibc_vs_jemalloc.png` and `docs/soak_jemalloc_timeline.png`. These tracked image assets are documentation dependencies, not unreferenced generated debris.
13. The Provider contract/RED plan explicitly labels the implementation paused and the contract as a design/acceptance reference rather than active implementation instructions. No documentation correction is indicated by this inventory.

## Disposition
- Proven obsolete source modules/files: **none**.
- Proven unreferenced dashboard components/pages: **none** in the inspected import graph.
- Tracked documentation images with no references: **none** among the four tracked image assets.
- Candidate for separate review only: `round_timestamp_down` has no in-repository consumer found; its containing module is public.
- Separate correctness-review candidate: `scripts/verify_data.py`; no cleanup conclusion.
- T1 HTTP Provider: referenced/tested but deliberately paused and not runtime-integrated; preserve unchanged.
- Existing files edited/deleted: **0**.
- Tests/builds: **NOT RUN**; this is a static reference inventory.
- Overall: **PASS** for the bounded read-only reference inventory; runtime behavior and external consumers of public library APIs remain **NOT VERIFIED**.

## Next step and boundaries
- Repository-wide static reference inventory is complete for the tracked 118-entry tree and inspected source/import paths.
- If continuing cleanup, the next safe step is a proposal-only disposition list for owner review; no source/config deletion is justified by this evidence.
- Keep the separate `scripts/verify_data.py` correctness review out of cleanup unless separately authorized.
- Do not change dependencies, CI/workflows, runtime wiring, Provider status, collector behavior, T0, VPS, database, credentials, exchange connectivity, or trading.
- Canonical session handoff remains only in Core repository `kenznguyen/btcusdt-telegram-signals2`, branch `feature/core-operational-proof-20261112`, path `docs/PROJECT_SESSION_HANDOFF.md`. This T1 report is evidence, not a handoff.
