> Approved on 2026-11-09. This is the plan for editing Cash Gear ERP data in CashGear Sheet: Phases 14 to 19 here, and Track E in the ERP repo. `PLAN.md` lists the phases, `PROGRESS.md` records what is done, and `decisions.md` holds the answers and defaults.

# Edit ERP Data in Sheet: integration plan

## Context

Cash Gear ERP users need to bulk-edit master data with spreadsheet tools, then validate, review and commit the changes back. Vendors comes first; Customers, Items, price lists and inventory master data follow on the same design.

CashGear Sheet 1.4.0 is local-only:
- It has no server, HTTP layer, URL handling or translated UI.
- It saves only `.xlsx`.
- Its engine is the only cell store.

This plan adds an **ERP-connected worksheet** beside normal workbooks, which keep behaving exactly as before. It also proposes the ERP API the sheet needs. **That API doesn't exist yet:** every ERP endpoint, table and handler named here is new, and is marked *proposed*.

What was inspected, read-only, on 2026-11-09:
- This repo.
- The ERP solution at `D:\CashGear\Projects\CashGear` (`CashGearERP.sln`).
- Its React client at `D:\CashGear\Projects\ERPClient_React`.

The plan assumes the solution's `CashGear.UnifiedAPI` is what serves `api.cashgearerp.com`, which `ERPClient_React/.env` points at.

---

## A. Current architecture analysis

### A.1 Sheet: what can be reused, and what stands in the way

**Record identity**
- No row or cell identity exists. A row carries only `LineInfo { size, hidden, style, custom }` (`crates/cgs-model/src/sheet.rs:164-173`). Only tables (`Table.id`) and drawings have IDs.
- Sort copies values rather than moving rows. Each target row is written as an `Op::Cells` copy of its source row, input and style together (`crates/cgs-engine/src/sort.rs:416-444`). Formulas are adjusted as if copied (`copied_input` → `moved_formula`, `command.rs:2902-2925`). Notes, validation and conditional formats stay where they were.
- Cut and paste (`Move`) copies contents, styles and rich text (`command.rs:3140-3273`).
- **Consequence:** whatever must travel with a record has to be a real cell in the sorted area. Anything that must not reach a saved file has to live outside the cells.

**Sorting a table**
- The ribbon's sort and Custom Sort expand to a table's data area when the active cell is inside it (`packages/ui/src/data/useDataTools.tsx:340-363`, `dataArea(table)`).
- The filter menu sorts the AutoFilter area minus its first row (`packages/ui/src/data/FilterMenu.tsx:571-588`).
- The Sort dialog also offers *Sort left to right* (`SortDialog.tsx:146-148`), and the engine doesn't refuse it (`sort.rs:301-305`).
- Sort moves only visible rows (`sort.rs:316-324`).

**Where cells change**
- `Session::apply_cells` (`command.rs:1201-1300`) is the one path for cell contents: typing, paste, fill, clear, sort, move, replace, and their undo and redo. It takes pre-images from `CalcEngine::set_cells`.
- It can't tell where an edit came from. `perform` keeps only a label (`:2366-2393`).
- Undo and redo call `apply_op` directly (`:2592`, `:2622`). The rollback in `perform_steps` ignores errors (`:2515-2518`).
- Structural edits and `ShiftCells` go through `apply_axis` → `calc.structural` (`:1543-1700`, `shift.rs:251-300`).
- Table ranges and options change through `Op::Tables` and `Op::BookTables` (`:1868-1877`). Turning a header row off moves the range first and then clears the old header row (`tables.rs:759-772`).
- Cells can also change with no edit to them:
  - a formula typed into an empty table column fills it as a calculated column (`tables.rs:1177-1233`);
  - tables grow when typing beside them (`table_growth`, `tables.rs:1137`);
  - dynamic arrays spill (`CellInput::Spill`).

**Protection**
- `protection_refusal` (`crates/cgs-engine/src/protect.rs:232-418`, called at `command.rs:3284`) is enforced in the engine and is Excel-strict:
  - sort needs every cell in the area unlocked (`protect.rs:365-367`);
  - deleting rows needs every cell out to XFD unlocked;
  - protecting or unprotecting clears undo (`:580-586`).
- It can't express "read-only but sortable". Its shape (checked before a command, with Excel's message) is the model to follow, not the mechanism to reuse.
- Possible gap: Paste checks only the `to` area as sent (`protect.rs:289`), and `paste_command` then grows that area to the source size (`command.rs:3008-3018`). Untested; Phase 16 checks it.

**Validation and text**
- Data validation runs only when a single cell is typed (`command.rs:3298-3320`, `dv.rs:292-323`). Paste, fill and typing into many cells skip it.
- Every JSON input command parses text as typing does (`input.rs:38`): leading zeros go, and dates follow the locale.
- `PasteExternal` parses each cell without looking at the target's Text format (`clip.rs:689-699`). All-mode paste copies the source style over the target's format (`command.rs:3064-3069`).
- No typed bulk-values command exists. `PasteExternal` with TSV is the only one-command bulk write (`clip.rs:647-700`).
- Values are `CalcValue { Empty, Number, Bool, Text, Error }` (`crates/cgs-calc/src/lib.rs:38-44`); a date is a number with a date format.

**Commands, undo and the WASM boundary**
- 82 commands, JSON with a `kind` tag (`command.rs:127-684`), dispatched by `Engine::apply_json` (`:4024`).
- `History` holds inverse Ops, up to 100 steps or 256 MiB (`:954-1021`).
- `cgs-wasm` exports one `Engine` struct (`crates/cgs-wasm/src/lib.rs:120`): `apply_command(json)`, `query`, `view_block`, `take_dirty`, the save steps and others. Errors become `JsError`.

**Tiles and per-cell decoration**
- A ViewBlock cell carries `u16 flags`; bits 8–15 are free (`view_block.rs:109-124`).
- Marks (24 bytes each: bar, icon, note, invalid, thread, error; `:51-61`) are built in `tile.rs:1345-1402` and drawn by `drawMarks` (`packages/engine-client/src/tile-draw.ts:346-441`).
- `build_tile` reads 64 rows across the tile's 16 columns plus 16 on each side (`tile.rs:796-814`).
- `TileInput` is built in eight places, PDF, print and HTML copy among them (`clip.rs:830`, `find.rs:235`, `format.rs:1380/1440/1465`, `lib.rs:414/761/797`).

**Dropdowns**
- `validation_json` (`dv.rs:430-457`) answers the `validationAt` query.
- The grid shows a list button when the answer has `dropdown && items` (`useDataTools.tsx:244-247`). Picking an item sends `{kind:'input', text}` (`:1022`).

**Worker client**
- Comlink, with both workers started from the main thread (`packages/engine-client/src/client.ts:175-317`).
- Every call goes through `guarded()` (`engine-worker.ts:968-987`), and a disposed client rejects with `ENGINE_STOPPED`.
- Changes come back as tile numbers through `watchChanges` (`engine-worker.ts:405-435`), not as cells.

**Crash recovery**
- Each command's JSON goes to a journal in OPFS `cgs-working` (`engine-worker.ts:690-703`, `:116-301`), and `openRecovered` replays it (`:822-871`).
- The journal is read whole and split on newlines, and replay errors are swallowed (`:139-147`, `:854-860`).
- `sweepWorkingCopies` deletes files that have no journal (`:180-191`).
- The journal starts with the first edit, through `protectSource` (`AppShell.tsx:771-776`).

**UI**
- `AppShell.runCommand` (`packages/ui/src/AppShell.tsx:763-861`) is the one command queue.
- Feature modules (`useDataTools`, `usePivotTools`, `useChartTools`, `useProtectTools`) each take a Host object and return `state`, `actions`, `refresh`, `overlays` and ribbon tabs (`useDataTools.tsx:46-68`).
- Contextual tabs are `CgRibbonTab.visible` (`ribbon-data.tsx:559-562`).
- Dialogs use `CgWindow` and `CgMessageBox`. `CgDialog` doesn't exist.
- `NoteLayer` (`data/NoteLayer.tsx:98`) shows how a DOM layer follows cells, through `grid.cellBox()` and `grid.onFrame()`.
- `SheetCanvas.tsx:24-44` forwards grid events by name.

**Shared library (`@cashgear/ui` at `ef1277e`)**
- Useful here: `CgRibbon` (with key tips), `CgWindow`, `CgMessageBox`, `CgGrid`, `CgStatusBadge`, `CgToastProvider`, `CgProgressBar`, `CgLoadingPanel`, `CgTooltip`, `CgListBox`.
- `CgGrid` is record-based, with `dirty`/`conflict` persistence states, `originalItem`, `changedFieldIds` and batch results (`Grid/CgGrid.types.ts:25-29, 190-239`). It can't be the sheet grid, but it fits a review list of changes.
- Missing: a message bar, and labels or colours for contextual-tab groups.

**Startup and platform**
- `SheetPlatform` (`packages/ui/src/platform.ts:56-107`) has nothing about network, auth or URLs.
- `main.tsx` reads nothing from `location`.
- The static host rewrites every path to `index.html` (`apps/web/public/staticwebapp.config.json:11-21`).

**Hosting**
- Web is served at `sheet.cashgearerp.com` with COOP `same-origin`, COEP `require-corp` and CORP `same-origin` (`staticwebapp.config.json:2-6`, `apps/web/vite.config.ts:13-17`).
- COOP cuts the link to the window that opened the sheet, so the ERP can't `postMessage` to it.
- The service worker ignores cross-origin requests (`apps/web/sw.js:66-71`).

**Desktop**
- The CSP `connect-src` allows only `self`, `ipc` and `cgs-file` (`apps/desktop/src-tauri/tauri.conf.json:23`), so any ERP fetch is blocked.
- The only plugins are dialog and updater. There is no deep link.

**Languages**
- No i18n exists: strings are hard-coded English, and `index.html` has `lang="en"` and no `dir`.
- Sheets can be right-to-left (Phase 11). The app's own Arabic UI is under "Later".

**Tests**
- Playwright runs Chrome and Edge against `pnpm preview` on port 4173 (`playwright.config.ts`), with `cgsTest` hooks (`AppShell.tsx:1273-1355`) and `expectGolden`.
- Vitest runs in Node. There are no HTTP mocks.

**Benchmarks to protect** (`docs/PROGRESS.md`)

| Measure | Value |
| --- | --- |
| Key to painted frame during recalculation, p99 | 44.4 ms |
| Next edit at 1M formulas | 46 ms |
| Tile requests, p99 | 21.9 ms |
| 1M-row filter, median | 151 ms |
| Wheel scroll | 99.1% of frames within 17 ms |
| Paste and sort latency | Never benchmarked |

### A.2 ERP: facts the design has to fit

**API host**
- `CashGear.UnifiedAPI`, net10.0, with controllers only (`Program.cs:160, 370`).
- Routes are RPC-style, module first: `api/payable/Vendor/GetVendors`, `[HttpPost("UpdateItem")]`.
- Each class carries a role policy (`[Authorize(Policy="Payable")]`).
- No API versioning and no response envelope. JSON is System.Text.Json camelCase.
- Errors are `{ message }`. Item endpoints use `{ message, correlationId, errors:[{field, message}] }` (`CG.Infrastructure/CGModels/APIModel/ItemUpdateModels.cs:46-57`).
- Responses carry `X-Company-Id` and `X-Correlation-Id`.

**Vendors**
- `VendorModel` (`CG.Infrastructure/CGModels/PurchModel/VendorModel.cs:11-95`): primary key `(Company, VendorAccount)`, an identity `RecId`, `VendorName` (150, required), `PayMethod` and `VendorGroup` (required foreign keys), `Currency`, `DebitLimit`, `Blocked` (int), contact fields, `Token` (64), and `CreatedBy`/`ModifiedBy`/`ModifiedDate`.
- **No RowVersion.**
- UnifiedAPI's `VendorController` only reads, and returns raw entities, `Token` included.
- `VendorCmdService.UpdateEx` (`CashGear.Operation/Modules/Payable/Vendor/VendorCmdService.cs:163-199`):
  - copies eight fields, last write wins;
  - is registered only in the Blazor host (`CashGear.Operation/Config/BLConfig.cs:564`);
  - uses `ValidateSave` (`:42-70`), whose messages are hard-coded Arabic.

**Patterns to copy**
- **Concurrency:** about 60 entities have `[Timestamp] RowVersion`, sent as base64, with 409 on mismatch. `CustPaymentCmdController.cs:155-200` compares tokens explicitly (`:561-583`) and maps the failure to 409 (`:637-643`).
- **Per-field errors:** `UpdateItemAsync` (`CashGear.Inventory/Product/Product/ItemCmdService.ItemUpdate.cs:28-247`) collects every field error before writing.
- **Transactions, logging and cache:** customer maintenance writes a SystemLog row inside its transaction and evicts cache tags (`CustomerMaintenanceService.cs:182-229`).
- **Privileges:** `CallerPrivileges` (`CashGear.UnifiedAPI/Services/CallerPrivileges.cs:17-44`) reads per-unit privileges (`UserUnitsPrivlage`; Vendors is unit 2004, with View, Edit, AdvEdit and the rest), and `LoadOwnedRows` limits ownership.

**Authentication**
- ASP.NET Core Identity, plus a home-grown **long-lived API key per user and company** (`Guid("N")`, with no expiry set; `CashGear.IdentitySystem/Services/ApiKeyService.cs`).
- `ApiKeyAuthenticationHandler` (`CashGear.UnifiedAPI/Services/ApiKeyAuthenticationHandler.cs:37-170`):
  - reads the key from `X-API-Key`, then the raw `Authorization` value, then `?api_key=`;
  - caches a valid key for 4 h;
  - emits `UserId`, `CompanyId`, roles and the raw key as claims.
- There is no JWT, OIDC, refresh token or one-time code. Login returns the user's existing key.
- `ERPClient_React` keeps the key in `localStorage` (`src/auth/AuthContext.tsx:42-128`).

**Tenancy**
- A database per tenant (the company id names the connection string; Kit and Dot share one), plus a `Company` column in every key.
- There are no query filters, so every query filters by hand.
- The company comes from the `CompanyId` claim (`Services/TenantService.cs:26-48`), and `TenantRegistry` allow-lists active companies.

**Data access**
- EF Core 10 on SQL Server through `IDbContextFactory`, with hand-run `DbScripts/*.sql` and no migrations.
- MARS is on, so there are no savepoints.
- Audit covers posted ledger rows only (`CG.Infrastructure/Audit/PostedDataAuditor.cs`). Master data gets only `ModifiedBy`/`ModifiedDate`.

**Cache, CORS and limits**
- FusionCache keeps `{Company}:Vendors` for 8 h, in memory, with no backplane between UnifiedAPI and the Blazor host.
- CORS is `AllowAnyOrigin` (`Program.cs:47`).
- There are no idempotency keys, and UnifiedAPI has no rate limiting.

**Tests**
- xUnit, mostly on SQLite in memory (`SqliteRowVersionContext` neutralises RowVersion), with opt-in LocalDB tests.
- WebApplicationFactory is used only in Portal's tests.
- UnifiedAPI's own development configuration points at a **live shared SQL Server**.

**ERP UI**
- `ERPClient_React/src/features/payable/pages/VendorList.tsx`: a read-only table with client-side Export to Excel.
- The client is React 19 with `@cashgear/ui`, i18next (en/ar, RTL) and a `fetch` wrapper (`src/lib/apiClient.ts`).
- The main ERP UI (a Blazor host) is a separate repo and isn't on this PC.

---

## B. Proposed architecture

### B.1 Components

```text
 ERPClient_React (Vendors list)            CashGear Sheet (browser tab; desktop later)
  "Edit in Cash Gear Sheet"                ┌ main thread ───────────────────────────────────┐
   │ 1 CreateTicket (X-API-Key)            │ packages/ui/src/erp  ERP Data tab, dialogs,    │
   │ 2 open sheet…/erp#launch=<ticket>  ──►│   status badge, hover   (commands/queries only)│
   ▼                                       │ packages/erp-connector  session, fetch, paging,│
 UnifiedAPI (proposed routes)              │   validate/commit/refresh orchestration        │
  api/system/SheetLaunch   ◄── 3 Redeem ───│        │ record pages as ArrayBuffer (transfer)│
  api/sheet/Session  (X-Sheet-Session)     │ packages/engine-client (comlink, guarded)      │
  api/payable/VendorSheet  ◄── 4 HTTPS ────└────────┼───────────────────────────────────────┘
   Schema · Query · Commit(dryRun) ·         engine worker: cgs-wasm → cgs-engine
   CommitStatus                                ├ cgs-bind (new, pure): bindings, id/version/
  existing Identity, tenancy, privileges,      │   originals per key, guard, changeset, rules
  EF Core + SQL Server, SystemLog              ├ Formualizer cells (the only cell store)
                                               └ tiles + state flags → tile worker
```

### B.2 Who owns what

| Layer | Owns | Never |
| --- | --- | --- |
| `cgs-bind` (new crate, pure Rust, no I/O) | Bindings; record key → `{id, version, originals}`; field rules; guard decisions; changeset; the three-way refresh merge; record states | Networking, auth, business rules beyond the schema's constraints |
| `cgs-engine` | Bind, refresh and apply-result commands; guard calls in the cell, table and structural paths; the `changeset`, `recordAt` and `bindingSummary` queries; state flags in tiles | Talking to the ERP |
| `cgs-wasm` / `engine-client` | Typed JSON, transferred record pages, new journal entries, `guarded()` | Parsing records on the main thread |
| `packages/erp-connector` (new, TypeScript, no React) | Session, HTTP, retries, cancellation, error mapping, orchestration of open, validate, commit, refresh and resolve | Holding cells; tokens in storage, URLs or logs |
| `packages/ui/src/erp` (new) | Ribbon tab, dialogs, status badge, hover details | Reading or diffing cells itself |
| UnifiedAPI (proposed) | Authentication, tenant, record and field authorization, field allow-list, business validation, RowVersion checks, transactions, audit, idempotency | Trusting the client for anything beyond ids, versions and new values |

The calculation engine itself (Formualizer, `cgs-calc`) is unchanged. ERP networking, auth and business rules stay outside Rust.

### B.3 Key engineering decisions

1. **Identity: a locked key cell, not a row number.**
   - Each bound row holds its business key in a read-only cell (Vendors: `VendorAccount`, primary key `(Company, VendorAccount)`), so the key travels with every sort.
   - `cgs-bind` maps key → `{id, version, originals}`, where `id` is the ERP's surrogate (`RecId`). Commits address records by `id` and `version`.
   - `RecId`, versions and original values never sit in cells, so they never reach a saved file.
   - If the ERP renames a key mid-session, the version moves and a conflict follows. Refresh by id then rewrites the key cell and the map.
   - Duplicate or unknown keys make a bind or a changeset fail outright. No user action can produce one, but the engine checks anyway.

2. **One content rule guards every path.** `cgs-bind` checks the real list of edits inside `apply_cells`, so there is no per-command prediction to get wrong. Within a bound table:
   - Editable cells change freely.
   - Any edit that touches a key, read-only or header cell must be a **whole-row move**: each touched row is written across the full table width with the contents of exactly one touched row, so the set of rows stays the same.
     - Passes: sort and its undo.
     - Fails: paste, fill, move, replace, Remove Duplicates and left-to-right sort.
     - A refusal refuses the whole command, with a message in Excel's manner.
   - Undo, redo and rollback run with a Session flag that turns the guard off. They only replay edits that were allowed, and a rollback must never be refused halfway.
   - `Op::Tables` and `Op::BookTables` are guarded: a bound table's size, columns, header and totals flags, name and calculated columns can't change.
   - Structural edits are refused only where they would resize the table: inserting or deleting rows or columns inside it, `ShiftCells` across it, merges, Convert to Range, deleting its sheet. Moving the whole table, by inserting rows above it for example, is harmless, because the binding stores the table id, not coordinates.
   - Table auto-growth and calculated-column fill are off for bound tables.
   - Sorting a bound table always uses its exact data area, top to bottom. `by_columns` is refused.
   - **Consequences:** deleting rows never deletes ERP records, no record is ever created by accident, and formatting is never limited.

3. **Text stays text.**
   - Bound text and lookup fields take input as text on every path: typing, paste, external paste, fill, replace, Convert to Number and dropdown picks. So `0100…` mobiles and `001` codes survive.
   - The number-stored-as-text triangle is hidden on those fields.

4. **Typed loading.**
   - Record pages reach the engine worker as transferred bytes, and `cgs-bind` parses them with serde. The main thread never builds a record object.
   - `bindRecords` writes typed values without `parse_input`: text as text, decimals at their scale, dates as serials with a date format, booleans as TRUE/FALSE.
   - It creates the table, sets each column's format, freezes the header row and turns on filter buttons.
   - It always opens a **new workbook**, because binding clears undo and the user's own workbook stays untouched.

5. **The diff runs in Rust.**
   - The `changeset` query compares the `calc.cell_input` of each row's key and editable cells with the originals, in O(rows × editable fields). Only changes are returned.
   - Formatting never counts.
   - Changing a value and changing it back drops the field by construction.
   - Formula and Spill inputs are flagged "convert to values to commit" and left out, so formula results never overwrite ERP data by themselves.
   - A dirty-record set, kept up to date in `apply_cells`, feeds the status badge without a full diff on every edit.

6. **States come from tiles.**
   - Tile build sets per-cell state in flag bits 8–11: modified, invalid, conflict, committed, formula and read-only. It adds one 64×1 read of the key column per tile, plus a map lookup per row.
   - `tile-draw.ts` paints the states with theme tokens. `mirror.ts` mirrors them on right-to-left sheets as it mirrors everything else.
   - `TileInput.binding` is `None` for PDF, print and HTML copy, so states never print or copy.
   - A sheet without a binding pays one empty check.

7. **Dropdowns come without rules.**
   - Inside `validation_json`, a binding fallback lists a lookup column's values, capped, so the existing dropdown button and menu work with no data-validation rule written to the file.
   - Membership is checked by local rules and by the server, because data validation alone checks only single-cell typing.

8. **Refresh is a three-way merge per field.** C is the cell, O the original, S the server's value now.

   | Case | Result |
   | --- | --- |
   | C = O | Take S |
   | C ≠ O and S = O | Keep C, and adopt the new version once every field of the record merged |
   | C = S | Converged: nothing pending |
   | Otherwise | **Conflict**: keep the old version, so a commit can't pass the server's check |

   - Refresh re-queries the bound ids, not the original filter, so records that stop matching the filter (after a Blocked change, say) don't disappear from the table. Records gone from the ERP are marked "not in ERP".
   - Applying a commit's result rewrites only cells that still hold the value that was sent.
   - Edits on the bound sheet wait while a commit is in flight.
   - Refresh, Commit and Resolve clear undo, as Excel's data refresh does, so undo can never bring stale ERP values back as "changes".

9. **The connection is session state.**
   - Save and Save As write an ordinary workbook: the table, its values and formats, with no ids, versions, originals, URLs or tokens.
   - Saving and committing stay separate.
   - Crash recovery keeps a capped draft in OPFS on the device (B.5).

### B.4 Web, desktop and offline

**Launching from the ERP**
- `ERPClient_React` calls `CreateTicket` with the user's API key, then opens `https://sheet.cashgearerp.com/erp#launch=<ticket>` in a new tab.
- The ticket rides in the **fragment**, which browsers never send to servers. `erpLaunch()` reads it and removes it with `history.replaceState` before anything else runs.
- The static host already rewrites `/erp` to `index.html`.

**Session handoff**
- The sheet redeems the ticket for a sheet session (F). `postMessage` isn't an option, because COOP `same-origin` cuts `window.opener` across origins.

**Cross-origin requests**
- CORS-mode `fetch` with a custom header works under COEP `require-corp`, because COEP restricts only no-cors subresources.
- The sheet needs no header change. The ERP narrows CORS for the sheet routes to the sheet's origins.
- The service worker ignores cross-origin requests, so ERP calls always go to the network.

**Session expiry while editing**
- Editing carries on locally, and ERP commands show **Reconnect**.
- The ERP's Reconnect opens a tab that hands its new ticket to the original tab over `BroadcastChannel` (same origin only), then closes.
- A tab refuses a session whose company, user or entity doesn't match its binding.
- A commit whose outcome is unknown is looked up with `CommitStatus` before any retry.

**Desktop**
- The same connector runs in the WebView, with `connect-src` widened to the API's origin (Phase 19).
- Launching from the ERP needs a `cashgear-sheet://` deep link. The desktop app starts the flow and keeps a PKCE-style verifier, and the ERP binds the ticket to its challenge, so an intercepted link can't be redeemed.
- That design is written in Phase 19 and built later. Until then the desktop app keeps working with normal workbooks.

**Offline**
- Normal sheets work offline as today.
- A bound sheet can be edited offline, but Validate, Commit and Refresh stay off while requests fail.
- The server's version check means an offline draft can never silently overwrite newer ERP data.

### B.5 Persistence, Azure Blob and the saved file

**Recovery draft**
- ERP data at rest stays on the device only, in the existing crash-recovery journal in OPFS.
- New journal entries:
  - the bind payload, minified, versioned and capped;
  - "commit pending" before a commit is sent, and "commit result" after it.
- The ticket and the token are never journalled.
- The draft is removed on commit, discard, clean close or save, as recovery data is today.
- A recovered draft opens disconnected. It must reconnect, then refresh by version, and it never commits by itself.
- A failed replay of the bind says "This ERP connection can't be recovered" rather than replaying later edits onto the wrong state.

**The saved file**
- No connection data is written: no `customXml`, no hidden sheet, no data-validation rule from lookups.
- The key column is an ordinary business field.

**Azure Blob is not needed for the first release:**
- The ERP database stays authoritative.
- Drafts are small.
- Paging covers size.
- Workbook history is out of scope.

Later, Blob could hold drafts shared across devices: the changeset and base versions per user, encrypted at rest and expiring. Never whole workbooks or snapshots.

**What survives save and reopen:** nothing about the connection. A reconnectable saved file is an optional extension (I.3). It would hold ids and versions in a `customXml` part, never tokens or URLs, and would need new `cgs-xlsx` code to add parts.

### B.6 Performance targets

Measured on the reference machine in Chrome WASM. Network time is reported separately.

| Measure | Target | Baseline it protects |
| --- | --- | --- |
| Bind 1,000 records × 20 fields, from response to first paint | ≤ 300 ms | New |
| Bind 10,000 records, the MVP cap per sheet | ≤ 1.5 s, with progress and Cancel | New |
| `changeset`, 1,000 × 20 / 10,000 × 20 | ≤ 10 ms / ≤ 100 ms | New |
| Local rules after pasting 1,000 rows | ≤ 50 ms | New |
| Key to paint on a bound sheet, p99 | Within 10% of the same sheet unbound | 44.4 ms |
| Tile build with states, p99 | Within 10% | 21.9 ms |
| Normal sheets | No change beyond noise | `input-speed`, `filter-speed`, scroll |
| Memory for originals | ≤ 64 bytes per editable value (about 1.3 MB at 1,000 × 20) | WASM's 4 GiB ceiling |

**How the targets are met**
- Pages are parsed in WASM from transferred bytes, so React sees only counts, labels and the changes.
- Changes are diffed in Rust and returned in proportion to what changed.
- Validation and commit requests carry only changed records.
- Pages are fetched with `AbortController` and the load panel shows progress.

`bench/erp-speed.mjs` measures each row, and the existing benches show normal sheets unchanged.

---

## C. Data contracts (proposed, new)

The types live in `packages/erp-connector/src/contract.ts`, and C# DTOs in UnifiedAPI mirror them.

**Transport**
- The session token goes in `X-Sheet-Session`, never in `Authorization`, because the API-key handler reads a raw `Authorization` value as a key and looks it up uncached.
- Sheet responses are `Cache-Control: no-store`, because UnifiedAPI's `UseResponseCaching` sits between authentication and authorization.

**Routes**
- Routes follow the ERP's module-first convention.
- The entity's route comes from the session (`entityRoute`), so the sheet hard-codes no entity.
- Validate is Commit with `dryRun: true`: one code path on the server.

### C.1 Launch and session

```jsonc
// POST api/system/SheetLaunch/CreateTicket          (ERP client; X-API-Key; policy of the entity's module)
→ { "entity": "vendors", "filter": { "blocked": false, "search": "noor" }, "language": "ar" }
← { "ticket": "k3J…(43 chars, 256-bit)", "expiresAt": "2026-11-09T10:02:00Z",
    "url": "https://sheet.cashgearerp.com/erp#launch=k3J…" }

// POST api/sheet/Session/Redeem                     (anonymous; Origin allow-listed; tenant from the ticket row)
→ { "ticket": "k3J…" }
← { "token": "…(opaque)", "idleExpiresAt": "…", "maxExpiresAt": "…",
    "company": "Kit", "userId": "u17", "userName": "Sara", "language": "ar",
    "entity": "vendors", "entityRoute": "api/payable/VendorSheet", "filter": { "blocked": false, "search": "noor" } }

// POST api/sheet/Session/Renew   ← { "idleExpiresAt": "…" }
// POST api/sheet/Session/End     ← 204
```

### C.2 Entity schema

```jsonc
// GET {entityRoute}/Schema
← { "entity": "vendors", "schemaVersion": "vendors.1",
    "label": { "en": "Vendors", "ar": "الموردون" },
    "keyField": "vendorAccount", "idField": "recId",
    "maxPageSize": 1000, "maxRecords": 10000, "maxCommitRecords": 500,
    "fields": [
      { "name": "vendorAccount", "label": { "en": "Vendor account", "ar": "حساب المورد" },
        "type": "text", "maxLength": 50, "editable": false },
      { "name": "vendorName", "label": { … }, "type": "text", "maxLength": 150, "required": true, "editable": true },
      { "name": "vendorGroup", "label": { … }, "type": "lookup", "lookup": "vendorGroups", "required": true, "editable": true },
      { "name": "payMethod", "label": { … }, "type": "lookup", "lookup": "paymentModes", "required": true, "editable": true },
      { "name": "currency", "label": { … }, "type": "lookup", "lookup": "currencies", "editable": false, "editReason": "needsAdvEdit" },
      { "name": "mobile", "label": { … }, "type": "text", "maxLength": 50, "editable": true },
      { "name": "blocked", "label": { … }, "type": "boolean", "editable": false, "editReason": "needsAdvEdit" },
      { "name": "debitLimit", "label": { … }, "type": "decimal", "scale": 2, "editable": false, "editReason": "notEditable" },
      { "name": "modifiedDate", "label": { … }, "type": "dateTime", "editable": false } ],
    "lookups": {
      "vendorGroups": [ { "code": "LOCAL", "label": { "en": "Local", "ar": "محلي" } } ],
      "paymentModes": [ … ], "currencies": [ … ] } }
```

**Field types:** `text`, `integer`, `decimal` (with `scale`), `boolean`, `date`, `dateTime`, `lookup`.

**Constraints:** `required`, `maxLength`, `min`, `max`, `scale` and `lookup`.

**Editability**
- `editable` is already resolved for **this caller**. `editReason` says why a field is read-only: `notEditable`, `needsEdit` or `needsAdvEdit`.
- The server re-checks every field anyway.

### C.3 Record snapshot (keyset paging)

```jsonc
// POST {entityRoute}/Query
→ { "filter": { "blocked": false, "search": "noor" }, "ids": null, "afterId": 0, "pageSize": 1000 }
   // refresh sends "ids": [4411, 4412, …] and no filter
← { "records": [
      { "id": 4411, "key": "V-1001", "version": "AAAAAAAAB9E=",
        "values": { "vendorAccount": "V-1001", "vendorName": "Al Noor", "vendorGroup": "LOCAL",
                    "payMethod": "CASH", "mobile": "01001234567", "blocked": false,
                    "debitLimit": "15000.00", "modifiedDate": "2026-10-30T08:12:00Z" } } ],
    "nextAfterId": 5410, "total": 1240, "asOf": "2026-11-09T10:00:05Z" }
```

### C.4 Changeset

```jsonc
// POST {entityRoute}/Commit     (dryRun: true = Validate; nothing is written)
→ { "commitId": "0b6c5a1e-…", "company": "Kit", "schemaVersion": "vendors.1", "dryRun": false,
    "records": [
      { "id": 4411, "key": "V-1001", "version": "AAAAAAAAB9E=",
        "changes": { "payMethod": { "from": "CASH", "to": "BANK" },
                     "mobile": { "from": "01001234567", "to": "01009876543" } } } ] }
```

### C.5 Validation result (400)

```jsonc
← 400 { "message": "2 vendors have errors.", "correlationId": "…",
        "errors": [ { "id": 4411, "key": "V-1001", "field": "payMethod", "code": "lookup.notFound",
                      "message": "طريقة الدفع غير موجودة" } ] }
```

### C.6 Conflicts (409)

```jsonc
← 409 { "message": "1 vendor was changed by someone else.", "correlationId": "…",
        "conflicts": [ { "id": 4411, "key": "V-1001", "version": "AAAAAAAACQ0=",
                         "values": { /* the record now */ }, "fields": ["payMethod"],
                         "modifiedBy": "omar", "modifiedAt": "2026-11-09T10:04:11Z" } ] }
```

### C.7 Commit response (200)

```jsonc
← 200 { "commitId": "0b6c5a1e-…", "committedAt": "2026-11-09T10:05:00Z",
        "records": [ { "id": 4411, "version": "AAAAAAAACQ1=", "values": { /* as persisted */ } } ] }

// GET {entityRoute}/CommitStatus/{commitId}
← { "status": "committed", …same body as 200… }  |  { "status": "unknown" }
```

### C.8 Rules for values and errors

**Wire formats**

| Type | On the wire | In the sheet |
| --- | --- | --- |
| Decimal | String (`"15000.00"`) | Compared at its `scale`; values beyond 15 significant digits are refused locally, because cells hold doubles |
| `date` | `YYYY-MM-DD` | A serial with a date format |
| `dateTime` | ISO 8601 with offset | Read-only in the MVP |
| Empty | `null` | Empty cell; `null` and `""` compare equal |
| Boolean | `true`/`false` | TRUE and FALSE. Vendors' `Blocked` int is mapped by the server |

**Concurrency**
- `from` is for audit and diagnostics only. The server judges concurrency by `version` alone, strictly (decisions, 2026-11-09).
- Every changed record must carry a version.

**Error codes**
- Every error carries a `code` from a fixed list, so the sheet shows its own English or Arabic text. The server's `message` is the fallback.
- The list: `required`, `maxLength`, `type`, `range`, `scale`, `lookup.notFound`, `field.readOnly`, `privilege.denied`, `record.notFound`, `record.locked`, `version.missing`, `version.conflict`, `batch.tooLarge`, `company.mismatch`, `schema.changed`, `session.expired`.

**Status codes**
- 401 means the session is expired or invalid.
- 403 means the privilege is missing.
- 413 means too many records.
- 429 and 503 carry `Retry-After`.

---

## D. Module changes

### D.1 New in this repo

| Path | What |
| --- | --- |
| `crates/cgs-bind/` | `binding.rs` (Binding, BoundField, FieldKind), `records.rs` (key → id, version, originals, errors, conflict), `guard.rs`, `changeset.rs`, `merge.rs`, `rules.rs` (local constraints), `state.rs`. Pure Rust, no new third-party crates beyond `serde`/`serde_json`, already in the tree |
| `packages/erp-connector/` (`@cgs/erp-connector`) | `contract.ts`, `validate.ts` (hand-written type guards, no new dependency), `http.ts` (timeouts, `AbortController`, backoff, error mapping, 401/409/413/429), `session.ts` (redeem, renew, end, BroadcastChannel reconnect, scope check), `connector.ts` (open, page, validate, commit, refresh, resolve), tests |
| `packages/ui/src/erp/` | `useErpTools.tsx` (the Host pattern of `useDataTools`), `ribbon-erp.tsx`, `ErpLoadPanel.tsx`, `ReviewChangesDialog.tsx`, `ConflictsDialog.tsx`, `ConnectionInfoDialog.tsx`, `ErpCellLayer.tsx` (hover details), `messages.ts` (en/ar) |
| `tests/erp-mock/` | A Node mock of the contract with no dependencies: two companies sharing one store, seeded vendors (Arabic and English names, leading-zero mobiles, decimals), ticket and session lifetimes on a controllable clock, versions that move on each write, idempotency, failure injection (`/__mock/*`: drop the response after writing, 401, 429, 500, latency, a concurrent edit), CORS with preflight |
| `tests/e2e/erp.spec.ts` | End to end in Chrome and Edge, with the mock on a second origin |
| `bench/erp-speed.mjs` | The targets in B.6 |
| `docs/erp-contract.md` | Routes, C# DTOs, error codes and the RowVersion rollout plan, for Track E |

### D.2 Existing files touched

| File | Change |
| --- | --- |
| `Cargo.toml` | `cgs-bind` in `members` and `default-members` |
| `crates/cgs-engine/src/lib.rs` | `Session` holds `bindings: cgs_bind::Bindings` |
| `crates/cgs-engine/src/command.rs` | New commands (`bindRecords`, `refreshRecords`, `applyCommitResult`, `setRecordStates`, `resolveConflict`, `discardChanges`, `freezeBinding`). The guard is called in `apply_cells`, `apply_axis`, `Op::Tables`/`Op::BookTables` and `ShiftCells`. A guard-off flag is set in `step_back`, `step_forward` and rollback. Text coercion runs on the input, paste, fill, replace and convert paths. No growth for bound tables |
| `crates/cgs-engine/src/tables.rs` | No calculated-column fill and no growth for bound tables |
| `crates/cgs-engine/src/sort.rs` | The bound-table rule: exact data area, top to bottom |
| `crates/cgs-engine/src/dv.rs` | Lookup fallback in `validation_json` |
| `crates/cgs-engine/src/tile.rs`, `view_block.rs` | State flags; `TileInput.binding`; the ViewBlock version is bumped |
| `crates/cgs-engine/src/query.rs`, `ask.rs` | `changeset`, `recordAt`, `bindingSummary` |
| `crates/cgs-engine/src/protect.rs` | The paste-area fix, if Phase 16 confirms the gap |
| `crates/cgs-wasm/src/lib.rs` | `bind_records(bytes)`, `refresh_records(bytes)` |
| `packages/engine-client/src/types.ts`, `client.ts` | Commands, queries, byte methods with transfer |
| `packages/engine-client/src/engine-worker.ts` | Byte-payload methods inside `guarded()`; journal entries for bind payloads and commit pending/result; a failed binding replay reported |
| `packages/engine-client/src/view-block.ts`, `tile-draw.ts` | Decode and paint states with theme tokens |
| `packages/ui/src/AppShell.tsx` | Mount `useErpTools`; launch when the engine is ready, through `runCommand`; `beforeunload` while ERP changes are pending; `document.title` |
| `packages/ui/src/platform.ts` | `erpLaunch(): ErpLaunch \| null` |
| `apps/web/src/main.tsx`, `platform/web.ts` | Read and strip `#launch=` first; the desktop returns null |
| `packages/ui/src/grid/SheetCanvas.tsx` | Forward any new grid event by name |
| `playwright.config.ts` | A second web server for the mock |
| `pnpm-workspace.yaml` | The new package |
| `apps/desktop/src-tauri/tauri.conf.json` | Phase 19: the API origin in `connect-src` |

Nothing generic needs adding to `CashGear.React.UI` for the MVP: dialogs, grid, badge, progress and ribbon all exist. Labels for contextual-tab groups would be a Track U item if wanted later.

### D.3 Track E: the ERP repo (proposed, new code following its conventions)

Track E0 reviewed this table against the code on 2026-11-09, and `erp-contract.md` has what changed: a concrete `VendorSheetConnector` first, no ownership or row-lock rules for vendors, CGService's scripts in `CashGear.IdentitySystem/DbScripts` behind a `SheetDbContext`, the ERP's script names, and `ERPClient_React` as its own repository.

| Path in `D:\CashGear\Projects\CashGear` | What |
| --- | --- |
| `CashGear.UnifiedAPI/Controllers/API/V1/System/SheetLaunchController.cs` | `CreateTicket`, under the API key and the entity module's policy |
| `CashGear.UnifiedAPI/Controllers/API/V1/Sheet/SheetSessionController.cs` | `Redeem` (anonymous, rate-limited, Origin allow-list), `Renew`, `End` |
| `CashGear.UnifiedAPI/Controllers/API/V1/Payable/VendorSheetController.cs` | `Schema`, `Query`, `Commit`, `CommitStatus`; `[Authorize(AuthenticationSchemes="SheetSession", Policy="Payable")]` |
| `CashGear.UnifiedAPI/Services/SheetSessionAuthenticationHandler.cs` | Reads `X-Sheet-Session`; calls `SetCurrentTenant`; emits the API-key handler's claims except the raw key; checks on each request that the parent key and the company are still active, since sessions outlive the key cache |
| `CashGear.UnifiedAPI/Services/Sheet/ISheetEntityConnector.cs`, `VendorSheetConnector.cs` | Field allow-list and mapping (never the raw entity; never `Token`); rules from `ValidateSave`, with per-field errors as `UpdateItemAsync` collects them; lookups |
| `CashGear.UnifiedAPI/Services/Sheet/SheetCommitService.cs` | One transaction per batch; versions re-read under `UPDLOCK, HOLDLOCK`, so every conflict is reported at once; all or nothing; `SheetCommit` for idempotency; `SheetChangeAudit` plus a SystemLog summary; `ModifiedBy`/`ModifiedDate`; eviction of the `Vendors` and `{Company}:Vendors` tags; Query reads the database (`AsNoTracking`), not the cache, and applies `LoadOwnedRows` and `SysSecRules` |
| `CashGear.UnifiedAPI/DbScripts/` | `Vendors.RowVersion rowversion`; `SheetCommit (Company, CommitId, UserId, PayloadHash, Result, CreatedAt)`; `SheetChangeAudit (Company, CommitId, Entity, RecId, Field, OldValue, NewValue, UserId, At, CorrelationId)` in each tenant database; `SheetLaunch` and `SheetSession` (hashed, with expiry) in CGService |
| `CG.Infrastructure/CGModels/PurchModel/VendorModel.cs` | `[Timestamp] public byte[] RowVersion`, **deployed only after every tenant database has the column** |
| `CG.Infrastructure/CGModels/APIModel/Sheet*.cs` | The DTOs of C |
| `CashGear.UnifiedAPI.Tests` (new) | WebApplicationFactory suite (I.4) |
| `ERPClient_React/src/features/payable/pages/VendorList.tsx`, `src/api/sheet.ts` | An "Edit in Cash Gear Sheet" button passing the list's current filter, behind a per-company setting until Phase 18 |

---

## E. User experience

### E.1 Opening

- The ERP's Vendors list gets **Edit in Cash Gear Sheet**, which carries its current search and filters.
- A new tab opens the sheet with a load panel: "Loading vendors · 400 of 1,240", with Cancel.
- Then a new workbook, "Vendors – Kit", shows one table named `Vendors`:
  - headers in the ERP user's language;
  - the header row frozen and filter buttons on;
  - columns sized to fit.
- Arabic users get a right-to-left sheet.
- Read-only columns carry a lock in their header and a faint tint. Both are display only, never formatting, so they don't reach the saved file.
- Sorting, filtering, formatting, copying, freezing, zooming, printing and saving all work as on any table.
- Typing, paste, fill, clear and dropdowns work in editable columns.
- In read-only columns they're refused with "This column is read-only in Cash Gear ERP." Inserting or deleting rows in the table is refused with "Rows can't be added or deleted in ERP data."

### E.2 The ERP Data tab

A contextual tab, visible while a bound sheet is active and placed after Table Design. It uses `CgRibbon` items and the existing icon set.

| Group | Commands | Enabled when |
| --- | --- | --- |
| Data | Refresh | Connected, no commit in flight |
| Changes | Validate, Review Changes, Commit, Discard Changes | There are changes; Commit also needs connected and online |
| Conflicts | Resolve Conflicts, Next Conflict | There are conflicts |
| Connection | Connection Info, Reconnect | Always; Reconnect when the session has expired |

**Key tips:** ERP gets its own letters, chosen when the tab is built, following the Phase 13 rule (no tip starts another).

### E.3 Cell states

Painted from theme tokens in light and dark, mirrored on right-to-left sheets, and never printed.

| State | Look | Meaning |
| --- | --- | --- |
| Modified | Accent bar on the cell's leading edge, plus a light tint | Value differs from the original |
| Invalid | Red border | A local rule or the server refused it |
| Conflict | Warning hatch and "!" | The ERP's version moved, and the fields clash |
| Committed | Green bar | Committed; it clears on the next edit or refresh |
| Formula | Dotted bar | A formula or spill; convert to values to commit |
| Read-only | Faint tint, with a lock in the header | Can't be edited |

The exact colours are proposed in Phase 16, with a mock-up for your choice.

**Hover**
- A DOM layer like `NoteLayer` shows the field, "Original 30 → New 45", any error in the user's language, and who changed a conflicting record and when.

**Status bar**
- A `CgStatusBadge` reads, for example, "Vendors · 12 changed · 2 invalid · 1 conflict · Sara @ Kit". It turns to a warning when the session expires or the network drops.

### E.4 Validation and review

**Validate**
1. Local rules run first (type, required, length, scale, range, lookup), in milliseconds.
2. Then the server's dry run.
3. Errors land on their cells.
4. The status bar says how many, and Next Error moves through them.

**Review Changes** is a `CgWindow` with a `CgGrid`:
- **Columns:** record (key and name), field, original, new, status and message.
- **Filters:** All, Invalid, Conflicts, In hidden rows. Fill and paste can reach rows a filter hides, so they're listed rather than blocked, as Excel doesn't block them.
- **Actions:** Go to cell, Discard selected changes.
- Commit: "Commit 37 changes to 12 vendors", then a confirmation that states the counts.

**More than 500 changed records:** Commit says to commit in smaller filtered groups. Larger batches are a later extension.

**Discard Changes** writes the originals back as one undoable step. It changes only cells, never the originals, so undoing it is safe.

### E.5 Commit, conflicts and refresh

**Commit**
1. Edits on the bound sheet pause.
2. The request goes with a fresh `commitId`.
3. On success, cells that still hold the sent values take the persisted values, versions move, those cells show Committed, and undo clears.
4. On 400, cells show Invalid and nothing is written.
5. On 409, nothing is written, and conflicting fields show Conflict.

**Resolve Conflicts** shows each field's Mine, ERP now and Original, with:
- **Keep mine:** rebase onto the new version, so the edit stays pending.
- **Take ERP's:** the cell takes the server value.
- Either applies per field, per record or to all.

**Refresh**
- Re-queries the bound ids and merges three ways (B.3 item 8).
- Non-overlapping edits are rebased visibly: the status bar names how many records merged.
- Records gone from the ERP are marked "not in ERP" and left out of commits.

### E.6 Languages

**ERP texts**
- Labels, lookups and the server's codes come in the ERP user's language, from the launch.
- The ERP tab, dialogs and messages use a small en/ar catalogue (`erp/messages.ts`).
- Arabic users' bound sheets open right-to-left (Phase 11), and the dialogs take `direction="rtl"`.

**The rest of the app** stays English until the app's Arabic UI phase under "Later". That phase can take the same catalogue approach.

---

## F. Security model

**Authentication handoff** (answered 2026-11-09)

The launch ticket:
- 256-bit random, stored as a SHA-256 hash.
- Single-use, and expires after about 120 s.
- Bound to user, company, entity and filter, and to the API key that created it.
- It travels only in the URL fragment and is removed on arrival.
- On its own it can call nothing.

Redeem:
- Accepts only allow-listed `Origin`s: `sheet.cashgearerp.com`, the dev and preview hosts, and later the desktop's.
- Sets the tenant from the ticket row, never from a header.
- Is rate-limited per IP.

The sheet session:
- An opaque 256-bit token, stored hashed in CGService.
- 30 minutes idle and 10 hours maximum; `Renew` slides the idle time.
- Revocable with `End`, and whenever its parent API key or the company is deactivated (checked on every request).
- Scoped to `api/sheet/*` and the launched entity's route only. Every other endpoint still takes API keys only, because the default scheme stays `ApiKey`.

The user's API key never reaches the sheet. The token is held in `erp-connector`'s memory only: never in `localStorage`, OPFS, the journal, URLs, error reports or logs.

**Tenant isolation**
- The company comes only from the session's claims, through `SetCurrentTenant` and `TenantRegistry`.
- Commit repeats `company`, and the server refuses a mismatch with `company.mismatch`.
- Every query filters `Company`. Ids address records only within the session's company.
- Idempotency and audit are keyed by company too, because Kit and Dot share a database.
- In the sheet, a tab refuses a reconnected session whose company, user or entity differs from its binding.

**Authorization**
- Opening needs unit 2004 View; committing needs Edit.
- Sensitive fields need AdvEdit or stay read-only (I.2).
- Ownership and row locks would apply to Query and Commit alike, but Track E0 found none for vendors: `SysSecRules` is unused and `LoadOwnedRowsPurch` covers purchase orders only (`erp-contract.md`).
- The schema reports each field's editability for the caller, and the server re-checks every field on dry run and commit.
- `CallerPrivileges` falls back to another company's row for any non-admin user (Track E0), so the sheet's routes use a company-strict check.

**Business validation**
- Done on the server, from the ERP's own rules (`ValidateSave`), extended with per-field checks. Local rules only make feedback fast.

**Concurrency** (answered 2026-11-09)
- `RowVersion` on `Vendors`, compared strictly and explicitly under `UPDLOCK, HOLDLOCK` inside the transaction.
- A change made by anyone since loading is a conflict, and nothing is ever written without a version.
- Conflicts are resolved by the user (E.5), never by overwriting.

**Transactions**
- Batches of up to 500 records, all or nothing, in one transaction. MARS rules out savepoints, so it's the whole batch or nothing, with the ERP's safe-rollback pattern.

**Idempotency**
- The client makes a `commitId` (UUID v4) per commit attempt and reuses it on every retry.
- `SheetCommit (Company, CommitId)` is written in the same transaction as the changes, together with a hash of the payload.
- A retry returns the stored result; the same id with a different payload is refused.
- `CommitStatus` answers after a lost response or a crash.

**Audit**
- One `SheetChangeAudit` row per changed field (old and new value, user, commit id, correlation id, time), plus a SystemLog summary row, all in the same transaction.
- `ModifiedBy` and `ModifiedDate` are set as `UpdateEx` sets them.

**CORS and caching**
- The ERP narrows CORS for `api/sheet/*` and the entity routes to the sheet's origins.
- Responses are `no-store`.
- Sheet reads bypass FusionCache. Commits evict the UnifiedAPI cache tags.

**Files and logs**
- No connection data in the xlsx.
- States never print or copy.
- The journal never holds the ticket or the token.
- Ids appear in logs only with the correlation id.

---

## G. Implementation phases

Every phase follows CLAUDE.md:
1. Read PLAN, PROGRESS and decisions.
2. Ask the phase's questions in one batch, and log the answers.
3. Record evidence for each exit criterion.
4. Tag `phase-N` and `v1.(N-9).0`, with a CHANGELOG section and one version across the app.

Intermediate releases expose nothing half-built: the sheet acts only on a valid ticket, and the ERP button stays hidden per company until Phase 18.

Track E runs in the ERP repo alongside, in stages E0 to E4. Its git workflow is agreed when it starts. Its tests never touch the live database, and you run its database scripts and deploy it.

### Phase 14: Contract and mock ERP (v1.5.0)

- **Objective:** a typed contract both sides build against, proven by a test suite before any code depends on it.
- **Dependencies:** this plan.
- **Tasks:**
  1. Create `packages/erp-connector` with `contract.ts` (every type in C), `validate.ts` (response guards) and the error-code list.
  2. Write `docs/erp-contract.md`: routes, C# DTOs, status codes, the RowVersion rollout plan (script every tenant database, then deploy), and the table scripts.
  3. Build `tests/erp-mock/server.mjs` as in D.1.
  4. Write the contract suite (`packages/erp-connector/src/contract.test.ts`), which runs against `ERP_BASE_URL` (default: it starts the mock). It covers:
     - ticket single use and expiry;
     - session idle and maximum expiry, and `End`;
     - schema shape;
     - keyset paging;
     - dry run writes nothing;
     - commit is atomic;
     - 409 lists every conflict;
     - an idempotent retry writes once;
     - the same id with another payload is refused;
     - another company's ids are refused;
     - read-only and privileged fields are refused;
     - 413 over the batch limit.
  5. Track E0: review the contract against UnifiedAPI's code.
- **Files:** `packages/erp-connector/`, `tests/erp-mock/`, `pnpm-workspace.yaml`, `docs/erp-contract.md`.
- **Tests:** the contract suite on the mock in CI; licence checks unchanged.
- **Acceptance:**
  - The suite passes on the mock in CI.
  - You review and sign off the contract and the rollout plan.
- **Risks:** the contract changes when Track E meets real code. E0 reviews it against UnifiedAPI before Phase 15 builds on it.
- **Deliverables:** the package with its types, the mock, the suite, the contract document.

### Phase 15: Read-only slice, end to end (v1.6.0)

- **Objective:** ERP vendors open in the sheet and can be sorted, filtered and refreshed, with every bound cell read-only. This proves CORS, COEP, the handoff, hosting and the identity guard before editing exists.
- **Dependencies:** Phase 14; Track E1 for the real-API check.
- **Tasks:**
  1. Add `erpLaunch()` to the platform. The web reads and strips `#launch=` before rendering; the desktop returns null.
  2. In `erp-connector`, build `http.ts`, `session.ts` (redeem, idle tracking) and `connector.open()`: schema, then pages, then bind, with progress and Cancel.
  3. Create `cgs-bind`:
     - bindings, fields, the record map and value conversion;
     - `bindRecords` over transferred bytes (typed writes, column formats, the table, the frozen header, filter buttons, fitted widths);
     - the identity guard (whole-row rule, `Op::Tables`, structural, no growth or calculated columns, the bound-table sort rule) with **every** bound column read-only;
     - Refresh as a plain replace by ids, since nothing can be edited yet.
  4. Add `bind_records` in `cgs-wasm`, plus engine-worker and client methods. Recovery stays off for bound workbooks until Phase 18.
  5. In the UI:
     - `useErpTools` with the load panel;
     - the ERP Data tab with Refresh and Connection Info;
     - the status badge and `document.title`;
     - refusal messages.
  6. Add the mock as a second web server in Playwright (another port, so another origin, under `pnpm preview`'s production headers).
  7. Track E1:
     - the `SheetLaunch` and `SheetSession` tables and controllers;
     - `SheetSessionAuthenticationHandler`;
     - `VendorSheet/Schema` and `Query`;
     - the React button behind its setting;
     - xUnit and WebApplicationFactory tests;
     - the contract suite against UnifiedAPI run locally on a LocalDB tenant.
- **Files:** D.1 and D.2 for the parts named; D.3 for E1.
- **Tests:**
  - **Rust:**
    - a property test: random sorts (every key and order), filters, undo and redo over 1,000 records keep every key with its own record's values;
    - every refused path (left-to-right sort, header off, totals, resize, rename, row and column insert or delete inside the table, ShiftCells, merge, Remove Duplicates, Convert to Range, deleting the sheet, growth, calculated columns);
    - typed values: leading zeros, Arabic text, decimals at scale, dates, empty and null.
  - **e2e:**
    - open 1,240 vendors from the mock with English and with Arabic labels;
    - sort and filter, then check row by row against the mock through a `cgsTest` hook;
    - refused edits show their message;
    - expired and replayed tickets are refused;
    - the fragment is gone from `location` and history;
    - Save As writes a workbook that holds no ids or versions, checked with the existing readers.
  - **Bench:** binding 1,000 and 10,000 records.
- **Acceptance:**
  - The bind targets in B.6 are met.
  - e2e passes in Chrome and Edge.
  - The contract suite passes against UnifiedAPI on LocalDB.
  - The existing fuzz, matrix, save checks and benches are unchanged.
- **Risks:**
  - Azure Static Web Apps or COEP surprises: tested under the production headers locally, then on the real host.
  - Track E1 may slip: the sheet's exit doesn't wait for it beyond the local LocalDB check.
- **Deliverables:** a read-only ERP view behind the hidden button.

### Phase 16: Editing and change tracking (v1.7.0)

- **Objective:** editable fields with field-level tracking and visible states. Nothing leaves the sheet yet.
- **Dependencies:** Phase 15.
- **Tasks:**
  1. Editable fields from the schema.
  2. The guard's editable-cell rule.
  3. Text coercion on every input path for bound text and lookup fields.
  4. The number-stored-as-text triangle hidden on those fields.
  5. Formula and spill flags.
  6. Check the paste-area protection gap and fix it in `protect.rs` if it is real, with a test.
  7. `changeset` over `cell_input`, and the dirty-record set.
  8. Local rules.
  9. State flags (bits 8–11, ViewBlock version bump), painted in `tile-draw.ts` in light and dark and mirrored; `TileInput.binding` set to `None` for PDF, print and HTML copy.
  10. The dropdown fallback, capped, with picks written as text.
  11. The hover layer, badge counts, Discard Changes, and Next Error.
- **Files:** `cgs-bind` (`changeset.rs`, `rules.rs`, `state.rs`), `command.rs`, `tables.rs`, `dv.rs`, `tile.rs`, `view_block.rs`, `protect.rs`, `view-block.ts`, `tile-draw.ts`, `packages/ui/src/erp/*`.
- **Tests:**
  - **Rust property test:** random typing, paste (internal and external), fill, Ctrl+D, clear, move, replace, convert, sort, filter, undo and redo over 1,000 records. Invariants:
    - every key stays with its record;
    - read-only and key cells never change except by whole-row moves;
    - `changeset` equals an oracle diff of typed inputs between the start and now.
  - **Unit tests:** coercion cases (`0100…`, `001`, `1-2`, `1,23`); decimals at scale and past 15 digits; empty versus null; formulas and spills left out; changing a value back clears it.
  - **e2e:**
    - edit, paste and fill down in editable columns;
    - paste refused into a read-only column;
    - undo back to the original clears Modified;
    - state goldens in light, dark and right-to-left.
  - **Benches:** key to paint and tile p99 within 10%, changeset timings; normal-sheet benches, the matrix, save checks and fuzz unchanged.
- **Acceptance:** all of the above.
- **Risks:**
  - A write path the guard misses: the phase lists every `calc.set_cells`, `structural` and `Op::Tables` caller, with a test for each.
  - Tile cost: measured.
- **Deliverables:** safe editing with states.

### Phase 17: Validate, review and commit (v1.8.0)

- **Objective:** changes reach the ERP safely and atomically.
- **Dependencies:** Phase 16; Track E2.
- **Tasks:**
  1. Validate: local rules, then a dry run, with errors set on cells by id and field (`setRecordStates`).
  2. The Review dialog (`CgWindow` and `CgGrid`, filters including hidden rows, Go to cell, Discard selected).
  3. Commit:
     - a new `commitId`;
     - edits frozen;
     - the batch-limit message;
     - retry with the same id on network errors;
     - `applyCommitResult` writing only cells that still hold what was sent;
     - Committed states;
     - undo cleared.
  4. 400 and 409 states.
  5. Track E2:
     - `Commit` with `dryRun`;
     - the `Vendors.RowVersion` script and rollout;
     - `SheetCommit` and `SheetChangeAudit`;
     - SystemLog, privileges, ownership, version re-read under lock;
     - cache eviction;
     - WebApplicationFactory and LocalDB tests.
- **Tests:**
  - **e2e:**
    - commit, then the mock's data equals the oracle;
    - 400 marks cells and writes nothing;
    - 409 marks conflicts and writes nothing;
    - a response dropped after the write, then a retry: exactly one write (the mock counts);
    - over 500 records is refused;
    - Review lists changes in hidden rows.
  - **ERP:** the suites in I.4.
- **Acceptance:**
  - e2e passes in Chrome and Edge.
  - The contract suite passes against UnifiedAPI on LocalDB, including conflicts and idempotency.
  - No commit path writes without a version.
- **Risks:**
  - The RowVersion rollout order: the model change is deployed only after every tenant database has the column.
  - The Blazor host's last-write-wins `UpdateEx` can still overwrite sheet commits. It is listed in H, not fixed silently.
- **Deliverables:** commit end to end.

### Phase 18: Conflicts, refresh, sessions, recovery and languages (v1.9.0)

- **Objective:** concurrency, long sessions and crashes are handled, and the feature is ready for users.
- **Dependencies:** Phase 17; Track E3.
- **Tasks:**
  1. The three-way refresh by ids.
  2. Resolve Conflicts (per field, per record, all) and Next Conflict.
  3. Session `Renew`; 401 handling during any operation; Reconnect over BroadcastChannel with the scope check; `CommitStatus` after an unknown outcome.
  4. Recovery: journal entries for the bind payload (minified, versioned, capped) and for commit pending and result. A recovered draft opens disconnected, then reconnects, then refreshes. The failed-replay message.
  5. The en/ar catalogue; Arabic sheets right-to-left; right-to-left goldens.
  6. Track E3: `Renew`, `End`, `CommitStatus`; the button turned on per company.
- **Tests:**
  - **Rust:** a property test of the merge covering every C/O/S case, including key renames and records gone from the ERP.
  - **e2e:**
    - a concurrent edit in the mock leads to a conflict, then resolve, then commit;
    - non-overlapping concurrent edits rebase without a conflict;
    - 401 during a commit, then reconnect, then `CommitStatus`, with nothing written twice;
    - kill the page, reopen and recover: a draft is offered, opens disconnected, and is never committed by itself;
    - a reconnect token for another company is refused;
    - Arabic goldens.
- **Acceptance:** the above in Chrome and Edge; the existing suites unchanged.
- **Risks:**
  - Journal size: capped, with a clear message past the cap.
  - BroadcastChannel needs the original tab to be open; when it isn't, the new tab offers to reopen the draft instead.
- **Deliverables:** the MVP, complete.

### Phase 19: Hardening and the real ERP (v1.10.0)

- **Objective:** production readiness against the real API.
- **Dependencies:** Phase 18; Track E deployed by you to a test tenant.
- **Tasks:**
  1. The contract suite and a scripted e2e against UnifiedAPI on a test tenant (never the live database).
  2. `bench/erp-speed.mjs` against B.6.
  3. Full regression.
  4. A security review of the sheet and of Track E.
  5. Track E4: CORS narrowed, rate limits on the session endpoints, monitoring through the correlation id.
  6. The desktop: `connect-src` widened, and the deep-link design written (built later).
  7. Docs: `DEPLOYMENT.md` and CLAUDE.md gotchas.
- **Acceptance:**
  - The targets in B.6 are met.
  - Every CLAUDE.md check is green.
  - You sign off after trying it on the test tenant.
- **Risks:** a test tenant must exist; that is a question at the start of Phase 19.
- **Deliverables:** a release ready to turn on per company.

### G.1 Test coverage map

| Required area | Where it is covered |
| --- | --- |
| Correct record-to-row mapping | Phase 15 and 16 Rust property tests; e2e row checks against the mock |
| Sorting and filtering after editing | Phase 16 property test; e2e |
| Copy and paste into editable and protected fields | Phase 16 unit tests and e2e |
| Undo, redo and dirty state | Phase 16 property test (changeset equals the oracle after undo and redo); e2e |
| Duplicate and missing identifiers | Phase 15 bind refusal tests; changeset checks |
| Invalid field values | Phase 16 local rules; Phase 17 server dry run |
| Decimal, date, empty and null values | Phase 15 and 16 unit tests; contract suite |
| Arabic and English formatting | Phase 15 labels; Phase 18 catalogue and right-to-left goldens |
| Concurrent ERP modifications | Phase 17 409 e2e; Phase 18 merge property test and e2e; LocalDB tests in Track E |
| Authentication expiry | Contract suite; Phase 18 e2e |
| Tenant and field permission enforcement | Contract suite; Track E WebApplicationFactory |
| Failed and retried commits | Phase 17 e2e (dropped response); contract suite |
| Partial network failures | Mock failure injection in Phases 17 and 18 |
| Idempotent retries | Contract suite; Phase 17 e2e; Track E |
| Saving and reopening workbooks | Phase 15 Save As check; `pnpm test:save`; the Excel matrix in Phase 19 |
| Normal spreadsheet regression | The full CLAUDE.md check list each phase; existing benches |

---

## H. Risks and trade-offs

**Engine and safety**
- **A write path that avoids the guard.** Anything that changes cells outside `apply_cells`, `Op::Tables` or `structural` would slip past it. Phase 16 lists every caller and tests each one; new commands added later must call the guard.
- **Clearing undo at refresh, commit and resolve** is less forgiving, but it is the only way undo can't write stale ERP values back. Excel's data refresh clears undo too.
- **The three-way merge is what stops a stale draft from winning.** A wrong merge rule would let an old version through, so it gets a property test over every case.

**Performance**
- **Tile cost:** one extra key-column read per tile, and flags rather than marks. Measured against a 10% budget.
- **Bound workbooks up to 10,000 records**, with originals at most 64 bytes per editable value.

**File compatibility**
- **Hidden-row edits** are flagged in Review, not blocked, as in Excel.
- **A saved bound workbook is an ordinary table,** so Excel opens it cleanly and it holds nothing hidden. The cost is that it can't reconnect. A reconnectable file would need new `cgs-xlsx` part writing and a `customXml` schema, and is left for later.
- **Doubles:** decimals are compared at their scale and sent as strings. Values past 15 significant digits are refused rather than rounded.

**ERP consistency** (the ERP's own gaps, listed rather than fixed silently)
- The Blazor host's `UpdateEx` is last-write-wins: its edits can overwrite a sheet commit, though a sheet commit never overwrites them.
- The Blazor host's cache serves vendors for up to 8 hours.
- `CallerPrivileges` can fall back to another company's row for any non-admin user; the sheet's routes check strictly.

**Rollout and testing**
- **RowVersion rollout:** `[Timestamp]` on the shared `VendorModel` breaks vendor reads on any tenant database without the column, so the order is fixed: script every tenant database, then deploy either host.
- **Live database:** UnifiedAPI's development configuration points at a live shared SQL Server. Track E's tests use SQLite and LocalDB only, and you run the scripts.

**Recovery**
- The journal is read whole and split on lines. The bind payload is minified and capped, and a failed replay says so rather than replaying edits onto the wrong state.

**Extensibility**
- **More entities:** a server-driven schema and `ISheetEntityConnector` mean Customers, Items, price lists and inventory master data need only a new connector and route on the ERP side. The sheet doesn't change.
- **Create and delete** later become an `op` per record in the changeset. The guard refuses them today, so nothing has to be undone to add them.
- **Real-time collaboration and general database connectors** stay out. `cgs-bind` speaks only "records with keys, versions and fields", so a future source can reuse it.

---

## I. Questions

### I.1 Blocking decisions: answered on 2026-11-09

| Question | Answer |
| --- | --- |
| How the sheet gets ERP access | A one-time launch ticket in the URL fragment, redeemed for a short-lived, revocable, scoped sheet session. The API key never leaves the ERP |
| How a commit detects concurrent ERP edits | `RowVersion` added to `Vendors` in every tenant database before deploy. Strict: any change since loading is a conflict, and Refresh rebases non-overlapping edits visibly before you commit |
| Who builds Track E | Claude, in `D:\CashGear\Projects\CashGear` (UnifiedAPI and the `ERPClient_React` button), with tests on SQLite and LocalDB only. You run the database scripts and deploy. That repo's git workflow is agreed when Track E starts |
| Where ERP data may live outside the ERP | A capped crash-recovery draft in OPFS on the device only, cleared on commit, discard or close. Save and Save As write a plain workbook with no connection data |

### I.2 Recommended defaults (say if any should change)

**Languages**
- The ERP tab, dialogs and messages in English and Arabic, chosen by the ERP user's language.
- Arabic sheets open right-to-left.
- The shell stays English until the app's Arabic UI phase.

**Formulas in ERP fields**
- Allowed, flagged, and left out of commits until converted to values.

**Vendors fields for the MVP**
- Editable with Edit: name, tax file, mobile, address, pay method and vendor group, from the fields `UpdateEx` writes today.
- Needing AdvEdit: Blocked and Currency.
- Read-only: bank account and debit limit, because bulk bank-account edits are a payment-fraud risk; keys; audit columns.
- `Token` is never sent.

**Limits**
- 10,000 records per sheet, in pages of 1,000.
- Commits of up to 500 records, all or nothing.

**Sessions**
- 30 minutes idle and 10 hours maximum. Tickets last 120 s.

**Where data opens**
- A new workbook per launch. Bound sheets never go inside your open workbook.

**Platform**
- Web only for the MVP.
- Desktop gets the connector's `connect-src` in Phase 19, and a deep link later.

**Releases**
- One minor release per phase. The ERP button stays hidden per company until Phase 18.

### I.3 Optional future improvements

- Creating and deleting ERP records.
- Lazy paging past 10,000 records, and batches past 500 committed in order.
- Reconnectable saved workbooks.
- Drafts shared across devices in Azure Blob.
- OpenID Connect in `CashGear.IdentitySystem`, replacing tickets and sheet sessions with standard sign-in.
- A FusionCache backplane, so the Blazor host sees sheet commits at once.
- Optimistic concurrency in the Blazor host's `UpdateEx`.
- Customers, Items, price lists and inventory master data connectors.
- The desktop deep link.
- Labels and colours for contextual-tab groups in `CashGear.React.UI`.

### I.4 ASP.NET Core integration tests to add in Track E

**Host setup**
- A `CashGear.UnifiedAPI.Tests` project with `WebApplicationFactory<Program>`.
- It replaces `TenantDbContextFactory` and `TenantRegistry` with test versions:
  - two companies sharing one database, as Kit and Dot do, plus one with its own;
  - SQLite for logic, with `SqliteRowVersionContext`'s approach for the column;
  - opt-in LocalDB SQL Server, behind an environment variable as the ERP's existing SQL tests are, for real `rowversion`, `UPDLOCK` and transaction behaviour.
- It never uses the live configuration.

**Cases**

| Area | Cases |
| --- | --- |
| Tickets | Single use; expiry; a ticket from another origin refused; rate limit |
| Sessions | Idle and maximum expiry; `End`; parent key revoked mid-session; company deactivated |
| Scope | A sheet token refused on every non-sheet endpoint; an API key refused on the sheet routes |
| Tenancy | Another company's ids invisible to Query and refused by Commit; `company.mismatch` |
| Fields | Read-only fields refused; AdvEdit fields without the privilege refused |
| Records | Ownership and row locks |
| Commit | Validation errors collected for every field before any write; 409 lists every conflicting record; a batch with one bad record writes nothing (rollback) |
| Idempotency | A retry with the same `commitId` returns the stored result and writes once; the same id with another payload is refused |
| Audit | Rows written in the transaction, absent after a rollback |
| Cache | Tags evicted after a commit |
| Contract | The sheet's suite (`ERP_BASE_URL`) run against the test host |
