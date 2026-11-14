# CashGear Sheet: agent notes

CashGear Sheet is an Excel-like spreadsheet for the browser and desktop, built on one Rust/WASM core with a React UI and a Tauri 2 shell. `docs/PLAN.md` holds the phased plan, `docs/PROGRESS.md` records what is done and the evidence for it, and `docs/decisions.md` logs every answer and default.

## Workflow

- `cashgear-sheet`: work only on `main` and push everything to `origin/main`. No other branches, no extra worktrees, no PRs.
- `CashGear.React.UI` (`D:\CashGear\Projects\CashGear.React.UI`): commit and push to its existing branch, `claude/cashgear-react-ui-library-xq8tsz`. Never create a branch there.
- Make small Conventional Commits scoped by crate or package, for example `feat(xlsx): …` or `fix(ui): …`. Every commit builds and passes the checks below.
- Work one phase at a time:
  1. Read PLAN, PROGRESS and decisions.
  2. Ask the phase's questions, plus any new ones, in one batch with the recommended option first. Wait for the answers before writing code.
  3. Log the answers in `docs/decisions.md` with the date.
- For a small choice, pick the sensible default and log it. For anything expensive to reverse, stop and ask.
- End of phase:
  1. Record the evidence for each exit criterion in `docs/PROGRESS.md`.
  2. Commit and push, then tag `phase-N` once remote CI is green. Phase 5 is `v0.1.0-mvp` and Phase 9 is `v1.0.0`. From Phase 10 on, each phase is also a minor release: a `CHANGELOG.md` section, one version bump across the app, crates and packages, and the tag `v1.(N-9).0` (Phase 10 is `v1.1.0`). Publishing the drafted release is the user's.
  3. Report what was done, each exit criterion's result and any known gaps.
  4. Show the next phase as a start-session card: `start_session` with `initiation: "own_initiative"`, `context: "fresh"` and `use_worktree: false`, so it works on `main` in this checkout. Then stop.
- Never commit secrets, signing keys or test workbooks. Workbooks come from `scripts/fetch-test-workbooks`, and private ones stay out of git.

## Hard limits

- No GPL, AGPL or LGPL in anything shipped. `cargo deny` and an npm licence check fail CI. Test-only tools such as LibreOffice and the Open XML SDK never ship.
- The UI never touches cell data. It sends engine commands and draws the snapshots that come back.
- The formula engine is the only cell store. `cgs-model` holds everything else, so there are never two copies of every cell.
- Saving never damages what the app can't edit. Untouched zip entries are copied raw, and changed parts are patched at source byte spans, not re-serialized.
- Dark mode's colour swaps are for display only. They never reach the saved file.
- Missing generic components are built in `CashGear.React.UI`, then consumed by bumping the pinned commit. The canvas grid, formula bar and cell editor stay in this repo.

## Layout

See "Architecture" in `docs/PLAN.md`.

- `crates/`: `cgs-model`, `cgs-calc`, `cgs-numfmt`, `cgs-refs`, `cgs-xlsx`, `cgs-pivot`, `cgs-engine`, `cgs-wasm` (the only JS-facing API) and `cgs-native` (desktop only).
- `apps/web`: Vite PWA. `apps/desktop/src-tauri`: Tauri 2 shell, left out of `default-members`.
- `packages/ui`: React shell and canvas grid, on `@cashgear/ui`. `packages/engine-client`: workers, comlink client and tile cache.
- `packages/erp-connector` and `packages/workspace-connector`: the sheet's side of Cash Gear ERP's and Cash Gear Workspace's contracts (`docs/erp-contract.md`, `docs/workspace-contract.md`), with their mocks in `tests/erp-mock` and `tests/workspace-mock`. `apps/web/src/platform/cloud.ts` is the third `SheetPlatform`, for Workspace's workbooks (Track S).

Browser-only Rust dependencies sit under `cfg(target_arch = "wasm32")`.

CashGear Video (`D:\CashGear\Projects\cashgear-moviepro`) is the model to follow:
- `scripts/build-wasm.mjs`: the WASM build.
- `packages/ui/src/timeline/`: canvas outside React.
- `packages/engine-client/src/client.ts`: the worker client.
- `packages/ui/src/platform.ts`: the platform interface.

## Commands

First run `pnpm install` and `pnpm setup:wasm` (builds the pinned `wasm-bindgen` into `.tools/`). `pnpm dev:web` and `pnpm dev:desktop` start the app. Checks run in CI, in this order:

```bash
cargo fmt --all --check
pnpm build
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p cgs-wasm --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
pnpm typecheck && pnpm lint && pnpm format:check && pnpm test
pnpm test:save
pnpm test:e2e
pnpm desktop:build && pnpm desktop:smoke
pnpm deny && pnpm licenses:check && pnpm licenses:selftest
```

Releases are in `docs/DEPLOYMENT.md`. A `v*` tag runs `.github/workflows/release.yml`, which runs `pnpm desktop:installer-test` (install, double-click, update and uninstall, on Windows) and drafts the GitHub release.

## Gotchas

Found in Phase 1, measured in `docs/phase1-report.md`:

- Formualizer comes from our fork at an exact commit, through `[patch.crates-io]` in the root `Cargo.toml`. To take an upstream release, rebase the fork and move the pin, then rerun `bench/`.
- Formualizer's own xlsx importer leaves saved spill values in place, so it can look right without calculating. Compare against Excel, not against it.
- The first edit after opening builds the graph in one step that can't be divided: 4 s at 1,000,000 formulas. Start it right after opening.

Found in Phase 2:

- Calamine builds a full matrix of its cells, so a value at a far corner makes it ask for terabytes. `fidelity` skips such sheets, and the reader's own tests cover them.
- Excel refuses `0.00e+00` in a file's format but `TEXT` accepts it, so `cgs-numfmt` allows it only for `TEXT`.
- Office on this PC has an Arabic UI on en-US Windows: `Range.NumberFormat` refuses `[Red]`, and `TEXT` wants colour names in Arabic. Check a format by opening a file that holds it.
- PowerShell can't run `.ps1` files here. Pass the script as `-Command`, as `bench/run-excel-text.mjs` does.
- Excel won't open a workbook whose drawing parts have no content types, so fixtures in `tests/fixtures/workbooks.mjs` must stay valid. Run `bench/run-excel-text.mjs` on them after changing them.

Found in Phase 3:

- Excel refuses a damaged or invalid workbook over COM where a person would get a repair prompt, so `Workbooks.Open` throwing is the signal. `bench/run-excel-matrix.mjs` opens broken fixtures first to prove it still sees that. Add a broken fixture when you find a new way to upset Excel.
- `#SPILL!`, `#CALC!` and other newer errors can't go in a cell as plain `t="e"`: Excel refuses the file. They need rich value metadata.
- `tests/fixtures/vba.mjs` builds a real VBA project. A storage's children sort by code unit, not by locale, or Excel can't find `_VBA_PROJECT`.

Found in Phase 4:

- The Excel on this PC is 2016 (16.0): no dynamic arrays, XLOOKUP, LET, CONCAT, TEXTJOIN or IFS, and `Range.Formula2` doesn't exist. Compare with `.Formula`, and don't use those functions in anything Excel has to agree with.
- A PowerShell function returning a Range unrolls it to cells, so use `return , $range`. COM needs `[double]` casts for numeric `.Value2`.
- Over COM, setting part of a legacy array can be silently ignored rather than refused, typing beside a table expands it unless `AutoExpandListRange` is off, and `+A1` stays text where typing makes a formula. A new table column's default name is Arabic here.
- `Workbooks.Open` refusing a file doesn't say why. A cell with a text result and no `t="str"` is enough.
- Paste needs the browser's own `paste` event, so the grid must not `preventDefault` Ctrl+V on keydown.
- The first edit's `protectSource` is asynchronous, so commands run through one queue in `AppShell`. Never call `engine.command` around it.

Found in Phase 6:

- Excel's AutoCorrect table settings (`AutoExpandListRange`, `AutoFillFormulasInLists`) are saved with Excel and outlive the COM session, so a script that changes them must put them back.
- Excel's `ApplyFilter` works a dynamic average out again rather than reading the file's `val`, and an error in the column makes the average 0.
- `Validation.Value` treats a list typed into the rule as case-sensitive and a list read from cells as not. Typing takes either in any case.
- `DisplayFormat` shows conditional fills, fonts and colour scales, but not data bars or icons.
- Whole-number doubles have zero low bits, so `FxHashMap` on their bits collides badly. Spread them first, or use a direct-mapped table.
- Bench scripts must build the harness tools they run. `editfuzz` once ran a build hours old.

Found in Phase 8:

- Excel refreshes a file that asks for `refreshOnLoad` just after `Workbooks.Open` returns, and turns COM calls away meanwhile (`RPC_E_CALL_REJECTED`). The bench's Excel scripts register a message filter that retries; copy it into any new one.
- A pivot table's items must point inside its cache's items, group items too, or `Workbooks.Open` refuses the file. `bench/run-pivot-check.mjs` has Excel open and refresh everything the app saves.
- Excel on this PC writes pivot captions in Arabic ("Row Labels", "Grand Total", "(blank)"), but date group months stay "Jan". `pivotcheck::arabic()` holds the words; the app's are English.
- With "For empty cells show" set, Excel saves 0 in those cells and shows the caption.
- On refresh, new items go last in a field sorted by hand, hidden if the field's filter hides others, and in display order in a sorted field. Ties in a sort by value keep the order they had.

Found in Phase 9:

- The engine holds about 430 bytes per formula and 14 per value in WASM, so about 10M formulas reach the 4 GB cap (`bench/measure-scale.mjs`). A column of one relative formula costs almost nothing, so a memory test needs distinct formulas or many columns.
- A trap in any call breaks the module, and wasm-bindgen then fails every later call with "recursive use of an object". Every worker call goes through `guarded()` in `engine-worker.ts`, and a disposed client's calls reject with `ENGINE_STOPPED`, which the shell ignores.
- Tauri's `fileAssociations` takes a type's default, and its uninstaller writes an empty default into `HKCU\Software\Classes\.<ext>`, which hides Excel's. The file types are registered by `apps/desktop/src-tauri/windows/hooks.nsh` instead.
- Tauri's registry key for a product is `Software\<publisher>\<product>`, and a silent uninstall keeps the app's data folder.
- `tauri::generate_context!` needs `serde_json` in the desktop crate once a plugin has settings.
- This checkout's `.cargo/config.toml` (excluded through `.git/info/exclude`) points Formualizer at `../formualizer-fork`. Keep that folder at the pinned commit, or local builds differ from CI's.

Found in Phase 10:

- Excel breaks pages in its printer's dots, not in points: rows print about 3% shorter and columns 8% wider than on screen, and both factors follow the workbook's normal font (`Units` in `crates/cgs-engine/src/print.rs`, measured for 24 fonts). Measure with Microsoft Print to PDF as Excel's `ActivePrinter`, which Excel takes only once a workbook is open.
- `HPageBreaks` and `VPageBreaks` leave breaks out or refuse indexes their `Count` gave. Read breaks with `INDEX(GET.DOCUMENT(64), n)` and `65`; over COM a bare `GET.DOCUMENT` returns only the first. It also lists where the printed range starts and ends.
- Excel prints from A1, keeps manual breaks along a side Fit To leaves automatic, and takes `sheetFormatPr defaultRowHeight` from the normal font, not the attribute.
- On this PC Calibri has Arabic glyphs, so the screen draws Arabic in Calibri. When the PDF can't use installed fonts, pages measure Arabic in the bundled Noto, or text won't fit its cells.
- Noto Sans Arabic draws dots as marks that many letters share. A PDF needs a code per glyph and text, and marks as outlines, or copying gives the wrong letters. pdf.js treats an empty ToUnicode entry as missing and falls back to the code.
- `requestAnimationFrame` never fires in a hidden window or browser pane. Wait for a frame or a timeout.
- Carlito and Liberation reserve their names, so ship them unmodified; stripping hinting would be a modified version.
- `target/debug` grows to about 100 GB. When links fail with LNK1318 or LNK1140, D: is full: clear `target/debug/incremental`.
- A PowerShell function named `Measure` is shadowed by the `Measure-Object` alias.
- Excel crashes (RPC unavailable) opening a sheet for printing when its header picture's VML shape type has no `v:formulas`. Copy the shape type Excel writes.
- `ExportAsFixedFormat` through Microsoft Print to PDF puts A4 sheets on Letter paper, though it breaks their pages for A4.
- A line from `Get-Content` carries PowerShell's provider properties, and `ConvertTo-Json` serialises them all (1.2 GB for 25 lines). Cast to `[string]` first.
- Formualizer's `for_each_range_value` visits every cell of its range. Styled blank rows can make a sheet's used range all of XFD1048576, so read values only as far as `value_bounds` says.
- `pnpm dev:web` rebuilds the WASM when Rust changes and reloads the page, which breaks a bench run mid-book. Let it settle first.

Found in Phase 11:

- Excel 2016 mirrors a right-to-left sheet's cells, borders, gridlines, data bars, notes' corners, buttons and drawings, but not alignment: Latin text stays left, numbers right, and text overflows on its aligned side. Icon sets and diagonal borders aren't mirrored, and rotated text turns the other way. CopyPicture shows it; trust it over memory.
- Arrow keys on a right-to-left sheet go the way they point, Tab and Home go by column order. To measure keys, make Excel visible, `AppActivate` its caption and use `SendKeys`.
- The grid stays in left-to-right positions. `grid/mirror.ts` mirrors drawing, and the controller mirrors pointer x on the way in and DOM overlays on the way out. A `Proxy` over a 2D context must `Reflect.get` with the real context as receiver.
- A tile carries only each cell's right and bottom lines, so column A's left border and row 1's top border never draw, in either direction.
- Right-to-left goldens are cropped from the canvas's right (`expectGolden(..., { fromRight: true })`), where column A is.
- With D: nearly full, Playwright reports "Target crashed" and spawns fail with EPERM before any link error. Check free space first.

Found in Phase 12:

- In Chrome each `memory.grow` costs about a quarter of a millisecond, and dlmalloc grows by what each request needs. `crates/cgs-wasm/src/alloc.rs` grows 64 MiB at a time; profile WASM with `node scripts/build-wasm.mjs --release --profiling` and `bench/profile-read.mjs`.
- A save is a future that stops at `Pace` points (`crates/cgs-xlsx/src/pace.rs`). Anything that streams a whole part during a save needs a point in its loop, or it holds the worker again.
- A headless browser takes the system's launch of an installed web app ("Handling STARTUP request") and opens no window. Open with tests run headed.
- Chrome asks once whether an installed app may open file types, in a dialog UI Automation can press. Edge under automation answers that itself as a no, and then refuses files even from `PWA.launchFilesInApp`.
- `bench/generate-speed-books.mjs` skips a book that exists, so a book from before a generator fix keeps the old bytes. Delete it to write it again.
- Formualizer's `ingest_has_no_formula_plane_producer_sites` test reads its own source split on an LF-only marker, so it fails on a CRLF checkout whatever the change.

Found in Phase 13:

- Excel 2016 on this PC groups dates into new fields named "Months (Date)" and so on, even for one period, and gives the date field `par` on the last. Excel's own pivots in `bench/` were made this way.
- Formualizer's custom functions see values only. GETPIVOTDATA needs its reference's place, so fork commit `4b581d3` added `register_custom_function_with_references`.
- `scan::edit_tag` applies the first edit it's given for an attribute and drops a second one for the same name.
- `SheetCanvas` forwards each grid event by name, so a new `GridEvents` handler must be added there too, or it never fires.
- `CgRibbon` key tips listen on `window` in the capture phase, so they see letters before the grid. They skip Ctrl chords, so `Control+Alt+n` still opens a book.
- Over COM, a trendline's `DataLabel.Text` is empty, so compare its fit with LINEST and LOGEST, not with its label.
- A built-in cell style goes into the file the first time it is applied, with Excel's `builtinId`. A file with no `cellStyleXfs` gets Normal first.

Found in Phase 14:

- `docs/erp-contract.md` is the ERP contract, and `packages/erp-connector/src/contract.test.ts` proves a server keeps it. Change all three (the types, the mock, the doc) together, and run the suite against the mock.
- `fetch` refuses ports on the Fetch spec's blocked list with "bad port", 4190 among them. The mock's CLI defaults to 4180.
- Node's `fetch` sends no `Origin` unless it's set, where a browser always sends one on a cross-origin POST. The suite sets it, and Redeem refuses a request without one.
- The mock's `/__mock/fault` `match` is a regular expression over the path: `VendorSheet/Commit` matches CommitStatus too, so anchor it with `$`.
- A workspace package with no dependencies gets no importer in `pnpm-lock.yaml`, and `--frozen-lockfile` still passes.

Found in Phase 15:

- A bound ERP table is guarded in layers: `binding_refusal` before commands, for each refusal's message, then the content rule in `apply_cells`, the table checks in `Op::Tables` and `Op::BookTables`, the shape check in `apply_axis` and sheet removal. A new path that writes cells, tables or the sheet's shape must go through one of them; `tests/erp.rs` replays every refusal with the command checks off (`set_erp_precheck(false)`).
- Undo, redo and every rollback run inside `Session::unguarded`. A refused part of `Op::Many` takes back what went before it.
- ERP record pages reach the engine as bytes (`erp_page`) and are staged on `Engine`, not the session, so they survive opening the new workbook they bind into.
- Playwright starts the mock ERP on 4180 with `--sheet-url http://127.0.0.1:4173` and reuses one already running: a hand-started mock with another `--sheet-url` sends launches to the wrong page. Stop it first.
- `cgsTest.shownText` (the engine's `copyText`) includes rows a filter hides; read a row's height to see whether it shows.
- The dev server's WASM is unoptimised and takes seconds to bind 2,000 records. Time binding with `bench/measure-erp-bind.mjs` on `pnpm build`.
- The browser pane refuses `127.0.0.1`; open the dev server and the mock as `localhost`, which the mock's origins allow.
- Bash heredocs here turn `\t` and `\u…` inside a script's strings into the characters themselves. Write scripts with the Write tool, or build such strings with `chr()`.
- Against a hand-started mock with only `ERP_TEST_KEY_EDITOR`, set `ERP_TEST_SKIP_KEYS=V-1005`, or the 413 case picks the locked vendor.
- `[Timestamp]` isn't on `VendorModel`: UnifiedAPI reads `Vendors.RowVersion` with its own SQL until every tenant database is scripted. `CashGear.UnifiedAPI.Tests/SheetContractRun.md` runs the contract suite against UnifiedAPI on LocalDB, after printing and checking every connection string.

Found in Phase 16:

- Bound text and lookup fields keep what's typed as text. Typing, external paste and Replace coerce the raw text with `bound_text_typed` before Excel's parsing loses it; `coerce_bound_values` in `apply_cells` turns any other number or TRUE written there into text. A new path that parses typed text must call `bound_text_typed`, or `001` becomes 1. Convert to Number skips bound text cells.
- Record states follow the cells through `note_bound_writes` at the end of `apply_cells`, undo and redo included. Anything that changes the originals (refresh) must call `restate_all`, or the badge and tiles go stale.
- A spill into an empty bound cell isn't a write, so tiles, `erpCell`, the changeset and the badge's counts read `spill_areas()` for it.
- The Schema's `min` and `max` come as strings too (the mock sends `"min": "0"`), as decimals travel. Rust tests built with numbers passed; only the browser found it.
- ViewBlock 5: cell flag bits 8 to 14 are ERP states, set only through `TileInput.erp`, which only the screen's tiles get, so pages, PDF and copies never show them.
- The browser pane's `type` action sends text without keydown events, which the grid needs to start editing. Drive edits there with `cgsTest.command`, and keyboard paths with Playwright.
- The dev server in the browser pane offers to recover working copies from earlier sessions on each load. Close the prompts; don't discard them.
- From Chrome 157, an installed web app can't read a file Open with launches from a folder File System Access counts as the system's, such as `%LOCALAPPDATA%\Temp`: `getFile()` throws a SecurityError. The PWA test launches its file from its own output folder.

Found in Phase 17:

- What the ERP answered lives in `Binding::marks` (`cgs-bind/src/marks.rs`): errors by key and slot with the value sent, conflicts by key, and the last commit's saved fields. `note_record` lays them over the states and drops those the cells outgrew. Anything that replaces originals must take or clear them: `apply_refresh` clears all, `take_commit` one record.
- A commit freezes the bound table (`erp_frozen`): `guard_cells` refuses every write into it and `command()` refuses undo and redo, until `erp_apply_commit` lets go or the shell calls `erpFreeze(false)`. The network call runs outside the shell's command queue; only the freeze, the states and the result go through it.
- A commit is safe to send again only with the same `commitId` and payload. After no answer, a 500 (the ERP's own COMMIT can fail with its outcome unknown) or a 503, keep the id; `payloadKey` tells whether the next commit is the same payload.
- `cgs-bind` decodes versions as strict base64: the last character before `=` must leave the unused bits zero. `AAAAAAAACQ0=` reads, `AAAAAAAACQ1=` is refused as no version.
- The mock's `/__mock/fault` appends faults and takes the first that matches; one with `times: 0` lasts until `/__mock/reset`. A lost answer for a commit needs `times: 4`: the request and the HTTP layer's three retries.
- Python here writes `\r\n` in text mode, and some docs check out with CRLF. Read with universal newlines and write with `newline=''`, or Prettier and diffs see every line changed. Git normalises on commit (`eol=lf`).
- The Bash tool can refuse a heredoc whose text holds apostrophes ("unexpected EOF while looking for matching"). Write the script with the Write tool and run it.
- On CI's Chrome 156, an Open with that comes as soon as an installed app's registration shows can go unanswered: no question, no window, nothing in Chrome's log. `pwa.spec.ts` waits for the launcher and chooses Open with again until Chrome asks. To see what a CI browser shows, attach `windowTexts` (UI Automation), since the app's window isn't a page of the test's context.
- To bisect a CI-only e2e failure, a temporary step before `pnpm test:e2e` can run a few specs with `--output` of its own (and add that folder to the artifact). Take it out again before tagging.

Found in Phase 18:

- Refresh, a 409 and Resolve are one merge (`cgs-bind/src/merge.rs`, `Binding::plan_merge` and `apply_merge`, driven by `Session::erp_merge` in `cgs-engine/src/erp_merge.rs`). A record adopts the ERP's version only when no field clashes and the answer carried every field; anything new that replaces originals or versions must go through it, or a stale draft can pass the version check. `tests/erp_merge.rs` holds the oracle.
- A conflict mark keeps its clashing fields marked even when typed back to their originals, since the ERP holds another value there; the mark goes only when the record has no changes at all, keeping its old version until the next refresh.
- A refresh's conflicts take who and when from the entity's own `modifiedBy` and `modifiedDate` fields, since Query carries no author; a 409 carries them itself.
- `CgGrid`'s `onSelectionChange` gives typed key tokens (`string:3:4` for the key `3:4`), not the keys `keySelector` returned. Phase 17's Discard Selected never matched them.
- A bound workbook's recovery draft (`engine-worker.ts`) journals the bind payload (`{"b":…}`), commands, and each ERP step with its input (`r` refresh deltas, `s` marks, `a` commit results, `v` resolves), plus `p` (something waits on the ERP) and `q` (a commit's outcome unknown). Replay must be exact: an ERP step that fails drops the draft. A draft restarts from `erp_draft()` whenever nothing waits after an ERP step, and is offered only while `p` is 1 or `q` is set.
- A tab waiting on `BroadcastChannel` must answer before closing its channel: `waitForHandover` closes itself after replying `taken`.
- `Intl.PluralRules('ar')` picks the form by the last two digits: 1,240 is `many` (`موردًا`), 100 is `other`.
- `pnpm build:wasm --release` spends about 17 minutes in binaryen. For e2e iterations `node scripts/build-wasm.mjs --release --profiling` skips it (never ship that build); run the full `pnpm build` before the standing checks.
- Playwright's `page.close({ runBeforeUnload: false })` leaves a page's OPFS draft for the next page of the same context, which is how the recovery e2e kills a page.
- A mock fault list answers the first match: `drop: 'after', times: 1` then `status: 401, times: 0` on `VendorSheet/Commit$` gives a commit that landed with a 401 on every resend, Chrome's own included.

Found in Phase 19:

- Binding loads the new workbook's sheet as a file's sheet loads (`load_fresh_sheet` in `crates/cgs-engine/src/erp.rs`: `begin_sheet`, `load_rows`, `finish_sheet`), then notes each cell's patch, style and row itself. Through `set_cells` it cost about 10 µs a cell in WASM. Anything that writes a whole new sheet at once can do the same, but only into an empty sheet with nothing to undo.
- A launch opens the blank workbook and readies the tile worker's fonts (`warmText`) as soon as the schema arrives, while the records download (`prepareFor` in `useErpTools.tsx`). `bindStaged` takes that promise. If the load then fails, `closeBlank` puts the workbook away.
- The grid holds its redraws while a binding writes, fits and settles (`holdView`), so its tiles are made once at their fitted widths. Every change in between used to rebuild and redraw them.
- A binding's recovery draft starts 300 ms after the sheet shows (`erpStartDraft`), or before the first edit or ERP step after the bind settles, whichever comes first. Journal lines from before then wait in memory and follow the draft's base, which is the records as the ERP sent them; anything that journals waits while the base is written.
- Tile builds once keyed derived styles with `format!("{dxf:?}")`, which cost more than the tile's text. Never key a per-cell cache with `Debug`.
- Screen tiles fill each row's neighbouring cells of one colour as one rectangle, and draw gridlines after tints (`drawBackgroundRuns`). Edges are whole pixels, so the picture is the same. Pages keep cell-by-cell drawing.
- `bench/profile-erp-bind.mjs` profiles the main thread and both workers. For readable main-thread names, build the web app with `pnpm --filter @cgs/web exec vite build --minify false`.
- Setting `fillStyle` or `font` on a software `OffscreenCanvas` costs microseconds each time. The first text in a font pays its loading and its script's shaping, about 30 ms for Calibri with Arabic.
- The `ready` load phase comes only once reads go quiet (`QUIET_MS`), long after `loaded`. Don't wait for it to time anything.
- The live API accepts only `Origin: https://sheet.cashgearerp.com` (Track E4), so a localhost sheet can't talk to it. Runs against the test tenant drive the deployed sheet with `pnpm erp:tenant`, which loads the keys from `%USERPROFILE%\.cashgear\sheet-erp-test.env` and blanks them out of its output.
- The live API counts 20 launches a minute per client address. The contract suite opens a session in nearly every case, so on a real server it waits out 429s; `pnpm erp:tenant` gives it a 300 s timeout per test.
- Playwright's `keyboard.type` sends characters that aren't on the keyboard, such as Arabic, without keydown events, so the grid never starts editing. Type ASCII, or use `cgsTest.command`.
- FusionCache logs its cache keys at Information, so UnifiedAPI's API-key cache key is now the key's SHA-256 (Track E4). Never put a secret in a cache key there.

Found in Track S1 and S2:

- `docs/workspace-contract.md` is Cash Gear Workspace's contract: change it, `packages/workspace-connector/src/contract.ts` and `tests/workspace-mock` together, as the ERP's three.
- `readErpLaunch` used to take a `#launch=` fragment off any path, so it would have used up Workspace's ticket on `/cloud`. It now leaves `/cloud` alone; a new launch route must be left out of it too.
- A save's 412 whose `current.sha256` is the bytes just sent is the sheet's own save landing after a lost answer, and counts as saved. A test of a real conflict must save different bytes from the version someone else saved.
- The mock Workspace's Blob Storage is a second origin, one port above its API (4281). Every mock origin the page reaches must be in `MOCK_APIS` (`apps/web/src/csp.ts`), or the preview's CSP refuses it.
- An ordinary workbook doesn't set the page's title; ERP data and Workspace's workbooks set their own (`– CashGear Sheet`).
- Playwright's `page.close({ runBeforeUnload: true })` fires `pagehide`, so a cloud tab's End reaches the mock.

Found in Track S3 to S5:

- `/embed` is Workspace's frame (`sheet-embed.1`, `packages/ui/src/embed`), a second page of the build (`apps/web/embed.html`) that never starts the editor. Its validators are as strict as Workspace's: a field the contract doesn't name fails the message. `tests/contracts/sheet-embed-vectors.json` is Workspace's file, copied with its commit; change neither by hand.
- workspace.cashgearerp.com and sheet.cashgearerp.com are one site, so a framed `/embed` shares the sheet's storage and its service worker. The embed's engine starts `inMemoryOnly`, or its start sweeps other tabs' working copies.
- Vite's preview sets `preview.headers` as each file is served, over any earlier middleware's. Headers that differ by path (`/embed`'s `frame-ancestors`) are set by `pagePolicies` in `vite.config.ts`, never in `preview.headers`.
- The build's entries are named `main` and `embed`, so its scripts are `main-*.js`, `embed-*.js` and a shared chunk, not `index-*.js`.
- Headless Chrome hides scrollbars (`--hide-scrollbars`), so a frame never needs room for one. `embed.spec.ts` turns them back on with `test.use({ launchOptions })`, which Playwright allows only at a file's top level.
- A `ResizeObserver` can fire while an answer is being made; `resize` waits until the answer is sent, or it arrives before the `loaded` it belongs to.
- `pnpm notices` writes the About box's notices; a web build without it (`pnpm --filter @cgs/web build`) fails `pwa.spec.ts`'s About case. Run the script through pnpm.
- `tests/embed-parent` holds 4273, Workspace's e2e port, and Playwright never reuses it: stop Workspace's e2e stack before Sheet's suite, or the run fails at once.

Carried over from CashGear Video, where they were measured:

- WebView2 never asks Tauri for the script of a worker started by a worker, so such a worker never loads on desktop. Start workers from the main thread, or copy `nested-worker.ts`.
- Tauri IPC carries about 11 MB/s and a custom protocol about 45 MB/s. Read files through a protocol, never through IPC.
- The CG library's overlays copy `data-cg-theme` from `<html>`, and some of its styles read `--cg-color-*` tokens.
- `@cashgear/ui` is also the name of a private package in `cashgear-archive`. Ours is the git dependency on `MohamedSadat/CashGear.React.UI`.
- `CgGrid` is record-based with no cell-range mode, so it can't be the sheet grid.
