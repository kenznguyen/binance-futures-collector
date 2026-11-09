# T1 kline decimal preservation — implementation and CI evidence

Date: 2026-11-09 UTC
Branch: `feature/t1-connection-sidecar-integration-20261107`
Scope: repair the confirmed WebSocket kline decimal-loss path only.

## Defect and baseline

- Baseline source SHA: `6e20fe94f3ad64cc450e73def80a167fe371e49e`.
- `src/websocket/message_handler.rs::process_kline` calls `format_decimal_str` before constructing `KlineCsv`.
- Baseline `src/models.rs::format_decimal_str` parsed source strings through `f64` and formatted to 12 decimal places.
- Baseline `src/utils/csv_writer.rs::write_kline` parsed each decimal again through `f64`, formatted to 12 places, and used `unwrap_or(0.0)` on parse errors.
- This loses source precision/lexical form and can silently turn malformed strings into zero.

## TDD reproduction

- Test-only source checkpoint: `c0a8a6fe506d6c8bf4596dbf1e0b8d0c15bf7ae2`.
- GitHub Actions run: [42644618882](https://github.com/kenznguyen/binance-futures-collector/actions/runs/42644618882).
- Collector job result: RED as intended; 43 passed, 3 failed.
- Failures:
  - exact source decimal lexeme changed from `123456789.123456789123456789` to `123456789.123456791043`;
  - kline writer did not reject malformed/non-finite input;
  - helper-level decimal-string preservation failed.
- This confirms the tests reproduce the pinned defect. The earlier test-only workflow attempts that failed at `cargo fmt` are not treated as behavioral RED evidence.

## Fix

- `src/models.rs::format_decimal_str` now returns the source string unchanged; no `f64` parse or normalization occurs before the kline writer.
- `src/utils/csv_writer.rs::write_kline` validates all eight source decimal fields before opening/creating the output file, then writes their original strings unchanged.
- The lexical validator accepts plain decimal and exponent forms, including optional sign, while rejecting empty, malformed, NaN/Infinity, whitespace, CSV delimiters and embedded newlines.
- Invalid kline input returns an error and does not create a CSV file.
- No new dependency was added. No changes were made to T0/T2, runtime/VPS, T1 activation, order execution, or the independent T1 sidecar.

## Verified source checkpoint

- Writer fix commit: `268c4e85b5eef45f4f5d5788391057e341b012ed`.
- Writer blob SHA: `aa88bfb9f8b4d71bf70dd5c9333b527cf34b5d8e`.
- Decimal helper fix commit: `ad03560b6409de3c28688fb25d57fa49aed66a9f`.
- Decimal helper blob SHA: `9b195b921eda2863f3769eaac34b3bd0bae48bd1`.

## Green CI evidence for the implementation

- GitHub Actions run: [42644995874](https://github.com/kenznguyen/binance-futures-collector/actions/runs/42644995874).
- Event: push; exact tested HEAD: `ad03560b6409de3c28688fb25d57fa49aed66a9f`.
- Overall conclusion: SUCCESS; all five jobs PASS.
- Collector job: format, Clippy, release build, `cargo test --all`, sidecar format, sidecar Clippy and sidecar offline tests all PASS.
- Test log counts: collector library 46/46; sidecar library 12/12; sidecar integration 27/27. No failures or ignored tests in those reported groups.
- Docker build, dashboard backend tests, dashboard frontend build/tests and security audit jobs also PASS.
- Artifact: `13686259637`, `t1-collector-linux-x86_64-ad03560b6409de3c28688fb25d57fa49aed66a9f`.
- Artifact digest: `sha256:eea348cc8cc370b9368abfc661655fa607aa0cb0ae0d9530ee81e081eecd1e04`.
- Artifact expires: `2026-11-16T15:30:10Z`. This is a CI build artifact, not a deployment or activation authorization.

## Latest branch-head qualification

- The branch subsequently added two extra lexical-validator edge tests at commit `bbd591895421e383af83d68afc921ebc6df2a1d4`; these are test-only additions after the green run above.
- Two workflow attempts for that exact test-only HEAD ended in `startup_failure` before jobs executed: runs `42645075541` and `42645083006`. They are not test failures and do not establish PASS.
- Therefore the implementation behavior is CI-verified at `ad03560b6409de3c28688fb25d57fa49aed66a9f`; the extra lexical edge tests and exact latest branch HEAD remain NOT VERIFIED pending a fresh workflow run.

## Scope and status

- Decimal-loss bug: FIXED at the tested source checkpoint.
- Implementation CI: PASS at `ad03560b6409de3c28688fb25d57fa49aed66a9f`.
- Latest branch HEAD with additional tests: NOT VERIFIED.
- This does not prove target VPS filesystem behavior, source completeness, Binance REST reconciliation, FINAL_ONLY/product identity implementation, or production readiness.
- No deployment, VPS operation, T1 activation, T0/T2 change, live order, exchange credential use, or destructive data operation was performed.
