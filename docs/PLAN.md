> Approved on 2026-11-01. This file is the plan; progress lives in `PROGRESS.md` and decisions in `decisions.md`.

# CashGear Sheet: Implementation Plan

## Context

The goal is an Excel-like spreadsheet that runs on the web and on desktop from one Rust core, with React for the UI and Tauri 2 for the desktop shell. The must-have is opening real `.xlsx` files, and saving must not damage what the app can't edit yet.

It follows the pattern of CashGear Video (`cashgear-moviepro`) in layout, WASM build, phase template and git rules. Each phase runs in its own new session, started from a card.

## Decisions

| Decision | Choice | Reason |
| --- | --- | --- |
| Core location | One Rust core as WASM in a worker, on web and inside Tauri | One code path and one test suite, as in CashGear Video. A native desktop host is decided by benchmarks in Phase 9 |
| Saving xlsx | Unsupported content stays byte-for-byte | Charts, pivots and macros still show in Excel after our save |
| Formula engine | Wrap an existing MIT/Apache engine behind a trait; Phase 1 benchmark picks | Hundreds of Excel functions without writing them. Leaning Formualizer: IronCalc recalculates every formula each time |
| v1 scope | Editing, formulas, formatting, multiple sheets, sort, filter, tables, conditional formatting, data validation, charts, pivots | Print and PDF come after v1 |
| Arabic | Arabic and mixed text display correctly; grid stays left to right | Direction stays in one place, so RTL sheets can come later |
| Theme | Light and dark mode everywhere, following the system with a manual switch | Uses `data-cg-theme` from `@cashgear/ui`; the grid follows it too |
| Collaboration | None; static PWA with no server | Files stay on the device |
| UI library | `@cashgear/ui`, commit-pinned git dependency as in CashGear Video | Missing generic components are built in `CashGear.React.UI` (Track U) |
| Sheet grid | Canvas grid, formula bar and cell editor stay in this repo | They are tied to the engine's data protocol. `CgGrid` is record-based and has no cell ranges |
| File format | `.xlsx` is the save format; no private format | Crash-recovery data is kept separately |
| Licences | No GPL, AGPL or LGPL in anything shipped | `cargo deny` and an npm licence check fail CI |
| Name | CashGear Sheet, crate prefix `cgs-` | Matches the sibling apps |

Default targets until Phase 0 confirms them: Windows 10/11 x64 first, macOS second; Chrome and Edge first, Safari and Firefox best-effort. Grid limits match Excel: 1,048,576 rows by 16,384 columns. All numeric exit thresholds below are proposals, confirmed in each phase's questions.

## Working rules

`CLAUDE.md` repeats these, so new sessions inherit them.

**Every phase**
1. Read `docs/PLAN.md`, `docs/PROGRESS.md` and `docs/decisions.md`.
2. Ask the phase's questions plus any new ones in one batch, recommended option first, and wait for answers before writing code.
3. Log answers with dates in `docs/decisions.md`. Pick and log small choices; stop and ask about anything expensive to reverse.

**End of every phase**
1. Record evidence for each exit criterion in `docs/PROGRESS.md`.
2. Commit, then tag `phase-N`.
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
- **Tile worker:** draws text and cells on `OffscreenCanvas`. Main starts it, because WebView2 can't load a worker started by a worker (see `nested-worker.ts` in moviepro). It talks to the engine over a `MessageChannel`.

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
- Binary `ViewBlock` tiles of 64 rows by 16 columns (24-byte cells plus a UTF-8 heap) are transferred, not sent as JSON. Stale tiles stay visible during recalculation.
- Scrollbars are self-drawn, because browsers can't size an element for 1M rows.
- Offset lookups use a lazy Fenwick tree over `Float64Array`, with hidden rows as 0. The direction flag lives here.
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
- [ ] 200+ files run, with a per-function report.
- [ ] Winner matches 99.5% of formula cells and has slice p99 of 50 ms or less.
- [ ] Decision logged. If neither passes, stop and ask.

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
- [ ] The test set opens without recalculation, and visible values match `calamine` and Excel.
- [ ] Size target met; 60 fps scrolling on 1M rows.
- [ ] Arabic and mixed-text golden images pass in light and dark, and web and desktop match.
- [ ] Default-styled workbooks are readable in dark mode.

### Phase 3: Preserving save
**Goal:** save writes a valid `.xlsx` where everything untouched survives. This comes before editing so the riskiest promise is proven early, driven by test edits through the Rust API.

**Questions:** Overwrite by default, or Save As the first time? Keep macros when saving `.xlsm`? Set recalculate-on-open in saved files?

**Tasks**
1. Span-patching writer, raw entry copy, element order table, append-only styles and strings, `calcChain` removal.
2. Atomic swap-write and re-index. Overwriting invalidates web `File` reads, so decide whether to copy the source to OPFS on open.
3. Save report; new blank workbook.
4. Spike `refreshOnLoad` for pivots.

**Exit**
- [ ] Clean entries are hash-identical.
- [ ] No new Open XML SDK validator errors compared with the source.
- [ ] Zero Excel repair prompts across the COM-open matrix.
- [ ] Chart, pivot and macro fixtures still work in Excel after saving.

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
- [ ] 1,000 random edits match Excel via COM.
- [ ] Input stays under 100 ms during recalculation.
- [ ] Files saved after structural edits open with no repair prompts.

### Phase 5: Ribbon and formatting (tag `v0.1.0-mvp`)
**Goal:** it looks and feels like Excel for daily work.

**Questions:** Same ribbon tabs as Excel? Key tips?

**Tasks:** Home ribbon on `CgRibbon`; font, fill, borders, alignment, wrap; number format dialog; format painter; merges; resize, auto-fit and hide; freeze panes; find and replace; context menus.

**Exit**
- [ ] Formats round-trip through Excel, and existing style indices are unchanged.
- [ ] Visual goldens are reviewed against Excel.

### Phase 6: Data tools
**Goal:** sort, filter, tables, conditional formatting, data validation.

**Questions:** Excel's sort semantics exactly? Keep extended (x14) CF read-only at first? Which table styles?

**Tasks:** sort; autofilter; tables with structured references; CF including data bars, colour scales and icons; DV with dropdowns; name manager; notes; hyperlinks.

**Exit**
- [ ] Filtering 1M rows takes under 200 ms.
- [ ] Rules match Excel on test files and round-trip without repair prompts.

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
- [ ] Benchmarks and the host decision are recorded.
- [ ] Installer test covers install, update and uninstall.
- [ ] PWA works offline, and double-clicking `.xlsx` opens the desktop app.

## After v1
Print and PDF export; RTL sheets; `.xls`, `.xlsb` and `.ods` import; collaboration only if that changes. Macros are preserved on save but never run.

## Top risks, by the earliest phase that exposes them
- **Phase 1:** wrong formula values, no way to yield during calculation, memory use, fast-changing upstream.
- **Phase 2:** font metric drift when Calibri isn't installed.
- **Phase 3:** Excel repair prompts; whether `refreshOnLoad` is enough for pivots.
- **Phase 4:** stale references in retained parts after structural edits.

Engine facts above were read from published packages and source on 2026-11-01. Nothing has been compiled yet, which is why Phase 1 exists.

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
