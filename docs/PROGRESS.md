# Progress

What is done, and the evidence for it. The plan is in `PLAN.md`, and decisions are in `decisions.md`.

| Work | Status | Tag |
| --- | --- | --- |
| Planning | Done 2026-11-01 | |
| Track U: components in `CashGear.React.UI` | All seven components built, pinned at `65500a73`; Firefox can't run here, so `npm run verify` stops there | |
| Phase 0: Scaffold, toolchain, CI | Done 2026-11-01 | `phase-0` |
| Phase 1: Formula engine bake-off | Done 2026-11-02: Formualizer, with the accuracy target moved to Phase 4 | `phase-1` |
| Phase 2: Open xlsx | Done 2026-11-02 | `phase-2` |
| Phase 3: Preserving save | Done 2026-11-05 | `phase-3` |
| Phase 4: Editing, formulas, undo | Done 2026-11-05: 3 of 4 exit criteria met, and you accepted 99.35% on the fourth | `phase-4` |
| Phase 5: Ribbon and formatting | Done 2026-11-05, with your sign-off on the visual review | `v0.1.0-mvp` |
| Phase 6: Data tools | Not started | |
| Phase 7: Charts | Not started | |
| Phase 8: Pivot tables | Not started | |
| Phase 9: Scale and release | Not started | |

## Planning

- The plan was approved on 2026-11-01 and saved as `docs/PLAN.md`.
- Engine and library facts were read from published packages and source. Nothing has been compiled yet, so Phase 1 tests them.

## Phase 0: Scaffold, toolchain, CI

A canvas tile drawn in a worker from WASM engine data shows in the browser and in Tauri, and CI enforces quality and licences. The data is a test pattern served by `cgs-engine` through the real `ViewBlock` format.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| `pnpm dev:web` shows the pattern and a CashGear UI control, in light and dark | Pass | 12 of 12 Playwright tests against the running dev server (`E2E_BASE_URL=http://127.0.0.1:5173 pnpm test:e2e`), in Chrome 156.0.8078.79 and Edge 156.0.4314.58. Screenshots checked by eye |
| `pnpm dev:desktop` shows the same, in light and dark | Pass | Launched on the reference machine and attached over CDP (WebView2 156.0.4314.58). The shared `tests/e2e/pattern.mjs` checks passed in both themes, with the same first-tile hash as the web, 2069359672. The real window was captured with its title bar, which follows the theme |
| CI is green | Pass | [Run 41313708990](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41313708990) at `379d3a7`: Windows job 14 min, licence job 1 min. CI's artifact shows 12 of 12 browser tests with none flaky, and the desktop smoke test passing on WebView2 156.0.4314.50 |
| Adding a GPL dependency makes CI fail | Pass | Every CI run does this with fixtures: `pnpm licenses:selftest` logged "cargo-deny rejected the GPL-3.0 fixture" and "npm license check rejected the GPL-3.0 fixture". Real packages were also tried locally and reverted: `shakmaty` 0.30.2 (GPL-3.0-or-later) made `cargo deny` exit 4, and `tinymce` 7.9.1 (GPL-2.0-or-later) made `pnpm licenses:check` exit 1 |

**Other evidence**

- **Release desktop build:** `pnpm desktop:build && pnpm desktop:smoke` passes on this machine and in CI. It covers `crossOriginIsolated`, shared memory with the engine worker, both worker scripts loading, the native command, the pattern in both themes, the window theme, the choice surviving a restart, going back to the system theme, and no CSP refusals.
- **Theme:** browser tests cover following the system, an override beating the system, the override surviving a reload, and going back to System. In dark mode the bars stay as stored, and background, gridlines and automatic text come from the palette.
- **Every commit:** each of the 8 commits passed the checks that exist at that commit, run one by one in a clean clone.
- **WASM size:** 17,899 bytes, 8,072 gzipped, against the 4 MB budget (CI's `wasm-size.json`).
- **Unit tests:** 30 in Vitest and 7 in Rust.
- **Advisories:** `cargo deny check advisories` is clean. It is informational and doesn't fail CI.

**Known gaps**

- macOS was neither built nor run: there is no Mac here, and CI builds on Windows only, as decided.
- Safari and Firefox weren't run; both are best-effort.
- Only Vite and Tauri set COOP/COEP. Whichever static host Phase 9 picks must set them too.
- A real GPL package never reached remote CI, because that would turn `main` red and branches aren't allowed. Remote CI shows the rejection through fixtures, and real packages were tried locally.
- The grid is only the test tile: no headers, selection, scrolling or real data until Phase 2. `TileCache` never evicts tiles yet.
- CI warns that `actions/checkout@v4`, `actions/setup-node@v4` and others target Node 20 and are being run on Node 24. It still passes.

**Findings**

- Playwright over CDP doesn't list workers that started before it attached, so the desktop smoke test reads worker scripts from the page's resource entries.
- Tiles composite in `requestAnimationFrame`, so a hidden page shows an empty grid until it is shown. This appeared in the app's hidden browser pane, where the canvas had the right pixels by the next read.

## Phase 1: Formula engine bake-off

Measured 2026-11-02. Neither engine reached the 99.5% target, so the phase stopped for a decision. You chose Formualizer and moved the target to Phase 4, for real documents including your private workbooks. The full results and the per-function report are in [phase1-report.md](phase1-report.md).

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| 200+ files run, with a per-function report | Pass | 2,989 public and 10 generated workbooks ran in Chrome 156 WASM. 696 files have scored cells on Formualizer and 690 on IronCalc. The report covers 524 and 525 functions |
| Winner matches 99.5% of formula cells | **Not met; moved to Phase 4 by your decision** | All Excel-saved files, 10% cap: Formualizer 86.05%, IronCalc 94.67%. Real documents only: Formualizer 98.22% (pooled 98.45%), IronCalc 98.51% (pooled 98.75%) |
| Winner has slice p99 of 50 ms or less | Pass | Every slice measured: Formualizer 13.9 ms, longest 1,343 ms. IronCalc failed at 99.9 ms, longest 15,318 ms, and 2 of 10 scale workbooks timed out at 5 minutes |
| Decision logged | Pass | Formualizer, in `decisions.md` |

**Other evidence**

- **Structural edits against Excel over COM:** 2 seeds, 1,000 edits each, compared with Excel after every edit. Formualizer matched 2,000 of 2,000, comparing about 2.36M cells. IronCalc matched 484 of 2,000.
- **Cached values without calculating:** both engines read them back with nothing calculated. Formualizer in every file, IronCalc except 28 cells in one file. Formualizer then recalculates only what an edit touches, and a contract test proves it with a deliberately wrong cached value.
- **`calc_slice`:** proven with evaluation that stops and carries on, which both forks added. Contract tests ran a 2,000-formula chain at a zero budget on both engines, with visible cells first, and still run it on Formualizer.
- **WASM size:** engine and adapter, 2.21 MB gzipped for Formualizer and 0.62 MB for IronCalc, against the 5 MB budget.
- **Licences:** both engines and all their dependencies pass `pnpm deny`.
- **CI:** [run 41369440155](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41369440155) at `30c7281`, green with Formualizer alone, on the fork pin.
- **Track U:** no new commits on `claude/cashgear-react-ui-library-xq8tsz`, so the `@cashgear/ui` pin stays at `96d2499`.

**Known gaps**

- Your private workbooks aren't in this run. The harness is ready for them (`CGS_PRIVATE_WORKBOOKS`, `--set=private`), and Phase 4 needs them.
- The fork stays pinned until the upstream PRs land: [#556](https://github.com/PSU3D0/formualizer/pull/556), [#557](https://github.com/PSU3D0/formualizer/pull/557) and [#558](https://github.com/PSU3D0/formualizer/pull/558). The last one's public API snapshot can't be generated on Windows, so that PR's CI check may need it from Linux.
- Spilled arrays after structural edits go wrong in Formualizer, and the structural test's seed has no arrays. See the report.
- Formualizer's graph build on the first edit can't be divided: 4 s at 1,000,000 formulas.
- Timings were measured with another project's benchmark running on this PC, so they are on the pessimistic side.

**Findings**

- Formualizer's own importer scores spilled cells by leaving the saved values in place, so comparing against it makes the engine look better than it is at arrays. Our adapter clears them.
- A published crate can stop building against its own dependency range: `formualizer-workbook` 0.13.0's `xlsx-recalc` feature doesn't compile with `zip` 7.4. We don't enable that feature, but it matters if Phase 2 ever does.
- `SheetJS/test_files` is blocked on GitHub, so it isn't in the corpus.

## Phase 2: Open xlsx

Real `.xlsx` files open and look right, read-only, on web and desktop. Measured on 2026-11-02 on the reference machine in Chrome 156 and WebView2 156.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| The test set opens without recalculation | Pass | Saved values that are wrong on purpose stay on screen after opening and after the graph is built (`opening_calculates_nothing_and_shared_formulas_expand`, `nothing_calculates_until_preparing_finishes_and_after_it_either`). 3,376 of 3,397 public files, all 29 generated and fixture files, and all 5 of your private files open. The other 21 are refused with a reason and none crash: 13 damaged, 6 with no workbook, 2 crafted to ask for more text than memory holds |
| Visible values match `calamine` | Pass | Public: 4,305,449 cells compared, 4,305,385 match (99.9985%). Each of the 64 differences was read by hand, and none is shown to be our error: see below. Generated and fixtures: 60,993,400 of 60,993,400. Private: 5,082 of 5,082 |
| Visible values match Excel | Pass | Excel's own text over COM: 4,580 of 4,580 cells on generated workbooks, 245 of 245 on fixtures, 1,448 of 1,448 on your private files. Format codes, against the agreed 99.5%: 99.852% of 44,696 cells, across all 82 built-in ids and 522 codes found in the public files. The one miss in that set is `[DBNum1]` numerals |
| A 25 MB file shows within 10 s | Pass | 24.2 MB with 2.58M formulas (4.3M cells): 8.5 s, 1,045 MiB. 23.6 MB of values: 4.2 s, 65 MiB |
| A 100 MB file of values opens within 60 s | Pass | 96.6 MB, 16M cells on two sheets: first sheet showing at 10.1 s, everything read at 18.7 s, 229 MiB |
| A file past memory gets a clear message | Pass | 140.8 MB with 15M formulas: "This workbook is too large to open here: it ran out of memory." A new engine then starts, and the next file opens (`bench/measure-out-of-memory.mjs`) |
| 60 fps on 1M rows: 95% of frames within 17 ms, none over 50 ms | Pass | 1,048,576 rows. Wheel: 99.1% within 17 ms, longest 19.2 ms. Scrollbar drag, top to bottom and back: 95.5% within 17 ms, p95 17.0 ms, longest 19.4 ms (`bench/results/speed.json`) |
| A fast drag fills in within 100 ms of stopping | Pass | 81 ms |
| Arabic and mixed-text golden images, light and dark | Pass | 7 golden images pass in Chrome and Edge. The first version was compared by eye with the same workbooks rendered by Excel, which found the differences listed under Findings |
| Web and desktop match | Pass | `pnpm desktop:smoke` opens the same workbooks through the dialog's path and the `cgs-file` protocol, and compares them with the same 6 golden images in both themes: 0 pixels differ |
| Default-styled workbooks are readable in dark mode | Pass | `defaults-dark` golden, and `dark_mode_swaps_theme_colours_and_rescues_low_contrast_text`: theme black swaps, fixed black moves to 4.5:1, fills stay as stored |
| CI is green | Pass | [Run 41475914358](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41475914358) at `47477d0`, in 13 minutes: 48 browser tests and the desktop smoke test on GitHub's Windows runner, where the golden images differ from this PC's by at most 0.002% of pixels, and the licence job |

**The 64 differences from `calamine`**

| Cells | What | Who is right |
| --- | --- | --- |
| 35 | `<v>    0.0003</v>`, a number with spaces in front | Ours reads the number, calamine gives text |
| 23 | `t="b"` holding `false` or `FALSE` | Ours reads false, calamine reads anything but `0` as true |
| 3 | Inline strings in CDATA | Ours reads the text, calamine gives nothing |
| 1 | `<v>2 </v>` indexing a shared string | Ours reads index 2, calamine reads 0 |
| 1 | `<si>` with a plain `<t>` and then rich runs, a synthetic file | Ours joins them, calamine stops at the first. Not checked in Excel, as public files never open there |
| 1 | A markup-compatibility test file with `<t>` inside an ignorable element | Ours joins them, calamine stops at the first. Not checked in Excel, for the same reason |

Calamine also couldn't open 28 files or read 22 sheets that ours reads, and 3 sheets hold a cell so far out that its full matrix won't fit. Those have nothing to compare with, and the reader's own tests cover the cases they show.

**Other evidence**

- **Your private workbooks:** 5 files and 0.1 MB, so they say nothing about speed. All show in under 0.4 s with no page errors, and 24 of 24 formula cells match Excel's saved values when recalculated. Nothing about them reached the repo but these counts.
- **`TEXT`:** the bake-off's `TEXT` cells went from 46.98% to 91.78% (1,942 of 2,116). Real documents are at 98.23% overall, up from 98.22%. The remaining `TEXT` misses are mostly POI's stress tests of `?` and `#` in exponents, plus files such as one saved by a German Excel, whose values can't match here.
- **Against Phase 1:** the 15.5 MB file with 1M formulas now shows in 3.9 s and uses 356 MiB once its graph is built, against 14.4 s and 936 MiB through Phase 1's reader.
- **While the graph builds:** it takes 18 s at 2.58M formulas, one sheet at a time, starting after reads have been quiet for 500 ms. The page stays responsive, with a longest frame of 33 ms, but tiles asked for meanwhile wait for the current sheet.
- **WASM size:** 7.83 MB, 2.62 MB gzipped, against the 5 MB budget.
- **Unit tests:** 197 in Rust and 66 in Vitest. Browser tests: 48, 24 each in Chrome and Edge.
- **Licences:** `pnpm deny` and `pnpm licenses:check` pass. Carlito is checked in with its OFL text in `packages/engine-client/src/fonts`.
- **Track U:** `CgStatusBar` and `CgSlider` are at `d9296d5` on the library's branch, and the pin is bumped. 95 of 95 of their browser tests pass in Chromium, Firefox and WebKit.

**Known gaps**

- Conditional formatting, table banding and sparklines don't draw until Phase 6, and charts and images are placeholders until Phase 7. Rich text shows in one style. Pattern fills draw flat and gradients as their first colour.
- Formulas with no saved value show blank, and `fullCalcOnLoad` is ignored, until Phase 4 schedules calculation. Files from other writers can look empty until then.
- Rows with no stored height fit their largest font, but not wrapped text. Excel stores the height of rows it fitted, so this only touches files from other writers.
- Text overflows at most one tile (16 columns) in from outside a tile. At zoom levels or display scales that aren't whole numbers, merged text crossing a tile edge can sit a pixel out.
- `[DBNum]` numerals and Thai and Hebrew calendars aren't implemented, and Um Al-Qura uses the tabular Hijri calculation.
- Aptos isn't installed here, so its digit width comes from files Excel saved, and its text uses Carlito's widths on a PC without it.
- Ctrl+arrow keys wait for Phase 4's shortcut map. Split panes that aren't frozen show as one pane.
- `npm run verify` in `CashGear.React.UI` doesn't pass on this PC, in three gates, none in the new components. A rich text editor test times out under load, and also does on the clean commit. 607 older visual tests fail, with one inspected diff showing only text differences and Inter not installed here. `verify:package` hits a vendor file's line endings, as it also does on the clean commit.
- Your private set is small. Phase 4's 99.5% will mean more from documents with more formulas, if you have them.
- macOS, Safari and Firefox weren't run.

**Findings**

- An accelerated `OffscreenCanvas` sends its text to the GPU process's one main thread, which then has no time to composite the visible canvas: 8.2 s busy out of about 9 s during a drag. Rastering in the worker's own software canvas took the drag from 88% to 96% of frames within 17 ms.
- `save` and `restore` also cover a canvas's `direction`, so caching it draws alternate right-to-left cells wrongly. Only comparing with Excel's picture showed this.
- Excel aligns General text by its first strong character, whatever the cell's reading order says.
- Intl separates the time from AM/PM with a narrow space, which doesn't belong in a format code.
- XML line endings need normalizing by hand, as `quick-xml` leaves `\r\n` in text.
- Excel's digit widths are uneven by size, from hinting, and the default column's padding is `2 * ceil(d / 4) + 1`.
- Excel refuses a workbook whose drawing parts have no content types, so test fixtures must be complete.
- The desktop smoke test failed once in CI on a docs-only commit: on restart it took a busy old WebView2 for a dead one, and attached as that browser exited. It now tells a timeout from a refused connection, and retries attaching. It also keeps its recent files apart from the real app's.

## Phase 3: Preserving save

Save writes a valid `.xlsx` where everything untouched survives, on web and desktop. There is no editing in the UI yet, so edits come through the Rust API, as planned. Measured on 2026-11-05 on the reference machine, with Excel 16.0.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Clean entries are hash-identical | Pass | 0 of 31,260 entries differ across 3,367 edited public files, compared by SHA-256 of their raw bytes and again through the `zip` crate, which shares no code with the writer. Also 0 of 62 on generated files and 0 of 16 on your private files. With no edits, all 3,376 public files that open save byte for byte |
| No new Open XML SDK validator errors compared with the source | Pass | `DocumentFormat.OpenXml` 3.3.0 against `Microsoft365`: 0 files with new errors out of 3,367 public, 16 generated and 5 private. Public errors fell from 2,494 to 2,188. No file the SDK could open stopped opening |
| Zero Excel repair prompts across the COM-open matrix | Pass | 22 of 22 edits chosen to provoke a repair, 16 of 16 generated files and 5 of 5 of your private files open clean. Excel refuses a bad file over COM where a person sees a repair prompt, and the matrix proves it can see that: 7 of 7 deliberately broken files are refused on every run |
| Chart, pivot and macro fixtures still work in Excel after saving | Pass | Chart and picture: 4 of 4 parts identical, 1 chart and 2 shapes present, and the chart reads the sheet. Pivot: 5 of 5 parts identical, and a refresh gives 1,030 and 1,090 from the edited data. Macro: `vbaProject.bin` identical, and Excel ran it from the source and the no-edit save (42) and from the edited save (200). All from `bench/run-excel-matrix.mjs` |
| CI is green | Pass | [Run 41928189717](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41928189717) at `3be2c86`, in 20 minutes: browser tests, the desktop smoke test, and the save check on fixtures. Excel isn't on the runner, so the matrix runs here |

**Other evidence**

- **Values read back:** every cell of each saved file is compared with what the edited engine holds. Generated: 3,271,294 of 3,271,294. Your private files: 5,107 of 5,107. Public: 4,348,241 of 4,362,012. All 13,771 public differences are explained below, and none is a value the writer lost.
- **On the web:** Save overwrites through the browser's swap file, Save As moves on to the new file, and a second save reads from the first's result. A failed write leaves the file and the edits alone. A browser with no file handles gets a download. 16 of the 64 browser tests, 8 each in Chrome and Edge.
- **On the desktop:** bytes go through the protocol to a file beside the target, reach the disk, and replace it in one step. `pnpm desktop:smoke` covers overwrite, a second save, Save As, a changed file being refused, and a working copy rescuing edits when the file changes after the first edit.
- **Working copy:** nothing is copied until the first edit. After that the source can change or vanish, and the save still has what it needs. A file that changed before the first edit refuses the edit and says why.
- **A check before every swap:** what a save wrote is read back against its plan, by sum and length for every part and by parsing every changed part, before anything replaces the original.
- **Pivots:** `refreshOnLoad` is enough. See `decisions.md`.
- **Speed, native, one edited sheet of each:** a 16 MB file in 5.5 s, 25 MB with 4.3M cells in 13.5 s, 25 MB of values in 8.6 s, and 100 MB of values in 19.4 s, where one of its two sheets changed. Parts a save doesn't change are only copied.
- **WASM size:** 8.15 MB, 2.73 MB gzipped, against the 5 MB budget.
- **Unit tests:** 265 in Rust and 70 in Vitest. Browser tests: 64, 32 each in Chrome and Edge.
- **Your private workbooks:** 5 files, read on this PC only, with temporary saves deleted afterwards. Only these counts reached the repo.

**The 13,771 differences between the engine and the saved file**

| Cells | What | Why |
| --- | --- | --- |
| 13,451 | The saved file has a value and the engine has none | Array results. The first edit makes the engine calculate arrays again, and where it can't rebuild one, the file keeps what Excel saved. 53 of the 55 files showing this have array formulas. The other 2 are copies of one file holding a string index past the end of its table, which a new string then fills. Our broken fixture for that case shows Excel refuses such a file |
| 314 | The engine has a value and the file has none | Results the file can't hold, such as `#SPILL!`, `#CIRC!` and `#N/IMPL!`, and spills that grew. Excel works them out when it opens the file |
| 3 | A number in a table header reads back as text | Excel requires that, so it is what we write |
| 3 | A formula that depends on the edit kept its old saved value | Known gap below. These are the same 2 workbooks' cells |

**Known gaps**

- Formualizer's change list missed a changed cell in 2 distinct public workbooks, one cell each. Their saved values stay as they were until Excel recalculates on opening, which it does. Phase 4 should find out why before relying on that list.
- 4 public files can't be edited, because the engine can't get their formulas ready: one self-reference, one parser failure and two `#REF!`. They stay open and showing, and editing says why. 2 crafted or damaged files refuse a save that would touch their broken part, and say so.
- A dynamic array that grows isn't stored past its old area, as the file's `ref` isn't updated yet. Excel rebuilds it on opening.
- Saving holds the engine worker for the whole save, so tiles wait meanwhile. The status bar says "Saving…". Bringing this into slices is left for Phase 9's scale work. WASM timings weren't measured.
- A sheet can have a new `mergeCells` only if it has none. Phase 5 needs to extend an existing one.
- The edit hook is only for tests, so the status bar still says Read-only, and grid tiles aren't redrawn after an edit, until Phase 4.
- Protected View can't be driven over COM, so what a downloaded file does with `refreshOnLoad` until someone enables editing wasn't measured.
- macOS, Safari and Firefox weren't run.

**Findings**

- Excel refuses `#SPILL!` and `#CALC!` as plain error cells, because it only stores them with rich value metadata. The matrix found this.
- A storage's children must sort by code unit. A locale-aware sort put `_VBA_PROJECT` in the wrong place, and Excel refused the file with no clue why.
- Workbooks that link to other workbooks failed in the engine's graph build, which could have closed a business file just after opening. Such a formula now keeps its saved value and stays out of the engine.
- Public files showed more than the fixtures did: styles with a namespace prefix, a part named twice, a part nothing links to, a content type for a part that isn't there, UTF-16 declarations, and rows with no addresses.
- A 256 KB buffer per cell took a 16 MB save to 21 s. Timing a large file is what showed it.

## Phase 4: Editing, formulas, undo

You can type, calculate, undo, save and reopen in Excel. Measured on 2026-11-05 on the reference machine, in Chrome 156, with Formualizer pinned at fork commit `47ddf40e`. The Excel here is 2016 (16.0).

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| 1,000 random edits match Excel via COM | Pass | `bench/run-edit-fuzz.mjs`, 250 episodes of 5 edits on each of 2 seeds. Seed 1: 1,160 steps match and 0 diverge, over 126,536 cell comparisons. Seed 2: 1,135 match and 0 diverge, over 121,948. Another 23 were refused by both, and 4 were silently ignored by Excel and refused here. 178 steps weren't reached, as 39 episodes stopped at a circular reference and the rest at a refusal |
| Input stays under 100 ms during recalculation | Pass | `bench/measure-input.mjs`: 5 recalculations of 1,000,000 formulas, each correct at the end. Key to painted frame: p50 21.7 ms, p99 44.4 ms, longest 47.1 ms. Frames: p99 19.8 ms, longest 21.7 ms. The longest wait for the engine, reported separately: p99 114 ms, longest 323 ms. An edit reaches the engine in p50 53 ms, p99 117 ms |
| Files saved after structural edits open with no repair prompts | Pass | `bench/run-excel-matrix.mjs`: 29 structural edits over each of 8 fixtures, 232 of 232 clean. All 11 broken controls refused, so the check can see a repair. Your 5 private files: 5 of 5 clean after rows, columns, a new sheet and a rename. 434 of 434 files saved by the fuzz opened clean and matched Excel over 46,412 cells. Chart, pivot and macros still work after rows go in |
| Formula cells match Excel's stored values at 99.5% on real documents, private workbooks included | **Not met: 99.350%, accepted by your decision** | Chrome WASM, same scoring as Phase 1: 345 documents, 72,016 formula cells, 10% cap 99.350%, pooled 99.435%, up from 98.272% and 98.496%. Your private set adds 1 Excel-saved file with 24 formula cells, all matching. 99.5% needs at most 313 misses, and there are 407. See below |

**Why 99.5% wasn't reached**

Most of the 407 remaining misses are outside what a formula engine can do from the file:

| Cells | Cause |
| --- | --- |
| 157 | VBA and XLL functions, and what depends on them, whose code we can't run |
| About 100 | Saved values that aren't a fresh Excel calculation: PhpSpreadsheet and XlsxWriter placeholders, stored `#NAME?` for functions the engine computes, stored `""` for computed text |
| 66 | `FORECAST.ETS` and `.CONFINT`, which aren't implemented. Excel's fit lands on a 0.001 grid, but its objective wasn't identified |
| 36 | `_FV`, which needs rich-data parts the engine isn't given |
| 27 | Formulas reading sheets that aren't in the file |
| 21 | Rich-value cells storing `#VALUE!` where the real error is `#SPILL!`, and odds and ends |

The scoring, the file split, the tolerance and the skip list didn't change to get here. You chose to accept this result and tag the phase, rather than change the scoring or keep saved values in place of calculating them. See `decisions.md`.

**What else was checked**

- Typed text: `bench/run-excel-input.mjs`, 153 of 153 spellings read as Excel reads them, and 10 more follow what typing does because COM treats them differently.
- Browsers: 134 of 134 tests in Chrome and Edge, including 35 new ones for typing, pointing, undo, fill, clipboard, structure, moving around, manual calculation, CSV and recovery.
- Desktop: the smoke test passes, including saving.
- Saving in WASM, one edit and then the whole save: 25 MB of values 11.9 s, 25 MB of formulas 16.8 s, 1,048,576 rows 26.2 s, 100 MB 25.1 s. The page's longest frame during any of them was 24 ms.
- WASM gzip: 2.97 MB, against the 5 MB budget the build enforces.
- Dark mode, licences, the Open XML validator and the save check are all clean.

**Engine fixes, for your decision on upstream PRs**

28 commits on `MohamedSadat/formualizer`, branch `cashgear/0.13.0`, `05f93f5a..47ddf40e`, plus 4 more from earlier in the phase. Nothing public went out.

- A scalar function runs once for each value of an array, from a new table of parameter classes.
- `ROW` and `COLUMN` return arrays, and read the numbers a reference names.
- `INDEX` reads a table column.
- Intersection and union operators, and `SINGLE`, which is how a file writes `@` inside a formula.
- Defined names work out their values after stored values are adopted, and can hold characters beyond ASCII.
- A spill shows over an edit that emptied the cell.
- `ISERROR`, `ISERR` and `ISNA` see what can't be evaluated. `ISFORMULA` works.
- `LOOKUP` skips what it can't compare. `SUMIF` reads arrays of `INDIRECT` references.
- `SIN` and `COS` match Excel where the result is only the remainder.
- `GROUPBY` totals by default, and `FORMULATEXT` and `CELL` give what a person reads.
- Fixed arrays with family execution on, volatile formulas dirty after opening, sheet names like `R23` quoted, and `#REF!` rather than a panic for a table on a deleted sheet.

The fork's own test suite has 3,353 passing and 1 failing, `ingest_has_no_formula_plane_producer_sites`, which also fails on a clean checkout. Some of its expectations changed to match Excel's stored values: `ROW` and `COLUMN` of a range, `CONCATENATE` over a range, a `GROUPBY` total row, 15-digit complex numbers.

**Known gaps**

- The 99.5% target, above.
- Saving takes 12 to 26 s on large files, and the engine does nothing else meanwhile, though the page stays responsive.
- A very fast typist queues up: at about 14 edits a second during a recalculation, edits land late and the calculation takes longer, as each edit makes it plan again. Nothing is lost, and keys still show at once.
- Dynamic arrays can't be compared here, as Excel 2016 has none. They have contract tests, and their saved files open.
- Recovery is tested in Chrome and Edge. The desktop uses the same worker code, but the smoke test doesn't crash it. Closing the desktop window with unsaved edits doesn't ask yet, though recovery covers it.
- Inserting cells that shift sideways, with its dialog, is left to Phase 5's menus. Tables don't expand when you type beside them until Phase 6.
- One pivot save was refused once in the matrix while Chrome ran 4 workers beside it. The same file opened clean 3 times alone, and in the final run. It's likely a refresh-on-open racing the check, but I didn't prove that.
- A nested `@` saves as `_xlfn.SINGLE`, as Excel 365 writes it, which Excel 2016 can't calculate. It only reaches a file for cells that get rewritten.
- 8 cells match by keeping their saved value, as a name pointing at another workbook now follows the existing rule for direct links.
- `CgTabs` starts measuring over whenever its list of tabs changes identity. The shell works around that, and the library could stop it.

**What was learned**

- Each failure the matrix found was a file that looked fine to our own reader: a frozen pane with a split of 0, a sparkline reading `Data!#REF!`, a group with no sparklines, and a deleted shared-formula master that left a whole sheet in its old coordinates while its table moved.
- Only the private files had their relationship prefix declared on each sheet, which made every added sheet invalid. Fixtures written by hand all shared the shape that hid it.
- A measurement has to check its own answers. The first input run looked fine until the last value read 4 instead of 7, and that was the script reading a cell while commands were still queued.
- A fuzz that lets calculation finish between steps can't see what happens during one, so a test now covers edits between slices.

## Phase 5: Ribbon and formatting

The app looks and works like Excel's Home tab for daily work. Measured on 2026-11-05 on the reference machine, in Chrome 156, with Excel 2016 (16.0) and Formualizer pinned at fork commit `47ddf40e`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Formats round-trip through Excel, and existing style indices are unchanged | Pass | `bench/run-excel-format.mjs`: formats go on through the engine, Excel reads each one back over COM and saves its own copy, and our reader reads that copy. 181 of 181 formatted cells agree in all three readings, 181 of 181 show the same text, 34 of 34 lines and 6 of 6 sheet settings agree. 251 existing styles in 8 fixtures and 269 cells no format reached are unchanged. Your 5 private files, as temporary copies: 5 opened, 647 styles and 22,092 cells unchanged, 15 of 15 formatted cells agree and come back from Excel the same |
| Visual goldens are reviewed against Excel | Pass: you signed off on 2026-11-05 | `bench/run-visual-review.mjs` has Excel draw three generated workbooks with CopyPicture and the app draw the same ranges in Chrome, light and dark. The side-by-side page lists every difference. The e2e goldens hold the showcase and formatting workbooks in light and dark |

**What else was checked**

- Saving after formatting: `bench/run-excel-matrix.mjs` now makes 61 edits on each of 8 fixtures, 32 of them new: formats, borders, patterns, merges, sizes, hiding, freezing, shifting cells and replacing. 488 of 488 saved files open in Excel with no repair. All 12 broken controls are refused, one of them new. Your 5 private files: 5 of 5 clean, and 5,107 of 5,107 cells match.
- Editing against Excel: `bench/run-edit-fuzz.mjs` now shifts cells too. On 2 seeds of 250 episodes, 2,209 steps match Excel and 3 don't, over 242,845 cell comparisons. All 411 saved files open clean and match what Excel makes of the same edits. The 3 are table behaviour that Phase 6 owns, below.
- Browsers: 182 of 182 tests in Chrome and Edge, 48 more than in Phase 4, for the ribbon, Format Cells, Find and Replace, the format painter, merges, sizes, freezing, shifting cells and the context menus.
- Goldens: the showcase in light and dark, made by the engine's own commands, and the formatting workbook with a rich-text cell.
- WASM gzip: 3.10 MB, against the 5 MB budget.
- Desktop: closing the window with unsaved edits asks first.
- Licences, the Open XML validator and the save check are clean.
- The matrix and the round trip ran before the last fixes below, which change what cells hold rather than how files are written. The fuzz numbers are from the final code.

**Track U**

`CgRibbon`, `CgColorPicker`, `CgGallery`, `CgBorderPicker` and 114 spreadsheet and formatting icons (Fluent System Icons, MIT) are built in `CashGear.React.UI`, beside `CgStatusBar` and `CgSlider` from Phase 2. `CgTabs` no longer measures itself over when a parent passes a new list of the same tabs. The pin is at `65500a73`. The library records every gate passing except Firefox, which can't launch on this PC, so `npm run verify` as one command still stops there.

**Found and fixed on the way**

- Renaming a table heading left the table's calculated column reading the old name, and Excel refused the file. The matrix found it, and a broken fixture now keeps it checked.
- A `{=...}` array whose formula a shift, a move or a heading rename rewrote became a single formula, so a smaller result left cells empty where Excel shows `#N/A` or repeats it. A fill could also write into part of an array.
- Formulas typed since the last calculation were skipped by shifts, moves and heading renames, and at any time with manual calculation.
- Deleting cells that take a range's end, sideways, now trims the range, as Excel does.
- A lone number in a fill counts on when the source isn't all numbers, as Excel's AutoFill does.
- A rich cell's bigger runs now make its row taller, as Excel fits it.
- Increase Decimal on a scientific format no longer adds digits to the exponent.
- The font boxes committed twice, and a row refit could take the keyboard from them mid-typing.

**Known gaps**

- From the fuzz, all for Phase 6's tables: filling `Sales[Cost]` across moves the column in Excel, deleting cells inside a table deletes table rows in Excel and is refused here, and Excel writes `[Cost]` for a table's own column.
- From the review: text turned up 90° sits nearer the left of its cell than in Excel, stacked text clips a little differently, and there are no green error triangles.
- Cell Styles moved to Phase 6, and key tips to after v1. Sheets can't be hidden or shown yet, and zoom isn't saved.
- Pasting or moving a rich cell makes it plain. Superscript and subscript aren't drawn.
- A format on part of a column crosses at most 200,000 cells. Whole-column borders don't reach row 1's top edge. The format painter doesn't copy row and column sizes.
- There is no circular-reference warning that names the cells.
- Excel 2016 can't check dynamic arrays or newer functions, as before.
- Outside this phase: the Azure Static Web Apps workflow you added fails, because it looks for an app in `./apps` and finds none to build. The repo's own CI is separate.
