# Decisions

Every answer and default, with its date. Newest section last.

## 2026-11-01: Planning

**Answered by you**

| Question | Answer |
| --- | --- |
| Which UI repo? | `CashGear.React.UI` (`@cashgear/ui`) |
| Saving an opened xlsx with content the app can't edit | Keep it intact, byte for byte |
| Formula engine | Wrap an existing engine behind a trait; Phase 1 benchmark picks |
| v1 features beyond editing, formulas, formatting and sheets | Sort, filter, tables, conditional formatting, data validation, charts, pivot tables. Print and PDF come after v1 |
| Arabic and RTL | Arabic and mixed text display correctly; grid stays left to right |
| Desktop core | Same WASM as web; a native host only if Phase 9 benchmarks call for it |
| Collaboration | None; local files only, no server |
| Missing UI components | Create them in `CashGear.React.UI` |
| Git in this repo | Push everything to `main` |
| Git in `CashGear.React.UI` | Use the existing branch; create no branches |
| Theme | Light and dark mode are both supported |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| Name | CashGear Sheet, crate prefix `cgs-` | Matches the sibling apps |
| File format | `.xlsx` is the save format; no private format | Crash-recovery data is kept separately |
| Sheet grid location | Canvas grid, formula bar and cell editor stay in this repo | They are tied to the engine's data protocol |
| Saving method | Patch source byte spans instead of re-serializing parts | Unknown elements and attributes survive automatically |
| Cell storage | The formula engine is the only cell store | Avoids two copies of every cell |
| Undo | Our command stack owns it, not the engine | Formualizer's engine undo has open bugs (#303, #412) |
| Grid drawing | Tile worker draws; main thread composites | Drawing and recalculation never block input |
| Dark mode colours | `dk1`/`lt1` theme colours and automatic colours swap for display only | Default black text stays readable, and the file never changes |
| Targets until Phase 0 confirms | Windows 10/11 x64 first, macOS second; Chrome and Edge first | Same as CashGear Video |
| Licences | No GPL, AGPL or LGPL in anything shipped | Same as CashGear Video |

**Evidence read for the choices above (nothing compiled yet)**

| Item | Finding |
| --- | --- |
| Formualizer 0.13.0 | Dirty-graph recalculation; row and column insert with names; published WASM 10.0 MB raw, 3.6 MB compressed; moved from 0.10.1 to 0.13.0 in 25 days |
| IronCalc 0.9.1 | `evaluate()` recomputes every formula; row and column insert; published WASM 2.5 MB raw, about 1 MB compressed |
| Both engines | Neither can yield during a calculation in browser WASM today |
| `umya-spreadsheet` 3.1.1 | Reads and writes xlsx, with WASM through a `js` feature; its docs don't mention keeping unknown parts |
| `rxls` 0.1.x | Keeps untouched parts byte for byte, but can't insert or delete rows or columns |
| `CgChart` | SVG, with bar, line, area, pie and donut; no scatter |

## 2026-11-01: Phase 0

**Answered by you**

| Question | Answer |
| --- | --- |
| Targets | Windows 10/11 x64 first, macOS second. Chrome and Edge first, Safari and Firefox best-effort. CI builds the desktop app on Windows only for now |
| CI | GitHub Actions in `MohamedSadat/cashgear-sheet`, and the repo stays public (free minutes; any later licensing protects official builds only) |
| Reference test machine | This PC: Xeon E5-2680 v3 (12 cores at 2.5 GHz), GTX 1650, 32 GB RAM, Windows 10 Pro 22H2, WebView2 156. All performance exit criteria are measured on it; CI checks function only |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| Toolchain pins | Rust 1.99.1, Node 24, pnpm 12.11.2, wasm-bindgen =0.2.129, Tauri 2.12 (`@tauri-apps/cli` and `api` 2.12.4), React 19.3.1, Vite 8.3.5, TypeScript 6.0.3, Vitest 4, Playwright 1.64.1, comlink 4.4.2, binaryen 123 | The stack CashGear Video already proved on this machine and in CI |
| Package names | npm `@cgs/*`, Tauri identifier `com.cashgear.sheet`, binary `cgs-desktop` | Same pattern as CashGear Video |
| Product licence | Proprietary: crates `publish = false` with `LicenseRef-CashGear-Proprietary`, npm packages `private`, no LICENSE file | Same as CashGear Video. Phase 9 decides free or licensed |
| Licence allow-list | Rust: MIT, MIT-0, Apache-2.0, BSD-2-Clause, BSD-3-Clause, MPL-2.0, Zlib, ISC, 0BSD, CC0-1.0, Unicode-3.0, Unlicense. npm adds Apache-2.0 WITH LLVM-exception, BSL-1.0 and BlueOak-1.0.0. GPL, AGPL, LGPL, SSPL and BUSL are denied | CashGear Video's list. `@cashgear/ui` (UNLICENSED, first party) is a named exception |
| WASM size budget | 4 MB gzipped, checked by `scripts/build-wasm.mjs` on release builds | Leaves room for Formualizer's published 3.6 MB. Phase 1 asks this question properly |
| `ViewBlock` layout | 24-byte little-endian header, then 24-byte cells, then a UTF-8 heap. A cell holds text offset, text length, fill and text colour (`0xRRGGBBAA`, alpha 0 means no fill or automatic), plus 8 bytes kept for Phase 2 | The plan fixes the cell size and heap. Phase 2 can change the header version |
| Tile sizes | 64 rows by 16 columns at 64 by 20 CSS pixels | The plan's tile, with cell sizes close to Excel's default |
| Theme storage | `localStorage` key `cgs-theme`, holding `system`, `light` or `dark` | Per-device convenience. It renders correctly without storage |
| Static hosting headers | Only Vite (dev and preview) and Tauri set COOP/COEP for now | Hosting is a Phase 9 question, and that host must set the same headers |
| Formatting scope | Prettier skips `docs/` and `CLAUDE.md` | The plan and logs are written by hand, and their tables stay compact |

## 2026-11-01: Phase 1

**Answered by you**

| Question | Answer |
| --- | --- |
| Which real workbooks join a private test set? | A folder of yours, path to come before the final measurement. It is read on this PC only and stays out of git. Only per-function counts reach the repo, with no file names or cell values. Excel COM opens these files read-only and never saves them |
| Fork if upstream refuses patches? | Yes, when needed. Open issues and PRs upstream from your GitHub account. If one is refused or still unmerged at the end of the phase, pin the patch as a public fork under `MohamedSadat` at an exact commit, and rebase it whenever a new upstream version is taken |
| WASM size budget | 5 MB gzipped, checked by `scripts/build-wasm.mjs` on release builds |
| Exit targets | As proposed: 200+ files; 99.5% of formula cells match Excel's stored values (relative 1e-9, skipping volatile and external), pooled across non-generated files with no file over 10% of the pool; slice p99 of 50 ms or less on the reference machine |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| Excel COM and downloaded files | COM opens only generated workbooks and your private ones, read-only, with macros and external links off. Downloaded public files are only parsed and compared against their stored `<v>` | Public files are untrusted, and the accuracy test doesn't need Excel to open them |
| Timing noise | Measure while the PC is in normal use and repeat noisy runs, rather than asking you to keep it idle | Timing is only needed at the end of the phase |
| Whose stored values count | A file must have `fileVersion appName="xl"` with a `rupBuild`, and not be set to manual calculation or recalculate on load | Other writers store zeros or nothing |
| Values that can't be reproduced | `NOW`, `TODAY`, `RAND`, `RANDBETWEEN`, `RANDARRAY`, `INFO`, `CELL`, external references, data tables and outside data (`CUBE*`, `RTD`, `WEBSERVICE`, `GETPIVOTDATA`, `STOCKHISTORY`, `IMPORTDATA`) load as their stored value and aren't scored | What reads them still sees what Excel saw. `OFFSET` and `INDIRECT` are scored, because their inputs fix their value |
| Loading for the comparison | One neutral xlsx reader for both engines, not their importers | Phase 2's reader feeds the engine the same way, and an importer that keeps saved spills can match without calculating |
| Documents and function test suites | Reported separately, split by file name in `bench/report.mjs` | The target pools both, and they behave very differently. The split is a judgement |
| Structural test | 200 episodes of 5 random edits per seed, on a generated seed with no macros or arrays, for 2 seeds. Formula text compares without spacing or `#REF!` spelling | One divergence can't taint other episodes |
| WASM stack for the bench | 8 MB for both engines | The default 1 MB is tight for deep chains. Phase 2 sets the shipped value |
| `CalcEngine` additions | `begin_load`, `finish_load`, `recalc_all`, and `first` is `&[SheetRect]` | Names and tables must exist before cells, and Ctrl+Alt+F9 needs a way in |
| Forks | `MohamedSadat/formualizer` branch `cashgear/0.13.0` at `7efd060`, and `MohamedSadat/IronCalc` branch `cashgear/0.9.1` at `c91da93`, pinned in the root `Cargo.toml` and allowed in `deny.toml` | `main` must build from public code, and the adapters need the patches. The loser's pin and adapter go when you decide |

**Waiting for your decision: which engine, and what happens to the accuracy target**

Neither engine reaches 99.5%, so by the phase's rule this stops here. Full results are in [phase1-report.md](phase1-report.md).

| | Formualizer | IronCalc |
| --- | --- | --- |
| All Excel-saved files, 10% cap (target 99.5%) | 86.05% | 94.67% |
| Real documents, 10% cap | 98.22% | 98.51% |
| Slice p99 (target 50 ms) | 13.9 ms | 99.9 ms |
| Structural edits matching Excel | 2,000 of 2,000 | 484 of 2,000 |
| A later edit on 1,000,000 formulas, p99 | 1.3 ms | 1,297 ms |
| 10,000 lookups or 6,000 aggregates | Seconds at most | Over 5 minutes |
| Corrupts data on row insert | No | Yes: text that looks numeric turns into numbers |
| WASM, gzipped | 2.21 MB | 0.62 MB |

Recommendation: Formualizer. Its misses are mostly specific functions, which can be fixed one at a time on the fork. IronCalc's are in how it works: no dependency graph, scanning lookups, and structural edits that corrupt data.
