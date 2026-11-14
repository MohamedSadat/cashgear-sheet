# Progress

What is done, and the evidence for it. The plan is in `PLAN.md`, and decisions are in `decisions.md`.

| Work | Status | Tag |
| --- | --- | --- |
| Planning | Done 2026-11-01 | |
| Track U: components in `CashGear.React.UI` | All seven components built, `CgChart` extended for spreadsheet charts, and `CgPivotFieldList` added; pinned at `bcd95bc2`. Firefox can't run here, so `npm run verify` stops there | |
| Phase 0: Scaffold, toolchain, CI | Done 2026-11-01 | `phase-0` |
| Phase 1: Formula engine bake-off | Done 2026-11-02: Formualizer, with the accuracy target moved to Phase 4 | `phase-1` |
| Phase 2: Open xlsx | Done 2026-11-02 | `phase-2` |
| Phase 3: Preserving save | Done 2026-11-05 | `phase-3` |
| Phase 4: Editing, formulas, undo | Done 2026-11-05: 3 of 4 exit criteria met, and you accepted 99.35% on the fourth | `phase-4` |
| Phase 5: Ribbon and formatting | Done 2026-11-05, with your sign-off on the visual review | `v0.1.0-mvp` |
| Phase 6: Data tools | Done 2026-11-06 | `phase-6` |
| Phase 7: Charts | Done 2026-11-06, with your sign-off on the chart review | `phase-7` |
| Phase 8: Pivot tables | Done 2026-11-07 | `phase-8` |
| Phase 9: Scale and release | Done 2026-11-07 | `v1.0.0` |
| Planning after v1 | Done 2026-11-07: Phases 10 to 13 added to `PLAN.md` | |
| Phase 10: Print and PDF export | Done 2026-11-08, with your sign-off on the PDF review and about 91% of public page counts accepted | `v1.1.0` |
| Phase 11: RTL sheets | Done 2026-11-08, with your sign-off on the right-to-left review | `v1.2.0` |
| Phase 12: Large books | Done 2026-11-09: Open with in Edge starts the app, and the file reaching it is checked in Chrome | `v1.3.0` |
| Phase 13: Excel parity polish | Done 2026-11-09 | `v1.4.0` |
| Planning the ERP integration | Done 2026-11-09: Phases 14 to 19 and Track E added to `PLAN.md`, designed in `ERP_SHEET_INTEGRATION_PLAN.md` | |
| Phase 14: ERP contract and mock | Done 2026-11-09, with your sign-off on the contract and the rollout plan | `v1.5.0` |
| Phase 15: Read-only ERP slice | Done 2026-11-09 | `v1.6.0` |
| Phase 16: Editing and change tracking | Done 2026-11-10 | `v1.7.0` |
| Phase 17: Validate, review and commit | Done 2026-11-10 | `v1.8.0` |
| Phase 18: Conflicts, refresh, sessions and recovery | Done 2026-11-11, with your sign-off on the Arabic texts | `v1.9.0` |
| Phase 19: ERP hardening | In progress 2026-11-11: B.6's targets met and the checks green; the tenant runs and your sign-off wait on the test tenant's setup | `v1.10.0` |
| Track E: the ERP side, in `D:\CashGear\Projects\CashGear` | E0 done 2026-11-09: the contract reviewed against UnifiedAPI's code, read-only. E1 done 2026-11-09: tickets, sessions, Schema and Query in UnifiedAPI, and the hidden button in `ERPClient_React`. E2 done 2026-11-10: Commit with dry runs, RowVersion, idempotency and audit. E3 done 2026-11-11: Renew, End and CommitStatus, and Kit turned on. E4 done 2026-11-11: CORS narrowed to the live sheet, rate limits, a log line per request, API keys out of the logs, the per-company switch ending sessions, Dot turned on, and the test vendors' seed | |
| Track S: Cash Gear Workspace's workbooks, in this repo | S1 and S2 done 2026-11-13: a cloud `SheetPlatform` and the `/cloud` launch, built against a mock of Workspace's sheet-session routes. The Arabic texts wait on your sign-off. S3 to S5 done 2026-11-14: Workspace's embed (`/embed`, `sheet-embed.1`), its previews and chart views, Workspace's vectors and a browser suite, and `sheet-session.1`'s suite run against Workspace's real API | |

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

## Phase 6: Data tools

Sort, filters, tables, conditional formatting, data validation, names, notes, hyperlinks and cell styles work as Excel 2016 does them, from the ribbon and the grid. Measured on 2026-11-06 on the reference machine, in Chrome 156, with Excel 2016 (16.0) and Formualizer pinned at fork commit `4dd2e77a`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Filtering 1M rows takes under 200 ms | Pass: 151 ms | `bench/measure-filter.mjs`: a value-list filter on column H of `rows-1m.xlsx` (1,048,576 rows), from the command to the next frame with the filtered view, in Chrome against the release build. Runs of 201, 151, 126, 139 and 151 ms; the median is 151. It was 432 ms before two fixes: row sizes now go to the page as runs, so a filter that hides most rows no longer sends a million records, and each value is matched through a small table in front of the hash map. Natively the same filter takes 94 ms |
| Rules match Excel on test files and round-trip without repair prompts | Pass | `bench/run-excel-rules.mjs` builds 43 cases through the engine and has Excel judge each saved file over COM: conditional formats by `DisplayFormat` (10 cases: cell values, text, blanks and errors, dates, top and bottom, averages, duplicates, formulas and colour scales), validation by `Validation.Value` (8 cases), filters after Excel's own `ApplyFilter` (16 cases), and sorts against Excel's own `Range.Sort` of the unsorted copy (9 cases). 43 of 43 agree, 1,572 of 1,572 comparisons. Saving: `bench/run-excel-matrix.mjs` now makes 90 edits on each of 8 fixtures, 29 of them new: tables, sorts, filters, conditional formats, validation, names, notes, links, cell styles and Remove Duplicates. 720 of 720 saved files open in Excel with no repair, and all 12 broken controls are still refused. Your 5 private files: 5 of 5 clean, and 5,107 of 5,107 cells match |

**What else was checked**

- Editing against Excel: `bench/run-edit-fuzz.mjs`, now with Excel's table AutoCorrect settings on as they are by default, 250 episodes on each of 2 seeds. Seed 1: 1,120 steps match and 0 diverge, over 123,614 cell comparisons. Seed 2: 1,100 match and 0 diverge, over 120,290. All 414 saved files open clean and match what Excel makes of the same edits. Phase 5's 3 table differences are gone.
- Browsers: 190 of 190 tests in Chrome and Edge in CI, 8 more than in Phase 5. The new `tests/e2e/data.spec.ts` covers a filter button and its menu, sorting from the Data tab, Format as Table with the Table Design tab, and validation's message.
- Unit tests: 163 in Vitest and 521 in Rust, among them the engine's tests for tables, sorting, filters, conditional formats, validation, names, notes, links, cell styles and Remove Duplicates.
- WASM gzip: 3.50 MB, against the 5 MB budget.
- Licences, the save check and the Open XML validator are clean in CI.

**Track U**

Nothing new was needed in `CashGear.React.UI`: the galleries use `CgGallery`, and the dialogs `CgWindow`, `CgTabs`, `CgComboBox`, `CgCheckBox`, `CgColorPicker` and `CgGrid`. The pin stays at `65500a73`.

**Found and fixed on the way**

- A list typed into a validation rule is case-sensitive when Excel circles invalid cells, but one read from cells isn't, and typing takes either in any case.
- An error anywhere in a column makes Excel's above or below average 0, for filters. Excel works the average out again when it applies the filter, and writes it into the file, as we now do.
- A cell deleted sideways in a table took the table's column from the sheet but not from the table's part, which Excel repaired. The part is now rewritten, its calculated column formulas too.
- A defined name typed as `$A$1` was saved with no sheet, which Excel repaired. A bare reference is now the active sheet's.
- With AutoExpand on, a fill beside a table grows it over every column it writes, a formula typed under a table leaves it alone, and a formula typed beside its headings becomes a heading of its value.
- Deleting or inserting some cells moved the cells and merges but left notes, links, conditional formats and validation where they were. They now move with the cells, and go with deleted ones.
- After a table lost a column, saving renamed the heading that moved into its place, because it judged headings by the table's old part, and Excel refused the file. A table's calculated column could also keep reading a column that had gone.
- The edit fuzz ran an old build of its tool, since it never built it, and turned Excel's AutoCorrect table settings off without putting them back. It now builds the tool, runs with Excel's defaults, and restores whatever it found.

**Known gaps**

- Extended (x14) conditional formats and validation from newer Excel are drawn, listed and kept, but not edited, as decided. After rows or columns move, their kept XML doesn't move with them.
- `DisplayFormat` can't show data bars or icons, so those were checked by eye and by the engine's own tests, not against Excel.
- Cell Styles give cells a style's look but leave them linked to Normal, so Excel's gallery highlights Normal. The file's own named styles aren't listed, and New and Modify Cell Style wait.
- Custom table styles from a file are drawn and can be applied, but not created or changed.
- Notes are edited in a small dialog, not in a box beside the cell, and can't be shown permanently (Show/Hide, Show All). The author is `Author` until there's a user name setting. A sheet with threaded comments keeps its notes as they are.
- A link opens with `window.open`, which the desktop shell may block; there's no Remove Hyperlink in the context menu yet.
- The filter menu's Sort by Color and Filter by Color are greyed out, and so are icon filters. A Custom AutoFilter doesn't start from the column's current custom filter.
- Validation formulas typed in the dialog are checked by the engine when they run, not as they're typed. "Apply these changes to all other cells with the same settings" isn't there.
- Dates typed into the validation and filter dialogs are read as en-US, and the 1904 date system isn't handled there.
- A formula typed into a table heading becomes 0, as Excel makes it, rather than its value.
- On this PC, Excel's "Include new rows and columns in table" and "Fill formulas in tables to create calculated columns" are off, most likely left so by earlier fuzz runs. Excel's default is on. The fuzz now puts back what it finds, so they stay off until you turn them on in Excel Options > Proofing > AutoCorrect Options > AutoFormat As You Type.
- Outside this phase: the Azure Static Web Apps workflow still fails, as before.

## Phase 7: Charts

Charts from files draw live from their cells, and new ones can be made, changed, moved, sized and deleted, as Excel 2016 does them. Pictures show and can be inserted, and chart sheets fill their tab. Measured on 2026-11-06 on the reference machine, in Chrome 156, with Excel 2016 (16.0), `CgChart` pinned at `814fa187` and Formualizer pinned at fork commit `4dd2e77a`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Five chart types match Excel on review, and stay readable in dark mode | Pass: you signed off on 2026-11-06 | `bench/run-chart-review.mjs` has Excel 2016 make a workbook over COM: a clustered column, a clustered bar, a line with markers, a pie, a scatter, a combination with a line on a secondary axis, a stacked area, a doughnut, a stacked column and a chart sheet. Excel exports each chart with `Chart.Export`, and the app draws the same file in Chrome, light and dark. `review.html` shows them side by side. Axis scales follow Excel 2016's own rule, measured over COM on 920 charts by `bench/run-chart-axes.mjs`: all 920 minimums and maximums match, and 917 major units. In the review every chart has Excel's scale, colours, gap widths, gridlines, legend place and fonts. The differences are listed under known gaps |
| Edits write valid XML, and extension elements survive | Pass | The same bench has the app make 53 edits to the charts of Excel's own workbook: titles, legends, data labels, fills, fonts, layouts, colours, gap widths, a type change, Switch Row/Column, a move, and the chart sheet's chart. Excel opens the saved file with no repair. In the 10 chart parts, every extension element Excel wrote is still there: 33 extension lists, 23 series ids (`c16:uniqueId`), 10 `c14:style`, 20 `mc:AlternateContent` and 30 `c16r3` elements, 88 of 88 counts. All 18 chart style and colour parts are byte for byte the same. `bench/run-excel-matrix.mjs` now makes 99 edits on each of 8 fixtures, 9 of them new: a chart of each of 19 kinds, elements added and taken off, looks, axis scales, type and data changes, charts moved, sized and deleted, a picture inserted, and rows inserted and deleted under a chart. 792 of 792 saved files open in Excel with no repair, and all 12 broken controls are still refused. Your 5 private files: 5 of 5 clean, and 5,107 of 5,107 cells match. The writer's own tests check that an unchanged chart keeps its bytes, that edits keep `extLst`, and that a new type keeps each series' extensions |

**What else was checked**

- Editing against Excel: `bench/run-excel-rules.mjs` 43 of 43 cases, 1,572 of 1,572 comparisons.
- Edit fuzz: `bench/run-edit-fuzz.mjs`, 250 episodes on each of 2 seeds. Seed 1: 1,120 steps match and 0 diverge, over 123,614 cell comparisons. Seed 2: 1,100 match and 0 diverge, over 120,290. All 414 saved files open clean and match what Excel makes of the same edits.
- Reading: `chartscan` reads all 767 chart parts in the 3,445 test workbooks: 670 of the six editable kinds, 68 with a 3-D view drawn flat, 1 newer `cx:` chart shown as a placeholder, none unreadable, no panics.
- Browsers: 199 tests in Chrome and Edge in CI, 9 more than in Phase 6 (one existing Format Cells test needed a retry in Edge). The new `tests/e2e/charts.spec.ts` makes a chart from Insert, changes its type from Chart Design, moves it, sizes it, deletes it and undoes that with the mouse and keyboard, inserts a picture, and holds a chart's golden images in light and dark. The formatting workbook's goldens now show its real chart and picture.
- Unit tests: 172 in Vitest and 545 in Rust. These include the reader, the writer, axis scaling against Excel's measured values, dark mode's colours, chart commands with undo, saving and reading back, chart formulas following inserted rows, chart sheets and pictures.
- WASM gzip: 3.69 MB, against the 5 MB budget.
- Licences, the save check and the Open XML validator are clean in CI.

**Track U**

`CgChart` gained a document mode (`appearance`): title, legend and axis titles inside the SVG, exact size, no card styling, and per-element fonts, fills, borders, gridlines, tick marks and crossings. It also gained scatter series, Excel's smoothed lines, gap width and overlap, exact ticks, data labels made of parts, per-point outlines and explosion, element hit-testing with `highlightedElement`, and `interactive={false}`. Built in `CashGear.React.UI` on its branch at `814fa187`, with 984 unit tests and 696 Chromium checks passing there. WebKit ran the chart cases; Firefox can't launch on this PC.

**Found and fixed on the way**

- A scatter chart changed to another type gave both new axes the scatter's x axis's id, and Excel refused the file. The matrix found it.
- References typed into Select Data without a sheet were saved that way; a chart part must name the sheet, and Excel repaired the file.
- Anchors wrapped in `mc:AlternateContent`, as Excel writes newer `cx:` charts, were read twice.
- Line charts put the first category on the axis, where Excel 2016 puts categories between tick marks; plain scatter charts drew lines.
- A drawing's layout refresh after an edit took the chart out of its selection.

**Known gaps**

- From the review: Excel lists a horizontal bar chart's legend in reverse order, and draws legend keys for bars as wider rectangles. A pie is drawn a little larger than Excel's, and bars in a combination chart a little narrower. An automatic title says "Chart Title" in English, where Excel uses its UI language (Arabic on this PC).
- 3 of 920 measured axes choose a different major unit: horizontal axes with 7-digit labels, where Excel allows for the labels' width.
- Radar, stock, surface, bubble and 3-D charts are drawn as their nearest flat chart and can only be changed by Change Chart Type. Newer `cx:` charts stay placeholders, as decided.
- The format pane covers fill, outline, text, axis bounds, units, number format and gap width; gradients, patterns, effects, shadows and trendlines are kept but not edited. Chart Styles are eight looks, not Excel's full gallery. There is no Picture Format tab, so pictures can't be cropped or recoloured. EMF and WMF pictures show as named placeholders.
- Shifting cells (Insert or Delete with shift) doesn't move what a chart reads; inserting and deleting rows, columns and sheets does.
- A chart's legend can't be laid out by hand, and its title and legend move with automatic layout only.
- Outside this phase: the Azure Static Web Apps workflow still fails, as before.

## Phase 8: Pivot tables

Pivot tables from files refresh and can be rearranged, and new ones can be made from a range or a table, as Excel 2016 does them. Measured on 2026-11-06 and 07 on the reference machine, in Chrome 156, with Excel 2016 (16.0), `@cashgear/ui` pinned at `bcd95bc2` and Formualizer pinned at fork commit `4dd2e77a`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Refresh matches Excel on test files | Pass | `bench/run-pivot-check.mjs` has Excel 2016 make 40 pivot tables over generated sales, one per sheet: every summary, Show Values As (all 14 kinds), compact, outline, tabular and mixed forms, values on rows and columns, report filters with one and several items, hidden items, label and value sorts, date and number groups, custom and absent subtotals, blank rows, repeated labels, collapsed items, captions, number formats and empty and error text. Excel then edits the source (a new region, a new product, a blank, a later date, new values) and refreshes them all. **Refresh from the file's own records** (`pivotcheck`): 40 of 40 match Excel exactly in both of Excel's files, cells, location, `rowItems` and `colItems`, over 2,198 and 2,716 cells. **The app's refresh after the same edits**, read from the source through the engine (`pivotrefresh`): 40 of 40 match Excel's refresh, cell for cell and line for line, over 2,716 cells. **Excel's refresh of what the app saved**: Excel opens the refreshed file and the file after 15 more changes and two new pivots, refreshes every pivot itself, and all 40 and 42 show exactly what the app wrote, over 2,716 and 2,789 cells. **Public test files** (only those Excel saved, never opened in Excel): 243 pivots; 191 can be laid out from their records, and 178 of them match Excel's saved output exactly. The other 13 are listed under known gaps |
| No repair prompts | Pass | Excel opens every file the app saved above with no repair. `bench/run-excel-matrix.mjs` now makes 105 edits on each of 8 fixtures, 6 of them new: a pivot made and laid out, an empty pivot on a new sheet, one rearranged 13 ways (layouts, forms, subtotals, grand totals, repeated labels, styles, sorts, filters, custom subtotals, report filter choice, blank rows, collapsing, empty and error text), one refreshed after its source changed, one deleted, and rows put in above one. 840 of 840 saved files open with no repair, and all 12 broken controls are still refused. The fixture's own pivot, refreshed by the app after an edit, opens and gives 1,030 and 1,090 in Excel, as Excel's own refresh does. Your 5 private files: 5 of 5 clean, and 5,107 of 5,107 cells match. The Open XML validator finds nothing in the saved pivot files |

**What else was checked**

- Pivot styles: Excel 2016's 84 built-in pivot styles, all 28 element types, read over COM and checked against Excel's own saved copies. `bench/run-pivot-looks.mjs` compares every pivot cell's fill, font colour and weight, and edges with Excel's `DisplayFormat`: on Excel's own 40 pivots, 33 look exactly as Excel shows them, and 2,685 of 2,781 cells. The rest are listed under known gaps.
- Editing against Excel: `bench/run-excel-rules.mjs` 43 of 43 cases, 1,572 of 1,572 comparisons.
- Edit fuzz: `bench/run-edit-fuzz.mjs`, 250 episodes on each of 2 seeds. Seed 1: 1,120 steps match and 0 diverge, over 123,614 cell comparisons. Seed 2: 1,100 match and 0 diverge, over 120,290. All 414 saved files open clean and match what Excel makes of the same edits.
- Browsers: 209 tests in Chrome and Edge, 10 more than in Phase 7. The new `tests/e2e/pivots.spec.ts` makes a pivot from Insert onto a new sheet, lays it out from the field list, changes the report layout from Design, refreshes after the source changes, filters from the button on Row Labels, changes the summary in Value Field Settings, and holds a pivot's golden images in light and dark.
- Unit tests: 175 in Vitest and 554 in Rust, among them the reader, the refresh, the writer and the engine's pivot commands with undo.
- WASM gzip: 3.83 MB, against the 5 MB budget.

**Track U**

`CgPivotFieldList` is Excel's PivotTable Fields pane: a field checklist with search, Filters, Columns, Rows and Σ Values areas with drag and drop and a keyboard path, each item's menu (move, remove, field and value field settings), deferred update, a read-only state, and stacked or side-by-side sections. Built in `CashGear.React.UI` on its branch at `bcd95bc2`, with 1,013 unit tests passing and 14 new browser checks in Chromium and WebKit. Firefox can't launch on this PC, and the library's existing browser suites weren't rerun, as the change only adds files.

**Found and fixed on the way**

- A number field only ever summed has no items listed in its cache. Put on an axis, its pivot items pointed at nothing and Excel refused the file. The matrix found it.
- Grouped cache fields were copied as they were while their pivot items pointed at new group items (a new year), and Excel refused the file.
- An empty `groupItems` was written for a date field that other fields group, where Excel writes a bare `fieldGroup`.
- Excel turns COM calls away while it refreshes a file's pivots after opening it. The bench's Excel scripts now retry them.
- Count and Count Numbers look past an error in a single cell, but a total that gathers it shows the error, as Excel does.
- A share of a total shows 0 where nothing falls under a total that has something.

**Known gaps**

- Public files: 13 of 191 pivots differ from what the file holds: 4 with duplicate members in a filter (`mail`, two files each in two copies), 2 copies of a pivot on a chart sheet's workbook, 2 with a custom caption ("Filter rows"), 1 with a Hungarian error text in a cell, 1 with time labels in a format the check can't read, 1 with % Of over errors, 1 with a blank item's label and 1 with a single value field in columns. None of them is a file Excel made on this PC.
- Calculated fields and items, OLAP, and label and value filters are shown as saved and can't be changed here; they save byte for byte. External, consolidation and other-workbook sources can be rearranged from their records but not refreshed.
- Pivot styles: 7 of Excel's 40 pivots still differ in some cells, in tabular-form parent labels and values on rows. A pivot the app makes on a new sheet reads unstyled in Excel's `DisplayFormat`, though its style is in the file; the cause wasn't found.
- Excel's own cell `formats` on a pivot (formatting made by hand in Excel) are kept but not laid out again after a change of layout.
- Captions are English. Grouping is kept and refreshed, but new groups can't be made yet; nor can items be dragged by hand or renamed, and there is no Show Report Filter Pages, Change Data Source, Move PivotTable, Drill Down or GETPIVOTDATA from a click.
- Fitting columns after a layout is a second undo step.
- Outside this phase: nothing new.

## Phase 9: Scale and release

Benchmarks settle the native host question, and CashGear Sheet 1.0.0 ships: a free Windows installer that updates itself, and a web app that installs and works offline. Measured on 2026-11-07 on the reference machine, in Chrome 156 and WebView2 156, with Excel 2016 (16.0) and Formualizer pinned at fork commit `4dd2e77a`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Benchmarks and the host decision are recorded | Pass | `bench/measure-scale.mjs` opens a ladder of generated books in the release build in Chrome, checks the answers of two edits, and reads WASM's memory after each step (table below). WASM holds about 430 bytes per formula and 14 per value. 10,000,008 formulas open and recalculate in 4,042 MiB, just under the 4 GiB cap, and 15,000,000 run out after 8 minutes with "This workbook is too large to open here". 100M values take 1,379 MiB. `bench/harness/src/bin/scale.rs` runs the same steps natively: 3,348 MiB for the 10M book, and the 15M book opens in 4,407 MiB. Recalculation runs at the same speed natively; reading and building the graph take 2.4 times as long in WASM. You chose WASM only for v1 (`decisions.md`), so one engine and one code path stay, and the ceiling is about 10M formulas |
| An installer test covers install, update and uninstall | Pass | `pnpm desktop:installer-test` builds 1.0.0 and 1.0.1, signed with a throwaway certificate and updater key, as a product of its own. Install: 4.9 s, both programs signed, the notices beside the app, registered for `.xlsx`, `.xlsm`, `.xltx`, `.xltm` and `.csv`. Double-click: an `.xlsx` opened through the shell with the install's ProgID shows `Item 1` in A1 in 4.0 s. Update: the status bar offers 1.0.1 ten seconds after start, the Update dialog installs it in 3.2 s, and the installer starts 1.0.1, still registered. Uninstall: 2.6 s, nothing left but the app's empty data folder, which a silent uninstall keeps by design, and your own registry keys for the five types are exactly as before. The release workflow runs the same test on every tag |
| The PWA works offline, and double-clicking `.xlsx` opens the desktop app | Pass, with one check left to you | `tests/e2e/pwa.spec.ts`, in Chrome and Edge: the app installs through the browser's own commands with no installability errors, and its service worker caches the 20 files of the app shell (15 MB). The installed app then starts offline, in its own window and cross-origin isolated, where a workbook opens, draws and calculates. Double-click: the installer test above opens an `.xlsx` through the shell, and `pnpm desktop:smoke` starts the release build with a workbook, as Explorer does, and checks it opens once. Windows lets only you pick the default app for `.xlsx`, so a real double-click in Explorer, after choosing CashGear Sheet once in Open with, is yours to do |

**The scale ladder**

Chrome 156, the release build, one book at a time. Ready means everything is read and the graph is built.

| Book | File | First sheet shown | Ready | Ctrl+Alt+F9 | First edit | Next edit | WASM memory |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1M formulas | 9.9 MB | 5.2 s | 10.9 s | 1.8 s | 191 ms | 56 ms | 409 MiB |
| 2M formulas | 19.7 MB | 8.7 s | 20.1 s | 3.3 s | 236 ms | 46 ms | 808 MiB |
| 2.58M formulas (Phase 2's 25 MB) | 25.4 MB | 12.0 s | 29.9 s | 4.4 s | 250 ms | 47 ms | 1,045 MiB |
| 4M formulas | 39.4 MB | 8.3 s | 62.3 s | 8.4 s | 412 ms | 52 ms | 1,525 MiB |
| 6M formulas | 59.1 MB | 12.7 s | 112 s | 11.8 s | 522 ms | 49 ms | 2,447 MiB |
| 8M formulas | 78.8 MB | 10.3 s | 171 s | 16.9 s | 641 ms | 49 ms | 2,827 MiB |
| 10M formulas | 98.5 MB | 10.1 s | 264 s | 22.5 s | 712 ms | 46 ms | 4,042 MiB |
| 15M formulas | 147.7 MB | 14.1 s | Ran out of memory at 496 s | | | | 4 GiB |
| 16M values (Phase 2's 100 MB) | 101.3 MB | 8.7 s | 16.7 s | 12 ms | 724 ms | 45 ms | 231 MiB |
| 50M values | 318.8 MB | 11.1 s | 68.4 s | 11 ms | 2.1 s | 51 ms | 697 MiB |
| 100M values | 637.6 MB | 8.5 s | 120 s | 4 ms | 3.9 s | 45 ms | 1,379 MiB |

Every formula book's second edit gave the right answers in its row. The first edit includes copying the source for the working copy, which is why it grows with the file.

**Memory budgets**

| What | Budget | Evidence |
| --- | --- | --- |
| The engine | WASM's 4 GiB. Past 3 GiB the status bar says how much is used and asks for a save | `bench/measure-memory-budget.mjs` on the 10M book: open at 3,507 MiB, the status bar says "This workbook uses 3.4 of the 4 GB the engine can have: save your work before adding much more" |
| Running out mid-edit | The workbook closes, a new engine starts and offers the journal's edits, and nothing waits on the old one | The same check fills columns of formulas until the engine runs out. The status bar says "The engine ran out of memory: a workbook here can use up to 4 GB. A new engine is starting, and your edits since the last save can be recovered." The recovery dialog offers the 3 edits, and a new workbook then opens and calculates |
| Running out while opening | "This workbook is too large to open here", and the next file opens | The 15M book above, and Phase 2's check |
| Undo | 256 MiB of pre-images (Phase 4) | Unchanged |
| Drawn tiles | 512 MiB, least recently used first, tiles in view always kept | Unchanged since Phase 2 |

**What else was checked**

- Saving still opens clean in Excel: `bench/run-excel-matrix.mjs`, 105 edits on each of 8 fixtures. 840 of 840 saved files open with no repair, all 12 broken controls are refused, and 22 of 22 scenarios and 19 of 19 generated books are clean. New this phase, large books: the 2M-formula book and the 1,048,576-row book, after a value and after a row put in at the top, which moves every row. 4 of 4 open clean in Excel (20.5 to 54.4 MB saved). Your 5 private files: 5 of 5 clean, and 5,107 of 5,107 cells match.
- Editing against Excel: `bench/run-edit-fuzz.mjs`, 250 episodes on each of 2 seeds. Seed 1: 1,120 steps match and 0 diverge, over 123,614 cell comparisons. Seed 2: 1,100 match and 0 diverge, over 120,290. All 414 saved files open clean and match what Excel makes of the same edits.
- Rules: `bench/run-excel-rules.mjs`, 43 of 43 cases, 1,572 of 1,572 comparisons.
- Pivots: `bench/run-pivot-check.mjs` passes everything: 40 of 40 from Excel's records in both its files, the app's refresh 40 of 40 against Excel's, and Excel's refresh of the app's files 40 of 40 and 42 of 42.
- Browsers: 214 tests in Chrome and Edge, among them the new `tests/e2e/pwa.spec.ts` (installing, offline, and About's notices).
- The desktop: `pnpm desktop:smoke` passes, with a new step that starts the app with a workbook as Explorer does.
- Third-party notices: 333 crates, 5 npm packages, 4 libraries vendored in `@cashgear/ui` (Fluent icons, Leaflet, pdf.js and the rich text editor's packages) and Carlito, with 204 distinct licence texts (789 KB). Shown from File › About CashGear Sheet, shipped as `licences/THIRD-PARTY-NOTICES.txt` beside the installed app, and attached to the release.
- Licences: `pnpm deny`, `pnpm licenses:check` (154 packages) and the GPL self-test pass. The updater uses the system's TLS, as its default brings a licence the gate refuses.
- Unit tests: 175 in Vitest and 555 in Rust. Clippy is clean natively and for WASM.
- WASM gzip: 3.93 MB, against the 5 MB budget. The installer is 5.6 MB.
- CI: [run 42362546378](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/42362546378) at `560a8ab` is green, with the browser tests, the desktop smoke test, the save check and the licence gates. The Azure deploy of the same commit serves `sw.js` with `no-cache` and the isolation headers, and the manifest as `application/manifest+json`.

**Found and fixed on the way**

- A calculation slice that ran out of memory only logged it, so the engine stayed broken and the next command failed with wasm-bindgen's "recursive use of an object". The memory check found it. Every call into the engine worker now goes through one guard, a slice that traps tells the shell at once, and calls on a replaced engine are released instead of waiting for ever.
- Running out of memory after a workbook had opened still said it was too large to open, and left the engine broken. It now says what happened, starts a new engine and offers recovery.
- Tauri's own file associations would have taken `.xlsx` from Excel on install, and on uninstall left an empty default behind that hides Excel's. NSIS hooks register the types instead, and leave the default to you.
- The speed books had no `[Content_Types].xml`, which our reader doesn't need and Excel does. The generator writes it now.

**Known gaps**

- The real double-click in Explorer needs CashGear Sheet picked once as the `.xlsx` app, which only you can do. The installer test covers the shell's path to the app through the install's ProgID.
- Code signing uses a throwaway certificate until Azure Artifact Signing is set up, so SmartScreen warns. The release is drafted on GitHub, and publishing it is yours.
- `sheet.cashgearerp.com` needs its DNS record and the custom domain added in Azure (`docs/DEPLOYMENT.md`). Until then the app deploys to `proud-desert-008137810.4.azurestaticapps.net`.
- About 10M formulas is the ceiling. Reading a book that big takes over 4 minutes, and one past the ceiling takes 8 minutes to say so, as nothing predicts it before reading.
- Saving still holds the engine worker for the whole save (Phase 3's gap), so tiles wait meanwhile on large books.
- The installed web app doesn't take `.xlsx` files from the system (no `file_handlers` yet). macOS, Safari and Firefox weren't run.

## Phase 10: Print and PDF export

Sheets print and export to PDF with Excel's page setup, and their pages break where Excel 2016 breaks them. Measured on 2026-11-07 on the reference machine, in Chrome 156, with Excel 2016 (Office Professional Plus, Click-to-Run 16.0.19127.20860, x86) printing to Microsoft Print to PDF, and Formualizer pinned at fork commit `4dd2e77a`.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Page breaks match Excel's over COM on generated books covering every page setup option, and page counts match on the public files that hold page setup | Pass on generated books. Public files 91.3%, where you accepted 91.4% on 2026-11-07 | `bench/run-page-breaks.mjs` builds 36 books from `bench/print-books.mjs`: Letter, A4, Legal and A3 in both orientations; 50%, 75% and 130%; Fit To one page, one wide, one tall, 2 by 3 and over manual breaks; wide, narrow and odd margins; title rows and columns; manual breaks; hidden and resized lines; a row taller than a page; headings, gridlines and centring; over-then-down order; one and two print areas and a print area of whole rows; a range starting late; a formatted far cell; a 16-point and an Arial 10 normal font; a default column width; and everything at once. Excel reads its breaks with `GET.DOCUMENT(64)` and `(65)`, as `HPageBreaks` and `VPageBreaks` leave some out. 36 of 36 books have Excel's row breaks, column breaks and page count. **Public files:** 1,154 public books hold page setup; Excel opens 1,114 of them and the app reads them all. 1,721 of their 1,886 sheets (91.3%) have Excel's page count. The figure you accepted, 91.4%, came from a replay of the app against Excel's earlier counts. Excel's own count changed between its two runs on 10 sheets (LOGNORM from 4 to 6 pages, tdf137091 from 14 to 20), which accounts for the two sheets between them. The rest are listed under known gaps |
| PDFs are reviewed side by side with Excel's `ExportAsFixedFormat`, and you sign off. `qpdf --check` finds nothing, and text extracts in reading order, Arabic included | Pass: you signed off on 2026-11-07 | `bench/run-pdf-review.mjs` has Excel export 8 workbooks (the print, formatting, Arabic and pivot fixtures and 4 generated books) and the app export the same through File › Export as PDF, then draws both with pdf.js into one page, [published for review](https://claude.ai/artifact/WsDmJf9vUVffLYF6XZKsUA). All 8 have Excel's page count (4, 3, 2, 2, 12, 12, 12 and 14 pages), and qpdf 12.4.2 finds nothing in any of the app's PDFs. pdf.js reads the Arabic page's text back in reading order, every Arabic letter right. The review found five things, fixed before sign-off: gridlines and headings now print black as in Excel, `&D` prints the year in full, the dots under a final yeh were cut off by the cell, a cell starting with a number drew right to left, and Excel crashed opening the print fixture, whose header picture lacked its shape formulas |
| Page setup made here round-trips through Excel, and the matrix has no repair prompts | Pass | `bench/run-page-setup-roundtrip.mjs` makes six books in the app: landscape A4 fitted a page wide; scale, page order and first page number; margins and centring; sheet options; headers and footers with every field, fonts and different first, odd and even pages; and print areas, titles and breaks. Excel opens each with no repair, reads every setting as the app wrote it, and saves it again, and the app reads Excel's copy back the same: 6 of 6. `bench/run-excel-matrix.mjs`, with the print fixture added: 112 edits on each of 9 fixtures, among them page setup, print areas, titles and breaks. 1,008 of 1,008 saved files open in Excel with no repair, all 12 broken controls are refused, and 22 of 22 scenarios, 20 of 20 generated books and 4 of 4 large books are clean. |

**What else was checked**

- Fixtures: `bench/run-excel-text.mjs` on the 9 fixtures, the new print fixture included: all open, and 1,896 of 1,896 cells match Excel's text.
- Saving: page setup is saved by editing the file's own `pageSetup`, `pageMargins`, `printOptions`, `headerFooter`, `rowBreaks`, `colBreaks` and `sheetPr`, so attributes the app doesn't know, such as the printer settings' `r:id`, stay, and a sheet nobody changed keeps them byte for byte (`crates/cgs-engine/tests/print.rs`).
- Browsers: 226 of 226 end-to-end tests pass, 113 each in Chrome and Edge. The new `tests/e2e/print.spec.ts` sets a page up from the Page Layout tab and the Page Setup dialog, previews it in File › Print, exports a PDF and reads its text back, prints through the browser, and checks Normal view's dashed break lines against Excel's breaks.
- Unit tests: 177 in Vitest and 597 in Rust, among them pagination against Excel's measured breaks, page setup reading and writing with undo, header and footer codes, the PDF writer's shaping, subsets and marks, and the page limit.
- WASM gzip: 4.10 MB, against the 5 MB budget. The bundled PDF fonts (7.8 MB) load only when a PDF first needs them.
- Licences: pdf-writer, subsetter, rustybuzz, unicode-bidi and miniz_oxide are MIT or Apache-2.0. Carlito, Caladea, Liberation and Noto Sans Arabic are OFL-1.1, shipped unmodified with their licences and listed in the third-party notices. qpdf is a test tool and never ships.
- CI: green on `main` (build and test on Windows, and the licence gates), and the tags were pushed after it.

**Found and fixed on the way**

- Excel lays pages out in its printer's dots, not in points: rows print about 3% shorter and columns 8% wider than on screen, by factors that follow the workbook's normal font. Measured over COM for 24 fonts and sizes (`Units` in `crates/cgs-engine/src/print.rs`).
- Excel prints from A1, keeps manual breaks along a side Fit To leaves automatic, and takes the default row height from the normal font. A cell in the normal font made its row taller than the default, and rows of fonts under 12 points were a pixel taller than Excel's.
- What prints runs on through empty cells that text overflows into and covers one-cell and absolute pictures, but leaves out drawings that are hidden or set not to print. A page setup scale of 0 prints at 100%.
- A sheet styled to its last cell read 17 billion cells and laid out 38 million pages, which froze the engine. Values are now read only as far as they go, and pages past 10,000 are counted but not laid out.
- Noto Sans Arabic draws dots as marks that many letters share. A PDF needs a code for each glyph and its text, and marks drawn as outlines, or copying gives the wrong letters.

**Track U**

None this phase. The Page Layout tab uses the library's existing icons; print icons are a Track U job for a later phase.

**Known gaps**

- Public page counts: 165 of 1,886 sheets differ: Excel prints more pages on 91 and fewer on 74. The causes are: pivot tables Excel refreshes on open, which change their size before it counts; rows Excel makes taller to fit wrapped or larger text it measures itself; text overflow estimated from Calibri's widths; Excel skipping some blank pages and not others; and fonts whose factors weren't measured. 40 books Excel refuses to open, so they aren't compared. Excel's count isn't steady on some books, changing between runs on 10 sheets.
- From the review: Excel moves the table below a header picture taller than the top margin, and the app doesn't. Excel prints the gray125 pattern as solid grey and its thin borders slightly heavier. Arabic text overflowing left from a right-aligned cell starts a few pixels further right in the app.
- Overflowing text is measured with Calibri's widths scaled to the cell's font, so a long text in another font can reach a column more or less than in Excel. Text overflowing from a 255-character-wide column prints further in Excel than the text reaches.
- Column factors for normal fonts that weren't measured follow Calibri 11's.
- Comments don't print, at the end of the sheet or as displayed. The setting is kept and saved.
- Page Break Preview and Page Layout view aren't in this phase. Normal view shows the breaks as dashed lines.
- Chart sheets print as one landscape Letter page each, with Excel's normal margins. Their own page setup isn't read.
- Headers and footers keep pictures already in a file, but pictures can't be added or changed here, as decided.
- In a browser, installed fonts reach a PDF only once Local Font Access is granted. Otherwise Calibri, Cambria, Arial, Times New Roman and Courier New are drawn as their bundled twins with the same widths.

## Phase 11: RTL sheets

Right-to-left sheets look and work as in Excel 2016, save as such and print mirrored. Measured on 2026-11-08 on the reference machine, in Chrome 156 and Edge, with Excel 2016 (Office Professional Plus, Click-to-Run 16.0.19127.20860, x86), and Formualizer pinned at fork commit `4dd2e77a`. Answers are in `docs/decisions.md`: the Arabic UI later, new sheets left to right with a default direction setting, drawings placed from the right as Excel places them, and right-to-left breaks exact on generated books and public files.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Right-to-left sheets drawn by Excel's `CopyPicture` and by the app, in light and dark, are reviewed side by side, and you sign off | Pass: you signed off on 2026-11-08 | `bench/run-visual-review.mjs --rtl` has Excel 2016 draw three books with CopyPicture: the new right-to-left fixture (`rightToLeftBook` in `tests/fixtures/workbooks.mjs`: borders, fills, a diagonal, rotated and wrapped text, a note, a chart, data bars, an icon set, an autofilter and a frozen row), and the formatting and Arabic fixtures set right to left. The app draws the same ranges in Chrome in light and dark. `bench/run-pdf-review.mjs --rtl` adds Excel's `ExportAsFixedFormat` beside the app's PDFs. Both are on one [review page](https://claude.ai/artifact/BHmVfsmEp5dKHifKwWShqY). What Excel mirrors, the app mirrors: cells, borders, gridlines, data bars, notes' corners, filter and list buttons, and drawings placed from the right. What it leaves, the app leaves: alignment, icon sets and diagonals. Rotated text turns the other way, and text overflows on its aligned side |
| Keyboard and mouse e2e tests pass on mirrored sheets, and every left-to-right golden is unchanged | Pass | `tests/e2e/rtl.spec.ts`, 13 tests in each browser: arrows and Ctrl and Shift+arrows go the way they point, and Tab, Shift+Tab and Home go by column order, as Excel 2016 does under SendKeys; typing then an arrow; clicks with column A and the row headings on the right; dragging a selection and the fill handle at the bottom left; widening a column from its left edge; the editor growing left from its cell's right edge; saving, undoing and reading back; pages printed mirrored; the default direction for new workbooks and sheets; and four new goldens (formatting and Arabic, light and dark), cropped from the canvas's right. CI on `a4de8d2`: 252 of 252 end-to-end tests pass, 126 each in Chrome and Edge. No left-to-right golden changed in this phase (`git diff --stat phase-10..HEAD -- '*.png'` lists only the four new ones) |
| `rightToLeft` round-trips through Excel, page breaks of right-to-left sheets match Excel's, and the matrix, rules check and fuzz stay clean | Pass | `bench/run-rtl-roundtrip.mjs`, 6 cases: a sheet turned right to left beside ones left alone, one turned back, one edited and kept, a new workbook with a new right-to-left sheet, the Arabic sheet turned and turned back, and the moving parts turned. Excel opens each with no repair and reads every sheet's direction as the app wrote it, drawings stay in their cells, and the app reads Excel's own save back the same: 6 of 6. `bench/run-page-breaks.mjs --rtl`: 36 of 36 generated books set right to left have Excel's row breaks, column breaks and page count, and so do the 2 public files whose sheets are right to left. `bench/run-excel-matrix.mjs` with four direction edits added: 116 edits on each of 11 fixtures, the two new right-to-left ones included, 1,276 of 1,276 saves open with no repair, all 12 broken controls are refused, and 22 of 22 scenarios, 22 of 22 generated books and 4 of 4 large books are clean. `bench/run-excel-rules.mjs`: 43 of 43 cases and 1,572 of 1,572 cells agree. `bench/run-edit-fuzz.mjs`, seeds 1 and 2 with 250 episodes each: 0 steps diverged, and 414 of 414 saves open with no repair and match Excel's own result |

**What else was checked**

- Fixtures: `bench/run-excel-text.mjs` on 11 fixtures, the two right-to-left ones included: Excel opens all, and 3,489 of 3,489 cells match its text.
- PDFs: Excel and the app give the same page counts for the six right-to-left books (4, 2, 12, 12, 12 and 14 pages), the block sits against the right margin with the headings on the right as in Excel, and qpdf finds nothing in any of the app's PDFs.
- Saving: `rightToLeft` is written by editing the file's own `sheetView`, only where a sheet's direction changed, so a sheet nobody turned keeps its bytes (`crates/cgs-xlsx/src/patch_sheet_tests.rs`). A new PivotTable sheet takes its source's direction, and a new blank workbook can start right to left.
- Unit tests: 182 in Vitest and 603 in Rust, among them right-to-left tile drawing, overflow from the other sides, pages placed against the right margin, and the direction saved, read back and undone.
- WASM gzip: 4.31 MB, against the 5 MB budget.
- CI: green on `main`, and the tags were pushed after it.

**Found and fixed on the way**

- Excel 2016 doesn't mirror alignment: Latin text stays left and numbers right on a right-to-left sheet, and text overflows on its aligned side, so Latin text in column A is cut at the sheet's edge. Icon sets and diagonals aren't mirrored, and rotated text turns the other way.
- Excel prints a right-to-left sheet with the same breaks as left to right, against the right margin, with the headings on the right, and doesn't swap the header and footer sections.
- Notes' red corners printed on the app's pages. Excel never prints them, in either direction, so they no longer do.

**Track U**

None this phase. Sheet Right-to-Left uses the library's `align-right` icon; a proper icon is a Track U job for a later phase.

**Known gaps**

- Column A's left border and row 1's top border never draw, in either direction, because a tile carries only each cell's right and bottom lines. It shows most on a printed right-to-left sheet.
- From the review, not about direction: the chart shows a title where Excel shows its cached empty series, the app doesn't draw Excel's green error triangle, wrapped and rich text can break a word where Excel's measured widths differ, and Excel prints A4 sheets on Letter through Microsoft Print to PDF.
- The app's own UI stays in English and left to right, as decided; an Arabic UI is under Later in `docs/PLAN.md`.

## Phase 12: Large books

Saving no longer holds the engine, a book past the ceiling is refused in seconds, big books read in well under half the time, and the installed web app opens files from Open with. Measured on 2026-11-08 and 2026-11-09 on the reference machine, in Chrome 156 and Edge, with Excel 2016 (Office Professional Plus, Click-to-Run 16.0.19127.20860, x86), and Formualizer pinned at fork commit `b534673d`. Answers are in `docs/decisions.md`: a save in slices, edits queued behind it, refusal only when clearly past the ceiling, and Open with checked by an automated test.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| During a save of the 25 MB formula book, tiles and cell reads are served within 100 ms at p99, edits made meanwhile land after it with the right answers, and the saved file opens clean in Excel | Pass | `bench/measure-save-reads.mjs`, release build in Chrome: the 2.58M-formula book is opened and edited (A1 = 7), then saved through the app's own Save path (18 s) while the page asks, one after another, for a cell's text, an engine tile and a drawn tile at random rows. During the save, 794 requests: p50 7.1 ms, p95 24.4 ms, **p99 34.1 ms**, the slowest 75.8 ms, none over 100 ms (cells p99 13.4 ms, engine tiles 21.6 ms, drawn tiles 37.1 ms; with nothing going on, p99 21.9 ms). At 25% of the save an edit (A2 = 11) is made: it waits 13.9 s, lands after the save, and row 2 reads 11, 1, 11, 12, 12, with the workbook marked as changed again. Excel opens both saves with no repair and all 4,300,000 cells: the first shows row 1 as 7, 7, 49, 50, 50 and row 2 as before (2, 1, 2, 3, 3), and a second save shows row 2 as 11, 1, 11, 12, 12 |
| The 15M-formula book is refused within 30 s, and every book on the ladder still opens | Pass | `bench/measure-scale.mjs` on the whole ladder: the 15M book is refused **3.9 s** after it is chosen (Phase 9: 496 s, running out), with "This workbook is too large to open here: its cells would need about 6.1 GB, and a workbook here can use up to 4 GB." Every other book opens, recalculates and takes two edits, with the right answers in every formula book (table below). `bench/measure-out-of-memory.mjs`: refused after 4.1 s, the engine still holds 74 MiB, and the next file opens |
| The 10M-formula book is ready in under 175 s | Pass | **106.6 s** (Phase 9: 264 s, so 60% less), in 3,857 MiB (Phase 9: 4,042 MiB). A first run the same day gave 110.1 s |
| The installed web app opens an `.xlsx` from Open with, in Chrome and Edge | Pass in Chrome; in Edge the app starts from Open with, but Edge under automation turns the file away | `tests/e2e/pwa.spec.ts`, "the installed web app opens a workbook from Open with", in both browsers locally and in CI: the manifest offers the five types and the browser reports them as the installed app's file handlers; the ProgID the browser registered appears in `.xlsx`'s Open with list; ShellExecuteEx opens a workbook through it, as Explorer's Open with does. Chrome asks once whether the app may open these types, the test presses Open, and the app's own window shows `Launched.xlsx` with `Item 1` in A1, standalone, and lists it in recent files. In Edge, Open with starts the installed app in its window, but Edge settles that question by itself as a no under automation and then refuses files even from its own DevTools launch, so the file reaching the app is checked in Chrome only. A real Open with in Edge is yours to try |

**The scale ladder**

Chrome 156, the release build, one book at a time, on 2026-11-09. Ready means everything is read and the graph is built; Phase 9's figure is in brackets.

| Book | File | First sheet shown | Ready | Ctrl+Alt+F9 | First edit | Next edit | WASM memory |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1M formulas | 9.9 MB | 4.3 s | 8.9 s (10.9) | 1.8 s | 194 ms | 46 ms | 398 MiB (409) |
| 2M formulas | 19.7 MB | 8.0 s | 16.7 s (20.1) | 3.4 s | 243 ms | 45 ms | 766 MiB (808) |
| 2.58M formulas (Phase 2's 25 MB) | 25.4 MB | 10.4 s | 21.2 s (29.9) | 4.4 s | 274 ms | 62 ms | 971 MiB (1,045) |
| 4M formulas | 39.4 MB | 7.8 s | 34.2 s (62.3) | 8.2 s | 409 ms | 51 ms | 1,522 MiB (1,525) |
| 6M formulas | 59.1 MB | 11.3 s | 50.6 s (112) | 12.0 s | 556 ms | 58 ms | 2,237 MiB (2,447) |
| 8M formulas | 78.8 MB | 10.6 s | 74.8 s (171) | 16.6 s | 704 ms | 59 ms | 2,766 MiB (2,827) |
| 10M formulas | 98.5 MB | 9.3 s | 106.6 s (264) | 23.0 s | 795 ms | 49 ms | 3,857 MiB (4,042) |
| 15M formulas | 147.7 MB | Refused at 3.9 s (ran out at 496 s) | | | | | |
| 16M values (Phase 2's 100 MB) | 101.3 MB | 9.5 s | 18.5 s (16.7) | 5 ms | 889 ms | 62 ms | 266 MiB (231) |
| 50M values | 318.8 MB | 9.9 s | 55.1 s (68.4) | 7 ms | 2.3 s | 57 ms | 714 MiB (697) |
| 100M values | 637.6 MB | 10.1 s | 110.8 s (120) | 7 ms | 4.5 s | 44 ms | 1,418 MiB (1,379) |

Memory now grows 64 MiB at a time, so a reading can be up to that much over what is in use.

**Memory budgets**

`bench/measure-memory-budget.mjs` on the 10M book: open at 3,857 MiB, the status bar says "This workbook uses 3.8 of the 4 GB the engine can have: save your work before adding much more". Filling columns of formulas runs the engine out after 118.5 s with "The engine ran out of memory: a workbook here can use up to 4 GB. A new engine is starting, and your edits since the last save can be recovered.", the recovery dialog offers the 4 edits, and a new workbook then opens and calculates. Undo (256 MiB) and drawn tiles (512 MiB) are unchanged.

**Where reading went, and what changed**

`bench/profile-read.mjs` runs Chrome's sampling profiler in the engine worker while a book is read, on a build that keeps function names (`node scripts/build-wasm.mjs --release --profiling`). On the 2M book (20.8 s to ready on that build):

- 16% was `memory.grow`: dlmalloc grows memory by what each request needs, and each grow costs about a quarter of a millisecond in Chrome. The engine's allocator (`crates/cgs-wasm/src/alloc.rs`) is now dlmalloc with memory grown 64 MiB at a time.
- 10% was Formualizer copying its whole staged formula index, every sheet's entries, before building each sheet's graph, only to put it back if the build failed; with four sheets that copied 25M entries for the 10M book. Fork commit `b534673d` takes the sheet's part out whole instead.
- Staging each formula copied its sheet's name twice and lowercased its text. Neither happens now.

On the same build the 2M book then took 16.9 s and the 4M book 36.4 s. Building every sheet's graph in one step instead of one sheet at a time was tried and gained nothing (37.5 s), so the build stays divided by sheet.

**What else was checked**

- Saving: the sliced writer is the same code as before with points where it may stop, and `crates/cgs-engine/tests/sliced_save.rs` checks that a save in slices of no time writes exactly the bytes of a save in one go, that between slices cells read as they were while commands and calculation are refused, that the check of the saved file runs in slices and catches a damaged byte, and that a cancelled save leaves the edits to save. Inserted rows and shared strings are streamed in slices too.
- Refusal: `crates/cgs-engine/src/size.rs` checks that a sample overestimating a sheet is counted before anything is refused. The 10M book's sample stays under the line, so it is never counted in full.
- Fork: Formualizer's own tests, 3,353 of 3,354, the one failing reading its own source with LF line ends on this CRLF checkout, whatever the change.
- Unit tests: 182 in Vitest and 612 in Rust. Clippy is clean natively and for WASM.
- WASM gzip: 4.35 MB, against the 5 MB budget.
- CI: green on `main`, and the tags were pushed after it.

**Found and fixed on the way**

- The speed books had been written before the generator added `[Content_Types].xml`, and it skips books that exist, so Excel refused them and every save of them. They were written again.
- A save ran outside the command queue, so an edit made between the save and the workbook's move to the saved file could be dropped by the move. The save, the file's replacement and the move are now one job in the queue.

**Track U**

None this phase.

**Known gaps**

- Edge: Open with starts the installed app, but whether the file reaches it was checked in Chrome only, as Edge under automation refuses the files itself. Chrome asks the user once before an installed app opens a type, as it should.
- The first edit's graph build, a sheet at a time, still can't be divided: on the 10M book each sheet holds the worker for about 20 s once reads have gone quiet, and tiles not yet drawn wait meanwhile.
- About 10M formulas is still the ceiling. A book between about 10M and 12.5M formulas isn't refused early, and can still run out while reading, as before.
- During a save, typing still opens the cell editor, and what is typed waits; nothing says that the edit is waiting other than the status bar's "Saving n%".

## Phase 13: Excel parity polish

All ten candidates are in, in the order you chose them, each measured against Excel 2016 over COM before it was built. Measured on 2026-11-09 on the reference machine, in Chrome and Edge, with Excel 2016 (16.0), and Formualizer pinned at fork commit `4b581d3`. Answers and defaults are in `docs/decisions.md`: all ten candidates, Excel's SHA-512 password hash, and key tips by tapping Alt.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Each chosen candidate has an e2e test in Chrome and Edge, and an Excel COM check wherever COM can judge it | Pass | `tests/e2e/parity.spec.ts`, 26 tests, each passing in Chrome and Edge (table below). `bench/run-parity-check.mjs` makes each case through the engine, saves it, has Excel 2016 open the file over COM with no repair and read back what the case expects, and has the app read Excel's own save of it the same: **15 of 15 cases agree**. Key tips are a browser matter COM can't judge, so they have the e2e tests only |
| The matrix opens every saved file with no repair, the rules check passes, and the fuzz has 0 diverged | Pass | `bench/run-excel-matrix.mjs`: 116 edits on each of 11 fixtures, **1,276 of 1,276 saved files open with no repair**, all 12 broken controls are refused, 22 of 22 scenarios and 22 of 22 generated books are clean, the 9 feature checks (charts, pivot tables and macros kept after rows go in, pivot refreshes) pass, and 4 of 4 large books (20.5 to 54.4 MB saved) open clean. `bench/run-excel-rules.mjs`: **51 of 51 cases and 1,793 of 1,793 comparisons** agree with Excel, among them Phase 13's colour and icon sorts and filters. `bench/run-edit-fuzz.mjs`: 906 of 1,000 steps match Excel, **0 diverged**, 76 not reached past a step both refused; 166 of 166 saves open with no repair and match Excel's own result |

**Each candidate**

| Candidate | e2e, in Chrome and Edge | Excel over COM |
| --- | --- | --- |
| 1. Hiding and showing sheets; zoom saved per sheet | Hide moves to the next sheet and Unhide brings it back, both saved and undone; each sheet keeps its own zoom without being a change | Sheet states and the active sheet as Excel moves them; zoom per sheet; a fixture's sheet hidden beside its charts |
| 2. Sheet and workbook protection, Format Cells' Protection tab | Protect Sheet keeps locked cells, lets unlocked ones change and takes its password; Protect Workbook locks the sheets; a sheet can keep its cells from being selected | Excel reads every option and both passwords, and takes the passwords; with no password; with nothing selectable around a fixture's edits; taken off again |
| 3. Notes in a box, Show/Hide and Show All, Remove Hyperlink, user name | A note is written beside its cell by the user name and can show all the time; Remove Hyperlink from the cell menu | Notes' text, author, box and showing, as Excel 2016 gives them; Remove Hyperlink and Clear Hyperlinks |
| 4. Sort and filter by colour and icon | The filter menu sorts and filters by the colours its column has | In the rules check: fills, font colours, conditional colours and icons, sorted and filtered |
| 5. Error triangles, Error Checking, circular references | A smart tag offers the fix and Error Checking goes from cell to cell; a loop gets Excel's warning and the status bar names it | The rules Excel finds each cell breaks, and those ignored here |
| 6. Rich text, superscript and subscript | Characters selected while editing take their own font and keep it when copied | Each character's bold, italic, superscript and subscript as Excel reads it, typed, pasted and moved |
| 7. New and Modify Cell Style, the book's styles in the gallery | New Cell Style takes a cell by example, and Modify changes the cells linked to it | Built-in and new styles linked as Excel links them, Style Includes, and Modify's change on its cells |
| 8. Key tips | A tap of Alt shows the tabs' tips, letters open a tab and run its commands, two-letter tips, Esc, FF to the font box; Alt and a tab's letter, and F to the File menu | (A browser's keys; COM can't judge them) |
| 9. Pivot grouping, Drill Down, Report Filter Pages, GETPIVOTDATA | Group Field by period, Group Selection by hand, Ungroup; a click writes GETPIVOTDATA, a double-click shows details, filter pages copy the table | Excel refreshes the date, number and hand-made groups made here and shows exactly the app's labels; Excel reads the details table, the sheets in Excel's order with each page's filter, and calculates GETPIVOTDATA to the app's values |
| 10. Chart trendlines and a Picture Format tab | Add Chart Element › Trendline draws the fit and saves it as Excel writes it; Picture Format turns, sizes and resets a picture and gives it alt text | Excel reads the trendline's type, periods and name, and each picture's turn, flip, alt text and order |

**Excel 2016, measured first**

Each candidate began with a COM probe of what Excel 2016 does and writes; the defaults that follow from them are in `docs/decisions.md`. Among what the probes found: Excel saves a hidden active sheet's neighbour as the active one and moves `firstSheet`; it hashes passwords with SHA-512 and 100,000 spins; it writes a filter's colour as a differential format's `fgColor` with a solid fill; it groups dates into new fields named "Months (Date)" even for one period; it puts Show Report Filter Pages' copies in the field's order before the pivot table's sheet, making the last first; it puts Show Details' records in a TableStyleMedium2 table on a new sheet to the left; it writes a new trendline dotted and 1.5 points wide in its series' colour; and it turns a picture with `a:xfrm rot` and keeps its alt text in `descr`.

**What else was checked**

- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match.
- The full e2e suite: 288 of 288 in Chrome and Edge.
- Unit tests: 186 in Vitest and 647 in Rust; the library's 1,019. Clippy is clean natively and for WASM.
- Fork: Formualizer's custom functions can now take a reference's place (`register_custom_function_with_references`, commit `4b581d3`), for GETPIVOTDATA; its custom function tests pass, the new one among them.
- Library: `CashGear.React.UI` gained `CgRibbon` key tips and the Phase 13 icons, at `ef1277e` on its branch.
- CI: green on `main`, and the tags were pushed after it.

**Found and fixed on the way**

- A table made on a sheet added since opening wasn't saved, since that sheet had no part yet when tables were saved. Show Details found it.
- A second new pivot table on one sheet replaced the first's link in the sheet's new relationships, so Excel saw only one. The grouping check found it.
- Excel refused a file with conditional formats and a filter made here: new sheet elements weren't written in the schema's order. They are now sorted.
- A filter by colour read back inverted in Excel, until the differential format's colour was written as Excel writes it.
- A Phase 13 test raced on CI's slower runner, typing a note before its box had the keyboard; it waits for it now.

**Known gaps**

- Show All Notes isn't saved, by design: Excel keeps it in the application, not the file.
- Cell styles: Duplicate and Delete on a style's menu aren't here, and a right-click opens Modify directly where Excel shows a menu.
- Error checking: Excel's Circular References submenu, which lists the loops' cells to go to, isn't here; the status bar names the first.
- Pivot grouping offers Days, Months, Quarters and Years, not Seconds, Minutes and Hours; a group of groups isn't made here; Show Details doesn't fit its columns' widths as Excel does.
- GETPIVOTDATA recalculates with everything rather than only when a pivot table changes, as Excel's does.
- Trendlines: a forecast's periods past the last point aren't drawn, nor the equation and R² labels; a new trendline replaces the series' others; Excel asks which series in a dialog, and here it's the one selected or the first.
- Picture Format has no Crop, corrections, colour, effects or picture styles.
- Rich text: Paste Special's other choices paste plain text.
- Key tips: menus opened by a tip don't show tips of their own.
- Outside this phase: nothing new.

## Planning the ERP integration

- The plan was approved on 2026-11-09 and saved as `docs/ERP_SHEET_INTEGRATION_PLAN.md`, with Phases 14 to 19 and Track E added to `docs/PLAN.md`. Answers and defaults are in `docs/decisions.md`.
- It was drawn from reading this repo's crates, engine client, UI and hosting, and the ERP's `CashGearERP.sln` and `ERPClient_React`, read-only. Nothing was built or run, and nothing in the ERP repo changed. Section A of the plan lists what was found, with file references.

## Phase 14: ERP contract and mock

The contract the sheet and Cash Gear ERP build against, proven by a suite before any code depends on it. Done on 2026-11-09. Nothing in the app changed. Answers and defaults are in `docs/decisions.md` (2026-11-09, Phase 14): the Vendors field list, Kit as Track E's test tenant, Track E on the ERP repo's `main`, and the defaults Track E0's review led to.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| The contract suite passes on the mock in CI | Pass | `packages/erp-connector/src/contract.test.ts`: 55 cases in `pnpm test`, against a mock it starts on a free port. **55 of 55** locally and in CI on `main`; the tags were pushed only once CI was green. Against a mock started by hand with `ERP_BASE_URL`: 55 of 55 with `ERP_BASE_IS_MOCK=1`, and 34 passing with 21 skipped given only an editor key, as a real server with one key would run it |
| You sign off the contract and the rollout plan | Pass | Signed off on 2026-11-09, after Track E0's changes and with the Blazor host (`assistoWebApp`) in the rollout plan |

**The suite against plan G's Phase 14 cases**

| Case | Where |
| --- | --- |
| Ticket single use and expiry | `tickets`: redeem once; expire after 120 s (mock clock); unknown tickets answered as used ones; a refused Origin doesn't use a ticket up; a ticket alone calls nothing; an API key, not a session, makes one; View is needed |
| Session idle and maximum expiry, and End | `sessions`: 30 minutes idle and 10 hours; idle expiry, and only Renew slides it; ended at the maximum however often renewed; End; ended when the API key is revoked; an API key refused on the entity routes |
| Schema shape | `schema`: the vendors' key, limits, fields, labels and lookups; what an editor, an AdvEdit user and a viewer may edit, and why the rest is read-only |
| Keyset paging | `query`: pages by id to the end with every record in its wire formats, small pages agreeing with large ones, search and field filters, ids in id order, and limits refused |
| A dry run writes nothing | `commit`: checks everything, answers with versions unchanged, leaves the records and CommitStatus untouched; it also reports field errors and conflicts as a commit does |
| A commit is atomic | `commit`: one bad record writes none; every field error is listed before anything is written |
| 409 lists every conflict | `commit`: two of three records changed by another session give two conflicts, with the values and version now and the clashing fields; strict even when other fields changed; an edit elsewhere in the ERP is a conflict (mock) |
| An idempotent retry writes once | `commit`: the same commitId returns the stored body and moves the version once; a response lost after the write is found by CommitStatus and its retry writes once; one lost before it arrives is unknown and its retry writes once (mock) |
| The same id with another payload is refused | `commit`: `commit.reused`, nothing written; commitIds are per company |
| Another company's ids are refused | `query` never shows them though the database is shared; `commit` answers `record.notFound` and `company.mismatch` |
| Read-only and privileged fields are refused | `commit`: read-only and unknown fields are `field.readOnly`; Blocked and Currency need AdvEdit; a viewer is refused, dry run or not; an inactive lookup item can't be chosen but can be kept; a locked record (mock) |
| 413 over the batch limit | `commit`: 501 records answer 413, and 500 are taken |

Also: three cross-origin cases (preflight for the sheet's Origin with `X-Sheet-Session`, none for another Origin, the correlation id exposed), three of the mock's own (429 and 503 with `Retry-After`, a 401 mid-session and latency, an audit row per changed field), and on every response the correlation id, `no-store`, the error body's shape and known error codes.

**Proving the suite.** Each fault was put into the mock in turn, and the suite caught it: no version check failed 3 cases, a dry run that writes failed 1, no idempotency store failed 4, and other companies' ids let through failed 1. The mock was put back unchanged.

**Delivered**
- `packages/erp-connector` (`@cgs/erp-connector`, private, no dependencies): `contract.ts` with every type in plan C, the routes, headers, error codes and status codes; `validate.ts` with a hand-written guard for every response and `recordProblems` for a record's wire formats; 13 guard tests.
- `tests/erp-mock`: `server.mjs` and `seed.mjs`, Node with no dependencies, as plan D.1 describes, and `pnpm erp:mock`.
- `docs/erp-contract.md`: routes, the commit's checks in order, status and error codes, C# DTOs, table scripts, the RowVersion rollout plan, how to run the suite against UnifiedAPI, the mock, and Track E0's review.
- `pnpm-workspace.yaml` needed no change: it already takes `packages/*`.

**Track E0.** The contract was reviewed against `D:\CashGear\Projects\CashGear` read-only: files and `git log` only, with no build, no database and no user secrets, and nothing changed there. Then the Blazor host at `D:\CashGear\Projects\assistoWebApp` was read the same way for the rollout plan. What it changed in the contract: lookups carry `active: false`, `payMethod` is payment terms, `currency` is required, `dateTime` keeps the host's offset, Vendors have no ownership or row locks, the cache is evicted by the `Vendors` tag, CGService's scripts go in IdentitySystem, and script names follow the ERP's. Its notes for E1, E2 and E4 are in `docs/erp-contract.md`.

**What else was checked**
- Unit tests: 254 in Vitest (68 new) and 647 in Rust. Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.5.0 everywhere and its changelog notes.
- `pnpm test:save`, the e2e suite and the desktop build and smoke weren't run locally, as nothing they cover changed; CI ran them on `main`.

**Known gaps**
- The suite hasn't run against UnifiedAPI, whose sheet routes don't exist yet: E1 and E2 build them, and Phases 15 and 17 run the suite on LocalDB.
- The cases that need the mock's clock or failure injection skip against a real server; Track E's WebApplicationFactory tests cover them.
- The mock doesn't model API-key expiry dates, a deactivated company or rate limits, except as injected failures.
- A `dateTime` in the hour a DST change repeats can show an hour out. Those fields are read-only.
- The Blazor host lets Edit change Blocked, where the sheet asks AdvEdit, and its cache sees a sheet commit within 8 hours. Both are the ERP's own rules, left as they are.
- `ERPClient_React`'s git workflow wasn't answered; it is among Phase 15's questions, since E1 is the first to change that repository.
- Outside this phase: nothing new.

## Phase 15: Read-only ERP slice

Cash Gear ERP's vendors open in the sheet from a launch link, as a bound table that sorts, filters and refreshes, with every bound cell read-only. Done on 2026-11-09, with Track E1 in the ERP repos. Answers and defaults are in `docs/decisions.md` (2026-11-09, Phase 15): the ERP's own column order and labels with lookups as codes, refusing a list over 10,000, the load panel's wording, `ERPClient_React` on `main` and pushed, the button opening exactly the list's rows, a server setting that keeps the button hidden per company, and the binding targets carried to Phase 19.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| Rust property tests: random sorts, filters, undo and redo over 1,000 records keep every key with its own record, and every refused path is tested | Pass | `crates/cgs-engine/tests/erp.rs`. `random_sorts_filters_undo_and_redo_keep_every_key_with_its_record` runs 120 seeded steps over 1,000 records of 14 fields (sorts on up to three random keys either way, group filters and clearing them, undo, redo, and refused edits among them), and after every step checks that each row's key holds exactly its own record's values and the binding's id for it. `every_path_that_would_break_a_record_is_refused` refuses typing in a key, a value and a heading, typing into several areas, array input, Clear, fill, paste, Paste Values, cut and paste out of and into the table, external paste, Replace and Replace All, Convert to Number, rows and columns inserted or deleted inside the table, ShiftCells, merges, header off, totals on, a totals function, rename, resize, Convert to Range, an overlapping table, deleting the sheet, Remove Duplicates, left-to-right sorts and sorts of part of the table: each with its message, and nothing changed. `the_guards_inside_the_engine_refuse_on_their_own` replays 26 of them with the command checks off, and the content rule, table and shape guards refuse every one. `formatting_moving_the_table_and_typing_beside_it_are_allowed` shows formatting, Paste Formats, table styles, typing beside the table (no growth, no calculated column) and moving the table by inserting rows above it all pass. 9 tests, and 20 more in `cgs-bind` |
| e2e in Chrome and Edge: open, sort, filter and refresh against the mock; replayed and expired tickets are refused; the fragment leaves no trace; Save As holds no ids or versions | Pass | `tests/e2e/erp.spec.ts`, 6 tests, **12 of 12** in Chrome and Edge, against the mock on its own origin (127.0.0.1:4180, the sheet on :4173 under the preview's COOP and COEP). 2,174 active vendors open in English and are checked row by row against the mock after a Sort Z to A from the Data tab, a filter, and a Refresh that finds an ERP-side change by id. Arabic opens right to left with Arabic headings. Typing, Delete and refused commands show their messages. A replayed and an expired link are refused with the approved wording. The address is `/` with no fragment, and history holds no launch entry. Save As writes no `customXml`, ids, versions, routes or user ids |
| Binding meets its targets, and the contract suite passes against UnifiedAPI on LocalDB | Contract: Pass. Binding: targets missed, accepted by you and carried to Phase 19 | **Contract:** against a real UnifiedAPI process on LocalDB (`CashGearSheet_CGService` and `CashGearSheet_ASTDataTest`, every connection string printed and checked as LocalDB before anything ran), with `ERP_TEST_STAGE=E1` and all five keys: **24 passed, 34 skipped, 0 failed** (tickets 9 of 9 that run off the mock, Status among them; sessions' lifetimes and scope; schema 4 of 4; query 6 of 6, the five-field search among them; CORS 3 of 3). The skipped are Commit (E2), End (E3), and the cases that need the mock's clock or failure injection, which UnifiedAPI's own tests cover. **Binding** (`bench/measure-erp-bind.mjs`, release build in Chrome, 20 fields, median of 5, from the pages' bytes to the grid's first paint): **1,000 records in 697 ms** (target 300 ms) and **10,000 in 2,562 ms** (target 1,500 ms). For 1,000: staging 68 ms, opening the workbook 75, binding 215, fitting 180, showing 40 and painting 115. You accepted these for Phase 15, and Phase 19 keeps B.6's targets (decisions, 2026-11-09, Phase 15) |

**Found and fixed on the way**

- Writing a block of cells was quadratic in its rows: Formualizer regrows every column when a write passes a sheet's last row, and its read of a cell slows as a batch fills a column. 10,000 records took 29 s in Chrome (14.8 s natively). `set_cells` now writes each sheet's farthest value first and skips reading cells known to be empty: 2.6 s in Chrome (0.6 s natively), and loading a CSV gains the same. A contract test checks that a large batch's pre-images stay exact.
- A refused part of `Op::Many` left what went before it applied, with no undo step. It now takes that back, for every command.
- AutoFit leaves a heading's filter button out, as Excel's does, which clipped ERP headings; ERP data fits with room for it.

**Delivered**

- `crates/cgs-bind`: bindings, the schema, record pages read from bytes, typed values, versions as 8 bytes, refresh by id, and the guard's content rule. Serde only.
- `cgs-engine`: `erp_begin`, `erp_page`, `erp_bind`, `erp_refresh`, `erp_settle`, `binding_json` and `binding_ids`; the guard in `apply_cells`, `Op::Tables`, `Op::BookTables`, `apply_axis` and sheet removal, with command checks before them; no growth or calculated columns for bound tables.
- `cgs-wasm` and `@cgs/engine-client`: the same, with record pages transferred as bytes, and no working copy or journal for a bound workbook.
- `@cgs/erp-connector`: `readErpLaunch` and `erpApiBase`, `ErpHttp`, `ErpSession`, `openRecords` and `refreshRecords`, with 15 tests against the mock.
- `@cgs/ui`: `useErpTools` with the load panel, the ERP Data tab (Refresh, Connection Info), the status badge, the window's title, and refusals in a message box and at the first key. The web platform reads and strips `#launch=` before the app renders.
- The contract: the Status route and its per-company switch, Query's search over what the Vendors list searches, and the ERP's column order and labels, in the doc, the mock and the suite, with `ERP_TEST_STAGE` for a server part way through Track E.
- `tests/e2e/erp.spec.ts`, with the mock as Playwright's second web server, and `bench/measure-erp-bind.mjs`.

**Track E1.** Built by a fork of this session in the ERP repos, and checked here.
- `D:\CashGear\Projects\CashGear` (`main`, pushed `6d95cfc4..076933fc`): the contract's DTOs (`4858b81f`); `Create_SheetSessions.sql` and `Add_VendorsRowVersion.sql` as the contract gives them (`75cae54f`); `SheetLaunchController` (Status, CreateTicket), `SheetSessionController` (Redeem), the `SheetSession` scheme with per-request key and company checks, `VendorSheetController` (Schema, Query) with `VendorSheetConnector`, company-strict privileges, the `SheetOrigins` CORS policy, and a middleware for the headers and error body (`7133ee1c`); and `CashGear.UnifiedAPI.Tests`, 47 passing on SQLite and 5 more on LocalDB (`076933fc`). `VendorModel` is unchanged: Query reads `Vendors.RowVersion` with its own SQL.
- `D:\CashGear\Projects\ERPClient_React` (`main`, pushed `d3d797f..a68819d`): the "Edit in Cash Gear Sheet" button, shown only when Status says so (an error or a 404 hides it), and off past 10,000 vendors. 199 tests pass, 13 of them new. Your uncommitted changes there were left out and untouched.
- `CashGear.UnifiedAPI.Tests/SheetContractRun.md` has the steps to run the contract suite against UnifiedAPI on LocalDB.

**What else was checked**

- Unit tests: 273 in Vitest (19 new) and 678 in Rust (31 new). Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match, as in Phase 13.
- `bench/run-edit-fuzz.mjs`: 906 of 1,000 steps match Excel, **0 diverged**, 76 not reached; 166 of 166 saves open with no repair and match Excel's own result, as in Phase 13.
- `bench/run-excel-matrix.mjs`: 116 edits on each of 11 fixtures, **1,276 of 1,276 saved files open with no repair**, all 12 broken controls are refused, 22 of 22 scenarios and 22 of 22 generated books are clean, the 9 feature checks (charts, pivot tables and macros kept after rows go in, pivot refreshes) pass, and 4 of 4 large books open clean, as in Phase 13.
- The full e2e suite: **300 of 300** in Chrome and Edge (Phase 13's 288 and the 12 new).
- `pnpm desktop:build` and `pnpm desktop:smoke`: the desktop app builds, and its smoke run passes in WebView2 157 (goldens within tolerance, overwrite and Save As, recent files, the theme kept).
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.6.0 everywhere and its changelog notes. Release WASM: 4,571,267 bytes gzipped, of the 5,000,000 budget.

**Known gaps**

- Binding misses B.6's targets (0.70 s and 2.56 s, against 0.3 s and 1.5 s); Phase 19 carries them.
- Green number-stored-as-text triangles show on bound mobiles and tax files; hiding them on bound text fields is a Phase 16 task.
- The load panel, ribbon and messages are English for Arabic sessions until Phase 18's catalogue. Headings and lookups follow the ERP user's language.
- Sessions aren't renewed yet (Renew is Track E3, in Phase 18), so a session ends 30 minutes after opening. Refresh then says the session has ended, and so does the badge.
- A bound workbook keeps no recovery copy until Phase 18's capped draft.
- The ERP's button stays hidden for every company: `SheetLaunch:Companies` is empty until Phase 18. Before a company is listed, `Create_SheetSessions.sql` must run in CGService and `Add_VendorsRowVersion.sql` in that company's database. UnifiedAPI with E1 can be deployed before that.
- The icon set has no refresh or information icon, so Refresh uses the circular arrow and Connection Info the link.
- Outside this phase: two `CashGear.Operation.Tests` cases fail in the ERP repo (`ShipOrderPartnerListTests`, `no such column: s.DueDate`), from its commit `129d0dcb` of 2026-11-04, not from Track E1.

## Phase 16: Editing and change tracking

Cash Gear ERP's editable fields can be edited in the sheet, with every field tracked against what the ERP sent and its state on its cell; nothing leaves the sheet yet. Done on 2026-11-10. Answers and defaults are in `docs/decisions.md` (2026-11-10, Phase 16): the amber state colours from the mock-up, formulas flagged and left out until converted to values, local rules as you edit, and lookup dropdowns with code and label.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| A Rust property test over typing, paste, fill, move, replace, sort, filter, undo and redo: keys stay with their records, read-only cells never change, and the changeset equals an oracle diff | Pass | `crates/cgs-engine/tests/erp_edit.rs`, `random_edits_keep_keys_read_only_cells_and_the_changeset_with_the_oracle`: 160 seeded steps over 1,000 records of 14 fields (9 editable): typing one cell or many (leading zeros, `1-2`, `1,23`, formulas, lookups in lower case, inactive and unknown codes, over-long text, decimals past their scale, dates, booleans, and typing read-only cells, which must be refused), internal paste all and values (shifted onto read-only columns, which must be refused), external paste, fill and Ctrl+D, clear, cut and paste, Replace, Convert to Number, sorts on random keys, filters, undo, redo, Discard Changes and typing originals back. After every step each row's key appears once and holds its record's read-only values, the binding knows its id, text fields hold text, and the changeset's records, fields, from and to, formulas and broken rules equal an oracle that reads every cell independently and diffs it with the records as sent, as do the badge's counts. **Proving it:** four faults put in one at a time were each caught: states not updated on writes (step 0), not updated on undo and redo (step 2), a changed date a day out (step 1), and read-only columns taken as editable (step 10). 12 tests in all, and 31 more in `cgs-bind` |
| Bound-sheet input and tiles within 10% of unbound, and normal-sheet benches, the matrix, save checks and fuzz unchanged | Pass | **Bound against unbound** (`bench/measure-erp-edit.mjs`, release build in Chrome 157, 10,000 records of 20 fields, the unbound sheet being the bound one saved and opened again, after the same 1,000-row paste, median of 3 rounds alternating which goes first): key to painted frame p99 **38.1 ms bound, 41.7 ms unbound**; drawn tiles p99 **65.5 ms bound, 80.3 ms unbound** (engine tiles 25.7 and 27.0). The 1,000-row paste of 9 fields took 276 ms bound and 394 ms unbound: local rules and states add nothing measurable, as text fields skip Excel's parsing (target 50 ms). **Changeset**, end to end from the page: **4.3 ms** for 1,000 records of 20 fields each with a change (target 10), 32.5 ms with every editable field changed, and **48.6 ms** for 10,000 (target 100). **Normal sheets**, against v1.6.0 built from its tag and measured on the same machine in the same hour, alternating, two rounds each: the 1M-row filter's median 221 and 232 ms (v1.6.0: 257 and 202); key to paint during the 1M-formula recalculation p99 79.9 and 89.0 ms (v1.6.0: 85.3 and 88.8), and once settled 42.1 and 41.4 (42.0 and 49.2); wheel scrolling 96.8% and 95.7% of frames within 17 ms (97.0% and 97.3%), dragging 91.5% and 91.8% (92.4% and 91.9%). `pnpm test:save`, `bench/run-edit-fuzz.mjs` and `bench/run-excel-matrix.mjs` give exactly Phase 15's results, below |

**Found and fixed on the way**

- The paste-area gap the plan suspected was real: Paste grows a destination smaller than its source, but protection checked only the area it was sent, so a paste into one unlocked cell could write locked cells beside it. Protection, and the ERP guard, now check the area a paste writes (`paste_area`), with a test.
- The mock's Schema sends a decimal's `min` as text (`"0"`), as the contract allows; the schema parser took only numbers, so an editor's launch failed in the browser while every Rust test, built with numbers, passed. Limits now take either.
- Convert to Number turned a bound `001` into 1, which the text rule then wrote back as `"1"`. It now leaves bound text fields alone.
- The changeset built a JSON tree per record (about 5 µs a record) and the worker sent its answer as a structured clone: 14.4 ms for 1,000 records and 114.6 ms for 10,000 in Chrome, past both targets. It now serialises typed parts, reads only the changed records' rows, and query answers cross as text that the page parses: 4.3 ms and 48.6 ms.

**Delivered**

- `cgs-bind`: editable fields from the schema (never the key), `rules` (required, length in UTF-16 units, type, range, scale and 15 digits, active lookup codes), `state` (each record's editable fields modified, invalid or formula, kept only for records with something to show, with the badge's counts; empty and blank equal, decimals compared at their scale), lookup lists with labels, and wire values for the changeset.
- `cgs-engine`: the command checks refuse only what reaches a key, a read-only column or a heading; text and lookup fields keep text on every path (`bound_text_typed` for typing, external paste and Replace, `coerce_bound_values` in `apply_cells` for the rest); states follow every write, undo and redo included; the `erpChanges`, `erpCell` and `erpNextError` queries, the `erpDiscard` command, lookup dropdowns in `validationAt` with no rule in the file; tile flags for Modified, Invalid, Formula, Read-only and a heading's lock (ViewBlock 5), on screen tiles only; no number-as-text triangles on bound text fields.
- `@cgs/engine-client`: the new types, ViewBlock 5, and the states painted in `tile-draw.ts` from the chosen colours, on the leading edge (the right on a right-to-left sheet) and in the light colours on pale fills in dark mode, never on pages.
- `@cgs/ui`: typing allowed in editable cells, Discard Changes (all, or in the selected cells) and Next Error on the ERP Data tab, the badge's counts, hover details, dropdowns with labels, Refresh asking before it replaces changes, and closing the tab asking while there are any.
- Tests: `crates/cgs-engine/tests/erp_edit.rs` (12, the property test among them), `erp_harness` shared with Phase 15's `erp.rs`, rules and states in `cgs-bind`, the paste-area check in `parity.rs`, 5 tile-drawing cases, and 6 e2e cases (12 runs) with state goldens in light, dark and right-to-left. `bench/measure-erp-edit.mjs`.

**What else was checked**

- Unit tests: 278 in Vitest (5 new) and 703 in Rust (25 new). Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match, as in Phase 15.
- `bench/run-edit-fuzz.mjs`: 906 of 1,000 steps match Excel, **0 diverged**, 76 not reached; 166 of 166 saves open with no repair and match Excel's own result, as in Phase 15.
- `bench/run-excel-matrix.mjs`: 116 edits on each of 11 fixtures, **1,276 of 1,276 saved files open with no repair**, all 12 broken controls are refused, 22 of 22 scenarios and 22 of 22 generated books are clean, the 9 feature checks pass, and 4 of 4 large books open clean, as in Phase 15.
- The full e2e suite: **312 of 312** in Chrome and Edge (Phase 15's 300 and the 12 new), the last after a fix to the test. The PWA's Open with failed in Chrome here and on CI, for v1.6.0 too: from Chrome 157 the installed app launches with the file but can't read it from `%LOCALAPPDATA%\Temp`, where the test wrote it (`getFile()` throws a SecurityError). The test now launches its file from its own output folder, and passes in Chrome and Edge.
- `pnpm desktop:build` and `pnpm desktop:smoke`: the desktop app builds, and its smoke run passes in WebView2 157 (goldens within tolerance, overwrite and Save As, recent files, the theme kept).
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.7.0 everywhere and its changelog notes.

**Known gaps**

- Refresh replaces changes, after asking, until Phase 18's three-way merge keeps them.
- Nothing is sent yet: Validate, Review and Commit are Phase 17 (Track E2 builds the Commit route).
- The ERP's messages, hover details and rule texts are English for Arabic sessions until Phase 18's catalogue; the engine names each rule by its contract code so the catalogue can word it.
- A bound workbook still keeps no recovery copy until Phase 18; closing the tab with changes asks first.
- Binding speed still misses B.6 (Phase 15's 0.70 s and 2.56 s), carried to Phase 19.
- Outside this phase: from Chrome 157, a workbook opened with the installed app from a folder Chrome counts as the system's (`%LOCALAPPDATA%\Temp`, where some mail programs open attachments) fails with Chrome's own SecurityError text. A plainer message is a small later fix.
- Outside this phase: on this machine today the 1M-row filter takes about 225 ms and key to paint during the 1M-formula recalculation reaches about 85 ms at p99, in v1.6.0 and v1.7.0 alike, where Phases 4 and 6 measured 151 ms and 44.4 ms on 2026-11-05 and 06. The filter is past its 200 ms target, the input still under its 100 ms. Neither bench was run in Phases 7 to 15, so when it changed isn't known.

## Phase 17: Validate, review and commit

Changes to Cash Gear ERP's vendors reach the ERP safely and atomically: Validate, Review Changes and Commit on the ERP Data tab, with Track E2's Commit route in UnifiedAPI. Done on 2026-11-10. Answers and defaults are in `docs/decisions.md` (2026-11-10, Phase 17): the schema's 500-record limit with Commit Changes in Visible Rows past it, the confirmation's wording, the ERP's errors and conflicts on cells cleared by editing with Committed as a green bar, and `Vendors.RowVersion` still read with UnifiedAPI's own SQL until every tenant database is scripted.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| e2e: a commit leaves the mock equal to the oracle; 400 and 409 write nothing; a dropped response and a retry write exactly once | Pass | `tests/e2e/erp.spec.ts`, "committing ERP data", 4 tests, **8 of 8** in Chrome and Edge against the mock on its own origin. **Oracle:** four fields typed into four vendors (a name, a mobile, a lookup, an address); Validate's dry run says "Cash Gear ERP checked 4 changes to 4 vendors and found no problems." and leaves the mock's writes, commits and audit untouched; Review lists the four with their originals; its Commit goes through the confirmation's agreed wording; after "Committed 4 changes to 4 vendors." the mock shows 4 writes, 1 commit and 4 audit rows, every record's fields equal the oracle, versions moved for exactly the four (Modified by `u17`), the sheet shows what the ERP persisted row by row, the counts are all 0, committed fields carry the green bar until edited, and Ctrl+Z does nothing. **400:** the mock's locked V-1005 is refused (`record.locked`), with the agreed panel text, the mock's state unchanged, one invalid cell whose hover names the code. **409:** another ERP user changes V-1006 through the mock; the commit is refused with the agreed text, nothing written beyond that user's edit, the badge reads "Vendors · 8 · 1 changed · 1 conflict · Sara @ Kit", the cell's details name `omar` and the value now, and Commit then refuses before sending; a Validate that finds V-1005's error keeps the conflict. **Lost answer:** every answer to Commit dropped after the write: the panel says it isn't known whether the changes were saved, the mock shows the 2 writes once, typing into the table is refused ("Wait until the commit to Cash Gear ERP finishes."), and Try again with the same id answers from the store: still 2 writes and 1 commit, values saved. **Limit:** 601 changed vendors are refused with the agreed text; filtered to one group, Review lists the rest under "In hidden rows (375)" and Commit Changes in Visible Rows confirms the 226 shown. The full suite: **320 of 320** in Chrome and Edge (Phase 16's 312 and the 8 new) |
| The contract suite passes against UnifiedAPI on LocalDB, conflicts and idempotency included | Pass | Run by the Track E2 fork as `CashGear.UnifiedAPI.Tests/SheetContractRun.md` describes, with `ERP_TEST_STAGE=E2` and all five keys, every connection string printed and checked as `(localdb)\MSSQLLocalDB` first: **43 passed, 15 skipped, 0 failed** (58). All 19 Commit cases that run against a real server passed: dry runs write nothing, atomic batches, every error before any write, 409 listing every conflict, strict versions, idempotent retries and `commit.reused`, commitIds per company, other companies' ids refused, read-only and AdvEdit fields refused, 413 past 500. Skipped: the 4 mock-only Commit cases (an edit elsewhere in the ERP, two lost-response cases, a locked record), End (E3), and the cases that need the mock's clock or failure injection, which UnifiedAPI's own tests cover. The host logged no errors |

The plan's acceptance also asks that no commit path writes without a version: every record the changeset sends carries the binding's version (`version_text`), UnifiedAPI refuses one without (`version.missing`) or with a stale one (409), and the e2e test above checks versions moved for exactly the committed records.

**Found and fixed on the way**

- A 400 cleared the sent records' conflict marks, though the ERP checks versions only once a commit has no errors: a Validate that found another error would hide a conflict. Conflicts are now kept through a 400, with a Rust test and an e2e step.
- Track E2 found that the database's own COMMIT can fail with its outcome unknown, answered as a 500. The sheet treats a 500 on Commit as it treats no answer: writes stay held and Try again sends the same commit. The contract says so.
- Chrome sends a POST again by itself when its kept-alive connection drops before an answer, so a mock fault counted by `times` ran out before the sheet's own retries did. The mock's faults can now be cleared (`{clear: true}`), and the lost-answer test drops every answer until it clears them.
- CI failed the PWA's Open with test in Chrome from the 1.8.0 commit on, while it passed on this machine and at `0bd0889` on CI. Bisecting on CI showed it passed alone and first in the job, and failed after the PWA spec's install test; Windows started the app's launcher, but Chrome 156 showed no question, no window and logged nothing, and the same launch 90 seconds later opened the workbook. A launch that comes as soon as the registration shows can go unanswered. The test now waits for the launcher Chrome writes and chooses Open with again every 20 seconds until Chrome asks, up to four times, still requiring the app to open the workbook: CI needed two launches. On failure it attaches what the browser's windows show.

**Delivered**

- `cgs-bind`: `marks` (the ERP's errors by key and field with the value sent, conflicts by key with the record now, and the last commit's saved fields), laid over the states in `note_record`; `set_marks`, `take_commit` and `read_record`; Committed, Conflict and server bits; a conflicts count.
- `cgs-engine`: `erp_set_states`, `erp_freeze` (the content rule refuses writes to the bound table, and undo and redo wait) and `erp_apply_commit` (only cells that still hold what was sent take the persisted values, read-only cells follow, versions and originals move, undo clears); `erpChanges` with hidden rows flagged and `visible` to leave them out; `erpReview`; hover details with the ERP's error, the conflict's author, time and value; Conflict and Committed tile flags (ViewBlock bits 12 and 13); the commit limit in the binding.
- `@cgs/erp-connector`: `sendCommit` (200, 400 and 409 as outcomes, sent again under the same `commitId` after no answer), `newCommitId`, `commitRecords` and `payloadKey`; errors keep their parsed body and 409 has its own kind.
- `@cgs/engine-client`: the new calls, types and the two states painted, mirrored on right-to-left sheets and never on pages.
- `@cgs/ui`: Validate, Review Changes (`ReviewChangesDialog`, a `CgGrid` in a `CgWindow`) and Commit with Commit All Changes and Commit Changes in Visible Rows on the ERP Data tab; the confirmation, the commit's panel (running, refused, unknown with Try again) and the refusal box; the badge's conflicts; every message in `erp/messages.ts`.
- Tests: `crates/cgs-engine/tests/erp_commit.rs` (5) and 5 more in `cgs-bind`; `packages/erp-connector/src/commit.test.ts` (5) against the mock; `erp/messages.test.ts` (7); a tile-drawing case; the 4 e2e tests (8 runs). `docs/erp-contract.md` has Track E2's notes: retries after a 500, versions in canonical base64, inactive currencies, and a racing retry waiting on its first attempt.

**Track E2.** Built by a fork of this session in `D:\CashGear\Projects\CashGear` and checked here (the commits on `origin/main`, `VendorModel` untouched, the locks in `SheetCommitService`).
- `main`, pushed `076933fc..f8424eee`: `Create_SheetCommit.sql` as the contract gives it (`8067f017`); `SheetCommitService` and `VendorSheetController.Commit` with the contract's checks in order, dry runs, versions re-read under `UPDLOCK, HOLDLOCK`, one transaction per commit with raw-SQL writes guarded by the version, an audit row per changed field, a SystemLog row, the `SheetCommit` row with the payload's hash, and `RemoveByTag("Vendors")`; inactive currencies sent with `active: false` (`24373720`); tests (`f8424eee`).
- `CashGear.UnifiedAPI.Tests`: **57 passed, 12 skipped** on SQLite, and **69 of 69** with LocalDB on: six concurrent commits of one record give one 200 and five 409s, six concurrent retries of one commitId write once, a failure inserting the `SheetCommit` row rolls back the vendors, audit and log, a real 500-record commit, and `UpdateEx` refusing a racing write once RowVersion is mapped. With the lock hints removed, both concurrency tests fail. `CashGear.Operation.Tests`: 3,221 passed, and the same 2 failures as before Track E1. The whole solution builds.

**What else was checked**

- Unit tests: 291 in Vitest (13 new) and 713 in Rust (10 new). Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match, as in Phase 16.
- In the browser pane, on the release build against the mock: the confirmation, a commit, a 409, and at 200% the green Committed bars, the hatched conflict with its "!", a red Invalid border and the badge "Vendors · 8 · 2 changed · 1 invalid · 1 conflict · Sara @ Kit"; the Review dialog with its filters' counts.
- `pnpm desktop:build` and `pnpm desktop:smoke`: the desktop app builds, and its smoke run passes in WebView2 157 (goldens within tolerance, overwrite and Save As, recent files, the theme kept).
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.8.0 everywhere and its changelog notes. Release WASM: 4,644,030 bytes gzipped, of the 5,000,000 budget.
- CI on `main` at `fb4be81`: both jobs green, **320 of 320** in Chrome and Edge. `phase-17` and `v1.8.0` were pushed on that commit once it was.
- Not run again: `bench/run-edit-fuzz.mjs`, `bench/run-excel-matrix.mjs` and the speed benches. Nothing they cover changed: saving and normal sheets are untouched, and on bound sheets the tiles read two more flag bits and the content rule one more flag.

**Known gaps**

- Resolve Conflicts and the three-way refresh are Phase 18: until then a conflicting record's changes can only be discarded, or replaced by a Refresh.
- CommitStatus is Track E3: after a lost answer the sheet sends the same commit again. If the user closes that panel and changes what would be sent, a commit that did land shows as conflicts on the next commit, which the version check keeps from saving twice.
- The ERP's messages are English for Arabic sessions until Phase 18's catalogue; the engine and the ERP name every error by its code so it can word them.
- A bound workbook still keeps no recovery copy until Phase 18; closing the tab with changes asks first.
- Before Commit is used against a tenant database, `Create_SheetCommit.sql` must run there (with `Add_VendorsRowVersion.sql`). Deploying UnifiedAPI with E2 is safe before that while `SheetLaunch:Companies` stays empty. `[Timestamp]` on `VendorModel` still waits for your word, and the Blazor host's `VendorDataService.UpdateAsync` (in `assistoWebApp`) hasn't been tested with RowVersion mapped; the ERP's own `UpdateEx` has.
- The mock takes any version it can decode, where UnifiedAPI asks for canonical base64; its currencies are all active.
- The Review dialog opens 1,080 pixels wide and scrolls in a narrower window.
- Binding speed still misses B.6 (Phase 15's 0.70 s and 2.56 s), carried to Phase 19.
- Outside this phase: the 2 `ShipOrderPartnerListTests` failures in the ERP repo (`no such column: s.DueDate`, from its commit `129d0dcb`).

## Phase 18: Conflicts, refresh, sessions and recovery

Concurrent ERP edits, long sessions and crashes are handled, the ERP texts speak Arabic to Arabic ERP users, and Cash Gear ERP's button is turned on for Kit. Done on 2026-11-11. Answers and defaults are in `docs/decisions.md` (2026-11-11, Phase 18): sessions of 30 minutes idle and 10 hours, renewed after input at most every 5 minutes; a 16 MB recovery draft; the Arabic reviewed on a page before the tag; Kit turned on; Resolve Conflicts as a dialog with three values; Reconnect that waits for the next launch; and the user's `[Timestamp]` left alone.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| A property test of the merge | Pass | `crates/cgs-engine/tests/erp_merge.rs`, `random_merges_keep_every_record_and_never_let_a_stale_value_through`: 90 seeded rounds over 240 records, each of sheet edits (values, values typed back, formulas), edits in the ERP aimed mostly at records the sheet changed (other values, the same value the sheet typed, empty values, renamed keys, records deleted and restored, Modified by), sorts, then a Refresh, a commit the oracle ERP checks by version (all of it when every version is current, else a 409 for the stale ones, which the engine merges), Resolve (Keep All Mine, Take All ERP's, or chosen fields) or Discard. An oracle applies plan B.3 item 8 by itself. After every step: each key once in its row with its id and the version the oracle expects, every tracked cell as the oracle says, the read-only Modified by as the ERP last sent it, the badge's conflicts, records gone and changed fields, and the safety property: no record the changeset would send at the ERP's current version leaves a field unsent that differs from the ERP's value, or sends one `from` anything but the ERP's value. Every case is reached (take 52,935, keep 2,491, converged 95, clash 74, renamed keys 46, records gone 221, records merged from a 409 16, fields resolved 44, records committed 280). **Proving it:** three faults put in one at a time were each caught: a record adopting the ERP's version despite a clash (round 0's refresh), a clash taking the ERP's value over the sheet's change (a cell check), and a gone record left in the changeset. Also `a_recovery_draft_replays_to_the_state_it_left`: a draft's bind payload, edits, a refresh delta and a resolve replayed on a new engine give the same cells, ids, versions and counts, and a draft of another format is refused. 6 more tests in `cgs-bind` (every merge case, renames, gone records, 409s with fields the ERP left out, Keep Mine adopting the version, the draft format) and one in `cgs-engine` (a refresh conflict's author and time) |
| e2e: concurrent edits conflict and resolve; 401 during a commit writes nothing twice; a recovered draft never commits by itself | Pass | `tests/e2e/erp.spec.ts`, "conflicts, refresh, sessions and recovery", 7 tests, ****14 of 14**** in Chrome and Edge against the mock on its own origin. **Conflict and resolve:** two fields typed in two vendors clash with Omar's edits and a third vendor's name is changed by Mona; Refresh says "Refreshed 8 vendors. 3 changed in Cash Gear ERP: 1 merged with your changes, 2 in conflict.", Mona's name shows under the pending address, Commit waits and offers Resolve, the dialog lists both fields with original, mine, the ERP's and "omar", Keep Mine and Take ERP's each by checkbox, then a commit saves mine over Omar's mobile, Omar's pay method and Mona's name together, 2 writes. **Rebase without conflict:** edits to other fields than the ERP's give a 409 whose merge leaves nothing in conflict ("2 vendors merged with your changes: commit again to send them."), and the next commit keeps both sides; Review's Discard Selected by checkbox. **401:** the commit lands, its answer is lost and every resend gets 401; the panel says the outcome isn't known and offers Reconnect, with every later Commit faulted to 500; a launch in a second tab hands its session over and closes, CommitStatus finds the commit and its result is applied: still 2 writes and 1 commit, and an edit then commits in the new session. **Another company:** a session handed over for Dot by a forged message is refused, its token ended in the ERP, and a real Dot launch opens its own sheet while the Kit tab still waits. **Recovery:** a page closed without a word leaves a draft offered at the next start, which opens "Disconnected" with both changes and Commit off; nothing is written; a launch reconnects it and it refreshes, still writing nothing until committed by hand. **Launch offer:** a launch with no tab waiting offers the matching draft, which recovers into its session and merges an ERP edit made meanwhile. **Arabic:** the tab, Refresh's report, the badge and Resolve in Arabic, the dialog right to left, and the right-to-left golden `erp-merge-rtl`. The full suite: ****334 of 334**** in Chrome and Edge |

The plan's tests also ask for every C/O/S case with key renames and records gone (the property test above), a reconnect token for another company refused and Arabic goldens (the e2e above), all in Chrome and Edge with the existing suites unchanged (below).

**The Arabic texts.** All 182 texts of the catalogue, with sample counts and names, beside their English on a private review page (https://claude.ai/artifact/Bm1Tj5rqHE5is86CgKLpXA). You signed them off as written on 2026-11-11.

**Found and fixed on the way**

- `CgGrid` reports selected rows by typed key tokens (`string:3:4`), so Phase 17's Review dialog never matched them and Discard Selected stayed off. Both dialogs read the tokens now, with an e2e step.
- A refresh's conflicts said nothing of who changed the record, since Query carries no author: they take the entity's own Modified by and Modified fields, as a 409's do.
- A conflict whose clashing field was typed back to its original left the record in conflict with nothing marked: the clashing fields now stay marked until resolved, since the ERP holds another value there.
- A tab taking a handed-over session closed its channel before saying so; it now closes itself after answering.
- The commit and contract suites compared the mock's whole state, its clock's `now` included, which ticked over a second in a slow run: they compare what was written.

**Delivered**

- `cgs-bind`: `merge` (the per-field case and merged originals), `Binding::plan_merge` and `apply_merge` (versions, conflict marks with the ERP's version, values and author, renamed keys moving their marks, records gone), `conflict_records`, `conflict_record`, `set_original`, the Gone state and count, and `draft` (the versioned compact records).
- `cgs-engine` (`erp_merge.rs`): Refresh, 409s and Resolve through one merge that clears undo; `erpConflicts` and Next Conflict; tile flag bit 15 for Not in ERP; `erp_draft`, `erp_restore`, `erp_replay_refresh`, `erp_pending`.
- `@cgs/erp-connector`: Renew after input, End, `handover`/`adopt`, `commitStatus`, and `reconnect.ts` (`offerHandover`, `waitForHandover`, scope checks).
- `@cgs/engine-client`: the bound workbook's recovery draft in the worker (16 MB, restarted when nothing waits, replayed exactly or dropped), its types, and the grey hatch for Not in ERP.
- `@cgs/ui`: the en/ar catalogue (`messages.ts`, `messages-ar.ts`, `texts.ts`), the Resolve Conflicts dialog, Next Conflict, Reconnect, CommitStatus before any resend, the draft offers at start and on launch, and the engine's refusals in the session's language.
- The mock refuses a CommitStatus id that isn't a UUID, as UnifiedAPI does; the contract notes Phase 18's rules.

**Track E3.** Built by a fork of this session in `D:\CashGear\Projects\CashGear` and checked here (the commits on `origin/main`, the user's `VendorModel.cs` and `CommissionStatementPDF.cs` still uncommitted and untouched).
- `main`, pushed `f8424eee..65699ea1`: Renew and End (`6d15ca9a`), CommitStatus (`3ae97295`), Kit in `SheetLaunch:Companies` with test hosts keeping their own list (`a6fbe284`), tests and `SheetContractRun.md` at E3 (`65699ea1`).
- `CashGear.UnifiedAPI.Tests`: **71 passed, 14 skipped** on SQLite; with LocalDB **84 passed, 1 failed**: `UpdateEx_today_overwrites…`, which fails the same before E3 because the user's uncommitted `[Timestamp]` makes `UpdateEx` refuse the racing write, as the rollout plan wants. The whole solution builds.
- The contract suite against UnifiedAPI on LocalDB at `ERP_TEST_STAGE=E3`, all five keys, every connection string checked as LocalDB first: **44 passed, 14 skipped, 0 failed**; the skipped cases need the mock's clock or faults, which the xUnit tests cover.

**What else was checked**

- Unit tests: 306 in Vitest (15 new) and 722 in Rust (9 new). Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match, as in Phase 17.
- The draft's size: the compact bind payload takes about 182 bytes a vendor on the mock's data, so a sheet at the 10,000-record cap takes about 1.7 MiB of the 16 MiB.
- `pnpm desktop:build` and `pnpm desktop:smoke`: the desktop app builds, and its smoke run passes in WebView2 157 (goldens within tolerance, overwrite and Save As, recent files, the theme kept).
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.9.0 everywhere and its changelog notes. Release WASM: 4,677,139 bytes gzipped, of the 5,000,000 budget.
- CI on `main` at `8847e52`: both jobs green, **334 of 334** in Chrome and Edge with no retries. `phase-18` and `v1.9.0` were pushed on that commit once it was, and the release workflow drafts 1.9.0.
- Not run again: `bench/run-edit-fuzz.mjs`, `bench/run-excel-matrix.mjs` and the speed benches. Nothing they cover changed: saving and normal sheets are untouched, and on bound sheets the tiles read one more flag bit.

**Known gaps**

- Desktop still has no ERP launch until Phase 19's deep link; a draft recovered there has nothing to reconnect to.
- Open Afresh on a launch's draft offer leaves the draft, which is offered again next time; its prompt at start can discard it.
- Edits outside the ERP data made before a draft restarts (after everything was committed, discarded, refreshed or resolved) aren't in the restarted draft.
- Binding speed still misses B.6 (Phase 15's 0.70 s and 2.56 s), carried to Phase 19.
- Before UnifiedAPI with E3 is deployed to ASTDataTest: `Create_SheetSessions.sql` (CGService), `Add_VendorsRowVersion.sql` and `Create_SheetCommit.sql`. `[Timestamp]` on `VendorModel` is still yours to commit.

## Phase 19: ERP hardening

ERP data is ready to turn on per company against the real API: binding meets plan B.6, the sheet and Track E are reviewed for security and hardened, and Track E4 is built. Started 2026-11-11. Answers and defaults are in `docs/decisions.md` (2026-11-11, Phase 19): Kit on the live API as the test tenant, dedicated `SHT-` test vendors, the keys in a file outside the repo, the deep link designed and built later, and Dot turned on during the phase.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| The performance targets in plan B.6 are met, and every standing check is green | Pass locally; CI on the final commit below | **B.6** (`bench/erp-speed.mjs`, release build in Chrome 157, the reference machine): binding 1,000 records of 20 fields from the response to the first paint, median of 7, **274 ms** (target 300; a second run of 9 gave 286), and 10,000 **744 ms** (target 1,500), with the workbook opened while the records download as a launch opens it; the changeset **4.8 ms** for 1,000 × 20 (target 10; 29.8 with every field changed) and **51.5 ms** for 10,000 (target 100); local rules after a 1,000-row paste add nothing measurable (the bound paste was 13 ms quicker; target 50); key to paint p99 **39.3 ms bound, 37.5 unbound** (within 10%); tiles with states p99 **5.4 ms bound, 13.7 unbound** in the engine, and drawn **57.7 / 64.1**; memory for originals **46.3 bytes an editable value** (52.6 with keys and maps; target 64), counted with wasm32's sizes. **Normal sheets**, against v1.9.0 built from its tag and served beside this build in the same hour, alternating: the 1M-row filter's medians 211 to 254 ms over eight rounds (v1.9.0: 206 to 253), with no CSP effect (the same build without its policy: 237 to 273); key to paint during the 1M-formula recalculation p99 77.4 and 83.7 ms (v1.9.0: 84.7 and 83.3), and once settled 34.5 and 37.1 (40.2 and 38.6); wheel scrolling 93.2% to 96.7% of frames within 17 ms over five rounds (v1.9.0: 95.6% to 96.7%), dragging 88.1% to 91.4% (89.4% to 91.4%); opening the 333k-row book 4.8 to 5.0 s to show (4.7 to 4.9). Everything else is in "What else was checked" |
| You sign off after trying it on the test tenant | Waiting | The live API at `api.cashgearerp.com` doesn't yet show the test tenant's setup: no `SHT-` vendors in Kit, the editor and viewer test users with more than their privileges, Dot off and no rate limits counted, so the build at ERP `d394b092` isn't the one serving it yet. **Contract suite against it** (`pnpm erp:tenant suite`, five keys, the live sheet's Origin, real commits only on `SHT-` vendors, so none were made): 22 passed, 24 failed on that setup (every write case finds no `SHT-` vendor; the privilege cases; Dot's 403; no 429 after 400 requests), 14 skipped (the mock's clock and faults). The tenant e2e (`pnpm erp:tenant e2e`) needs the `SHT-` vendors and wasn't run; it passes 12 of 12 in Chrome and Edge against the mock (`ERP_TENANT_ON_MOCK=1`) |

**What the tenant runs check, once its setup is in place**

- `pnpm erp:tenant suite`: the 58 contract cases against the live API, and the two rate-limit cases with `ERP_TEST_RATE_LIMITS=1`, waiting out each 429; it puts back every value it changed, and checks it.
- `pnpm erp:tenant e2e`: the deployed sheet in Chrome and Edge, as Kit's editor: opening the test vendors cross-origin isolated, editing, Validate and Commit, an edit made elsewhere conflicting and resolved, a commit whose answers are all lost found by CommitStatus and written once, a session ending, Connection Info naming the failure, and a new launch handing over, and an Arabic session right to left.

**Found and fixed on the way**

- **Binding** wrote each cell through Formualizer's `set_cells`, with a pre-image read per cell: 94 ms of 135 natively for 1,000 records. It now loads the new sheet as a file's sheet loads (20 ms).
- **Pages** were parsed through an untagged serde enum and a `HashMap` a record, with every text copied twice: 68 ms natively for 1,000 records, now 6.
- **Fitting** formatted every cell's display text only to rank candidates by length: 28 ms natively for 1,000 records, now 11, choosing the same cells.
- **Tile builds** keyed derived styles with `Debug` formatting per cell, and read every bound text cell as a number for a triangle bound text never shows: 148 ms of engine time for a bind's tiles, now 53, and every sheet with table styles or conditional formats gains.
- **Every engine change during a bind** made the grid rebuild and redraw its tiles at the unfitted widths; the grid now holds its redraws until the bind settles.
- **Screen tiles** set a fill and drew it per cell, 88 ms over five tiles on a table; neighbouring cells of one colour are one rectangle now, with the same pixels.
- **Each column's number format** was made once a field, reading the workbook's styles again each time; each format is made once.
- **The recovery draft** was written before the bind answered, 33 ms ahead of the first tiles; it now starts once the sheet shows, or before the first edit. An Edge e2e found that an edit made sooner than that waited in memory, so the first edit or ERP step after the bind now writes the draft first.
- **Raw API keys reached UnifiedAPI's logs** (Track E4's monitoring test found it): the API-key cache's entry key was the key itself, and FusionCache logs cache keys at Information. It's the key's SHA-256 now, and the handler logs a masked key (ERP `611a95ea`).
- **The security review** (an independent read of the sheet's ERP code and Track E): no high or medium findings. Fixed now: the connector followed redirects with `X-Sheet-Session` (L5, `redirect: 'error'`, with a test that fails without it); workbook links opened any scheme, `javascript:` included (I5, only `http`, `https` and `mailto` now); the web sent no CSP (L1, one now, and every browser test runs under it, with `csp.spec.ts` failing on any violation and passing against the live site in Chrome and Edge); a company taken off `SheetLaunch:Companies` kept its sessions for up to 10 hours (L3, ERP `658f4d68`). Known gaps below.
- The bind benches left their preview server running (killing the shell left its child), so a later test run reused a server with old headers; they stop it as the edit bench does.
- The tenant runner blanked only keys of 6 or more characters out of what it printed; it blanks any length.
- CI's typecheck failed on the first push: the redirect test imported `node:http`, and the connector has no Node types. Its servers moved into the mock's typed module.

**Delivered**

- `cgs-bind`: `WireValue` read straight from the token, pages' values borrowed by name, text moved into cells, and `Binding::memory`.
- `cgs-engine`: `load_fresh_sheet`, each format made once, faster fitting, derived styles compared rather than keyed, no number-as-text reading for bound text, `erp_memory_json`, and `tests/erp_speed.rs` (ignored) timing a bind natively.
- `@cgs/engine-client`: background runs, `warmText`, the draft started after the bind shows (`erpStartDraft`, held lines, arming before the first edit), and `erpMemory`.
- `@cgs/erp-connector`: redirects refused, the last failure with its correlation id, `onSchema`.
- `@cgs/ui`: `prepareFor` (the workbook and fonts while the records download), `closeBlank`, `holdView`, Connection Info's last failed request in English and Arabic, and links opening only web and mail addresses.
- `apps/web`: the content security policy (`src/csp.ts`) in `staticwebapp.config.json` and the preview, with `nosniff`.
- The desktop: `connect-src` adds `https://api.cashgearerp.com`, and `docs/desktop-deep-link.md` designs the `cashgear-sheet://` launch with PKCE.
- The contract: `rate.limited`, the mock's rate limits (`/__mock/limits`), the suite's rate-limit cases, writes kept to `ERP_TEST_WRITE_SEARCH` and put back on a real server, and 429s waited out there.
- `pnpm erp:tenant` (`scripts/erp-tenant.mjs`), `tests/tenant/erp-tenant.spec.ts` and `playwright.tenant.config.ts`.
- `bench/erp-speed.mjs`, `bench/erp-pages.mjs`, `bench/profile-erp-bind.mjs`.
- Docs: `DEPLOYMENT.md`'s ERP section, the contract's rate limits, and CLAUDE.md's Phase 19 notes.

**Track E4.** Built by a fork of this session in `D:\CashGear\Projects\CashGear` and checked here (the commits on `origin/main`, your `VendorModel.cs` and `CommissionStatementPDF.cs` still uncommitted and untouched).
- `main`, pushed `65699ea1..d394b092`: API keys out of the logs (`611a95ea`), `rate.limited` (`c3ab2055`), rate limits, CORS narrowed to the live sheet and one log line per sheet request (`2627d1a5`), `Seed_SheetTestVendors.sql` (`2487406f`), their tests and `SheetContractRun.md` at E4 (`8d21794e`), a company taken off the list losing its sessions (`658f4d68`, `cafb469a`), and Dot turned on (`d394b092`).
- `CashGear.UnifiedAPI.Tests`: **92 passed, 15 skipped** on SQLite after the switch fix; with LocalDB at `8d21794e`, **105 passed, 1 failed**, the known `UpdateEx_today_overwrites…` your uncommitted `[Timestamp]` flips. The launch settings' 16 pass with Dot on. The whole solution builds.
- The contract suite against UnifiedAPI on LocalDB at `ERP_TEST_STAGE=E3`, all five keys: **44 passed, 16 skipped, 0 failed**; its rate-limit cases against a host with the live limits: **2 of 2**, in 123 s.

**What else was checked**

- Unit tests: 319 in Vitest (13 new) and 723 in Rust (1 new, and the native bind timing ignored). Clippy is clean natively and for WASM; `cargo fmt`, typecheck, lint and format checks are clean.
- The full e2e suite on the release build: **338 of 340** in Chrome and Edge; of the two that failed, one timed out while the save checks shared the machine, and one was the draft's race above, fixed; then the ERP and CSP specs **52 of 52**, and the draft-recovery cases 10 of 10 repeated in Edge.
- Saving: `pnpm test:save`, 11 fixtures: an unedited save is identical in all 11, 49 of 49 entries checked after edits match, and 3,786 of 3,786 cells match, as in Phase 18.
- `bench/run-edit-fuzz.mjs`: 906 of 1,000 steps match Excel, **0 diverged**, 76 not reached; 166 of 166 saves open with no repair and match Excel's own result, as in Phase 15.
- `bench/run-excel-matrix.mjs`: 116 edits on each of 11 fixtures, **1,276 of 1,276 saved files open with no repair**, all 12 broken controls are refused, 22 of 22 scenarios and 22 of 22 generated books are clean, the feature checks (charts, pivot tables and macros kept after rows go in, pivot refreshes) pass, and 4 of 4 large books open clean, as in Phase 15. Its first run stopped when Excel failed to start over COM (`CO_E_SERVER_EXEC_FAILURE`), with no Excel process left; the second ran through
- `pnpm desktop:build` and `pnpm desktop:smoke`: the desktop app builds, and its smoke run passes (goldens within tolerance, overwrite and Save As, recent files, the theme kept).
- Licences: `pnpm deny` (licences and sources ok), the npm check over 154 packages, and both gates refusing the GPL fixture. `pnpm release:check` finds 1.10.0 everywhere and its changelog notes. Release WASM: 4,680,760 bytes gzipped, of the 5,000,000 budget.
- The live site after the push: it sends the content security policy and `nosniff`, and `csp.spec.ts`'s drawing, printing and PDF test passes against it in Chrome and Edge.

**Known gaps**

- The tenant runs and your sign-off wait on the test tenant's setup (above).
- The security review's open lows: Query reads a company's whole Vendors table on every page and the data routes aren't rate-limited (L2, a Track E change); key and company deactivation take up to `KeyCheckSeconds` (60 s) to bite; a launch link works in any browser it reaches (L4), which shows the company and user it opened.
- The rate limiter counts per process, so a host with several worker processes, or behind a proxy, needs another key for it.
- Binding's first paint waits on one tile worker drawing two tiles in turn; a second tile worker is the next lever.
- Desktop still has no ERP launch until the deep link is built.

## Track S1 and S2: Cash Gear Workspace's workbooks

Cash Gear Workspace opens its workbooks here: "Open in Cash Gear Sheet" starts a tab at `/cloud#launch=<ticket>`, which redeems the ticket for a file session, reads the workbook by signed URL and saves new versions with `If-Match`. Started 2026-11-12, built against a mock of Workspace's sheet-session routes, which Workspace's Phase 5 builds (Workspace's Q15). Answers and defaults are in `docs/decisions.md` (2026-11-12, Track S1 and S2): the bytes as the save's body, copies beside the original in Workspace, English and Arabic texts, and no release of its own. The wire is `docs/workspace-contract.md`.

**Exit criteria** (Workspace's `docs/INTEGRATIONS.md`, Track S, and the brief it started from)

| Criterion | Result | Evidence |
| --- | --- | --- |
| S2: `/cloud#launch=<ticket>` is read before the app renders, cut from the address bar and history at once, then redeemed at a Workspace API the build names, never the link | Pass | `launch.test.ts` (5 cases: the fragment cleared for good and bad tickets, other paths untouched, the API from the build, the mock only for a localhost page); `connector.test.ts` (the ERP's reader leaves `/cloud` alone); e2e "opens a Workspace workbook from a launch": the address is `/` with one history entry after the start page, the ticket used, the page cross-origin isolated |
| S1: a cloud `SheetPlatform` opens by ticket, reads by signed URL, and saves a new version with `If-Match` on the ETag | Pass | `apps/web/src/platform/cloud.ts`; `session.test.ts` (reads by a fresh signed URL and asks again once when Blob Storage refuses it; saves with `If-Match`, each from the last ETag); e2e "saves new versions with If-Match": two saves, `If-Match` the seed's ETag then the first save's, and the saved bytes hold the edit and the recalculated total |
| A stale ETag gets 412 and the user is offered "save a copy" or "reload" | Pass | e2e: a version saved by someone else meanwhile shows "A newer version was saved", naming who and when; Save a Copy makes "Budget 2027 (copy)" with the changes and the tab goes on saving to it; Reload opens the newer version and discards the changes, and the next save goes from it. Unit: the 412's `current`, and a lost answer's resend counted as saved once by its SHA-256 |
| The session is renewed while open (30 minutes idle, 10 hours at most) and ended on close | Pass | Unit: Renew at most every 5 minutes after input, sliding the idle expiry; a session idle for 30 minutes ends here without a request; a 401 ends it; End once, with `keepalive`. e2e: closing the tab ends the session in the mock; a session that ended keeps the edits, Reconnect takes the next launch's session over `BroadcastChannel`, and the tab saves from the version it had |
| The token lives in memory only, never in `localStorage` or a file | Pass | The token is a private field of `WorkspaceSession`; unit: it isn't in the session's JSON, values or text, and a failed request's error never carries it. e2e: after opening and a first edit (working copy and draft), the token is in none of `localStorage`, `sessionStorage`, cookies, the history entry, the address, IndexedDB's names or any OPFS file |
| Tested (unit and e2e against the mock), pushed to `main`, CI green | Pass | 40 new unit tests (359 in all); `cloud.spec.ts` 10 of 10 in Chrome and Edge. CI on `2960ff6` ([run 43316697418](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/43316697418)): licence gates, then the build and every standing check on Windows, with the whole e2e suite **360 of 360** in Chrome and Edge and the desktop smoke run. The live site then served `/cloud` cross-origin isolated, with Workspace's API in its `connect-src` |
| The Arabic texts | Waiting for your sign-off | 52 lines side by side: https://claude.ai/artifact/QXzBrrLgLpatoJF9RsRAFX (private) |

**What else was checked**

- View access: the badge says View only, Save offers Save a Copy, and a copy Workspace refuses offers this device instead (e2e). A workbook with no version opens blank and its first save sends `If-None-Match: *` (unit and e2e). An Arabic session words the badge and dialogs in Arabic, right to left (e2e). A used or expired link opens nothing and says why (e2e).
- The content security policy: every cloud test runs under the preview's, and none reports a violation; `csp.spec.ts` and `erp.spec.ts` pass in Chrome and Edge with the wider `connect-src` (72 of 72 with `cloud.spec.ts`), so ERP data is unchanged.
- Typecheck, lint and format checks are clean, and the npm licence check passes over 154 packages. No Rust changed.

**Delivered**

- `@cgs/workspace-connector`: the contract's types and guards, the `/cloud` launch and the API's origin, HTTP with ProblemDetails, retries and no redirects, the file session (Redeem, Renew, End, read, versions, copies) and Reconnect's handover.
- `tests/workspace-mock`: Workspace's sheet-session routes on 4280 and a signed-URL Blob Storage on 4281, with a clock, faults, levels, someone else's saves, trash and copies (`pnpm workspace:mock`). Playwright starts it.
- `@cgs/ui`: the platform's `cloud` facet, `useCloudTools` (the badge, the opening and reconnect panel, and the save dialogs), the cloud texts in English and Arabic, and a blank workbook for an `OpenedFile` that asks for one.
- `apps/web`: `platform/cloud.ts`, the third `SheetPlatform`, chosen in `main.tsx` for a `/cloud` launch; `connect-src` adds `https://workspace-api.cashgearerp.com`, and the preview's the mock's two origins.
- `@cgs/erp-connector`: its launch reader leaves `/cloud` alone.
- Docs: `workspace-contract.md`, `DEPLOYMENT.md`'s Workspace section, `PLAN.md`'s Track S, CLAUDE.md's notes.

**Known gaps**

- Workspace's routes don't exist yet: the live site's `/cloud` does nothing until Workspace's Phase 5 deploys them. Its Blob Storage's origin joins the live `connect-src` once the account exists, and the account's CORS must allow the sheet's origin (`DEPLOYMENT.md`).
- Desktop: no Workspace launch until the deep link.
- If Phase 19's `v1.10.0` is tagged after these commits, it carries this track's code too; the changelog's Unreleased section lists it.
- Track S3 to S5 followed on 2026-11-14 (below).

## Track S3 to S5: Cash Gear Workspace's embed

Cash Gear Workspace shows a workbook's range or chart live in its pages through Sheet's `/embed`, in a sandboxed iframe that speaks Workspace's contract `sheet-embed.1` (its `docs/INTEGRATIONS.md`, "Contract `sheet-embed.1`", at Workspace's `99aef32`). Started 2026-11-14 in a session of its own while Workspace's Phase 5 builds its side against its own mock embed. Answers and defaults are in `docs/decisions.md` (2026-11-14, Track S3 to S5); where the contract left a point open, Sheet's reading went to Workspace's session first, and Workspace's `INTEGRATIONS.md` took them as agreed.

**Exit criteria** (the brief Workspace's Phase 5 started this track with)

| Criterion | Result | Evidence |
| --- | --- | --- |
| S3: `/embed` loads one workbook read-only from `init.source` (a signed URL read in `cors` mode with no credentials, redirects or referrer, or transferred bytes) and draws one range: the first visible sheet for a null sheet, the used range cut to 5,000 cells with `capped` for a null range; `range.tooLarge` over 5,000 named cells, `workbook.tooLarge` over 20 MB | Pass | `source.test.ts` (the request's options; 403, 404, 5xx and network failures; over 20 MB by its length or while it streams, or as bytes); `book.test.ts` and `range.test.ts` (sheets in tab order, hidden and chart sheets left out, names matched without case, the used range from its first cell, the cut); e2e: a 2,000-row book loads `A1:F833` with `capped`, `A1:Z200` is `range.tooLarge`, 20 MB and one byte is `workbook.tooLarge`, transferred bytes load, the frame keeps nothing in OPFS |
| S3: the handshake exactly as written: `ready` to `window.parent` with `"*"`; `init` only from `window.parent`, from an allowed Workspace origin, with a port, once; everything after on the port; malformed messages dropped and never logged; `contract.unsupported` for an unknown contract | Pass | `protocol.test.ts` (real `MessageChannel` ports: wrong origin, wrong source, no port or two, a second `init`, an `init` that fails validation not counted, another contract answered and nothing after, malformed port messages unanswered and nothing logged, answers in order); e2e: the same from a parent on `127.0.0.1:4273`, a sibling frame from an origin Workspace doesn't have, an `init` aimed at another origin, and no console line holding a malformed message's marker |
| S3: `setView`, `setTheme` (theme, direction, locale), `refresh` (the same ETag reads nothing), `loaded`, `resize` and `error` with the listed codes | Pass | e2e: `setView` to another sheet's range written either way round, to a chart, and to a hidden or missing sheet or chart (`view.notFound`); `setTheme` draws dark, right to left and Arabic with no answer; `refresh` with the drawn ETag makes no blob read, a new version is read once; an expired signature is `source.forbidden` and a fresh URL loads; 404 and 503 are `source.unavailable`, as are bytes that aren't a workbook; a narrower frame is told `resize` with room for its scrollbar |
| S3: `/embed` is served with `frame-ancestors` listing Workspace's origins only, the rest of Sheet keeps `frame-ancestors 'none'`, in `staticwebapp.config.json` and `vite preview` | Pass | `csp.test.ts` (the live routes `/embed` and `/embed.html`, the rest unchanged); e2e: the preview's `/embed` lists the live and local origins and `/` says `'none'`; a parent on `127.0.0.1:4274` gets no frame, and Workspace's origin can't frame `/` |
| S3: works inside `sandbox="allow-scripts allow-same-origin"` from a parent that sends no COEP | Pass | Every e2e case runs that way: the frame isn't cross-origin isolated, gets no referrer, and draws |
| S4: `requestPreview` answered by `preview` (the same `id`, a PNG of the view as drawn within `maxWidth` by `maxHeight` at `scale`, at most 2 MB, transferred, with `width`, `height`, `etag`, `view`) | Pass | e2e: a range's PNG decoded: 200 CSS pixels wide at scale 2 is 400 pixels, light though the frame is dark; a chart's at 1.5; a right-to-left sheet's has column A on the right; a preview after an error answers that error with its `id`. The density steps down until the PNG fits 2 MB (`preview.tsx`) |
| S4: chart views (`view.kind: 'chart'`; `loaded.charts` lists `{id, title, sheet}`) | Pass | `book.test.ts` (ids from the file's names, a repeat numbered, chart sheets, titles at most 255); e2e: the chart draws in the frame and in its PNG |
| S5: Sheet's validators agree with every one of Workspace's vectors (`sheet-embed-vectors.json`, copied with its source commit) | Pass | `tests/contracts/sheet-embed-vectors.json`, Workspace's file at its `3e4d760`, byte for byte (`tests/contracts/README.md` keeps the commit and SHA-256); `vectors.test.ts` runs all 288 (177 from the parent, 111 from the embed), each as sent and after a structured clone, and every verdict matches |
| S5: a browser suite drives Sheet's real `/embed` from a parent on `http://127.0.0.1:4273` through the handshake, every message and the wrong-origin, wrong-version and malformed cases | Pass | `tests/e2e/embed.spec.ts`, 13 cases, in Chrome and Edge; every message the embed sent passed the contract's validator |
| S5: Sheet's `sheet-session.1` suite runnable against Workspace's real API | Pass | `packages/workspace-connector/src/contract.test.ts`: raw wire checks (Redeem from an allowed origin only and once, an unknown or missing ticket, Renew and End, an unknown token, the signed URL read with no credentials and its CORS, saves with `If-Match` and a stale ETag's 412 naming the version that won with its SHA-256 and saver, 428 without a precondition, 400 for what isn't an xlsx, 403 for a view session, `If-None-Match: *` on an empty workbook, a copy the session moves to, preflight and exposed headers) and the sheet's own session code. 13 pass against the mock; against Workspace's real API on 2026-11-14 (its `4970e0b`, `server-stack.mjs --e2e` with LocalDB and Azurite, tickets from its `sheet-launcher.mjs`), 12 pass and the one that needs the mock's clock skips: `WORKSPACE_API_URL=http://127.0.0.1:5280 WORKSPACE_LAUNCHER_URL=http://127.0.0.1:4285 pnpm workspace:contract`. The same run had the preview's `/embed`, served as `DEPLOYMENT.md` says, read a real Azurite signed URL from that API and answer `loaded` |
| Green in Sheet's CI | Pass | CI on `86cf903` ([run 43583125944](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/43583125944)): licence gates, then the build and every standing check on Windows, with the whole e2e suite **385 passed and 1 flaky** in Chrome and Edge (`parity.spec.ts`'s case at line 591 passed on its retry; this track doesn't touch it), `embed.spec.ts`'s 26 among them, and the desktop smoke run |

**What else was checked**

- The live views and pictures were looked at: a range in light and dark, a chart, a right-to-left sheet with column A on the right, and the PNGs, light while the frame was dark, at the asked density.
- `csp.spec.ts`, `cloud.spec.ts`, `open.spec.ts` and `pwa.spec.ts` pass with the build's second page and the per-path policies (the PWA install case once on a retry, as before).
- The engine: `print.rs`'s tests pass with its used range shared with the embed's, and clippy is clean.
- Typecheck, lint and format checks are clean; 416 unit tests before the vectors, then 705.

**Delivered**

- `@cgs/ui/embed`: the contract's types and validators, the handshake, the source reader, the range view on the engine's tiles, the chart view, previews, and the frame's English and Arabic labels; `apps/web/embed.html` and `src/embed.ts`.
- `cgs-engine`: the `usedRange` question. `@cgs/engine-client`: `init({ inMemoryOnly })`.
- `apps/web`: `/embed`'s `frame-ancestors` in `staticwebapp.config.json` and the preview, the service worker's offline `/embed`.
- `tests/embed-parent` (Workspace's page on 4273 and an origin it hasn't on 4274), `tests/e2e/embed.spec.ts`, the mock's `/__mock/content`, `tests/contracts` with Workspace's vectors, and `workspace-connector`'s `contract.test.ts` for any server (`pnpm workspace:contract`).
- Docs: `decisions.md`, `DEPLOYMENT.md` (the embed, and running the sheet against Workspace's development stack), `workspace-contract.md`, `PLAN.md`'s Track S, CLAUDE.md's notes.

**Known gaps**

- The live `/embed`, like `/cloud`, can't read a workbook until Workspace's storage account's origin is in the live `connect-src`.
- Range views draw cells only (your choice); charts and pictures over a range don't show in it.
