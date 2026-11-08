> Approved on 2026-11-01, with Phases 10 to 13 added on 2026-11-07 after `v1.0.0`. This file is the plan; progress lives in `PROGRESS.md` and decisions in `decisions.md`.

# CashGear Sheet: Implementation Plan

## Context

The goal is an Excel-like spreadsheet that runs on the web and on desktop from one Rust core, with React for the UI and Tauri 2 for the desktop shell. The must-have is opening real `.xlsx` files, and saving must not damage what the app can't edit yet.

It follows the pattern of CashGear Video (`cashgear-moviepro`) in layout, WASM build, phase template and git rules. Each phase runs in its own new session, started from a card.

## Decisions

| Decision | Choice | Reason |
| --- | --- | --- |
| Core location | One Rust core as WASM in a worker, on web and inside Tauri | One code path and one test suite, as in CashGear Video. A native desktop host is decided by benchmarks in Phase 9 |
| Saving xlsx | Unsupported content stays byte-for-byte | Charts, pivots and macros still show in Excel after our save |
| Formula engine | Formualizer behind a trait, on our fork at an exact commit | Phase 1 measured it against IronCalc. See `phase1-report.md` |
| v1 scope | Editing, formulas, formatting, multiple sheets, sort, filter, tables, conditional formatting, data validation, charts, pivots | Print and PDF come after v1 |
| Arabic | Arabic and mixed text display correctly; grid stays left to right in v1 | Direction stays in one place, so RTL sheets come in Phase 11 |
| Theme | Light and dark mode everywhere, following the system with a manual switch | Uses `data-cg-theme` from `@cashgear/ui`; the grid follows it too |
| Collaboration | None; static PWA with no server | Files stay on the device |
| UI library | `@cashgear/ui`, commit-pinned git dependency as in CashGear Video | Missing generic components are built in `CashGear.React.UI` (Track U) |
| Sheet grid | Canvas grid, formula bar and cell editor stay in this repo | They are tied to the engine's data protocol. `CgGrid` is record-based and has no cell ranges |
| File format | `.xlsx` is the save format; no private format | Crash-recovery data is kept separately |
| Licences | No GPL, AGPL or LGPL in anything shipped | `cargo deny` and an npm licence check fail CI |
| Name | CashGear Sheet, crate prefix `cgs-` | Matches the sibling apps |
| After v1 | Print and PDF, RTL sheets, large books, then Excel parity polish (Phases 10 to 13), each a minor release | Chosen on 2026-11-07. What wasn't chosen is under "Later" |

Default targets until Phase 0 confirms them: Windows 10/11 x64 first, macOS second; Chrome and Edge first, Safari and Firefox best-effort. Grid limits match Excel: 1,048,576 rows by 16,384 columns. All numeric exit thresholds below are proposals, confirmed in each phase's questions.

## Working rules

`CLAUDE.md` repeats these, so new sessions inherit them.

**Every phase**
1. Read `docs/PLAN.md`, `docs/PROGRESS.md` and `docs/decisions.md`.
2. Ask the phase's questions plus any new ones in one batch, recommended option first, and wait for answers before writing code.
3. Log answers with dates in `docs/decisions.md`. Pick and log small choices; stop and ask about anything expensive to reverse.

**End of every phase**
1. Record evidence for each exit criterion in `docs/PROGRESS.md`.
2. Commit, then tag `phase-N`. From Phase 10 on, each phase is also a minor release: its `CHANGELOG.md` section, the version bumped across the app, crates and packages, and the tag `v1.(N-9).0` (Phase 10 is `v1.1.0`), which drafts the release. Publishing it stays yours.
3. Report done, exit results and known gaps, then show the next phase as a start-session card that opens in a new session. Then stop.

**Git:**
- `cashgear-sheet`: work on `main` only, and push everything to `origin/main`. No other branches, worktrees or PRs. Use small Conventional Commits scoped by crate or package. Every commit builds and passes checks.
- `CashGear.React.UI`: commit and push to its existing branch, `claude/cashgear-react-ui-library-xq8tsz`. Never create a branch there.

## Architecture

```text
cashgear-sheet/
  crates/
    cgs-model/      styles, dimensions, merges, tables, drawings, package index; commands and undo (no I/O)
    cgs-calc/       CalcEngine trait and the chosen engine's adapter
    cgs-numfmt/     number format codes and date serials to display text
    cgs-refs/       rewrites references in formula text on insert, delete, rename
    cgs-xlsx/       package store, reader, span-patching writer
    cgs-pivot/      pivot refresh (Phase 8)
    cgs-engine/     ties it together; viewport snapshots, calculation scheduling
    cgs-wasm/       the only JS-facing API
    cgs-native/     desktop only: file I/O, read protocol, file association
  apps/web/                  Vite PWA
  apps/desktop/src-tauri/    Tauri 2 shell (left out of default-members)
  packages/ui/               React shell and canvas grid, on @cashgear/ui
  packages/engine-client/    workers, comlink client, tile cache
  scripts/                   build-wasm.mjs, fetch-test-workbooks
  bench/, tests/, docs/
```

**Threads**
- **Main:** React shell and input. It composites `ImageBitmap` tiles and draws selection, and never waits on the engine. The cell editor is a DOM overlay, so IME and bidirectional input work natively.
- **Engine worker:** runs `cgs-wasm`. Priority order is commands, then reads, then 8 ms calculation slices.
- **Tile worker:** draws text and cells on a software `OffscreenCanvas`, because an accelerated one holds up the GPU process's main thread. Main starts it, because WebView2 can't load a worker started by a worker (see `nested-worker.ts` in moviepro). It talks to the engine over a `MessageChannel`.

**Engine boundary.** The engine is the only cell store, so there are never two copies of every cell. `cgs-model` holds everything else. Our command stack owns undo, because Formualizer's has open bugs (#303, #412).

```rust
trait CalcEngine {
  fn begin_load(&mut self, b: &BookInfo) -> Result<()>;              // sheets, names, tables
  fn load_batch(&mut self, c: &SheetCells) -> Result<()>;            // with cached values
  fn finish_load(&mut self) -> Result<()>;                           // calculates nothing
  fn set_cells(&mut self, e: &[CellEdit]) -> Result<Vec<CellEdit>>;  // returns pre-images
  fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo>;
  fn recalc_all(&mut self) -> Result<()>;                            // Ctrl+Alt+F9
  fn calc_slice(&mut self, ms: u32, first: &[SheetRect]) -> Result<CalcProgress>;
  fn take_changed(&mut self) -> Vec<SheetRect>;
  fn read_block(&self, r: SheetRect, out: &mut dyn CellSink);
  fn formula_text(&self, at: CellRef) -> Option<String>;
}
```

Neither published engine could yield during a calculation in browser WASM. Phase 1 added evaluation that stops and carries on to both engines' forks, and `calc_slice` runs it with visible cells first. One formula, one cycle or one schedule build still can't be divided, so a slice can run over its budget by as much as the slowest of those.

**Grid data path**
- Binary `ViewBlock` tiles of 64 rows by 16 columns (24-byte cells, styles, extra text records and a UTF-8 heap, in one theme's colours) are transferred, not sent as JSON. Stale tiles stay visible during recalculation.
- Scrollbars are self-drawn, because browsers can't size an element for 1M rows.
- Offset lookups use sorted overrides with running totals, with hidden rows as 0. The direction flag lives here.
- Display text comes from Rust `cgs-numfmt`. `####` and General shortening finish in the tile worker, which knows measured widths.
- Theme: canvas colours come from a palette built from `--cg-*` tokens and passed to the tile worker, because workers can't read CSS. Switching theme redraws tiles. Background, gridlines, headers and selection follow the theme. In dark mode, the workbook's `dk1`/`lt1` theme colours and automatic colours swap for display only, so default black text stays readable. The saved file never changes.
- Model to reuse: moviepro's `packages/ui/src/timeline/` (`controller.ts`, pure `geometry.ts`, pure `draw.ts`). `draw.ts` takes any 2D context, so drawing can move threads if needed.

**xlsx saving (`cgs-xlsx`)**
- Untouched zip entries are copied raw with `zip`'s `raw_copy_file`.
- Parts we change are patched at source byte spans, not re-serialized, so unknown elements and attributes survive automatically. Unchanged rows are copied as bytes. New elements go at offsets from a `CT_Worksheet` order table.
- `styles.xml` and `sharedStrings.xml` are append-only, so existing indices stay valid.
- `calcChain` is removed along with its content-type override and workbook relationship; Excel rebuilds it.
- Rows inserted or deleted: `cgs-refs` rewrites names, CF, DV, `xm:f`, `xm:sqref` and chart `c:f`, shifts drawing anchors per `editAs`, shifts table `ref` and `autoFilter`, rewrites pivot sources and sets `refreshOnLoad`. Unknown extensions are left untouched and named in a save report.
- `umya-spreadsheet` doesn't document keeping unknown parts, and `rxls` can't insert rows, so neither fits as the writer. `calamine` and `rust_xlsxwriter` are test tools only.

**Platform switch.** A `SheetPlatform` interface like moviepro's `platform.ts`, chosen by `isTauri()` in `main.tsx`. Web uses File System Access pickers and drag and drop. Desktop uses Tauri dialogs and a custom read protocol, since Tauri IPC measured about 11 MB/s in moviepro against about 45 MB/s for a protocol. It writes via temp file and rename.

## Track U: missing components in `CashGear.React.UI`

This runs in that repo, independently of the phases, and can start alongside Phase 0.

**Build:** `CgRibbon` (composed from Tabs and Toolbar, with groups, collapse and overflow), `CgColorPicker` (theme palette with tints, standard, recent, automatic, no fill), `CgStatusBar`, `CgSlider` (zoom), `CgGallery` (table and CF styles), `CgBorderPicker`, and a spreadsheet and formatting icon set.

**Questions first:** icon source (Fluent System Icons, MIT, or Lucide, ISC)? Ribbon key tips now or later?

**Rules from that repo:** one folder per component (`CgX.tsx`, `.types.ts`, `.module.css`, `.stories.tsx`, `index.ts`), root exports only, logical CSS with Arabic labels, no new production dependencies (icons checked in with notices). It uses npm, not pnpm. All work is committed and pushed to its existing branch, `claude/cashgear-react-ui-library-xq8tsz`; no new branches.

**Exit:**
- [ ] `npm run verify` passes.
- [ ] Stories cover light, dark and RTL.
- [ ] Commit pushed and its hash reported. Track U doesn't edit this repo, so it never writes alongside a running phase; the next phase here bumps the pin.

Later gaps go here too when phases find them, such as scatter charts in `CgChart`.

## Phases

### Phase 0: Scaffold, toolchain, CI
**Goal:** a canvas tile drawn in a worker from WASM engine data shows in both browser and Tauri, with CI enforcing quality and licences.

**Questions:** Confirm targets? GitHub Actions on your account? Reference test machine?

**Tasks**
1. Cargo workspace with stub crates; pnpm workspace; pinned `@cashgear/ui`.
2. `scripts/build-wasm.mjs` on moviepro's model: pinned `wasm-bindgen`, no wasm-pack, gzip budget check.
3. Engine worker, tile worker, comlink client, and a test tile pattern.
4. Tauri 2 shell and the `SheetPlatform` interface.
5. Theme switch: follows the system, manual override remembered, sets `data-cg-theme`, and updates the Tauri window theme and the tile palette.
6. COOP/COEP headers on web and desktop.
7. CI: fmt, clippy (native and wasm32), tests, web and Tauri Windows builds, `cargo deny`, licence check and self-test.

**Exit**
- [ ] `pnpm dev:web` and `pnpm dev:desktop` show the same pattern and a CashGear UI control, in light and dark.
- [ ] CI is green, and adding a GPL dependency makes it fail.

### Phase 1: Formula engine bake-off
**Goal:** a measured choice of engine behind `CalcEngine`.

**Questions:** Which of your real workbooks can join a private test set? Fork if upstream refuses patches? WASM size budget (published Formualizer is about 3.6 MB compressed, IronCalc about 1 MB)?

**Tasks**
1. `scripts/fetch-test-workbooks`: public files fetched, not committed, plus generated ones.
2. Adapters for both engines and a `bench/` harness in Chrome WASM.
3. Measure formula cells against Excel's stored `<v>` values per function (relative 1e-9, skipping volatile and external), load time, first calculation, single-edit p99, longest uninterruptible slice, top-row insert, peak memory, optimized gzip size.
4. Run 1,000 random structural edits against Excel via COM.
5. Prove or work around loading cached values without calculating.
6. Try upstream patches for a browser-safe clock and a cancel token. Pin the winner exactly: Formualizer moved from 0.10.1 to 0.13.0 in 25 days.

**Exit**
- [x] 200+ files run, with a per-function report.
- [x] Slice p99 of 50 ms or less. The 99.5% match wasn't reached by either engine, so you moved it to Phase 4 for real documents. See `decisions.md`.
- [x] Decision logged: Formualizer.

### Phase 2: Open xlsx
**Goal:** real `.xlsx` files open and look right, read-only, on web and desktop.

**Questions:** Largest file that must open smoothly (proposal: 100 MB in 10 s)? Bundle Carlito (OFL) as the Calibri fallback? Recent files list now? In dark mode, show explicit cell colours exactly as stored, or adjust them when contrast is too low?

**Tasks**
1. `cgs-model`; `cgs-xlsx` reader: zip and quick-xml, lazy per-sheet parsing, shared strings, styles, theme colours, merges, dimensions, hidden rows, freeze panes, names.
2. `cgs-numfmt`; `ViewBlock` tiles.
3. Grid: geometry, draw, controller, scrollbars, zoom, Arabic text with direction set per cell, and the dark mode colour mapping.
4. Shell: `CgMenu`, `CgTabs` for sheets, `CgStatusBar`, theme switch.
5. Open via picker and drag and drop on web, dialog on desktop.
6. Placeholders for charts and images. Pivot output already shows, because it is ordinary cells.

**Exit**
- [x] The test set opens without recalculation, and visible values match `calamine` and Excel.
- [x] Size target met; 60 fps scrolling on 1M rows. The target became 25 MB in 10 s, and 100 MB of values in 60 s. See `decisions.md`.
- [x] Arabic and mixed-text golden images pass in light and dark, and web and desktop match.
- [x] Default-styled workbooks are readable in dark mode.

### Phase 3: Preserving save
**Goal:** save writes a valid `.xlsx` where everything untouched survives. This comes before editing so the riskiest promise is proven early, driven by test edits through the Rust API.

**Questions:** Overwrite by default, or Save As the first time? Keep macros when saving `.xlsm`? Set recalculate-on-open in saved files?

**Tasks**
1. Span-patching writer, raw entry copy, element order table, append-only styles and strings, `calcChain` removal.
2. Atomic swap-write and re-index. Overwriting invalidates web `File` reads, so decide whether to copy the source to OPFS on open.
3. Save report; new blank workbook.
4. Spike `refreshOnLoad` for pivots.

**Exit**
- [x] Clean entries are hash-identical.
- [x] No new Open XML SDK validator errors compared with the source.
- [x] Zero Excel repair prompts across the COM-open matrix.
- [x] Chart, pivot and macro fixtures still work in Excel after saving.

### Phase 4: Editing, formulas, undo
**Goal:** a working spreadsheet: type, calculate, undo, save, reopen in Excel.

**Questions:** Undo depth? Manual calculation mode in v1? How much formatting to keep when pasting from Excel?

**Tasks**
1. Commands and undo with pre-images, including formulas that became `#REF!`.
2. Selection, cell editor, formula bar, name box, reference highlighting, function autocomplete.
3. Fill handle; copy, cut and paste including from Excel (HTML and TSV); CSV open and export.
4. Insert and delete rows, columns and sheets; rename; `cgs-refs` tested differentially against the engine.
5. Calculation scheduling; autosave and crash recovery; shortcut map.

**Exit**
- [x] 1,000 random edits match Excel via COM.
- [x] Input stays under 100 ms during recalculation.
- [x] Files saved after structural edits open with no repair prompts.
- [x] Formula cells match Excel's stored values at 99.5% on real documents, with your private workbooks in the set (`bench/`, moved here from Phase 1, which measured 98.22%). Not reached: 99.35%, which you accepted. See `decisions.md`.

### Phase 5: Ribbon and formatting (tag `v0.1.0-mvp`)
**Goal:** it looks and feels like Excel for daily work.

**Questions:** Same ribbon tabs as Excel? Key tips?

**Tasks:** Home ribbon on `CgRibbon`; font, fill, borders, alignment, wrap; number format dialog; format painter; merges; resize, auto-fit and hide; freeze panes; find and replace; context menus.

**Exit**
- [x] Formats round-trip through Excel, and existing style indices are unchanged.
- [x] Visual goldens are reviewed against Excel.

### Phase 6: Data tools
**Goal:** sort, filter, tables, conditional formatting, data validation.

**Questions:** Excel's sort semantics exactly? Keep extended (x14) CF read-only at first? Which table styles?

**Tasks:** sort; autofilter; tables with structured references; CF including data bars, colour scales and icons; DV with dropdowns; name manager; notes; hyperlinks.

**Exit**
- [x] Filtering 1M rows takes under 200 ms.
- [x] Rules match Excel on test files and round-trip without repair prompts.

### Phase 7: Charts
**Goal:** charts from files display live, and new ones can be created.

**Questions:** Which chart types must be editable in v1? Extend `CgChart` (recommended) or use another renderer? Leave newer `cx:` charts as placeholders?

**Tasks**
1. Parse `c:chartSpace` (series `c:f`, type, axes) into a chart model.
2. Draw through `CgChart` in an overlay above the grid. It is SVG and has bar, line, area, pie and donut; add scatter and combination charts in Track U.
3. Create and edit from a selection; move and resize anchors; show images.

**Exit**
- [ ] Five chart types match Excel on review, and stay readable in dark mode.
- [ ] Edits write valid XML, and extension elements survive.

### Phase 8: Pivot tables
**Goal:** refresh existing pivots and create new ones.

**Questions:** Which aggregates in v1? Create in v1, or refresh only first? Reuse the field list from `CgPivotTable`, exporting it upstream if needed?

**Tasks:** parse `pivotCacheDefinition` and `pivotTableDefinition`; `cgs-pivot` refresh writing output to cells; field list UI; create from a range; `refreshOnLoad` instead of regenerating records.

**Exit**
- [ ] Refresh matches Excel on test files.
- [ ] No repair prompts.

### Phase 9: Scale and release (tag `v1.0.0`)
**Goal:** benchmarks settle the native host question, and the app ships.

**Questions:** Add a native desktop host if benchmarks call for it? Free, or licensed like CashGear Video? Code signing, web hosting, stores?

**Tasks:** large-workbook benchmarks against the 4 GB WASM limit and recalculation time; native host if needed; memory budgets; installer and updater; PWA service worker; `.xlsx` file association; third-party notices.

**Exit**
- [x] Benchmarks and the host decision are recorded: WASM only in v1, about 10M formulas. See `PROGRESS.md`.
- [x] Installer test covers install, update and uninstall.
- [x] PWA works offline, and double-clicking `.xlsx` opens the desktop app once it is chosen as the type's app.

### Phase 10: Print and PDF export (tag `v1.1.0`)
**Goal:** a sheet prints and exports to PDF with Excel's page setup, and its pages break where Excel breaks them.

**Questions:** Write PDF in Rust from a recording of the grid's own drawing (recommended), or print through the browser's dialog? Page Break Preview and Page Layout view in this phase, or Normal view only? Which font is embedded when the file's font isn't installed (Carlito for Calibri, as on screen)? Headers and footers with pictures, or text and fields only?

**Tasks**
1. Read and write page setup through span patching: `pageSetup`, `pageMargins`, `printOptions`, `headerFooter`, `rowBreaks` and `colBreaks`, and the `_xlnm.Print_Area` and `_xlnm.Print_Titles` names. `cgs-refs` moves them with structural edits.
2. Pagination in Rust: Excel's rule for where pages break, with scaling and fit to pages, print titles, manual breaks, hidden rows and columns, gridlines and headings, centring and page order.
3. A Page Layout ribbon tab (margins, orientation, size, print area, breaks, print titles, scale to fit, sheet options) and Excel's Page Setup dialog (Page, Margins, Header/Footer, Sheet).
4. PDF: pages drawn by the same `draw.ts` code into a recording 2D context and written as vector PDF with font subsets, so text is searchable and Arabic is shaped as on screen. Charts and pictures go in too. Pages always use the file's own colours, never dark mode's. The writer passes the licence gates.
5. File › Print: a preview of the pages, the system's print dialog on desktop and the browser's on web, and Export › PDF for the sheet, the selection or the workbook.

**Exit**
- [ ] Page breaks match Excel's `HPageBreaks` and `VPageBreaks` over COM on generated books covering every page setup option, and page counts match on the public files that hold page setup.
- [ ] PDFs are reviewed side by side with Excel's `ExportAsFixedFormat`, and you sign off. `qpdf --check` (test only) finds nothing, and text extracts in reading order, Arabic included.
- [ ] Page setup made here round-trips through Excel, and the matrix has no repair prompts.

### Phase 11: RTL sheets (tag `v1.2.0`)
**Goal:** right-to-left sheets look and work as in Excel, and save as such.

**Questions:** Translate the app's own UI into Arabic in this phase, with the shell mirrored when it is Arabic, or later? New sheets left to right with a default direction setting, as Excel does (recommended), or following the UI language? Charts and pictures placed from the right, as Excel places them?

**Tasks**
1. The direction flag, kept in one place since Phase 2, through geometry, tile drawing, headers, frozen and split panes, scrollbars and the selection.
2. Input mirrored: arrow keys, Tab, Enter, Home, End and Ctrl+arrows, the fill handle, dragging selections and sizes, the cell editor's direction, and where menus open.
3. Drawings, notes and filter buttons placed from the right, as Excel places them, with their anchors saved unchanged.
4. Sheet Right-to-Left on the Page Layout tab, where Excel 2016 has it, and a default direction setting. `sheetView rightToLeft` written through span patching.
5. General alignment and text overflow in right-to-left sheets as Excel does them, and Phase 10's pages and PDFs mirrored.

**Exit**
- [ ] Right-to-left sheets drawn by Excel's `CopyPicture` and by the app, in light and dark, are reviewed side by side, and you sign off.
- [ ] Keyboard and mouse e2e tests pass on mirrored sheets, and every left-to-right golden is unchanged.
- [ ] `rightToLeft` round-trips through Excel, page breaks of right-to-left sheets match Excel's, and the matrix, rules check and fuzz stay clean.

### Phase 12: Large books (tag `v1.3.0`)
**Goal:** saving no longer holds the engine, books past the ceiling are refused in seconds, big books read faster, and the installed web app opens files from the system. These are Phase 9's known gaps.

**Questions:** Save in slices between reads in the engine worker (recommended), or from a copy-on-write snapshot? Edits made during a save: queued behind it (recommended), or refused with a message? Refuse early only when the estimate is clearly past the ceiling (recommended), or also warn near it?

**Tasks**
1. Save in slices: the writer yields between rows and parts, tiles and reads are served between slices, edits wait in the command queue, and the status bar shows progress. The check before every swap stays.
2. Early refusal: estimate formulas and values from part sizes and a sample of each sheet's `<f>` and shared ranges before reading, against the measured 430 bytes per formula and 14 per value, and refuse a book past the ceiling at once with the existing message.
3. Faster reading: profile the 10M book's 264 s in WASM, where reading and building the graph take 2.4 times as long as natively, and fix what dominates. Formualizer changes go on the fork.
4. PWA `file_handlers` and `launchQueue` for the five file types, so the installed web app opens a file from Open with.
5. Phase 9's scale ladder and memory checks run again.

**Exit**
- [ ] During a save of the 25 MB formula book, tiles and cell reads are served within 100 ms at p99, edits made meanwhile land after it with the right answers, and the saved file opens clean in Excel.
- [ ] The 15M-formula book is refused within 30 s, and every book on the ladder still opens.
- [ ] The 10M-formula book is ready in under 175 s, a third less than Phase 9's 264 s.
- [ ] The installed web app opens an `.xlsx` from Open with, in Chrome and Edge.

### Phase 13: Excel parity polish (tag `v1.4.0`)
**Goal:** the daily-use gaps that v1 left are closed.

**Questions:** Which of the candidates below, and in what order? Sheet protection with Excel's password hash, or structure only? Which keys for key tips, given browsers take Alt+F and Alt+E?

**Candidates,** from Phases 5 to 8's known gaps:
- Sheet and workbook protection, with Format Cells' Protection tab.
- Hiding and showing sheets; zoom saved per sheet.
- Key tips.
- Editing rich text runs in the cell, superscript and subscript drawn, and rich cells keeping their runs when pasted or moved.
- Green error triangles and Error Checking; a circular reference warning that names the cells.
- Notes edited in a box beside the cell, Show/Hide and Show All; Remove Hyperlink; a user name setting for authors.
- Sort and filter by colour and by icon.
- New and Modify Cell Style, and the file's own named styles in the gallery.
- Pivot grouping made here, Drill Down, Show Report Filter Pages and GETPIVOTDATA from a click.
- Chart trendlines and a Picture Format tab.

**Tasks:** the chosen candidates, each with Excel 2016's behaviour measured over COM first, as in Phases 5 to 8.

**Exit**
- [ ] Each chosen candidate has an e2e test in Chrome and Edge, and an Excel COM check wherever COM can judge it.
- [ ] The matrix opens every saved file with no repair, the rules check passes, and the fuzz has 0 diverged.

## Later
Not chosen on 2026-11-07, and kept for a later plan:
- `.xls`, `.xlsb` and `.ods` import.
- Platforms: macOS, and Firefox and Safari in CI. Azure Artifact Signing needs no phase: the release workflow uses it once its secrets exist.
- Engine upkeep: rebasing the Formualizer fork onto current upstream, upstream PRs, `FORECAST.ETS`, and the 99.5% accuracy target.
- A native desktop host to pass the ~10M-formula WASM ceiling.
- Collaboration, only if that changes.
- The app's own UI in Arabic, with the shell mirrored when it is Arabic (deferred from Phase 11 on 2026-11-08).

Macros are preserved on save but never run.

## Top risks, by the earliest phase that exposes them
- **Phase 1:** settled for yielding during calculation. Still open: wrong formula values (98.22% on real documents), spilled arrays after structural edits, memory use (936 MiB at 1,000,000 formulas), and a fast-changing upstream that the fork must follow.
- **Phase 2:** font metric drift when Calibri isn't installed.
- **Phase 3:** Excel repair prompts; whether `refreshOnLoad` is enough for pivots.
- **Phase 4:** stale references in retained parts after structural edits.
- **Phase 10:** Excel's page breaks depend on the printer driver and font metrics, so matching them may need the reference machine's default printer fixed. Embedding fonts and shaping Arabic in PDF without a licence the gates refuse.
- **Phase 11:** every pointer and keyboard path assumes left to right, and the canvas's `direction` state already caused one Phase 2 bug.
- **Phase 12:** a save in slices must never write cells that an edit changed partway through it.

Phase 1 measured the engine facts above on 2026-11-02.

## Verification
Each phase's exit criteria are the test. Evidence goes in `docs/PROGRESS.md`. All UI and grid golden images run in both light and dark. Standing checks from Phase 0 on:

```bash
cargo fmt --all --check
pnpm build
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p cgs-wasm --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
pnpm typecheck && pnpm lint && pnpm format:check && pnpm test
pnpm test:e2e
pnpm desktop:build && pnpm desktop:smoke
pnpm deny && pnpm licenses:check && pnpm licenses:selftest
```

xlsx fidelity from Phase 2 on uses:
- Raw-entry hash comparison.
- Independent readers (`calamine`, openpyxl, LibreOffice).
- The Open XML SDK validator.
- An Excel COM-open matrix at each tag. Excel and dotnet are on this machine, and CI runs what it can without Excel.

These are test tools only and never ship.
