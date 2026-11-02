# Progress

What is done, and the evidence for it. The plan is in `PLAN.md`, and decisions are in `decisions.md`.

| Work | Status | Tag |
| --- | --- | --- |
| Planning | Done 2026-11-01 | |
| Track U: components in `CashGear.React.UI` | `CgStatusBar` and `CgSlider` done for Phase 2, the rest not started | |
| Phase 0: Scaffold, toolchain, CI | Done 2026-11-01 | `phase-0` |
| Phase 1: Formula engine bake-off | Done 2026-11-02: Formualizer, with the accuracy target moved to Phase 4 | `phase-1` |
| Phase 2: Open xlsx | Done 2026-11-02 | `phase-2` |
| Phase 3: Preserving save | Not started | |
| Phase 4: Editing, formulas, undo | Not started | |
| Phase 5: Ribbon and formatting | Not started | |
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
