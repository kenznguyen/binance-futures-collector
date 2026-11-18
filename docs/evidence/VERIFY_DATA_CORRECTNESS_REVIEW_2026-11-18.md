# T1 verify_data.py Correctness Review — 2026-11-18

## Scope and provenance
- Repository: `kenznguyen/binance-futures-collector`
- Branch: `main`
- Source HEAD reviewed: `6c4d44b6722612d105590bc4033ae34a11dc9a91`
- Reviewed script blob: `3ecd3306f46b9a40022cd5afbb2a4035e83cb888`
- Related source blobs:
  - `src/constants.rs`: `0bf50b5fe02f4f196b38481cde6ecab9e2adb941`
  - `src/models.rs`: `993419725268d1308645e3a37c15e9779a9e92cc`
  - `src/utils/csv_writer.rs`: `2c4538dcef7dfa758a854292031239c15f7c282b`
  - `docs/SCHEMA.md`: `895f87a8281a3f9b6d37d96b5f68568e4285bee6`
  - `docs/ORDERBOOK.md`: `d3e8f446258675cc5fb92751db220c9ae19a7b69`
- Method: static code and contract review only. No script execution, synthetic fixture, build, CI dispatch, production data, runtime, exchange, T0, VPS, or database access.
- Source files unchanged.

## Intended contract compared with implementation
The script docstring (lines 1–12) claims CSV/header validation, duplicate timestamp detection where applicable, 1-minute continuity, and order-book update-ID continuity. Its constants/comments (lines 21–29) additionally claim trades are unique by trade ID and define `TRADE_ID_COL`.

The repository schema establishes:
- Common `timestamp_ms` is CSV column 4 (zero-based index 3).
- Trade CSV columns are `exchange,market,datatype,timestamp_ms,datetime_utc,symbol,trade_id,...`; therefore `trade_id` is zero-based index 6.
- Depth updates contain `first_update_id`, `last_update_id`, and `prev_update_id`; continuity is defined by the previous event's `last_update_id` and current `prev_update_id`.
- The CSV writer emits these headers and fields as documented.

## Findings

### F-01 — Trade-ID uniqueness is not implemented
- Severity: **High**
- Evidence: `TRADE_ID_COL = 5` at line 28 is never read. The actual trade header places `trade_id` at index 6 (`src/constants.rs`, line 28); index 5 is `symbol`.
- Impact: duplicated aggregated trades can pass validation undetected, despite the script comments and schema stating trades are deduplicated by `trade_id`.
- Required remedy if implementation is authorized: validate the actual trade ID field at index 6, with explicit missing/invalid ID handling and a fixture containing repeated IDs. Consider whether uniqueness scope is per file, symbol/day, or across the full dataset; the current file-at-a-time traversal only supports per-file memory unless redesigned.

### F-02 — Order-book update continuity is not checked
- Severity: **High**
- Evidence: the script traverses the `orderbooks` directory as datatype `orderbooks`, but neither `UNIQUE_TS` nor `MINUTE_SERIES` contains that datatype. No code reads `prev_update_id` or `last_update_id`. The docstring nevertheless claims order-book update-ID continuity. `docs/SCHEMA.md` and `docs/ORDERBOOK.md` define the continuity chain.
- Impact: a broken `pu → u` chain in depth updates can be reported as clean.
- Required remedy if authorized: identify depth-update files by their actual path/header, parse the named ID columns, and flag when current `prev_update_id` does not equal the preceding row's `last_update_id`. Define behavior for out-of-order rows, duplicate IDs, file/hour boundaries, and gaps before implementing.

### F-03 — Malformed/short headers can crash instead of producing a validation result
- Severity: **Medium**
- Evidence: `check_file` obtains `ncols` from the first row (lines 46–54), checks each data row against that count (lines 57–60), then unconditionally reads `row[3]` (line 62). A file with a three-column header and a matching three-column data row raises `IndexError`. A short header with no data rows can pass without being rejected.
- Impact: malformed input can terminate the whole run with an uncaught exception; the promised non-zero validation result and issue report are not guaranteed.
- Required remedy if authorized: validate required header names and positions before reading data; catch/report file-level parse and I/O errors without masking them.

### F-04 — Invalid timestamps in minute series are silently ignored
- Severity: **Medium**
- Evidence: lines 67–72 catch `ValueError` from `int(ts)` and do nothing.
- Impact: corrupted or missing timestamps can disappear from continuity calculations, potentially yielding a false clean result.
- Required remedy if authorized: report invalid/empty timestamp values as issues and do not treat them as absent observations.

### F-05 — Minute continuity checks interval differences but not bucket alignment
- Severity: **Low**
- Evidence: lines 74–79 sort parsed timestamps and only compare adjacent differences with `60_000`.
- Impact: a sequence consistently offset from the UTC minute boundary can pass even though kline open timestamps should align to minute buckets. The reported gap count is the number of discontinuous adjacent pairs, not the number of missing buckets.
- Suggested remedy if authorized: validate epoch alignment (`timestamp_ms % 60_000 == 0`) and report missing-bucket count separately from gap-boundary count.

### F-06 — Header validation is weaker than the stated contract
- Severity: **Medium**
- Evidence: the code only checks whether a data row has the same number of columns as the first row and whether a row repeats the header. It does not compare the header against the expected schema for the datatype.
- Impact: a wrong/reordered header with matching row width can be accepted while the script reads the wrong field as timestamp.
- Required remedy if authorized: validate exact required column names (and either exact order or name-based indexing) for every supported datatype.

## Additional limitations
- The script returns immediately on the first issue found in a file (row-width mismatch, repeated header, duplicate timestamp), limiting diagnostic completeness.
- `TRADE_ID_COL` is a dead constant today; its wrong value is a latent implementation hazard rather than an active wrong-column check because it is unused.
- Gap detection uses sorted unique timestamps, so it does not report duplicate minute timestamps as a separate continuity issue for datatypes outside `UNIQUE_TS`.
- The traversal is per datatype directory and per CSV file; any intended uniqueness or continuity across file boundaries is not covered.

## Verdict
- `scripts/verify_data.py` is **NOT VERIFIED as a reliable data-integrity gate**.
- The docstring overstates implemented checks. F-01 and F-02 are direct missing validations; F-03 through F-06 can produce false negatives or unhandled failures.
- This is a correctness remediation candidate, **not** a proven obsolete file and **not** a deletion candidate.
- Tests/builds: **NOT RUN**. Findings are static-review findings only.
- Existing source/config files edited/deleted: **0**.
- Overall review: **BLOCKED for use as a comprehensive integrity gate**; **PASS for identifying the documented implementation gaps**.

## Recommended next step
Obtain explicit approval for a narrowly scoped TDD remediation of `scripts/verify_data.py`:
1. Add isolated temporary-directory fixtures for each supported CSV type and each failure case.
2. Make the tests fail first (RED) for trade-ID duplicates, order-book chain breaks, invalid timestamps, malformed headers, and aligned-minute gaps.
3. Implement only the validator behavior and update the docstring to match actual guarantees.
4. Run focused tests, then the full relevant CI checks; record exact source SHA, workflow run and artifacts.
5. Do not run against production data or modify collector/runtime behavior as part of this work.

Until that authorization and evidence exist, keep the script unchanged and do not rely on its current `no issues` output as proof of trade or order-book continuity.
