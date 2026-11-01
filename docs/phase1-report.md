# Phase 1 report: formula engine bake-off

Measured on 2026-11-02 on the reference machine (Xeon E5-2680 v3, 32 GB) in Chrome 156 WASM, one fresh worker per workbook, with an 8 ms slice budget. Other work was running on the PC at the time, including another project's benchmark. Both engines ran the same way.

Engines: Formualizer 0.13.0 at `MohamedSadat/formualizer@7efd060` and IronCalc 0.9.1 at `MohamedSadat/IronCalc@c91da93`. Both forks carry patches this phase added, listed below.

**Neither engine reaches the 99.5% target.** Formualizer passes everything else, by a wide margin. IronCalc is closer on accuracy, but fails the slice target and most of the structural test.

## Results

| Measure | Target | Formualizer | IronCalc |
| --- | --- | --- | --- |
| Files with scored formula cells | 200+ | 696 | 690 |
| Formula cells match, all Excel-saved files, 10% cap | 99.5% | **86.05%** (pooled 83.04%) | **94.67%** (pooled 94.53%) |
| Same, without either engine's own fixtures | | 91.26% (pooled 87.89%) | 93.38% (pooled 91.48%) |
| Real documents only, without fixtures | | 98.22% (pooled 98.45%) | 98.51% (pooled 98.75%) |
| Function test suites only, without fixtures | | 80.20% (pooled 75.23%) | 82.79% (pooled 83.82%) |
| Slice p99, every slice measured | 50 ms | **13.9 ms** | **99.9 ms** |
| Slices over 50 ms | | 34 of 45,128 | 999 of 63,078, not counting the two workbooks that timed out |
| Longest slice | | 1,343 ms | 15,318 ms |
| Scale workbooks that timed out at 5 minutes | | 0 of 10 | 2 of 10 |
| Random structural edits matching Excel (2 seeds) | | **2,000 of 2,000** | **484 of 2,000** |
| Cached values read back without calculating | | Yes, every file | Yes, except 28 cells in one file |
| WASM, engine and adapter, gzipped | 5 MB | 2.21 MB (6.84 MB raw) | 0.62 MB (1.84 MB raw) |

The target pools every Excel-saved file, and that pool is dominated by function test suites written to probe edge cases. On real documents the engines are nearly level. "Real documents" and "function test suites" are split by file name in `bench/report.mjs`, so the split is a judgement, not a measurement.

## Scale, in Chrome

| 1,000,002 formulas | Formualizer | IronCalc |
| --- | --- | --- |
| Load, cached values shown | 10.1 s | 17.3 s |
| First edit after a cached load | 4.0 s, one long step to build the graph | 1.5 s, in slices |
| A later edit, p99 until settled | 1.3 ms | 1,297 ms |
| Full recalculation | 779 ms | 1,259 ms |
| Top-row insert, then settled | 1.1 s + 0.6 s | 27.6 s + 1.1 s |
| Peak memory | 936 MiB | 737 MiB |

| 300,000 formulas | Formualizer | IronCalc |
| --- | --- | --- |
| Load | 739 ms | 3,639 ms |
| First edit after a cached load | 1,240 ms | 476 ms |
| A later edit, p99 until settled | 0.4 ms | 417 ms |
| Top-row insert, then settled | 291 ms + 169 ms | 8,352 ms + 327 ms |
| Peak memory | 290 MiB | 251 MiB |

| 10,000 lookups over 20,000 rows, or 6,000 aggregates over 100,000 rows | Formualizer | IronCalc |
| --- | --- | --- |
| Lookups: first edit, full recalculation | 116 ms, 84 ms | Over 5 minutes |
| Aggregates: edit p99 until settled, its slice p99 | 3,132 ms, 15 ms | Over 5 minutes |

## What each engine's gaps are

**IronCalc's gaps are in how it works, so patches can't easily close them:**
- It has no dependency graph. Every edit recalculates every formula, and a slice can't divide a single formula over a whole column (15 s for 4 formulas in one file).
- Lookups and aggregates scan on every call, which is why those workbooks time out.
- Structural edits go wrong. `SUM(Block)` keeps a stale name, and references such as `SUM(Main!$C$1:C9)` become `#ERROR!`.
- **Inserting a row corrupts data:** text that looks like a number or date is converted (`"00000115050"` becomes `115050`, and `"10/02/2016"` becomes `42645`). A top-row insert changed values in 300 files, and the sampled cells show text changing in 239 of them.
- A defined name that isn't a plain reference or a `LAMBDA` gives `#NAME?`.

**Formualizer's gaps are mostly individual functions, which can be fixed one at a time:**
- Scalar functions don't spread over a range: `LEFT(A1:A9,1)` gives one value. One test file that passes ranges to scalar functions, `formulas/test.xlsx`, holds 11,819 of the suites' 14,893 misses.
- Without fixtures, `TEXT` format codes have 991 misses, and can go to `cgs-numfmt` in Phase 2 as a custom function. `CONVERT` has 1,424, from missing units. `PIVOTBY` and `GROUPBY` have 674. `T.TEST`, `XLOOKUP`, `LOOKUP` and `COUPNCD` miss on edge cases.
- A plain formula with a range inside a function, in a file saved before dynamic arrays, doesn't get implicit intersection yet. The adapter only handles formulas with no function in them.
- After a structural edit, some spilled arrays come back as `#SPILL!` or go missing. A top-row insert changed 2,518 cells across 100 files, and some of those are correct (`ROW()`). The structural test's seed has no arrays, so this is a known hole in that test.
- `FORECAST.ETS` and 3D references such as `SUM(Sheet1:Sheet3!A1)` aren't implemented.

**Long steps left in Formualizer:**
- The graph build on the first edit after opening can't be divided: 184 ms at 30,000 formulas, 1.2 s at 300,000, 4 s at 1,000,000. It only runs once, and it can be started right after opening.
- The first slice of a full recalculation of 1,000,000 formulas takes 309 ms, most likely building the schedule.
- A `WORKDAY.INTL` test file has one 1.3 s slice, the longest measured.

## Patches in the forks

No upstream PR is open yet; they wait for the engine decision.

**Formualizer (`cashgear/0.13.0`):**
- `evaluate_slice`: stops and carries on, keeping what it finished. A cancelled `evaluate_all_cancellable` marks everything dirty again, and chunking through `evaluate_cells` was 200 times slower. The browser-safe clock already existed as the `js-runtime` feature, and a cancel token already existed too.
- `adopt_stored_values`: opens without calculating, and a later edit recalculates only what depends on it.
- `mark_all_formulas_dirty`: a public full recalculation.
- Table references: dependencies narrowed to what a reference reads, so a totals row reading its own table is no longer `#CIRC!`. `T[[#This Row],[Col]]` and `T[[#Totals],[Col]]` now work.
- `CEILING.MATH` and `FLOOR.MATH` take their mode. Named ranges shrink when part of them is deleted. `ROWS(#REF!)` passes the error on. `VLOOKUP` looks before it checks the column.

**IronCalc (`cashgear/0.9.1`):**
- `evaluate_slice`: stops between formulas, and can evaluate chosen cells first. Its clock was already browser-safe.

Tests: IronCalc's 2,768 library tests pass. Formualizer's `formualizer-eval` and `formualizer-workbook` suites fail only on `run_formula_test_suite` and `native_facade_spill_digests`, and both also fail on a clean checkout on this PC.

## How this was measured

- **Corpus:** 2,989 unique public xlsx files at pinned commits, from `scripts/fetch-test-workbooks.mjs`, plus 10 generated scale workbooks that Excel calculated. Your private workbooks aren't in this run.
- **Stored values:** a file counts only if `fileVersion` says Excel saved it, and it isn't set to manual calculation or recalculate on load. Non-deterministic functions (`NOW`, `TODAY`, `RAND`, `RANDBETWEEN`, `RANDARRAY`, `INFO`, `CELL`), external references, data tables and outside data (`CUBE*`, `RTD`, `WEBSERVICE`, `GETPIVOTDATA`, `STOCKHISTORY`, `IMPORTDATA`) load as their stored value and aren't scored, so what reads them sees what Excel saw. `OFFSET` and `INDIRECT` are scored.
- **Loading:** both engines load through one neutral reader (`bench/harness/src/book.rs`), and neither engine's importer decides the result.
- **Structural test:** 200 episodes of 5 random inserts and deletes from a seed workbook, for 2 seeds, compared with Excel over COM after every edit. Each episode restarts from the seed, so one divergence can't taint the rest. Formula text is compared too, ignoring spacing and the spelling of `#REF!`.
- **Slices:** every `calc_slice` call during full recalculation and edits, pooled over all workbooks.

To reproduce, see `bench/README.md`.

# Scale workbooks in detail

## formualizer

10 files run, 10 finished, 0 failed.

### Slices

6400 slices: p50 10.7 ms, p99 15.3 ms, max 733 ms.

### Scale workbooks

| Workbook | Formulas | Load | First edit after a cached load | Its longest slice | First calc | Slices | Slice p99 | Slice max | Edit p50 | Edit p99 | Edit slice p99 | Edit slice max | Top-row insert | Then settle | Peak memory | Match |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| row-model-10k.xlsx | 30000 | 68.7 ms | 184 ms | 18.4 ms | 44.1 ms | 4 | 19.3 | 19.3 | 0.1 | 0.2 | 0.1 | 0.1 | 38.7 ms | 22.5 ms | 36 MiB | 100.000% |
| row-model-100k.xlsx | 300000 | 739 ms | 1240 ms | 18.6 ms | 231 ms | 16 | 80.5 | 80.5 | 0.3 | 0.4 | 0.1 | 0.1 | 291 ms | 169 ms | 290 MiB | 100.000% |
| row-model-333k.xlsx | 1000002 | 10128 ms | 4042 ms | 16.3 ms | 779 ms | 48 | 309 | 309 | 0.8 | 1.3 | 0.2 | 3.5 | 1094 ms | 583 ms | 936 MiB | 100.000% |
| chain-10k.xlsx | 9999 | 31.0 ms | 113 ms | 29.2 ms | 5.8 ms | 1 | 5.8 | 5.8 | 5.3 | 7.8 | 5.0 | 6.7 | 16.1 ms | 4.9 ms | 19 MiB | 100.000% |
| chain-100k.xlsx | 99999 | 212 ms | 466 ms | 49.9 ms | 35.4 ms | 4 | 9.6 | 9.6 | 49.0 | 60.9 | 10.8 | 11.6 | 99.0 ms | 51.7 ms | 77 MiB | 100.000% |
| lookups-5k.xlsx | 10000 | 67.4 ms | 116 ms | 19.8 ms | 84.3 ms | 8 | 19.2 | 19.2 | 0.1 | 0.2 | 0.2 | 0.3 | 22.6 ms | 51.1 ms | 35 MiB | 100.000% |
| aggregates-2k.xlsx | 6000 | 184 ms | 116 ms | 35.3 ms | 3174 ms | 303 | 11.8 | 23.0 | 831 | 3132 | 15.4 | 20.7 | 16.3 ms | 2483 ms | 77 MiB | 100.000% |
| text-50k.xlsx | 150000 | 316 ms | 0.0 ms | 0.0 ms | 1138 ms | 39 | 733 | 733 | 0.0 | 0.0 | 0.0 | 0.0 | 165 ms | 357 ms | 154 MiB | 100.000% |
| cross-sheet-8x20k.xlsx | 320000 | 599 ms | 3515 ms | 175 ms | 269 ms | 15 | 118 | 118 | 0.1 | 0.3 | 0.1 | 1.8 | 860 ms | 44.1 ms | 323 MiB | 100.000% |
| structural-seed.xlsx | 920 | 24.1 ms | 88.4 ms | 26.8 ms | 23.2 ms | 3 | 9.4 | 9.4 | 2.4 | 5.6 | 4.3 | 6.0 | 25.8 ms | 8.4 ms | 11 MiB | 100.000% |

## ironcalc

10 files run, 8 finished, 2 failed.
Failures: {"timeout":2}

### Slices

22367 slices: p50 8.0 ms, p99 20.5 ms, max 90.2 ms.

### Scale workbooks

| Workbook | Formulas | Load | First edit after a cached load | Its longest slice | First calc | Slices | Slice p99 | Slice max | Edit p50 | Edit p99 | Edit slice p99 | Edit slice max | Top-row insert | Then settle | Peak memory | Match |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| row-model-10k.xlsx | 30000 | 481 ms | 97.5 ms | 56.4 ms | 44.6 ms | 6 | 8.0 | 8.0 | 35.2 | 45.2 | 8.0 | 8.5 | 839 ms | 39.2 ms | 31 MiB | 100.000% |
| row-model-100k.xlsx | 300000 | 3639 ms | 476 ms | 76.8 ms | 399 ms | 48 | 28.5 | 28.5 | 346 | 417 | 21.7 | 32.6 | 8352 ms | 327 ms | 251 MiB | 100.000% |
| row-model-333k.xlsx | 1000002 | 17315 ms | 1516 ms | 123 ms | 1259 ms | 149 | 8.0 | 78.9 | 1146 | 1297 | 8.0 | 90.2 | 27626 ms | 1133 ms | 737 MiB | 100.000% |
| chain-10k.xlsx | 9999 | 218 ms | 80.6 ms | 67.3 ms | 5.9 ms | 1 | 5.9 | 5.9 | 5.4 | 8.0 | 7.9 | 8.0 | 176 ms | 5.6 ms | 15 MiB | 100.000% |
| chain-100k.xlsx | 99999 | 726 ms | 111 ms | 48.6 ms | 84.5 ms | 10 | 13.0 | 13.0 | 49.0 | 69.5 | 13.5 | 16.1 | 1521 ms | 49.5 ms | 70 MiB | 100.000% |
| lookups-5k.xlsx | | timeout | | | | | | | | | | | | | | |
| aggregates-2k.xlsx | | timeout | | | | | | | | | | | | | | |
| text-50k.xlsx | 150000 | 2213 ms | 0.0 ms | 0.0 ms | 351 ms | 43 | 15.5 | 15.5 | 0.0 | 0.0 | 0.0 | 0.0 | 4755 ms | 314 ms | 118 MiB | 100.000% |
| cross-sheet-8x20k.xlsx | 320000 | 2349 ms | 302 ms | 80.9 ms | 226 ms | 27 | 21.7 | 21.7 | 175 | 222 | 20.9 | 29.6 | 1535 ms | 173 ms | 177 MiB | 100.000% |
| structural-seed.xlsx | 920 | 155 ms | 187 ms | 54.8 ms | 98.9 ms | 11 | 11.8 | 11.8 | 76.2 | 90.7 | 11.4 | 12.1 | 24.3 ms | 89.4 ms | 15 MiB | 100.000% |

# Accuracy by source and by function

## formualizer

2989 files run, 2957 finished, 32 failed.
Failures: {"no xl/workbook.xml":12,"#REF!":1,"zip: invalid Zip archive: Variable-length field extends beyo":2,"zip: invalid Zip archive: Could not find EOCD":7,"missing part xl/worksheets/sheet8.xml":1,"zip: invalid Zip archive: Invalid CDFH offset in EOCD":2,"zip: invalid Zip archive: Invalid Central Directory header":1,"timeout":4,"missing part xl/worksheets/sheet2.xml":1,"Backend error in parser: ParserError at position 2: Unexpect":1}

| Set | Files | Formula cells | Match (pooled) | Match (10% cap) | Wrong value | Wrong type | Unsupported | Near zero | All cells match |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| All Excel-saved public files | 696 | 208858 | 83.041% | 86.050% | 16857 | 16022 | 2429 | 113 | 409 |
| Without either engine’s own fixtures | 447 | 132131 | 87.885% | 91.259% | 2011 | 12477 | 1507 | 13 | 333 |
| Documents (without fixtures or function test suites) | 345 | 72016 | 98.452% | 98.220% | 184 | 547 | 377 | 7 | 271 |
| Function test suites (without fixtures) | 102 | 60115 | 75.226% | 80.202% | 1827 | 11930 | 1130 | 6 | 62 |
|   calamine | 16 | 283 | 88.693% | 82.222% | 0 | 31 | 1 | 0 | 13 |
|   closedxml | 31 | 3066 | 92.335% | 92.718% | 71 | 14 | 150 | 0 | 25 |
|   epplus | 14 | 659 | 67.982% | 59.822% | 40 | 111 | 60 | 0 | 6 |
|   excelize | 2 | 3 | 66.667% | 75.000% | 1 | 0 | 0 | 0 | 1 |
|   exceljs | 7 | 222 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 7 |
|   formualizer | 1 | 5 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 1 |
|   formulas | 5 | 23571 | 49.654% | 72.165% | 543 | 10478 | 842 | 4 | 3 |
|   ironcalc | 248 | 76722 | 74.697% | 81.064% | 14846 | 3545 | 922 | 100 | 75 |
|   koala | 5 | 173 | 79.769% | 91.346% | 6 | 28 | 1 | 0 | 3 |
|   libreoffice | 46 | 8214 | 90.614% | 71.988% | 172 | 466 | 133 | 0 | 29 |
|   npoi | 8 | 6370 | 92.873% | 85.503% | 322 | 121 | 11 | 0 | 4 |
|   openxml-sdk | 12 | 28637 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 12 |
|   pandas | 4 | 340 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 4 |
|   phpspreadsheet | 77 | 3246 | 96.827% | 94.646% | 45 | 44 | 14 | 0 | 65 |
|   poi | 118 | 31937 | 95.811% | 88.569% | 730 | 389 | 212 | 7 | 86 |
|   pycel | 12 | 5471 | 93.073% | 86.832% | 72 | 300 | 5 | 2 | 1 |
|   xlcalculator | 56 | 19820 | 97.311% | 92.273% | 1 | 488 | 44 | 0 | 51 |
|   xlsxwriter | 34 | 119 | 58.824% | 58.785% | 8 | 7 | 34 | 0 | 23 |

### Per function (524 functions, misses first)

A cell counts for every function in its formula, so a miss can belong to another function in the same formula or to a precedent.

| Function | Files | Cells | Matched | Match | Unsupported |
| --- | --- | --- | --- | --- | --- |
| ROMAN | 2 | 20407 | 13632 | 66.801% | 0 |
| ARABIC | 2 | 20072 | 13337 | 66.446% | 0 |
| CONVERT | 2 | 3950 | 2293 | 58.051% | 0 |
| TEXT | 27 | 2116 | 994 | 46.975% | 0 |
| (no function) | 401 | 62052 | 61237 | 98.687% | 88 |
| IF | 61 | 11922 | 11217 | 94.087% | 415 |
| PIVOTBY | 1 | 588 | 160 | 27.211% | 3 |
| CEILING | 7 | 1570 | 1165 | 74.204% | 0 |
| REGEXTEST | 3 | 694 | 295 | 42.507% | 384 |
| CEILING.MATH | 4 | 4325 | 3979 | 92.000% | 0 |
| XLOOKUP | 5 | 731 | 388 | 53.078% | 0 |
| FLOOR.MATH | 4 | 4375 | 4068 | 92.983% | 0 |
| ACCRINT | 5 | 596 | 316 | 53.020% | 0 |
| FLOOR | 7 | 1512 | 1240 | 82.011% | 0 |
| GROUPBY | 2 | 282 | 26 | 9.220% | 6 |
| SUM | 147 | 2423 | 2177 | 89.847% | 63 |
| MMULT | 5 | 245 | 18 | 7.347% | 71 |
| FORMULATEXT | 18 | 375 | 151 | 40.267% | 0 |
| LOOKUP | 7 | 459 | 269 | 58.606% | 2 |
| ROW | 17 | 5927 | 5742 | 96.879% | 4 |
| T.TEST | 3 | 202 | 20 | 9.901% | 0 |
| MATCH | 14 | 749 | 569 | 75.968% | 2 |
| TTEST | 1 | 192 | 12 | 6.250% | 0 |
| COUPNCD | 2 | 176 | 1 | 0.568% | 72 |
| CONCAT | 13 | 630 | 460 | 73.016% | 1 |
| WEEK | 2 | 168 | 0 | 0.000% | 24 |
| LAMBDA | 12 | 185 | 18 | 9.730% | 80 |
| COUNTIF | 14 | 628 | 467 | 74.363% | 1 |
| IFERROR | 14 | 1639 | 1479 | 90.238% | 2 |
| DATE | 51 | 1430 | 1276 | 89.231% | 0 |
| CHAR | 6 | 558 | 408 | 73.118% | 0 |
| ROUND | 19 | 1034 | 889 | 85.977% | 75 |
| FREQUENCY | 3 | 218 | 74 | 33.945% | 2 |
| INDEX | 21 | 464 | 326 | 70.259% | 3 |
| UNICODE | 2 | 305 | 167 | 54.754% | 0 |
| LCM | 3 | 194 | 61 | 31.443% | 0 |
| GCD | 3 | 194 | 62 | 31.959% | 0 |
| DEC2OCT | 3 | 748 | 619 | 82.754% | 0 |
| DEC2BIN | 3 | 747 | 620 | 82.999% | 0 |
| DEC2HEX | 3 | 721 | 599 | 83.079% | 0 |
| FLOOR.PRECISE | 4 | 1488 | 1374 | 92.339% | 0 |
| ISO.CEILING | 4 | 1486 | 1372 | 92.328% | 0 |
| CEILING.PRECISE | 4 | 1486 | 1373 | 92.396% | 0 |
| OR | 16 | 1451 | 1345 | 92.695% | 2 |
| XMATCH | 3 | 450 | 347 | 77.111% | 0 |
| MINVERSE | 3 | 98 | 1 | 1.020% | 26 |
| COLUMN | 7 | 205 | 109 | 53.171% | 2 |
| VLOOKUP | 26 | 3520 | 3427 | 97.358% | 16 |
| VSTACK | 2 | 92 | 3 | 3.261% | 0 |
| SEQUENCE | 12 | 180 | 93 | 51.667% | 0 |
| WEEKDAY | 7 | 443 | 359 | 81.038% | 0 |
| SROUND | 1 | 155 | 72 | 46.452% | 83 |
| IMREAL | 2 | 449 | 369 | 82.183% | 0 |
| TRIMRANGE | 3 | 152 | 72 | 47.368% | 18 |
| NOT | 10 | 193 | 119 | 61.658% | 0 |
| ODDFPRICE | 2 | 75 | 1 | 1.333% | 59 |
| NA | 97 | 294 | 222 | 75.510% | 1 |
| AGGREGATE | 1 | 174 | 102 | 58.621% | 72 |
| FORECAST.ETS.CONFINT | 2 | 76 | 4 | 5.263% | 60 |
| VDB | 2 | 72 | 1 | 1.389% | 56 |
| SUBSTITUTE | 7 | 184 | 114 | 61.957% | 0 |
| IFS | 6 | 165 | 97 | 58.788% | 0 |
| XIRR | 3 | 95 | 27 | 28.421% | 0 |
| COUNTIFS | 7 | 403 | 336 | 83.375% | 1 |
| ISERROR | 12 | 290 | 226 | 77.931% | 9 |
| ISFORMULA | 4 | 89 | 25 | 28.090% | 0 |
| AND | 20 | 1410 | 1347 | 95.532% | 3 |
| ODDFYIELD | 2 | 64 | 1 | 1.563% | 48 |
| SUBTOTAL | 43 | 346 | 284 | 82.081% | 36 |
| SUMPRODUCT | 19 | 141 | 79 | 56.028% | 0 |
| SUMIF | 13 | 279 | 218 | 78.136% | 1 |
| HLOOKUP | 6 | 172 | 111 | 64.535% | 0 |
| YEARFRAC | 4 | 2108 | 2048 | 97.154% | 3 |
| MIRR | 2 | 288 | 228 | 79.167% | 0 |
| ANCHORARRAY | 15 | 183 | 123 | 67.213% | 1 |
| COUPDAYBS | 2 | 61 | 1 | 1.639% | 48 |
| ISNA | 9 | 1859 | 1800 | 96.826% | 0 |
| LINEST | 5 | 186 | 127 | 68.280% | 1 |
| COUPNUM | 2 | 60 | 1 | 1.667% | 45 |
| POWER | 16 | 6026 | 5968 | 99.038% | 1 |
| BESSELY | 2 | 150 | 92 | 61.333% | 0 |
| WORKDAY.INTL | 3 | 130 | 72 | 55.385% | 2 |
| LEFT | 12 | 212 | 155 | 73.113% | 0 |
| PROB | 2 | 59 | 4 | 6.780% | 43 |
| SCAN | 2 | 57 | 2 | 3.509% | 22 |
| COUPDAYSNC | 2 | 56 | 1 | 1.786% | 48 |
| COUPPCD | 2 | 56 | 1 | 1.786% | 48 |
| REGEXEXTRACT | 2 | 56 | 1 | 1.786% | 28 |
| IRR | 4 | 161 | 107 | 66.460% | 0 |
| SIGN | 5 | 153 | 99 | 64.706% | 2 |
| EXPAND | 2 | 55 | 1 | 1.818% | 17 |
| FORECAST.ETS | 2 | 54 | 1 | 1.852% | 38 |
| TEXTAFTER | 2 | 108 | 56 | 51.852% | 0 |
| ISOWEEKNUM | 4 | 84 | 32 | 38.095% | 0 |
| WEEKNUM | 3 | 2755 | 2704 | 98.149% | 0 |
| CONCATENATE | 15 | 277 | 226 | 81.588% | 0 |
| FILTER | 11 | 121 | 70 | 57.851% | 0 |
| ENCODEURL | 1 | 51 | 0 | 0.000% | 51 |
| INDIRECT | 21 | 3983 | 3933 | 98.745% | 8 |
| UPPER | 9 | 184 | 134 | 72.826% | 0 |
| REPLACE | 6 | 106 | 56 | 52.830% | 0 |
| COUPDAYS | 2 | 51 | 1 | 1.961% | 47 |
| TRANSPOSE | 7 | 139 | 90 | 64.748% | 3 |
| MUNIT | 2 | 71 | 23 | 32.394% | 28 |
| PERMUTATIONA | 2 | 49 | 1 | 2.041% | 33 |
| IMAGINARY | 2 | 449 | 402 | 89.532% | 0 |
| SORT | 7 | 288 | 242 | 84.028% | 3 |
| BESSELJ | 2 | 150 | 104 | 69.333% | 0 |
| NETWORKDAYS.INTL | 3 | 140 | 94 | 67.143% | 2 |
| TEXTBEFORE | 2 | 102 | 56 | 54.902% | 0 |
| MAX | 29 | 1009 | 966 | 95.738% | 8 |
| NPV | 7 | 129 | 86 | 66.667% | 0 |
| LOWER | 6 | 107 | 65 | 60.748% | 0 |
| WRAPROWS | 2 | 42 | 0 | 0.000% | 5 |
| BIN2OCT | 3 | 191 | 150 | 78.534% | 0 |
| MAKEARRAY | 2 | 42 | 1 | 2.381% | 16 |
| SINGLE | 10 | 41 | 0 | 0.000% | 35 |
| BIN2HEX | 3 | 161 | 121 | 75.155% | 0 |
| ODDLPRICE | 2 | 41 | 1 | 2.439% | 25 |
| ODDLYIELD | 2 | 41 | 1 | 2.439% | 25 |
| TREND | 3 | 180 | 141 | 78.333% | 1 |
| PPMT | 2 | 92 | 53 | 57.609% | 0 |
| VALUETOTEXT | 2 | 78 | 39 | 50.000% | 1 |
| WORKDAY | 5 | 91 | 53 | 58.242% | 2 |
| MONTH | 12 | 145 | 108 | 74.483% | 0 |
| SEARCH | 5 | 96 | 59 | 61.458% | 0 |
| REPT | 8 | 82 | 45 | 54.878% | 3 |
| RIGHT | 11 | 142 | 106 | 74.648% | 0 |
| CHOOSECOLS | 3 | 110 | 74 | 67.273% | 0 |
| EXACT | 7 | 88 | 52 | 59.091% | 0 |
| ADDRESS | 7 | 84 | 48 | 57.143% | 0 |
| GROWTH | 2 | 70 | 34 | 48.571% | 1 |
| ARRAYTOTEXT | 3 | 69 | 33 | 47.826% | 3 |
| NUMBERVALUE | 2 | 65 | 29 | 44.615% | 0 |
| _TRO_ALL | 1 | 57 | 21 | 36.842% | 4 |
| DISC | 2 | 37 | 1 | 2.703% | 32 |
| _FV | 1 | 36 | 0 | 0.000% | 36 |
| DAY | 12 | 261 | 226 | 86.590% | 0 |
| OCT2HEX | 3 | 163 | 128 | 78.528% | 0 |
| CUMPRINC | 2 | 62 | 27 | 43.548% | 0 |
| MID | 12 | 169 | 135 | 79.882% | 0 |
| TAN | 6 | 114 | 80 | 70.175% | 0 |
| FVSCHEDULE | 2 | 35 | 1 | 2.857% | 30 |
| COS | 9 | 846 | 813 | 96.099% | 0 |
| OCT2BIN | 3 | 164 | 131 | 79.878% | 0 |
| DAYS360 | 4 | 138 | 105 | 76.087% | 0 |
| IMCOT | 2 | 84 | 51 | 60.714% | 0 |
| SWITCH | 3 | 192 | 160 | 83.333% | 0 |
| COUNTA | 18 | 150 | 118 | 78.667% | 4 |
| DECIMAL | 2 | 131 | 99 | 75.573% | 0 |
| MINIFS | 4 | 96 | 64 | 66.667% | 1 |
| MAXIFS | 3 | 95 | 63 | 66.316% | 1 |
| NETWORKDAYS | 2 | 95 | 63 | 66.316% | 2 |
| IPMT | 2 | 92 | 60 | 65.217% | 0 |
| IMCSC | 2 | 84 | 52 | 61.905% | 0 |
| AREAS | 1 | 32 | 0 | 0.000% | 17 |
| BINOM.DIST | 3 | 108 | 77 | 71.296% | 0 |
| BETA.INV | 4 | 105 | 74 | 70.476% | 0 |
| AVERAGEIF | 5 | 101 | 70 | 69.307% | 1 |
| IMCSCH | 2 | 84 | 53 | 63.095% | 0 |
| AVERAGEIFS | 2 | 82 | 51 | 62.195% | 1 |
| XNPV | 4 | 66 | 35 | 53.030% | 0 |
| CUMIPMT | 2 | 62 | 31 | 50.000% | 0 |
| LOGEST | 2 | 48 | 17 | 35.417% | 1 |
| REGEXREPLACE | 2 | 33 | 2 | 6.061% | 17 |
| BAHTTEXT | 1 | 32 | 1 | 3.125% | 16 |
| DURATION | 1 | 32 | 1 | 3.125% | 17 |
| MDURATION | 1 | 32 | 1 | 3.125% | 17 |
| PRICEDISC | 1 | 32 | 1 | 3.125% | 16 |
| PRICEMAT | 1 | 32 | 1 | 3.125% | 16 |
| YIELDDISC | 1 | 32 | 1 | 3.125% | 16 |
| YIELDMAT | 1 | 32 | 1 | 3.125% | 16 |
| PERCENTOF | 1 | 31 | 0 | 0.000% | 16 |
| SIN | 17 | 1134 | 1104 | 97.354% | 0 |
| PV | 7 | 252 | 222 | 88.095% | 0 |
| SUMIFS | 6 | 117 | 87 | 74.359% | 1 |
| EOMONTH | 7 | 113 | 83 | 73.451% | 0 |
| IMLN | 3 | 85 | 55 | 64.706% | 0 |
| PRICE | 2 | 47 | 17 | 36.170% | 0 |
| IMSUM | 2 | 40 | 10 | 25.000% | 0 |
| EDATE | 10 | 117 | 88 | 75.214% | 0 |
| IMLOG10 | 2 | 84 | 55 | 65.476% | 0 |
| IMLOG2 | 2 | 84 | 55 | 65.476% | 0 |
| IMTAN | 2 | 84 | 55 | 65.476% | 0 |
| YIELD | 2 | 47 | 18 | 38.298% | 0 |
| AMORDEGRC | 1 | 32 | 3 | 9.375% | 16 |
| AMORLINC | 1 | 32 | 3 | 9.375% | 16 |
| BYROW | 2 | 32 | 3 | 9.375% | 15 |
| WRAPCOLS | 2 | 29 | 0 | 0.000% | 4 |
| EXP | 9 | 310 | 282 | 90.968% | 0 |
| OFFSET | 16 | 126 | 98 | 77.778% | 2 |
| UNIQUE | 8 | 123 | 95 | 77.236% | 0 |
| DAYS | 3 | 109 | 81 | 74.312% | 0 |
| VALUE | 7 | 92 | 64 | 69.565% | 0 |
| T.DIST | 3 | 85 | 57 | 67.059% | 0 |
| DB | 3 | 82 | 54 | 65.854% | 0 |
| HSTACK | 3 | 58 | 30 | 51.724% | 0 |
| RANK | 3 | 37 | 9 | 24.324% | 1 |
| FORECAST.ETS.SEASONALITY | 1 | 32 | 4 | 12.500% | 16 |
| FORECAST.ETS.STAT | 1 | 32 | 4 | 12.500% | 16 |
| LEN | 11 | 137 | 110 | 80.292% | 2 |
| CHOOSE | 10 | 120 | 93 | 77.500% | 5 |
| ERROR.TYPE | 5 | 95 | 68 | 71.579% | 0 |
| POISSON.DIST | 3 | 94 | 67 | 71.277% | 0 |
| IMSQRT | 2 | 84 | 57 | 67.857% | 0 |
| INTRATE | 2 | 28 | 1 | 3.571% | 23 |
| RADIANS | 6 | 1517 | 1491 | 98.286% | 0 |
| AVERAGE | 18 | 679 | 653 | 96.171% | 4 |
| SQRT | 37 | 137 | 111 | 81.022% | 4 |
| YEAR | 10 | 137 | 111 | 81.022% | 0 |
| CONFIDENCE.T | 3 | 87 | 61 | 70.115% | 0 |
| IMSIN | 2 | 84 | 58 | 69.048% | 0 |
| IMSINH | 2 | 84 | 58 | 69.048% | 0 |
| ISEVEN | 4 | 75 | 49 | 65.333% | 0 |
| ISODD | 4 | 75 | 49 | 65.333% | 0 |
| MODE | 3 | 66 | 40 | 60.606% | 0 |
| FIXED | 1 | 41 | 15 | 36.585% | 0 |
| COSH | 5 | 106 | 81 | 76.415% | 0 |
| LARGE | 5 | 81 | 56 | 69.136% | 0 |
| FORECAST | 2 | 55 | 30 | 54.545% | 0 |
| PERCENTRANK.INC | 3 | 42 | 17 | 40.476% | 0 |
| PERCENTRANK | 1 | 32 | 7 | 21.875% | 0 |
| TEXTSPLIT | 4 | 133 | 109 | 81.955% | 0 |
| DATEVALUE | 9 | 121 | 97 | 80.165% | 0 |
| NEGBINOM.DIST | 3 | 112 | 88 | 78.571% | 0 |
| SINH | 4 | 106 | 82 | 77.358% | 0 |
| BETA.DIST | 4 | 104 | 80 | 76.923% | 0 |
| GAMMA.DIST | 4 | 99 | 75 | 75.758% | 0 |
| MROUND | 4 | 94 | 70 | 74.468% | 0 |
| ATAN2 | 6 | 87 | 63 | 72.414% | 0 |
| IMEXP | 3 | 85 | 61 | 71.765% | 0 |
| IMSEC | 2 | 84 | 60 | 71.429% | 0 |
| T.DIST.RT | 3 | 82 | 58 | 70.732% | 0 |
| BINOM.DIST.RANGE | 2 | 81 | 57 | 70.370% | 0 |
| TAKE | 3 | 53 | 29 | 54.717% | 0 |
| PERCENTRANK.EXC | 2 | 41 | 17 | 41.463% | 0 |
| IMPRODUCT | 2 | 40 | 16 | 40.000% | 0 |
| QUARTILE | 1 | 32 | 8 | 25.000% | 0 |
| ROUNDUP | 7 | 166 | 143 | 86.145% | 0 |
| PMT | 5 | 121 | 98 | 80.992% | 0 |
| ASINH | 4 | 108 | 85 | 78.704% | 0 |
| ATANH | 4 | 104 | 81 | 77.885% | 0 |
| TANH | 4 | 104 | 81 | 77.885% | 0 |
| LOGNORM.DIST | 3 | 100 | 77 | 77.000% | 0 |
| IMSECH | 2 | 84 | 61 | 72.619% | 0 |
| DATEDIF | 4 | 79 | 56 | 70.886% | 0 |
| SMALL | 4 | 78 | 55 | 70.513% | 0 |
| ISPMT | 2 | 46 | 23 | 50.000% | 0 |
| RANK.EQ | 3 | 43 | 20 | 46.512% | 0 |
| IMSUB | 2 | 40 | 17 | 42.500% | 0 |
| FORECAST.LINEAR | 1 | 32 | 9 | 28.125% | 0 |
| ROWS | 8 | 4042 | 4020 | 99.456% | 4 |
| BASE | 3 | 257 | 235 | 91.440% | 0 |
| ROUNDDOWN | 7 | 193 | 171 | 88.601% | 0 |
| NPER | 5 | 166 | 144 | 86.747% | 0 |
| GAMMA.INV | 3 | 106 | 84 | 79.245% | 0 |
| ODD | 4 | 94 | 72 | 76.596% | 0 |
| GAMMALN.PRECISE | 4 | 88 | 66 | 75.000% | 0 |
| GAMMALN | 3 | 87 | 65 | 74.713% | 0 |
| IMCOSH | 2 | 84 | 62 | 73.810% | 0 |
| NORM.S.DIST | 3 | 80 | 58 | 72.500% | 0 |
| IMCOS | 2 | 80 | 58 | 72.500% | 0 |
| EFFECT | 2 | 76 | 54 | 71.053% | 0 |
| NOMINAL | 2 | 76 | 54 | 71.053% | 0 |
| PERCENTILE.INC | 3 | 49 | 27 | 55.102% | 0 |
| CLEAN | 5 | 45 | 23 | 51.111% | 0 |
| QUARTILE.INC | 3 | 42 | 20 | 47.619% | 0 |
| IMDIV | 2 | 42 | 20 | 47.619% | 0 |
| TYPE | 4 | 41 | 19 | 46.341% | 2 |
| CHISQ.TEST | 3 | 34 | 12 | 35.294% | 0 |
| PERCENTILE | 1 | 32 | 10 | 31.250% | 0 |
| MDETERM | 3 | 23 | 1 | 4.348% | 22 |
| RECEIVED | 1 | 23 | 1 | 4.348% | 18 |
| CODE | 1 | 293 | 272 | 92.833% | 0 |
| FV | 6 | 160 | 139 | 86.875% | 0 |
| TIME | 19 | 137 | 116 | 84.672% | 0 |
| LN | 5 | 130 | 109 | 83.846% | 0 |
| LOG | 5 | 127 | 106 | 83.465% | 0 |
| HOUR | 5 | 114 | 93 | 81.579% | 0 |
| ASIN | 6 | 113 | 92 | 81.416% | 0 |
| ATAN | 6 | 112 | 91 | 81.250% | 0 |
| TRIM | 7 | 112 | 91 | 81.250% | 0 |
| MINUTE | 5 | 110 | 89 | 80.909% | 0 |
| ABS | 12 | 109 | 88 | 80.734% | 0 |
| T.INV.2T | 3 | 104 | 83 | 79.808% | 0 |
| MOD | 6 | 101 | 80 | 79.208% | 0 |
| NORM.DIST | 3 | 98 | 77 | 78.571% | 0 |
| ISREF | 7 | 96 | 75 | 78.125% | 6 |
| EXPON.DIST | 4 | 91 | 70 | 76.923% | 0 |
| COTH | 2 | 85 | 64 | 75.294% | 0 |
| COT | 2 | 78 | 57 | 73.077% | 0 |
| T.DIST.2T | 3 | 76 | 55 | 72.368% | 0 |
| GAMMA | 3 | 73 | 52 | 71.233% | 0 |
| DDB | 2 | 72 | 51 | 70.833% | 0 |
| COMPLEX | 3 | 59 | 38 | 64.407% | 0 |
| IMCONJUGATE | 2 | 56 | 35 | 62.500% | 0 |
| PROPER | 3 | 56 | 35 | 62.500% | 0 |
| PERCENTILE.EXC | 3 | 50 | 29 | 58.000% | 0 |
| VAR.P | 5 | 50 | 29 | 58.000% | 2 |
| RANK.AVG | 3 | 43 | 22 | 51.163% | 0 |
| QUARTILE.EXC | 3 | 42 | 21 | 50.000% | 0 |
| F.DIST | 3 | 36 | 15 | 41.667% | 0 |
| CHITEST | 1 | 32 | 11 | 34.375% | 0 |
| IMPOWER | 1 | 32 | 11 | 34.375% | 0 |
| MIN | 22 | 1065 | 1045 | 98.122% | 2 |
| ISNUMBER | 12 | 142 | 122 | 85.915% | 0 |
| ACOS | 6 | 114 | 94 | 82.456% | 0 |
| LOGNORM.INV | 3 | 111 | 91 | 81.982% | 0 |
| SECOND | 5 | 110 | 90 | 81.818% | 0 |
| INT | 6 | 109 | 89 | 81.651% | 0 |
| TIMEVALUE | 5 | 107 | 87 | 81.308% | 0 |
| ACOSH | 4 | 106 | 86 | 81.132% | 0 |
| CONFIDENCE.NORM | 3 | 88 | 68 | 77.273% | 0 |
| SEC | 2 | 78 | 58 | 74.359% | 0 |
| DROP | 2 | 70 | 50 | 71.429% | 0 |
| AVERAGEA | 8 | 69 | 49 | 71.014% | 0 |
| DELTA | 3 | 63 | 43 | 68.254% | 0 |
| IMABS | 2 | 56 | 36 | 64.286% | 0 |
| IMARGUMENT | 2 | 56 | 36 | 64.286% | 0 |
| COLUMNS | 6 | 48 | 28 | 58.333% | 4 |
| ACCRINTM | 2 | 42 | 22 | 52.381% | 0 |
| DOLLARDE | 2 | 37 | 17 | 45.946% | 0 |
| LOG10 | 5 | 136 | 117 | 86.029% | 0 |
| TOCOL | 2 | 120 | 101 | 84.167% | 0 |
| NORM.INV | 3 | 109 | 90 | 82.569% | 0 |
| ERF | 3 | 100 | 81 | 81.000% | 0 |
| EVEN | 4 | 94 | 75 | 79.787% | 0 |
| NORM.S.INV | 3 | 92 | 73 | 79.348% | 0 |
| ISBLANK | 7 | 91 | 72 | 79.121% | 0 |
| FISHERINV | 3 | 86 | 67 | 77.907% | 0 |
| ACOTH | 2 | 83 | 64 | 77.108% | 0 |
| DEGREES | 8 | 81 | 62 | 76.543% | 0 |
| CSC | 2 | 78 | 59 | 75.641% | 0 |
| BINOM.INV | 3 | 70 | 51 | 72.857% | 0 |
| POISSON | 3 | 36 | 17 | 47.222% | 0 |
| CHISQ.DIST | 3 | 36 | 17 | 47.222% | 0 |
| CHISQ.DIST.RT | 3 | 36 | 17 | 47.222% | 0 |
| HYPGEOM.DIST | 3 | 36 | 17 | 47.222% | 0 |
| F.DIST.RT | 3 | 35 | 16 | 45.714% | 0 |
| BINOMDIST | 1 | 32 | 13 | 40.625% | 0 |
| T.INV | 3 | 106 | 88 | 83.019% | 0 |
| CSCH | 2 | 85 | 67 | 78.824% | 0 |
| SECH | 2 | 85 | 67 | 78.824% | 0 |
| CHOOSEROWS | 3 | 84 | 66 | 78.571% | 0 |
| ACOT | 2 | 83 | 65 | 78.313% | 0 |
| FACT | 5 | 72 | 54 | 75.000% | 0 |
| GESTEP | 2 | 62 | 44 | 70.968% | 0 |
| SYD | 2 | 53 | 35 | 66.038% | 0 |
| TRIMMEAN | 2 | 50 | 32 | 64.000% | 0 |
| PDURATION | 2 | 49 | 31 | 63.265% | 0 |
| F.INV | 3 | 36 | 18 | 50.000% | 0 |
| NORMDIST | 1 | 32 | 14 | 43.750% | 0 |
| SLN | 4 | 79 | 62 | 78.481% | 0 |
| ERF.PRECISE | 4 | 78 | 61 | 78.205% | 0 |
| ERFC.PRECISE | 4 | 78 | 61 | 78.205% | 0 |
| ERFC | 3 | 77 | 60 | 77.922% | 0 |
| RATE | 4 | 72 | 55 | 76.389% | 0 |
| N | 3 | 70 | 53 | 75.714% | 1 |
| COVARIANCE.P | 3 | 69 | 52 | 75.362% | 0 |
| COVARIANCE.S | 3 | 69 | 52 | 75.362% | 0 |
| AVEDEV | 4 | 63 | 46 | 73.016% | 0 |
| DEVSQ | 4 | 59 | 42 | 71.186% | 0 |
| INTERCEPT | 3 | 56 | 39 | 69.643% | 0 |
| SLOPE | 3 | 56 | 39 | 69.643% | 0 |
| BITAND | 2 | 54 | 37 | 68.519% | 0 |
| PERMUT | 2 | 49 | 32 | 65.306% | 0 |
| RRI | 2 | 43 | 26 | 60.465% | 0 |
| MODE.SNGL | 3 | 41 | 24 | 58.537% | 0 |
| FACTDOUBLE | 2 | 38 | 21 | 55.263% | 0 |
| WEIBULL.DIST | 3 | 36 | 19 | 52.778% | 0 |
| CHISQ.INV | 3 | 34 | 17 | 50.000% | 0 |
| CORREL | 2 | 33 | 16 | 48.485% | 0 |
| HARMEAN | 2 | 33 | 16 | 48.485% | 0 |
| KURT | 2 | 33 | 16 | 48.485% | 0 |
| PEARSON | 2 | 33 | 16 | 48.485% | 0 |
| RSQ | 2 | 33 | 16 | 48.485% | 0 |
| SKEW | 2 | 33 | 16 | 48.485% | 0 |
| SKEW.P | 2 | 33 | 16 | 48.485% | 0 |
| STEYX | 2 | 33 | 16 | 48.485% | 0 |
| COVAR | 1 | 32 | 15 | 46.875% | 0 |
| EXPONDIST | 1 | 32 | 15 | 46.875% | 0 |
| FDIST | 1 | 32 | 15 | 46.875% | 0 |
| GAMMADIST | 1 | 32 | 15 | 46.875% | 0 |
| GEOMEAN | 1 | 32 | 15 | 46.875% | 0 |
| LOGNORMDIST | 1 | 32 | 15 | 46.875% | 0 |
| NEGBINOMDIST | 1 | 32 | 15 | 46.875% | 0 |
| TDIST | 1 | 32 | 15 | 46.875% | 0 |
| WEIBULL | 1 | 32 | 15 | 46.875% | 0 |
| PI | 31 | 2287 | 2271 | 99.300% | 0 |
| TRUNC | 4 | 587 | 571 | 97.274% | 0 |
| BESSELK | 2 | 150 | 134 | 89.333% | 0 |
| FIND | 8 | 132 | 116 | 87.879% | 0 |
| SUMSQ | 5 | 98 | 82 | 83.673% | 0 |
| ISTEXT | 6 | 88 | 72 | 81.818% | 0 |
| MEDIAN | 6 | 69 | 53 | 76.812% | 0 |
| FISHER | 3 | 68 | 52 | 76.471% | 0 |
| STDEV | 4 | 63 | 47 | 74.603% | 0 |
| VAR | 4 | 63 | 47 | 74.603% | 0 |
| SORTBY | 2 | 60 | 44 | 73.333% | 0 |
| MINA | 5 | 55 | 39 | 70.909% | 0 |
| VARP | 3 | 50 | 34 | 68.000% | 0 |
| MAXA | 5 | 48 | 32 | 66.667% | 0 |
| SQRTPI | 2 | 43 | 27 | 62.791% | 0 |
| VAR.S | 4 | 41 | 25 | 60.976% | 0 |
| STDEV.P | 4 | 39 | 23 | 58.974% | 0 |
| STDEV.S | 4 | 38 | 22 | 57.895% | 0 |
| F.TEST | 3 | 37 | 21 | 56.757% | 0 |
| Z.TEST | 3 | 37 | 21 | 56.757% | 0 |
| DOLLARFR | 2 | 37 | 21 | 56.757% | 0 |
| STANDARDIZE | 2 | 36 | 20 | 55.556% | 0 |
| F.INV.RT | 3 | 35 | 19 | 54.286% | 0 |
| CHISQ.INV.RT | 3 | 34 | 18 | 52.941% | 0 |
| STDEVA | 2 | 33 | 17 | 51.515% | 0 |
| STDEVPA | 2 | 33 | 17 | 51.515% | 0 |
| VARA | 2 | 33 | 17 | 51.515% | 0 |
| VARPA | 2 | 33 | 17 | 51.515% | 0 |
| BETADIST | 1 | 32 | 16 | 50.000% | 0 |
| BETAINV | 1 | 32 | 16 | 50.000% | 0 |
| CHIDIST | 1 | 32 | 16 | 50.000% | 0 |
| CHIINV | 1 | 32 | 16 | 50.000% | 0 |
| CONFIDENCE | 1 | 32 | 16 | 50.000% | 0 |
| FINV | 1 | 32 | 16 | 50.000% | 0 |
| FTEST | 1 | 32 | 16 | 50.000% | 0 |
| GAMMAINV | 1 | 32 | 16 | 50.000% | 0 |
| HYPGEOMDIST | 1 | 32 | 16 | 50.000% | 0 |
| LOGINV | 1 | 32 | 16 | 50.000% | 0 |
| NORMINV | 1 | 32 | 16 | 50.000% | 0 |
| NORMSDIST | 1 | 32 | 16 | 50.000% | 0 |
| NORMSINV | 1 | 32 | 16 | 50.000% | 0 |
| TINV | 1 | 32 | 16 | 50.000% | 0 |
| ZTEST | 1 | 32 | 16 | 50.000% | 0 |
| STDEVP | 1 | 31 | 15 | 48.387% | 0 |
| UNICHAR | 3 | 293 | 278 | 94.881% | 0 |
| BITRSHIFT | 2 | 76 | 61 | 80.263% | 0 |
| ISERR | 3 | 70 | 55 | 78.571% | 0 |
| GAUSS | 2 | 68 | 53 | 77.941% | 0 |
| LET | 8 | 59 | 44 | 74.576% | 0 |
| PHI | 2 | 39 | 24 | 61.538% | 0 |
| ASC | 1 | 32 | 17 | 53.125% | 0 |
| LEFTB | 1 | 32 | 17 | 53.125% | 0 |
| REPLACEB | 1 | 32 | 17 | 53.125% | 0 |
| SUMX2PY2 | 4 | 31 | 16 | 51.613% | 0 |
| _TRO_TRAILING | 1 | 25 | 10 | 40.000% | 2 |
| BYCOL | 2 | 18 | 3 | 16.667% | 2 |
| REDUCE | 2 | 16 | 1 | 6.250% | 13 |
| COUNT | 16 | 665 | 651 | 97.895% | 3 |
| PRODUCT | 6 | 118 | 104 | 88.136% | 0 |
| ISLOGICAL | 5 | 83 | 69 | 83.133% | 0 |
| ISNONTEXT | 5 | 83 | 69 | 83.133% | 0 |
| BITLSHIFT | 2 | 76 | 62 | 81.579% | 0 |
| TEXTJOIN | 2 | 72 | 58 | 80.556% | 0 |
| BITOR | 2 | 52 | 38 | 73.077% | 0 |
| BITXOR | 2 | 52 | 38 | 73.077% | 0 |
| FINDB | 1 | 32 | 18 | 56.250% | 0 |
| LENB | 1 | 32 | 18 | 56.250% | 0 |
| RIGHTB | 1 | 32 | 18 | 56.250% | 0 |
| _TRO_LEADING | 1 | 26 | 12 | 46.154% | 2 |
| MIDB | 1 | 32 | 19 | 59.375% | 0 |
| SEARCHB | 1 | 32 | 19 | 59.375% | 0 |
| SHEET | 3 | 25 | 12 | 48.000% | 0 |
| MAP | 2 | 17 | 4 | 23.529% | 5 |
| HEX2DEC | 4 | 185 | 173 | 93.514% | 0 |
| HYPERLINK | 4 | 18 | 6 | 33.333% | 0 |
| CAL_MONTH | 1 | 12 | 0 | 0.000% | 12 |
| TBILLPRICE | 2 | 47 | 36 | 76.596% | 0 |
| SUMX2MY2 | 4 | 40 | 29 | 72.500% | 0 |
| TOROW | 1 | 36 | 25 | 69.444% | 0 |
| CRITBINOM | 1 | 32 | 21 | 65.625% | 0 |
| USDOLLAR | 1 | 11 | 0 | 0.000% | 11 |
| HEX2BIN | 3 | 200 | 190 | 95.000% | 0 |
| HEX2OCT | 3 | 200 | 190 | 95.000% | 0 |
| DCOUNT | 1 | 48 | 38 | 79.167% | 0 |
| DCOUNTA | 1 | 48 | 38 | 79.167% | 0 |
| TROUND | 1 | 11 | 1 | 9.091% | 10 |
| XOR | 4 | 183 | 174 | 95.082% | 0 |
| SUMXMY2 | 4 | 29 | 20 | 68.966% | 0 |
| CROUND | 1 | 10 | 1 | 10.000% | 9 |
| BIN2DEC | 3 | 115 | 108 | 93.913% | 0 |
| T | 4 | 70 | 63 | 90.000% | 0 |
| TBILLEQ | 2 | 46 | 39 | 84.783% | 0 |
| MODE.MULT | 3 | 34 | 27 | 79.412% | 0 |
| TRUE | 5 | 15 | 8 | 53.333% | 0 |
| FALSE | 5 | 13 | 6 | 46.154% | 0 |
| TBILLYIELD | 2 | 47 | 41 | 87.234% | 0 |
| DAVERAGE | 2 | 45 | 39 | 86.667% | 0 |
| DPRODUCT | 1 | 44 | 38 | 86.364% | 0 |
| DVARP | 1 | 41 | 35 | 85.366% | 0 |
| DSUM | 1 | 40 | 34 | 85.000% | 0 |
| DSTDEVP | 1 | 40 | 34 | 85.000% | 0 |
| BESSELI | 2 | 146 | 141 | 96.575% | 0 |
| COMBIN | 3 | 54 | 49 | 90.741% | 0 |
| DMIN | 1 | 41 | 36 | 87.805% | 0 |
| DSTDEV | 1 | 41 | 36 | 87.805% | 0 |
| DVAR | 1 | 41 | 36 | 87.805% | 0 |
| MULTINOMIAL | 2 | 29 | 24 | 82.759% | 0 |
| COMBINA | 1 | 36 | 32 | 88.889% | 0 |
| SERIESSUM | 1 | 8 | 4 | 50.000% | 0 |
| TEMPERATURE | 1 | 4 | 0 | 0.000% | 4 |
| GELU | 1 | 4 | 0 | 0.000% | 2 |
| OCT2DEC | 3 | 88 | 85 | 96.591% | 0 |
| DMAX | 1 | 41 | 38 | 92.683% | 0 |
| DOLLAR | 1 | 11 | 8 | 72.727% | 0 |
| MYFUNC | 2 | 3 | 0 | 0.000% | 3 |
| ISOMITTED | 1 | 3 | 0 | 0.000% | 3 |
| IFNA | 4 | 98 | 96 | 97.959% | 0 |
| QUOTIENT | 1 | 43 | 41 | 95.349% | 0 |
| D6 | 1 | 2 | 0 | 0.000% | 2 |
| DGET | 1 | 36 | 35 | 97.222% | 0 |
| BROUNDTO | 1 | 3 | 2 | 66.667% | 1 |
| MYFUNC2 | 1 | 1 | 0 | 0.000% | 1 |
| _XLL.DSSDATA | 1 | 1 | 0 | 0.000% | 1 |
| MYLAMBDA | 1 | 1 | 0 | 0.000% | 1 |
| COUNTWORDS | 1 | 1 | 0 | 0.000% | 1 |
| COUNTBLANK | 6 | 33 | 33 | 100.000% | 0 |
| MYFUNCTION | 1 | 2 | 2 | 100.000% | 0 |
| SUMM | 1 | 2 | 2 | 100.000% | 0 |
| XYZ | 1 | 2 | 2 | 100.000% | 0 |
| CRAPNAME | 1 | 1 | 1 | 100.000% | 0 |
| DDE | 1 | 1 | 1 | 100.000% | 0 |
| DP | 1 | 1 | 1 | 100.000% | 0 |
| _XLUDF.SUM | 1 | 1 | 1 | 100.000% | 0 |
| HELLO | 1 | 1 | 1 | 100.000% | 0 |
| SHEETS | 1 | 1 | 1 | 100.000% | 0 |
| AREA | 1 | 1 | 1 | 100.000% | 0 |

### Slices

38728 slices: p50 0.0 ms, p99 11.6 ms, max 1343 ms.

## ironcalc

2989 files run, 2953 finished, 36 failed.
Failures: {"no xl/workbook.xml":12,"zip: invalid Zip archive: Variable-length field extends beyo":2,"zip: invalid Zip archive: Could not find EOCD":7,"missing part xl/worksheets/sheet8.xml":1,"zip: invalid Zip archive: Invalid CDFH offset in EOCD":2,"zip: invalid Zip archive: Invalid Central Directory header":1,"timeout":8,"missing part xl/worksheets/sheet2.xml":1,"Incorrect row or column":1,"trap: unreachable":1}

| Set | Files | Formula cells | Match (pooled) | Match (10% cap) | Wrong value | Wrong type | Unsupported | Near zero | All cells match |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| All Excel-saved public files | 690 | 200774 | 94.533% | 94.666% | 1173 | 7851 | 1875 | 77 | 536 |
| Without either engine’s own fixtures | 444 | 124473 | 91.481% | 93.375% | 1022 | 7716 | 1845 | 21 | 328 |
| Documents (without fixtures or function test suites) | 341 | 63891 | 98.745% | 98.508% | 60 | 351 | 384 | 7 | 261 |
| Function test suites (without fixtures) | 103 | 60582 | 83.820% | 82.791% | 962 | 7365 | 1461 | 14 | 67 |
|   calamine | 16 | 283 | 89.399% | 83.333% | 0 | 30 | 0 | 0 | 13 |
|   closedxml | 30 | 3039 | 88.088% | 91.897% | 2 | 6 | 354 | 0 | 26 |
|   epplus | 14 | 659 | 70.713% | 51.306% | 49 | 85 | 59 | 0 | 4 |
|   excelize | 2 | 3 | 66.667% | 75.000% | 1 | 0 | 0 | 0 | 1 |
|   exceljs | 7 | 222 | 99.550% | 85.714% | 0 | 0 | 1 | 0 | 6 |
|   formualizer | 1 | 5 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 1 |
|   formulas | 5 | 23571 | 70.977% | 79.768% | 497 | 5369 | 968 | 7 | 3 |
|   ironcalc | 245 | 76296 | 99.512% | 98.780% | 151 | 135 | 30 | 56 | 207 |
|   koala | 5 | 173 | 79.191% | 91.320% | 0 | 36 | 0 | 0 | 3 |
|   libreoffice | 46 | 8165 | 93.484% | 75.031% | 185 | 160 | 187 | 0 | 31 |
|   npoi | 8 | 6370 | 93.705% | 68.988% | 63 | 327 | 11 | 0 | 4 |
|   openxml-sdk | 12 | 28637 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 12 |
|   pandas | 4 | 340 | 100.000% | 100.000% | 0 | 0 | 0 | 0 | 4 |
|   phpspreadsheet | 77 | 3246 | 95.471% | 91.993% | 30 | 100 | 17 | 0 | 59 |
|   poi | 116 | 24355 | 94.473% | 85.017% | 109 | 1052 | 178 | 7 | 81 |
|   pycel | 12 | 5471 | 89.856% | 87.727% | 70 | 449 | 29 | 7 | 4 |
|   xlcalculator | 56 | 19820 | 99.435% | 98.126% | 15 | 97 | 0 | 0 | 53 |
|   xlsxwriter | 34 | 119 | 60.504% | 60.467% | 1 | 5 | 41 | 0 | 24 |

### Per function (525 functions, misses first)

A cell counts for every function in its formula, so a miss can belong to another function in the same formula or to a precedent.

| Function | Files | Cells | Matched | Match | Unsupported |
| --- | --- | --- | --- | --- | --- |
| TEXT | 27 | 2116 | 859 | 40.595% | 0 |
| (no function) | 396 | 62033 | 61593 | 99.291% | 133 |
| PIVOTBY | 1 | 588 | 158 | 26.871% | 20 |
| GROUPBY | 2 | 282 | 26 | 9.220% | 8 |
| ROUND | 18 | 984 | 784 | 79.675% | 175 |
| YEARFRAC | 4 | 2108 | 1916 | 90.892% | 1 |
| T.TEST | 3 | 202 | 18 | 8.911% | 101 |
| TTEST | 1 | 192 | 8 | 4.167% | 111 |
| AGGREGATE | 1 | 174 | 0 | 0.000% | 174 |
| XLOOKUP | 6 | 732 | 562 | 76.776% | 1 |
| LOOKUP | 7 | 459 | 305 | 66.449% | 25 |
| IF | 61 | 4515 | 4363 | 96.633% | 40 |
| SUM | 147 | 2307 | 2162 | 93.715% | 72 |
| CHAR | 6 | 558 | 417 | 74.731% | 0 |
| UNICODE | 2 | 305 | 165 | 54.098% | 17 |
| COUPNCD | 2 | 176 | 36 | 20.455% | 17 |
| SUMIF | 13 | 279 | 149 | 53.405% | 0 |
| FORMULATEXT | 19 | 477 | 357 | 74.843% | 0 |
| LINEST | 5 | 186 | 97 | 52.151% | 1 |
| LAMBDA | 12 | 185 | 100 | 54.054% | 39 |
| SROUND | 1 | 155 | 72 | 46.452% | 83 |
| MATCH | 13 | 582 | 502 | 86.254% | 0 |
| FORECAST.ETS.CONFINT | 2 | 76 | 0 | 0.000% | 76 |
| TREND | 3 | 180 | 105 | 58.333% | 1 |
| COLUMN | 6 | 145 | 76 | 52.414% | 24 |
| IFS | 7 | 166 | 99 | 59.639% | 1 |
| ANCHORARRAY | 16 | 242 | 176 | 72.727% | 9 |
| NOT | 10 | 193 | 127 | 65.803% | 20 |
| VALUETOTEXT | 2 | 78 | 14 | 17.949% | 63 |
| SUBTOTAL | 43 | 346 | 286 | 82.659% | 43 |
| ROW | 14 | 292 | 233 | 79.795% | 24 |
| WEEKDAY | 7 | 443 | 389 | 87.810% | 0 |
| FORECAST.ETS | 2 | 54 | 0 | 0.000% | 54 |
| SUMPRODUCT | 18 | 138 | 85 | 61.594% | 0 |
| CONVERT | 2 | 3950 | 3901 | 98.759% | 0 |
| VLOOKUP | 23 | 1473 | 1424 | 96.673% | 16 |
| ISFORMULA | 4 | 89 | 40 | 44.944% | 0 |
| INDEX | 21 | 441 | 393 | 89.116% | 30 |
| INDIRECT | 21 | 148 | 100 | 67.568% | 5 |
| SCAN | 2 | 57 | 9 | 15.789% | 22 |
| AND | 21 | 1411 | 1364 | 96.669% | 3 |
| OR | 15 | 1371 | 1326 | 96.718% | 9 |
| IRR | 4 | 161 | 116 | 72.050% | 1 |
| XIRR | 3 | 95 | 51 | 53.684% | 0 |
| CONCATENATE | 15 | 277 | 234 | 84.477% | 0 |
| BESSELY | 2 | 150 | 108 | 72.000% | 0 |
| NPV | 7 | 129 | 89 | 68.992% | 0 |
| TRANSPOSE | 7 | 139 | 100 | 71.942% | 1 |
| MINVERSE | 3 | 98 | 59 | 60.204% | 0 |
| ISOWEEKNUM | 4 | 84 | 45 | 53.571% | 0 |
| DAY | 12 | 261 | 223 | 85.441% | 0 |
| GROWTH | 2 | 70 | 32 | 45.714% | 1 |
| MONTH | 12 | 145 | 108 | 74.483% | 0 |
| XMATCH | 3 | 450 | 414 | 92.000% | 0 |
| _TRO_ALL | 1 | 57 | 21 | 36.842% | 4 |
| _FV | 1 | 36 | 0 | 0.000% | 36 |
| NUMBERVALUE | 2 | 65 | 30 | 46.154% | 0 |
| DATE | 50 | 1427 | 1393 | 97.617% | 1 |
| TEXTAFTER | 2 | 108 | 74 | 68.519% | 34 |
| MIRR | 2 | 288 | 255 | 88.542% | 0 |
| ERROR.TYPE | 6 | 123 | 90 | 73.171% | 1 |
| CHOOSECOLS | 3 | 110 | 77 | 70.000% | 0 |
| FORECAST.ETS.SEASONALITY | 1 | 32 | 0 | 0.000% | 32 |
| FORECAST.ETS.STAT | 1 | 32 | 0 | 0.000% | 32 |
| VDB | 2 | 72 | 41 | 56.944% | 0 |
| BIN2OCT | 3 | 191 | 161 | 84.293% | 0 |
| BIN2HEX | 3 | 161 | 131 | 81.366% | 0 |
| BAHTTEXT | 1 | 32 | 2 | 6.250% | 30 |
| BESSELJ | 2 | 150 | 121 | 80.667% | 0 |
| COUPNUM | 2 | 60 | 31 | 51.667% | 0 |
| PRICE | 2 | 47 | 18 | 38.298% | 0 |
| YIELD | 2 | 47 | 18 | 38.298% | 0 |
| AMORDEGRC | 1 | 32 | 3 | 9.375% | 0 |
| AMORLINC | 1 | 32 | 3 | 9.375% | 0 |
| PRICEDISC | 1 | 32 | 3 | 9.375% | 0 |
| PRICEMAT | 1 | 32 | 3 | 9.375% | 0 |
| YIELDDISC | 1 | 32 | 3 | 9.375% | 0 |
| YIELDMAT | 1 | 32 | 3 | 9.375% | 0 |
| WORKDAY.INTL | 3 | 130 | 102 | 78.462% | 1 |
| FILTER | 11 | 121 | 93 | 76.860% | 0 |
| AREAS | 2 | 33 | 5 | 15.152% | 22 |
| DURATION | 1 | 32 | 4 | 12.500% | 0 |
| MDURATION | 1 | 32 | 4 | 12.500% | 0 |
| MMULT | 6 | 246 | 219 | 89.024% | 0 |
| MINIFS | 4 | 96 | 69 | 71.875% | 0 |
| MAXIFS | 3 | 95 | 68 | 71.579% | 0 |
| ADDRESS | 7 | 84 | 57 | 67.857% | 0 |
| IMSUM | 2 | 40 | 13 | 32.500% | 17 |
| SWITCH | 4 | 193 | 167 | 86.528% | 1 |
| DECIMAL | 2 | 131 | 105 | 80.153% | 0 |
| TEXTBEFORE | 2 | 102 | 76 | 74.510% | 15 |
| OFFSET | 16 | 72 | 46 | 63.889% | 1 |
| Z.TEST | 3 | 37 | 11 | 29.730% | 1 |
| ZTEST | 1 | 32 | 6 | 18.750% | 1 |
| TEXTSPLIT | 4 | 133 | 108 | 81.203% | 1 |
| COUPDAYBS | 2 | 61 | 36 | 59.016% | 0 |
| LOGEST | 2 | 48 | 23 | 47.917% | 1 |
| FIXED | 1 | 41 | 16 | 39.024% | 0 |
| BYROW | 2 | 32 | 7 | 21.875% | 13 |
| FLOOR.MATH | 4 | 4375 | 4351 | 99.451% | 0 |
| CEILING.MATH | 4 | 4325 | 4301 | 99.445% | 0 |
| YEAR | 10 | 137 | 113 | 82.482% | 0 |
| DATEDIF | 4 | 79 | 55 | 69.620% | 17 |
| HLOOKUP | 6 | 172 | 149 | 86.628% | 17 |
| GAMMALN.PRECISE | 4 | 88 | 65 | 73.864% | 0 |
| GAMMALN | 3 | 87 | 64 | 73.563% | 0 |
| LARGE | 5 | 81 | 58 | 71.605% | 0 |
| SMALL | 4 | 78 | 55 | 70.513% | 0 |
| SINGLE | 11 | 64 | 41 | 64.063% | 4 |
| ODDFYIELD | 2 | 64 | 41 | 64.063% | 17 |
| PERCENTILE.INC | 3 | 49 | 26 | 53.061% | 1 |
| QUARTILE.EXC | 3 | 42 | 19 | 45.238% | 1 |
| QUARTILE.INC | 3 | 42 | 19 | 45.238% | 1 |
| PERCENTILE | 1 | 32 | 9 | 28.125% | 1 |
| QUARTILE | 1 | 32 | 9 | 28.125% | 1 |
| ISNA | 7 | 117 | 95 | 81.197% | 0 |
| REGEXEXTRACT | 2 | 56 | 34 | 60.714% | 0 |
| FORECAST | 2 | 55 | 33 | 60.000% | 0 |
| PERCENTILE.EXC | 3 | 50 | 28 | 56.000% | 1 |
| IMPRODUCT | 2 | 40 | 19 | 47.500% | 19 |
| SEQUENCE | 13 | 189 | 169 | 89.418% | 2 |
| T.INV.2T | 3 | 104 | 84 | 80.769% | 0 |
| PPMT | 2 | 92 | 72 | 78.261% | 0 |
| COUPDAYSNC | 2 | 56 | 36 | 64.286% | 5 |
| COUPPCD | 2 | 56 | 36 | 64.286% | 5 |
| ISPMT | 2 | 46 | 26 | 56.522% | 0 |
| HYPGEOM.DIST | 3 | 36 | 16 | 44.444% | 0 |
| HYPGEOMDIST | 1 | 32 | 12 | 37.500% | 0 |
| REGEXTEST | 3 | 694 | 675 | 97.262% | 3 |
| NORM.INV | 3 | 109 | 90 | 82.569% | 0 |
| T.INV | 3 | 106 | 87 | 82.075% | 0 |
| EXPON.DIST | 4 | 91 | 72 | 79.121% | 0 |
| T.DIST | 3 | 85 | 66 | 77.647% | 0 |
| T.DIST.RT | 3 | 82 | 63 | 76.829% | 0 |
| DB | 3 | 82 | 63 | 76.829% | 0 |
| BINOM.DIST.RANGE | 2 | 81 | 62 | 76.543% | 0 |
| ACCRINTM | 2 | 42 | 23 | 54.762% | 0 |
| STANDARDIZE | 2 | 36 | 17 | 47.222% | 0 |
| BIN2DEC | 3 | 115 | 97 | 84.348% | 0 |
| NEGBINOM.DIST | 3 | 112 | 94 | 83.929% | 0 |
| BINOM.DIST | 3 | 108 | 90 | 83.333% | 0 |
| AVERAGEIF | 5 | 101 | 83 | 82.178% | 0 |
| LOGNORM.DIST | 3 | 100 | 82 | 82.000% | 0 |
| GAMMA.DIST | 4 | 99 | 81 | 81.818% | 0 |
| NORM.DIST | 3 | 98 | 80 | 81.633% | 0 |
| POISSON.DIST | 3 | 94 | 76 | 80.851% | 0 |
| VALUE | 7 | 92 | 74 | 80.435% | 0 |
| AVERAGEIFS | 2 | 82 | 64 | 78.049% | 0 |
| FREQUENCY | 2 | 77 | 59 | 76.623% | 0 |
| T.DIST.2T | 3 | 76 | 58 | 76.316% | 0 |
| DDB | 2 | 72 | 54 | 75.000% | 0 |
| ARRAYTOTEXT | 3 | 69 | 51 | 73.913% | 4 |
| RANK.EQ | 3 | 43 | 25 | 58.140% | 1 |
| BETA.DIST | 3 | 39 | 21 | 53.846% | 0 |
| RANK | 3 | 37 | 19 | 51.351% | 1 |
| POISSON | 3 | 36 | 18 | 50.000% | 0 |
| CHISQ.DIST | 3 | 36 | 18 | 50.000% | 0 |
| CHISQ.DIST.RT | 3 | 36 | 18 | 50.000% | 0 |
| F.DIST | 3 | 36 | 18 | 50.000% | 0 |
| F.INV | 3 | 36 | 18 | 50.000% | 0 |
| WEIBULL.DIST | 3 | 36 | 18 | 50.000% | 0 |
| BETA.INV | 3 | 35 | 17 | 48.571% | 0 |
| F.DIST.RT | 3 | 35 | 17 | 48.571% | 0 |
| F.INV.RT | 3 | 35 | 17 | 48.571% | 0 |
| CONFIDENCE.NORM | 2 | 33 | 15 | 45.455% | 0 |
| CONFIDENCE.T | 2 | 33 | 15 | 45.455% | 0 |
| BETADIST | 1 | 32 | 14 | 43.750% | 0 |
| BETAINV | 1 | 32 | 14 | 43.750% | 0 |
| BINOMDIST | 1 | 32 | 14 | 43.750% | 0 |
| EXPONDIST | 1 | 32 | 14 | 43.750% | 0 |
| FDIST | 1 | 32 | 14 | 43.750% | 0 |
| FINV | 1 | 32 | 14 | 43.750% | 0 |
| FORECAST.LINEAR | 1 | 32 | 14 | 43.750% | 0 |
| GAMMADIST | 1 | 32 | 14 | 43.750% | 0 |
| LOGNORMDIST | 1 | 32 | 14 | 43.750% | 0 |
| NEGBINOMDIST | 1 | 32 | 14 | 43.750% | 0 |
| NORMDIST | 1 | 32 | 14 | 43.750% | 0 |
| TDIST | 1 | 32 | 14 | 43.750% | 0 |
| WEIBULL | 1 | 32 | 14 | 43.750% | 0 |
| COUNTIF | 13 | 621 | 604 | 97.262% | 0 |
| SORT | 8 | 289 | 272 | 94.118% | 2 |
| PV | 7 | 252 | 235 | 93.254% | 0 |
| FV | 6 | 160 | 143 | 89.375% | 0 |
| NETWORKDAYS.INTL | 3 | 140 | 123 | 87.857% | 1 |
| CHOOSE | 11 | 122 | 105 | 86.066% | 1 |
| PMT | 5 | 121 | 104 | 85.950% | 0 |
| DATEVALUE | 9 | 121 | 104 | 85.950% | 17 |
| LOGNORM.INV | 3 | 111 | 94 | 84.685% | 0 |
| SECOND | 5 | 110 | 93 | 84.545% | 0 |
| GAMMA.INV | 3 | 106 | 89 | 83.962% | 0 |
| NORM.S.INV | 3 | 92 | 75 | 81.522% | 0 |
| IPMT | 2 | 92 | 75 | 81.522% | 0 |
| FISHERINV | 3 | 86 | 69 | 80.233% | 0 |
| NORM.S.DIST | 3 | 80 | 63 | 78.750% | 0 |
| ODDFPRICE | 2 | 75 | 58 | 77.333% | 17 |
| GAMMA | 3 | 73 | 56 | 76.712% | 0 |
| RATE | 4 | 72 | 55 | 76.389% | 0 |
| BINOM.INV | 3 | 70 | 53 | 75.714% | 0 |
| N | 3 | 70 | 53 | 75.714% | 17 |
| FISHER | 3 | 68 | 51 | 75.000% | 0 |
| GAUSS | 2 | 68 | 51 | 75.000% | 0 |
| PROB | 2 | 59 | 42 | 71.186% | 0 |
| SYD | 2 | 53 | 36 | 67.925% | 0 |
| TRIMMEAN | 2 | 50 | 33 | 66.000% | 0 |
| RANK.AVG | 3 | 43 | 26 | 60.465% | 0 |
| RRI | 2 | 43 | 26 | 60.465% | 0 |
| ODDLPRICE | 2 | 41 | 24 | 58.537% | 17 |
| ODDLYIELD | 2 | 41 | 24 | 58.537% | 17 |
| CHISQ.INV | 3 | 34 | 17 | 50.000% | 0 |
| CHISQ.INV.RT | 3 | 34 | 17 | 50.000% | 0 |
| CHIDIST | 1 | 32 | 15 | 46.875% | 0 |
| CHIINV | 1 | 32 | 15 | 46.875% | 0 |
| CONFIDENCE | 1 | 32 | 15 | 46.875% | 0 |
| CRITBINOM | 1 | 32 | 15 | 46.875% | 0 |
| GAMMAINV | 1 | 32 | 15 | 46.875% | 0 |
| LOGINV | 1 | 32 | 15 | 46.875% | 0 |
| NORMINV | 1 | 32 | 15 | 46.875% | 0 |
| NORMSDIST | 1 | 32 | 15 | 46.875% | 0 |
| NORMSINV | 1 | 32 | 15 | 46.875% | 0 |
| TINV | 1 | 32 | 15 | 46.875% | 0 |
| DAYS360 | 4 | 138 | 122 | 88.406% | 0 |
| TIME | 19 | 137 | 121 | 88.321% | 0 |
| HOUR | 5 | 114 | 98 | 85.965% | 0 |
| MINUTE | 5 | 110 | 94 | 85.455% | 0 |
| DAYS | 3 | 109 | 93 | 85.321% | 0 |
| MOD | 6 | 101 | 85 | 84.158% | 0 |
| SUMX2MY2 | 4 | 40 | 24 | 60.000% | 0 |
| PHI | 2 | 39 | 23 | 58.974% | 0 |
| FVSCHEDULE | 2 | 35 | 19 | 54.286% | 5 |
| REGEXREPLACE | 2 | 33 | 17 | 51.515% | 0 |
| ASC | 1 | 32 | 16 | 50.000% | 0 |
| SUMX2PY2 | 4 | 31 | 15 | 48.387% | 0 |
| CODE | 1 | 293 | 278 | 94.881% | 0 |
| NPER | 5 | 166 | 151 | 90.964% | 0 |
| TOCOL | 2 | 120 | 105 | 87.500% | 0 |
| SLN | 4 | 79 | 64 | 81.013% | 0 |
| BITAND | 2 | 54 | 39 | 72.222% | 0 |
| COUPDAYS | 2 | 51 | 36 | 70.588% | 5 |
| PDURATION | 2 | 49 | 34 | 69.388% | 0 |
| PERMUT | 2 | 49 | 34 | 69.388% | 0 |
| PERMUTATIONA | 2 | 49 | 34 | 69.388% | 0 |
| MAKEARRAY | 2 | 42 | 27 | 64.286% | 0 |
| _TRO_TRAILING | 1 | 25 | 10 | 40.000% | 2 |
| REDUCE | 2 | 16 | 1 | 6.250% | 13 |
| ROMAN | 2 | 20407 | 20393 | 99.931% | 0 |
| ARABIC | 2 | 20072 | 20058 | 99.930% | 0 |
| COUNTIFS | 6 | 353 | 339 | 96.034% | 0 |
| SUMIFS | 6 | 117 | 103 | 88.034% | 0 |
| DROP | 2 | 70 | 56 | 80.000% | 7 |
| BITOR | 2 | 52 | 38 | 73.077% | 0 |
| BITXOR | 2 | 52 | 38 | 73.077% | 0 |
| SQRTPI | 2 | 43 | 29 | 67.442% | 1 |
| WRAPROWS | 2 | 42 | 28 | 66.667% | 1 |
| FACTDOUBLE | 2 | 38 | 24 | 63.158% | 1 |
| SUMXMY2 | 4 | 29 | 15 | 51.724% | 0 |
| _TRO_LEADING | 1 | 26 | 12 | 46.154% | 2 |
| IFERROR | 13 | 1472 | 1459 | 99.117% | 0 |
| BITLSHIFT | 2 | 76 | 63 | 82.895% | 0 |
| BITRSHIFT | 2 | 76 | 63 | 82.895% | 0 |
| T | 4 | 70 | 57 | 81.429% | 0 |
| CEILING | 7 | 1570 | 1558 | 99.236% | 0 |
| FLOOR | 7 | 1512 | 1500 | 99.206% | 0 |
| TRUNC | 4 | 587 | 575 | 97.956% | 0 |
| UNICHAR | 3 | 293 | 281 | 95.904% | 0 |
| ROUNDDOWN | 7 | 193 | 181 | 93.782% | 0 |
| ROUNDUP | 7 | 166 | 154 | 92.771% | 0 |
| LOG | 5 | 127 | 115 | 90.551% | 0 |
| TAN | 6 | 114 | 102 | 89.474% | 0 |
| ATAN2 | 6 | 87 | 75 | 86.207% | 0 |
| CHOOSEROWS | 3 | 84 | 72 | 85.714% | 0 |
| MUNIT | 2 | 71 | 59 | 83.099% | 2 |
| LET | 8 | 59 | 47 | 79.661% | 0 |
| TAKE | 3 | 53 | 41 | 77.358% | 7 |
| CAL_MONTH | 1 | 12 | 0 | 0.000% | 0 |
| BASE | 3 | 257 | 246 | 95.720% | 0 |
| SORTBY | 2 | 60 | 49 | 81.667% | 0 |
| HYPERLINK | 4 | 18 | 7 | 38.889% | 0 |
| TROUND | 1 | 11 | 0 | 0.000% | 10 |
| USDOLLAR | 1 | 11 | 0 | 0.000% | 11 |
| ISERROR | 12 | 290 | 280 | 96.552% | 0 |
| SHEET | 3 | 25 | 15 | 60.000% | 0 |
| CROUND | 1 | 10 | 0 | 0.000% | 9 |
| MAX | 28 | 1003 | 994 | 99.103% | 0 |
| ACCRINT | 5 | 596 | 587 | 98.490% | 1 |
| LEFT | 12 | 212 | 203 | 95.755% | 0 |
| RIGHT | 11 | 142 | 133 | 93.662% | 0 |
| ISNUMBER | 11 | 140 | 131 | 93.571% | 0 |
| TIMEVALUE | 5 | 107 | 98 | 91.589% | 0 |
| SEARCH | 5 | 96 | 87 | 90.625% | 0 |
| WORKDAY | 5 | 91 | 82 | 90.110% | 4 |
| IMTAN | 2 | 84 | 75 | 89.286% | 0 |
| AVEDEV | 4 | 63 | 54 | 85.714% | 0 |
| TOROW | 1 | 36 | 27 | 75.000% | 0 |
| SEARCHB | 1 | 32 | 23 | 71.875% | 0 |
| PERCENTOF | 1 | 31 | 22 | 70.968% | 1 |
| RADIANS | 6 | 1517 | 1509 | 99.473% | 0 |
| SIN | 17 | 1134 | 1126 | 99.295% | 0 |
| COS | 9 | 846 | 838 | 99.054% | 0 |
| NETWORKDAYS | 2 | 95 | 87 | 91.579% | 2 |
| IMCOT | 2 | 84 | 76 | 90.476% | 0 |
| MODE | 3 | 66 | 58 | 87.879% | 0 |
| MINA | 5 | 55 | 47 | 85.455% | 0 |
| ROWS | 7 | 4031 | 4024 | 99.826% | 5 |
| IMREAL | 2 | 449 | 442 | 98.441% | 0 |
| SUBSTITUTE | 7 | 184 | 177 | 96.196% | 0 |
| UNIQUE | 8 | 123 | 116 | 94.309% | 2 |
| EDATE | 10 | 117 | 110 | 94.017% | 0 |
| ISERR | 3 | 70 | 63 | 90.000% | 0 |
| STDEV | 4 | 63 | 56 | 88.889% | 0 |
| VAR | 4 | 63 | 56 | 88.889% | 0 |
| DEVSQ | 4 | 59 | 52 | 88.136% | 0 |
| VARP | 3 | 50 | 43 | 86.000% | 0 |
| CHISQ.TEST | 3 | 34 | 27 | 79.412% | 0 |
| MAP | 2 | 17 | 10 | 58.824% | 4 |
| OCT2HEX | 3 | 163 | 157 | 96.319% | 0 |
| TRIMRANGE | 3 | 152 | 146 | 96.053% | 2 |
| OCT2DEC | 3 | 88 | 82 | 93.182% | 0 |
| MAXA | 5 | 48 | 42 | 87.500% | 0 |
| DISC | 2 | 37 | 31 | 83.784% | 1 |
| RSQ | 2 | 33 | 27 | 81.818% | 0 |
| STEYX | 2 | 33 | 27 | 81.818% | 0 |
| CHITEST | 1 | 32 | 26 | 81.250% | 0 |
| INTRATE | 2 | 28 | 22 | 78.571% | 1 |
| RECEIVED | 1 | 23 | 17 | 73.913% | 1 |
| WEEKNUM | 3 | 2755 | 2750 | 99.819% | 0 |
| COUNT | 17 | 666 | 661 | 99.249% | 1 |
| OCT2BIN | 3 | 164 | 159 | 96.951% | 0 |
| EOMONTH | 7 | 113 | 108 | 95.575% | 0 |
| ISTEXT | 7 | 101 | 96 | 95.050% | 0 |
| INTERCEPT | 3 | 56 | 51 | 91.071% | 0 |
| SLOPE | 3 | 56 | 51 | 91.071% | 0 |
| ENCODEURL | 1 | 51 | 46 | 90.196% | 0 |
| COLUMNS | 5 | 37 | 32 | 86.486% | 5 |
| DOLLARDE | 2 | 37 | 32 | 86.486% | 2 |
| DOLLARFR | 2 | 37 | 32 | 86.486% | 2 |
| STDEVA | 2 | 33 | 28 | 84.848% | 0 |
| STDEVPA | 2 | 33 | 28 | 84.848% | 0 |
| VARA | 2 | 33 | 28 | 84.848% | 0 |
| VARPA | 2 | 33 | 28 | 84.848% | 0 |
| MIN | 22 | 1065 | 1061 | 99.624% | 0 |
| AVERAGE | 17 | 666 | 662 | 99.399% | 0 |
| HEX2BIN | 3 | 200 | 196 | 98.000% | 0 |
| HEX2OCT | 3 | 200 | 196 | 98.000% | 0 |
| HEX2DEC | 4 | 185 | 181 | 97.838% | 0 |
| SUMSQ | 5 | 98 | 94 | 95.918% | 0 |
| ISBLANK | 8 | 93 | 89 | 95.699% | 0 |
| IMCSC | 2 | 84 | 80 | 95.238% | 0 |
| COT | 2 | 78 | 74 | 94.872% | 0 |
| CUMPRINC | 2 | 62 | 58 | 93.548% | 0 |
| IMDIV | 2 | 42 | 38 | 90.476% | 0 |
| IMSUB | 2 | 40 | 36 | 90.000% | 0 |
| COVARIANCE.P | 2 | 33 | 29 | 87.879% | 0 |
| COVARIANCE.S | 2 | 33 | 29 | 87.879% | 0 |
| PEARSON | 2 | 33 | 29 | 87.879% | 0 |
| COVAR | 1 | 32 | 28 | 87.500% | 0 |
| POWER | 16 | 6026 | 6023 | 99.950% | 0 |
| MID | 12 | 169 | 166 | 98.225% | 0 |
| COUNTA | 18 | 150 | 147 | 98.000% | 0 |
| LEN | 11 | 137 | 134 | 97.810% | 0 |
| ASINH | 4 | 108 | 105 | 97.222% | 0 |
| IMCSCH | 2 | 84 | 81 | 96.429% | 0 |
| IMSEC | 2 | 84 | 81 | 96.429% | 0 |
| IMSECH | 2 | 84 | 81 | 96.429% | 0 |
| REPT | 8 | 82 | 79 | 96.341% | 0 |
| SEC | 2 | 78 | 75 | 96.154% | 0 |
| VAR.P | 5 | 50 | 47 | 94.000% | 0 |
| CLEAN | 5 | 45 | 42 | 93.333% | 1 |
| VAR.S | 4 | 41 | 38 | 92.683% | 0 |
| STDEV.P | 4 | 39 | 36 | 92.308% | 0 |
| STDEV.S | 4 | 38 | 35 | 92.105% | 0 |
| LEFTB | 1 | 32 | 29 | 90.625% | 0 |
| MIDB | 1 | 32 | 29 | 90.625% | 0 |
| RIGHTB | 1 | 32 | 29 | 90.625% | 0 |
| STDEVP | 1 | 31 | 28 | 90.323% | 0 |
| MDETERM | 3 | 23 | 20 | 86.957% | 0 |
| MYFUNC | 2 | 3 | 0 | 0.000% | 3 |
| DEC2OCT | 3 | 748 | 746 | 99.733% | 0 |
| DEC2BIN | 3 | 747 | 745 | 99.732% | 0 |
| DEC2HEX | 3 | 721 | 719 | 99.723% | 0 |
| EXP | 9 | 310 | 308 | 99.355% | 0 |
| GCD | 3 | 194 | 192 | 98.969% | 0 |
| TRIM | 7 | 112 | 110 | 98.214% | 1 |
| ISREF | 8 | 98 | 96 | 97.959% | 1 |
| EXACT | 7 | 88 | 86 | 97.727% | 0 |
| IMEXP | 3 | 85 | 83 | 97.647% | 0 |
| IMLN | 3 | 85 | 83 | 97.647% | 0 |
| IMCOSH | 2 | 84 | 82 | 97.619% | 0 |
| IMLOG10 | 2 | 84 | 82 | 97.619% | 0 |
| IMLOG2 | 2 | 84 | 82 | 97.619% | 0 |
| ISLOGICAL | 5 | 83 | 81 | 97.590% | 0 |
| ISNONTEXT | 5 | 83 | 81 | 97.590% | 0 |
| ACOTH | 2 | 83 | 81 | 97.590% | 0 |
| IMCOS | 2 | 80 | 78 | 97.500% | 0 |
| CSC | 2 | 78 | 76 | 97.436% | 0 |
| EFFECT | 2 | 76 | 74 | 97.368% | 0 |
| NOMINAL | 2 | 76 | 74 | 97.368% | 0 |
| AVERAGEA | 8 | 69 | 67 | 97.101% | 1 |
| XNPV | 4 | 66 | 64 | 96.970% | 0 |
| F.TEST | 3 | 37 | 35 | 94.595% | 0 |
| CORREL | 2 | 33 | 31 | 93.939% | 0 |
| FTEST | 1 | 32 | 30 | 93.750% | 0 |
| D6 | 1 | 2 | 0 | 0.000% | 2 |
| PI | 31 | 2287 | 2286 | 99.956% | 0 |
| IMAGINARY | 2 | 449 | 448 | 99.777% | 0 |
| LCM | 3 | 194 | 193 | 99.485% | 0 |
| UPPER | 9 | 184 | 183 | 99.457% | 0 |
| SQRT | 36 | 136 | 135 | 99.265% | 0 |
| LOG10 | 5 | 136 | 135 | 99.265% | 0 |
| FIND | 8 | 132 | 131 | 99.242% | 0 |
| LN | 5 | 130 | 129 | 99.231% | 0 |
| PRODUCT | 6 | 118 | 117 | 99.153% | 1 |
| ACOS | 6 | 114 | 113 | 99.123% | 0 |
| ASIN | 6 | 113 | 112 | 99.115% | 0 |
| ATAN | 6 | 112 | 111 | 99.107% | 0 |
| INT | 6 | 109 | 108 | 99.083% | 0 |
| SIGN | 4 | 108 | 107 | 99.074% | 0 |
| LOWER | 6 | 107 | 106 | 99.065% | 0 |
| ABS | 12 | 106 | 105 | 99.057% | 0 |
| ACOSH | 4 | 106 | 105 | 99.057% | 0 |
| COSH | 5 | 106 | 105 | 99.057% | 0 |
| REPLACE | 6 | 106 | 105 | 99.057% | 0 |
| SINH | 4 | 106 | 105 | 99.057% | 0 |
| ATANH | 4 | 104 | 103 | 99.038% | 0 |
| TANH | 4 | 104 | 103 | 99.038% | 0 |
| IFNA | 4 | 98 | 97 | 98.980% | 0 |
| EVEN | 4 | 94 | 93 | 98.936% | 0 |
| ODD | 4 | 94 | 93 | 98.936% | 0 |
| COTH | 2 | 85 | 84 | 98.824% | 0 |
| CSCH | 2 | 85 | 84 | 98.824% | 0 |
| SECH | 2 | 85 | 84 | 98.824% | 0 |
| IMSIN | 2 | 84 | 83 | 98.810% | 0 |
| IMSINH | 2 | 84 | 83 | 98.810% | 0 |
| IMSQRT | 2 | 84 | 83 | 98.810% | 0 |
| ACOT | 2 | 83 | 82 | 98.795% | 0 |
| DEGREES | 8 | 81 | 80 | 98.765% | 0 |
| ISEVEN | 4 | 75 | 74 | 98.667% | 0 |
| ISODD | 4 | 75 | 74 | 98.667% | 0 |
| FACT | 5 | 72 | 71 | 98.611% | 0 |
| TEXTJOIN | 2 | 72 | 71 | 98.611% | 1 |
| MEDIAN | 6 | 69 | 68 | 98.551% | 0 |
| CUMIPMT | 2 | 62 | 61 | 98.387% | 0 |
| COMPLEX | 3 | 59 | 58 | 98.305% | 0 |
| IMABS | 2 | 56 | 55 | 98.214% | 0 |
| IMARGUMENT | 2 | 56 | 55 | 98.214% | 0 |
| IMCONJUGATE | 2 | 56 | 55 | 98.214% | 0 |
| PROPER | 3 | 56 | 55 | 98.214% | 0 |
| TBILLPRICE | 2 | 47 | 46 | 97.872% | 0 |
| TBILLYIELD | 2 | 47 | 46 | 97.872% | 0 |
| TBILLEQ | 2 | 46 | 45 | 97.826% | 0 |
| PERCENTRANK.INC | 3 | 42 | 41 | 97.619% | 0 |
| PERCENTRANK.EXC | 2 | 41 | 40 | 97.561% | 0 |
| TYPE | 4 | 41 | 40 | 97.561% | 0 |
| MODE.MULT | 3 | 34 | 33 | 97.059% | 0 |
| HARMEAN | 2 | 33 | 32 | 96.970% | 0 |
| KURT | 2 | 33 | 32 | 96.970% | 0 |
| SKEW | 2 | 33 | 32 | 96.970% | 0 |
| SKEW.P | 2 | 33 | 32 | 96.970% | 0 |
| FINDB | 1 | 32 | 31 | 96.875% | 0 |
| GEOMEAN | 1 | 32 | 31 | 96.875% | 0 |
| IMPOWER | 1 | 32 | 31 | 96.875% | 0 |
| LENB | 1 | 32 | 31 | 96.875% | 0 |
| PERCENTRANK | 1 | 32 | 31 | 96.875% | 0 |
| REPLACEB | 1 | 32 | 31 | 96.875% | 0 |
| MULTINOMIAL | 2 | 29 | 28 | 96.552% | 0 |
| GELU | 1 | 4 | 3 | 75.000% | 0 |
| BROUNDTO | 1 | 3 | 2 | 66.667% | 1 |
| MYFUNC2 | 1 | 1 | 0 | 0.000% | 1 |
| _XLL.DSSDATA | 1 | 1 | 0 | 0.000% | 1 |
| FLOOR.PRECISE | 4 | 1488 | 1488 | 100.000% | 0 |
| CEILING.PRECISE | 4 | 1486 | 1486 | 100.000% | 0 |
| ISO.CEILING | 4 | 1486 | 1486 | 100.000% | 0 |
| CONCAT | 13 | 630 | 630 | 100.000% | 0 |
| NA | 94 | 291 | 291 | 100.000% | 0 |
| XOR | 4 | 183 | 183 | 100.000% | 0 |
| WEEK | 2 | 168 | 168 | 100.000% | 0 |
| BESSELK | 2 | 150 | 150 | 100.000% | 0 |
| BESSELI | 2 | 146 | 146 | 100.000% | 0 |
| ERF | 3 | 100 | 100 | 100.000% | 0 |
| MROUND | 4 | 94 | 94 | 100.000% | 0 |
| VSTACK | 2 | 92 | 92 | 100.000% | 0 |
| ERF.PRECISE | 4 | 78 | 78 | 100.000% | 0 |
| ERFC.PRECISE | 4 | 78 | 78 | 100.000% | 0 |
| ERFC | 3 | 77 | 77 | 100.000% | 0 |
| DELTA | 3 | 63 | 63 | 100.000% | 0 |
| GESTEP | 2 | 62 | 62 | 100.000% | 0 |
| HSTACK | 3 | 58 | 58 | 100.000% | 0 |
| EXPAND | 2 | 55 | 55 | 100.000% | 0 |
| COMBIN | 3 | 54 | 54 | 100.000% | 0 |
| DCOUNT | 1 | 48 | 48 | 100.000% | 0 |
| DCOUNTA | 1 | 48 | 48 | 100.000% | 0 |
| DAVERAGE | 2 | 45 | 45 | 100.000% | 0 |
| DPRODUCT | 1 | 44 | 44 | 100.000% | 0 |
| QUOTIENT | 1 | 43 | 43 | 100.000% | 0 |
| MODE.SNGL | 3 | 41 | 41 | 100.000% | 0 |
| DMAX | 1 | 41 | 41 | 100.000% | 0 |
| DMIN | 1 | 41 | 41 | 100.000% | 0 |
| DSTDEV | 1 | 41 | 41 | 100.000% | 0 |
| DVAR | 1 | 41 | 41 | 100.000% | 0 |
| DVARP | 1 | 41 | 41 | 100.000% | 0 |
| DSUM | 1 | 40 | 40 | 100.000% | 0 |
| DSTDEVP | 1 | 40 | 40 | 100.000% | 0 |
| DGET | 1 | 36 | 36 | 100.000% | 0 |
| COMBINA | 1 | 36 | 36 | 100.000% | 0 |
| COUNTBLANK | 6 | 33 | 33 | 100.000% | 0 |
| WRAPCOLS | 2 | 29 | 29 | 100.000% | 0 |
| BYCOL | 2 | 18 | 18 | 100.000% | 0 |
| TRUE | 5 | 15 | 15 | 100.000% | 0 |
| FALSE | 5 | 13 | 13 | 100.000% | 0 |
| DOLLAR | 1 | 11 | 11 | 100.000% | 0 |
| SERIESSUM | 1 | 8 | 8 | 100.000% | 0 |
| TEMPERATURE | 1 | 4 | 4 | 100.000% | 0 |
| ISOMITTED | 1 | 3 | 3 | 100.000% | 0 |
| MYFUNCTION | 1 | 2 | 2 | 100.000% | 0 |
| SUMM | 1 | 2 | 2 | 100.000% | 0 |
| XYZ | 1 | 2 | 2 | 100.000% | 0 |
| CRAPNAME | 1 | 1 | 1 | 100.000% | 0 |
| DDE | 1 | 1 | 1 | 100.000% | 0 |
| MYLAMBDA | 1 | 1 | 1 | 100.000% | 0 |
| DP | 1 | 1 | 1 | 100.000% | 0 |
| _XLUDF.SUM | 1 | 1 | 1 | 100.000% | 0 |
| NOSUCHFUNCTION | 1 | 1 | 1 | 100.000% | 0 |
| HELLO | 1 | 1 | 1 | 100.000% | 0 |
| SHEETS | 1 | 1 | 1 | 100.000% | 0 |
| AREA | 1 | 1 | 1 | 100.000% | 0 |
| COUNTWORDS | 1 | 1 | 1 | 100.000% | 0 |

### Slices

40711 slices: p50 0.0 ms, p99 162 ms, max 15318 ms.

