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

## 2026-11-02: Phase 1 decision

**Answered by you**

| Question | Answer |
| --- | --- |
| Neither engine reaches 99.5%. Which engine, and what happens to the target? | Formualizer, on our fork. `phase-1` is tagged with 98.22% on real documents as the result. 99.5% on real documents, including your private workbooks, moves to Phase 4's exit, before the MVP |
| When do your private workbooks join? | Later, by Phase 4, before the accuracy target is judged |

**The evidence**

| | Formualizer | IronCalc |
| --- | --- | --- |
| All Excel-saved files, 10% cap (target 99.5%) | 86.05% | 94.67% |
| Real documents, 10% cap | 98.22% | 98.51% |
| Slice p99 (target 50 ms) | 13.9 ms | 99.9 ms |
| Structural edits matching Excel | 2,000 of 2,000 | 484 of 2,000 |
| A later edit on 1,000,000 formulas, p99 | 1.3 ms | 1,297 ms |
| 10,000 lookups or 6,000 aggregates | Seconds at most | Over 5 minutes |
| Text that looks numeric, after a row insert | Unchanged | Turned into numbers |
| WASM, gzipped | 2.21 MB | 0.62 MB |

Formualizer's misses are mostly specific functions, which can be fixed one at a time. IronCalc's gaps are in how it works. Full results are in [phase1-report.md](phase1-report.md).

**What follows from it**

| Item | Result |
| --- | --- |
| Engine pin | `=0.13.0` crates, taken from `MohamedSadat/formualizer` branch `cashgear/0.13.0` at `7efd060` by `[patch.crates-io]` in the root `Cargo.toml`. A new upstream release is taken by rebasing the fork on it |
| Upstream PRs | Opened from your account, as you agreed: table references [#556](https://github.com/PSU3D0/formualizer/pull/556), smaller Excel differences [#557](https://github.com/PSU3D0/formualizer/pull/557), time-boxed evaluation [#558](https://github.com/PSU3D0/formualizer/pull/558). The fork stays pinned until they land |
| IronCalc | Adapter, pin and own-importer comparison removed. Commit `720a89e` is the last that can run it. The public fork `MohamedSadat/IronCalc` is no longer used, and deleting it is your call |
| `TEXT` format codes | Formualizer has 991 misses here, so `cgs-numfmt` should replace `TEXT` as a custom function in Phase 2 |
| Scalar functions over ranges | The largest known gap, tracked upstream as PSU3D0/formualizer#529 |

## 2026-11-02: Phase 2

**Answered by you**

| Question | Answer |
| --- | --- |
| Largest file that must open smoothly | A 25 MB file (about 3M cells) is showing and scrollable within 10 s on the reference machine. A 100 MB file of mostly values opens within 60 s with progress showing. A file that runs out of memory gets a clear message. Phase 9 deals with the 4 GB limit |
| Calibri fallback | Bundle Carlito (OFL): four faces as WOFF2 with the notice, used only when Calibri is missing. Aptos has no free match, so it falls back to Carlito too |
| Recent files | Now: the last 10, in the File menu, on this device only. Safari and Firefox get no list, because they have no file handles |
| Explicit colours in dark mode | Fills stay as stored. Text changes on screen only when its contrast against its own background is under 3:1. Display only; it never reaches the saved file |
| Selection and formula bar (new) | In Phase 2, read-only: selection by click, drag and arrow keys, a name box, and a formula bar. Phase 4 makes them editable |
| Regional conventions (new) | Built-in date and number formats follow the browser's language through `Intl`. Tags inside a format code always win, including Arabic names and Hijri dates |
| Your private workbooks (new) | Now, not by Phase 4. Same rules as Phase 1: read on this PC only, and only counts reach the repo. The folder was given in the session and stays out of this repo. No screenshots of them are taken, because those would leave this PC |
| Exit thresholds (new) | Values: every cell matches `calamine` across the test set, with any difference explained. Display text: 99.5% matches Excel's own text over COM, with misses listed, on a generated workbook holding every built-in format and every format code found in the public files. The public files themselves never open in Excel. Scrolling: on a 1M-row sheet on the reference machine, 95% of frames within 17 ms and none over 50 ms, and a fast scrollbar drag can show empty cells until 100 ms after it stops |

**Evidence for those answers**

| Item | Finding |
| --- | --- |
| Phase 1's reader in Chrome | 14.0 MB of values: 7.1 s, 549 MiB. 15.5 MB with 1,000,002 formulas: 14.4 s, 936 MiB. By simple scaling, 100 MB would take 50 to 100 s and 4 to 6 GiB |
| Public test files (3,361 with styles) | Default font: Calibri 78%, Arial 10%, Aptos Narrow 6%. Fixed black font colour: 12.6%. A locale tag in a format code: 1.5%. Arabic text in shared strings: 4 files |
| This PC | en-US, Calibri installed, Aptos not, Excel 16.0 build 19127 |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| `@cashgear/ui` | Build only `CgStatusBar` and `CgSlider` now, and bump the pin. The rest of Track U waits for the phases that need it | No new commits on the branch since `96d2499`, and Phase 2 uses only these two |
| What "open" means | The active sheet shows correct values and scrolls. Other sheets carry on loading behind it, and a tab that isn't ready shows progress | The plan's lazy per-sheet parsing |
| File types | `.xlsx`, `.xlsm`, `.xltx` and `.xltm` open, and macros never run. `.xls`, `.xlsb`, `.ods`, password-protected and damaged files get a clear message, not a crash | The plan leaves other formats until after v1. The test set has 25 damaged files |
| Graph build after opening | Starts in the engine worker once tiles around the viewport are ready, one sheet at a time. Tiles asked for meanwhile wait for the current sheet | `build_graph_for_sheets` lets the build stop between sheets, though not within one |
| Not shown in Phase 2 | Conditional formatting and table banding wait for Phase 6, and charts and images are placeholders until Phase 7. Rich text shows in one style until a later phase needs it | The plan's scope |
| Other fonts | Only Carlito is bundled. Any other missing font, including for Arabic, uses the browser's fallback | Windows and macOS ship Arabic fonts |
| Formulas with no saved value, and `fullCalcOnLoad` | Show what the file stores: blank or the old value, until Phase 4 schedules calculation | "Opening must not recalculate" is an exit criterion, and files Excel saved always have values. Files from other writers can look empty until then |
| Rows with no stored height | Use the sheet's default, even if wrapped or large text would need more | Excel stores the height of rows it fitted. Only files from other writers are affected. Fitting needs text measurement the engine doesn't have, so it comes with row resizing in Phase 5 |
| Contrast rule in dark mode | Text under 3:1 moves to 4.5:1, not just to 3:1, keeping its hue where it can. Automatic text on a pale fill stays black. Borders follow the same rule against the sheet's background | 3:1 was the trigger you chose. Moving only that far leaves grey text that barely reads, and fixed black borders would vanish |
| Fills the grid can't draw yet | Pattern fills draw as a flat mix of their two colours, and gradients as their first colour | Excel's patterns are tiny and rare. Gradients wait until a phase needs them |

**What changed from the plan's design, and why**

| Item | Plan | Built | Reason |
| --- | --- | --- | --- |
| Offset lookups | Lazy Fenwick tree over `Float64Array` | Sorted overrides with running totals (`Axis` in `packages/ui/src/grid/geometry.ts`) | Costs nothing until a row differs, and the grid is read-only until Phase 5 resizes rows, when it can be rebuilt or replaced |
| `ViewBlock` | Header, cells, heap, with 8 spare bytes per cell | Version 2: a tile describes itself, with its row and column sizes, its own table of styles and borders, and extra records for merged and overflowing text. Colours are resolved for one theme | Drawing then needs nothing but the tile, and the colour rules live in Rust where they are tested. Switching theme asks for tiles again, with the old ones showing until the new ones arrive |
| `CalcEngine` | `load_batch` of owned cells | Adds `begin_sheet`, `load_rows` (borrowed values, in row order), `load_formula`, `mark_spill`, `finish_sheet` and `prepare_step` | Rows go straight into Formualizer's Arrow builder. A 15.5 MB file with 1M formulas now takes 356 MiB and 4.6 s to show, against 936 MiB and 14.4 s through `load_batch` in Phase 1 |
| Spills when the graph is built | Re-armed by `adopt` | Re-armed only by the first real edit | Otherwise preparing the graph would blank saved array results and calculate them |
| Large sparse sheets | Not covered | Stored cell by cell when the area is over 4M cells and more than 32 times the cells it holds | Formualizer's dense builder costs bytes for every cell in the area, so a value at `XFD1048576` would ask for terabytes |
| Column widths | Not covered | Excel's measured digit widths for 15 fonts at 7 sizes, in `cgs-model` | Hinting makes them uneven, so a formula gets some sizes wrong. Measured over COM on this PC. Aptos isn't installed here, so its width comes from files Excel saved |

**Found on the way**

| Item | Finding |
| --- | --- |
| Excel and lowercase exponents | A file's format code `0.00e+00` is refused, but `TEXT` accepts it, so `cgs-numfmt` allows it only for `TEXT` |
| Calendar `0E` | Excel shows it as an East Asian lunisolar calendar. Um Al-Qura is `0x17`. Both use the tabular Hijri calculation here for now, which is a known gap |
| This PC's Office | The UI language is Arabic on en-US Windows, so `Range.NumberFormat` refuses `[Red]` and `TEXT` wants colour names in Arabic. The format comparison works around it by opening files |

## 2026-11-04: Phase 3

**Answered by you**

| Question | Answer |
| --- | --- |
| What does Save do on a file opened from disk? | Overwrite, like Excel, through an atomic swap, with a save report. A new blank workbook, or a file with no writable handle, goes to Save As |
| Keep macros when saving `.xlsm`, and how is it proved? | Keep them, byte for byte. To prove it, Excel runs one macro over COM, with macros on only for that session and only in a fixture whose macro we generated. This is the one exception to Phase 1's rule. Every other file stays macros-off |
| Recalculate on open in saved files? | Only when cells changed: a save with edits sets `fullCalcOnLoad`, and a save with no edits leaves `workbook.xml` untouched |
| Copy the source to keep it readable? | On the first edit, not on open. Viewing costs nothing. The first edit copies the source to OPFS on web and app temp on desktop, and later saves read that copy. If the source has already changed by then, you are told at once. Copies are deleted on close or at the next launch |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| Save As and file types | A workbook keeps its own type: `.xlsm` stays `.xlsm`, and the dialog offers nothing else | Excel refuses a file whose name and contents disagree, and turning macros into `.xlsx` is a separate feature |
| Save with nothing changed | Save does nothing and stays disabled. Save As writes a file identical to the source | There's nothing to write, and it's the simplest proof that nothing is lost |
| What calculates | An edit through `edit_cells` calculates at once, in one call, and returns exactly which cells changed. Phase 4 brings this into slices | Saving needs the exact list, where a slice only knows which sheets moved |
| A cell in a table's header | The table part and the cell are kept in step. An empty or duplicate header gets a name of Excel's form, and a number or formula becomes text | Excel refuses a file where they disagree |
| A new cell's style | The row's if it has `customFormat`, or else the column's, as Excel does | The cell would otherwise lose the look of its row or column |
| Errors in saved cells | Only the classic errors go in as `t="e"`. Any other error result, such as `#SPILL!`, is left with no value for Excel to work out, and as a constant it's stored as text | Excel refuses these as plain error cells, because it only stores them with rich value metadata. The matrix found this |
| Formulas that read another workbook | They keep their saved value and aren't given to the engine, and the formula bar still shows them. Their bytes aren't touched | The engine refused them and took the whole sheet with them. Excel also keeps the saved value until links are updated |
| A workbook whose formulas the engine can't ready | It stays open and showing, and editing says why it can't | Getting ready for a first edit shouldn't take away a workbook that opened fine |
| Damaged parts | A sheet whose XML never ends can't be patched, so that save is refused with a message and the file stays as it is. Saves that don't touch it still work | Patching something unreadable would be guessing |
| Parts nothing links to | A new part takes a free name instead of the usual one, and a stale content type for that name is replaced | The file holds them, and they aren't ours to replace |
| Pivots after a cell edit | No `refreshOnLoad` | Excel leaves a pivot alone after an edit too. `refresh_pivots_on_load` is there for Phase 4's structural edits and Phase 8 |
| Excel's file metadata | `docProps` and the thumbnail stay as they are, so a file's "last modified by" doesn't name us | Only what an edit requires changes |
| Report | A status bar line, and a report in a `CgMessageBox` when it's clicked | A dialog after every save would get in the way |
| New workbook's shortcut | Ctrl+Alt+N | Browsers keep Ctrl+N, and a page never sees it |
| Validator | `DocumentFormat.OpenXml` 3.3.0 (MIT) in `tests/validator`, test-only, against `Microsoft365` | The current schema, so nothing passes for being too new. A .NET test tool never reaches the app's dependency tree |
| CI | `pnpm test:save` runs the fixtures through the save check and the validator. The COM matrix runs on the reference machine | GitHub's runners have .NET and no Excel |

**What changed from the plan's design, and why**

| Item | Plan | Built | Reason |
| --- | --- | --- | --- |
| Raw copy | `zip`'s `raw_copy_file` | Our own copy of each entry's bytes, from its local header up to the next entry | It needs no seekable output, so a save can stream to the browser, and it can't lose a header field the crate doesn't model. Output for an unedited file is byte-identical |
| Reading XML on the save path | `quick-xml` | A small tokenizer in `scan.rs` whose tokens are exact byte spans | Rebuilding tags from parsed events can't promise the same bytes |
| Writer's input | Values passed in | Patches say which cells differ, and values come from the engine when saving | The engine stays the only cell store |
| Styles | Appended at save | The whole of `styles.xml` is held and grown as styles are added | The model reads new styles back from it at once, and the part is small |

**The `refreshOnLoad` spike**

`refreshOnLoad="1"` on the cache definition is enough for Excel to bring a pivot up to date by itself. After changing a source cell from 10 to 1000, the file opened with the pivot's total at 1090, with no repair prompt and with the cache's records left as they were, so regenerating records isn't needed. Excel does this on a read-only open too. A pivot that only has the flag and no data change comes out the same. The very first read over COM right after opening came back empty twice in about ten opens, and every later read was right, so Excel seems to finish the refresh just after `Workbooks.Open` returns. Excel's Protected View can't be driven over COM, so what a downloaded file does until someone enables editing wasn't measured.

## 2026-11-05: Phase 4

**Answered by you**

| Question | Answer |
| --- | --- |
| Undo depth | 100 steps, with the oldest dropped once pre-images pass 256 MiB. Undo keeps working across a save, as in Excel |
| Manual calculation in v1 | Yes. A file's manual setting is honoured, and there is a menu toggle, F9, Shift+F9, Ctrl+Alt+F9 and a "Calculate" note in the status bar. The setting is only written to a file when you change it |
| Formatting kept when pasting from Excel | Values plus common formats read from Excel's HTML: number format, font name, size, bold, italic, underline, colour, fill, alignment, borders and merges. Formats are appended, so existing style indices stay valid. TSV when there is no HTML. Copying inside the app keeps formulas and styles |
| Autosave and crash recovery | Recovery only, and never the real file. Each command goes to a journal in OPFS or app data, beside a kept copy of the source, and the next launch offers to restore after a crash. Recovery data goes on save, on a clean close or when discarded |
| Scoring the 99.5% target | As Phase 1: 10% per-file cap, the same file split, relative 1e-9, the same skipped functions, in Chrome WASM, with private files in the pool. If it stalls below 99.5%, stop and show what's left, and don't change the scoring |
| Your private workbooks | Keep the 5 for now, scored and reported separately. You can add more to the folder before the final measurement |
| Engine fixes upstream | Fork only for now. Nothing public goes out this phase, and #556, #557 and #558 stay as they are. The report lists the fixes so you can decide on PRs later |
| 1,000 random edits | 2 seeds, compared with Excel over COM after every edit, mixing typing, formulas, fill, paste, row, column and sheet changes, undo and redo, on a seed with names, tables and arrays |
| Input under 100 ms | Keystroke or arrow key to painted result, p99, on the reference machine while 1,000,000 formulas recalculate. Commands queue behind at most one engine slice. If an undividable step runs over, the editor and selection stay live on the main thread, and the longest wait for the engine is reported separately |

**Defaults and design choices, for you to overrule**

| Choice | What it does, and why |
| --- | --- |
| Typed formulas | Always English names with comma separators, as files store them. Numbers and dates typed into cells follow the browser's language. Localised formulas belong with Phase 9's translations |
| What typing means | Read off Excel: `bench/run-excel-input.mjs` puts 163 odd spellings through Excel and `parse_input`. `1,23` is text, `1,2345` is 12,345, `%5` is 5%, `1:60` is a plain 0.0833. Over COM, `+A1` stays text where typing makes it a formula, so those 10 go by what typing does |
| Circular references | They come to 0 with the iterative setting off, and calculation carries on. Excel warns, which comes with Phase 5's dialogs. The edit fuzz stops an episode at a circle, as Excel's own answer then depends on the order it saw things |
| Tables | No auto-expanding when typing beside a table until Phase 6's table work. Headings are text and unique, renaming one follows into formulas, and deleting a table's headings without its data is refused, as in Excel |
| Arrays | Changing part of an array is refused, as in Excel. A new formula that returns many values spills and saves as a dynamic array with `xl/metadata.xml`, which Excel 2016 reads as a fixed one |
| Shift+F9 | Calculates what the sheet needs, not every formula on it |
| Saving during a calculation | The save finishes the calculation first, in slices, so a file never holds half of one |
| Opening | Volatile functions run after opening without marking the workbook changed. `fullCalcOnLoad`, or formulas with no saved value, start a full calculation unless the file is manual |
| CSV | A CSV opens as a new workbook, UTF-8 or else Windows-1252, with the delimiter worked out from the first lines, and saving it asks where. Export writes the sheet showing as its displayed text, as Excel's own CSV save does, with a byte-order mark so that Excel reads it as UTF-8 |
| Pasting from elsewhere | HTML gives values and the common formats, not formulas, since Excel's HTML holds results. Pasting within the app keeps formulas. A row or column's own style is only followed onto new cells up to 200,000 of them |
| Clipboard reading | Ctrl+V uses the browser's paste event, which asks for no permission. Paste from the menu has to ask the browser, and says to press Ctrl+V if refused |
| A key never waits for the engine | Enter moves the selection at once and the command follows in order. A formula the engine refuses reopens the editor on it with the reason in the status bar, so nothing has to be typed again |
| Inserting cells | Ctrl+- and Ctrl+Shift+= work on whole rows and columns. Shifting part of a sheet sideways, with its dialog, isn't in Phase 4's task list and comes with Phase 5's menus |
| Saving structural edits | A first pass rewrites each changed sheet into its final coordinates, and the Phase 3 patcher then applies cell changes to the result. Untouched parts stay raw bytes, so a file with no structural edit goes through exactly as before |
| Sheet numbers | A sheet keeps its number for life, and a deleted one stays as a hidden tombstone until the workbook closes, so undo and the tile cache need no renumbering. Tab order is a separate list |
| Undo and saving | Undoing back past what was saved shortens the list of book changes against the same file rather than adding inverses, so a save after undo is as if the change was never made |
| Extensions in a sheet | `sqref` and `xm:f` inside conditional formats, validation and sparklines follow moved cells. Any other extension stays as it was, and the save report names it |
| Recovery on the desktop | The native side holds the working copy, and a second copy goes to OPFS with the journal, so that web and desktop recover the same way |
| Comparing dynamic arrays | The Excel on this PC is 2016 (16.0), with no dynamic arrays, XLOOKUP, LET or `Formula2`. The fuzz can't compare those, so they're covered by contract tests and by opening saved files |

**Engine fixes in our fork, for you to decide on upstream PRs later**

Nothing public went out. These are on `MohamedSadat/formualizer`, branch `cashgear/0.13.0`:

- Sheet names that look like an R1C1 reference, such as `R23`, are quoted when printed.
- A table reference to a deleted sheet gives `#REF!` instead of a panic.
- A fixed array can be declared with family execution on.
- Volatile formulas can be marked dirty after opening.
- The further fixes from the accuracy work are listed in `PROGRESS.md`, under Phase 4.

**Answered by you at the end of the phase**

| Question | Answer |
| --- | --- |
| The 99.5% target came out at 99.350% with the 10% cap, 99.435% pooled. How should the phase end? | Accept 99.35% and tag `phase-4`, with the target recorded as not met and each remaining cause listed. The scoring stays as it was, and formulas calling unknown functions in VBA files aren't kept at their saved values to raise it |

## 2026-11-05: Phase 5

**Answered by you**

| Question | Answer |
| --- | --- |
| Ribbon tabs | Working tabs only: File, Home, Insert, Formulas and View, in Excel's order. Data joins in Phase 6, and others when they have real commands |
| Key tips | Later, after v1, since browsers claim keys like Alt+F and Alt+E. Phase 5 adds Excel's Ctrl shortcuts |
| Format Cells | Five tabs: Number, Alignment, Font, Border and Fill, including pattern fills, which the grid draws properly. Protection waits for sheet protection, and gradients stay preserved but can't be created |
| Cell Styles gallery | Moved to Phase 6, beside the table style gallery. Phase 5 keeps a file's named styles intact |
| Rich text | Draw each run's own style, with no editing of runs. Retyping a rich cell makes it plain, and an untouched cell's runs stay byte for byte |
| Row heights | Rows refit by themselves, as in Excel. A row with no fixed height refits when wrap, font size or typed text changes it, and saves without `customHeight`. Rows with no stored height from other writers' files fit when they first show. A row you've set stays as set |
| Visual review | I compare generated workbooks drawn by Excel and by the app and list each difference. You then get a side-by-side page and sign off before `v0.1.0-mvp` is tagged |
| Your private workbooks in the round-trip | Temp copies, counts only. Formats are applied in the app and saved to a temp file. Excel opens it and reads the formats back, re-saves only that temp copy, and our reader reads it back. Originals open read-only, and temp files are deleted afterwards |

**Defaults picked during the phase**

| Choice | Default |
| --- | --- |
| Font name and size boxes | A text box with a list (`datalist`) inside `CgRibbon`, so any name or size can be typed, as in Excel |
| Column auto-fit | Measured text plus 7 px, which matches Excel's double-click on Calibri 11 at 100% |
| Row auto-fit | Excel's measured AutoFit heights for 10 common fonts at 12 to 72 points, and 1.75 times the size for anything else or smaller |
| A whole column's font | Also sets the sheet's default row height, as Excel does, so `sheetFormatPr` changes with it |
| Theme tints | Kept to 32767ths, as Excel stores them, so a saved tint reads back the same through Excel |
| Text on a fill in dark mode | Keeps its stored colour when it reads well on the fill, as the fill itself stays as stored. Otherwise the usual dark-mode swap applies |
| Typing into a cell formatted as Text | Stays text, formulas included, as in Excel. Numbers already in such a cell are drawn as text |
| Shifting cells | Refused where it would cut a table, a merged area or an array, with Excel's reasons. Formulas, names and the ranges the file keeps follow the shift |
| Find | Up to 10,000 results in Find All. Wildcards `*`, `?` and `~` as in Excel |
| Pattern fills | Drawn from 4×4 bitmaps copied from Excel's own pixels at 100% |
| A table heading renamed | The table's calculated column and totals formulas follow the new name, or Excel refuses the file. A broken fixture keeps that checked |

**Signed off by you, 2026-11-05**

The visual review against Excel: three generated workbooks drawn by Excel 2016 and by the app, in light and dark, with the differences listed on the review page. You signed it off as it stands, so `v0.1.0-mvp` is tagged. The listed differences stay as known gaps.
