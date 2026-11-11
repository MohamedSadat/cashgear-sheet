# Changelog

Every release of CashGear Sheet, newest first. Versions follow semantic versioning, with one version
across the app, its crates and its packages. The release workflow takes each release's notes from
its section here.

## [1.10.0] - 2026-11-11

ERP data is ready to turn on per company: it opens several times faster, Cash Gear ERP's side is
hardened, and both are checked against the live API. Normal workbooks are unchanged, and their
tiles build faster.

- **Opening ERP data is faster:** from Cash Gear ERP's answer to the sheet on screen, 1,000
  vendors take under 0.3 s and 10,000 about 0.75 s (they took 0.8 s and 2.8 s). The new workbook
  opens while the records download, they go into it as a file's sheet loads, and the grid draws
  them once, at their fitted widths.
- **Tiles build faster** on every sheet with table styles, fills or conditional formats.
- **Connection Info** shows the last request Cash Gear ERP refused or didn't answer, with its
  reference, which finds it in Cash Gear ERP's log.
- A launch whose records fail to load leaves nothing open.
- **Security:** the web app sends a content security policy and is never framed; links in a
  workbook open only web and mail addresses; ERP requests refuse redirects, so a session can't
  follow one.
- **Desktop:** the app may reach Cash Gear ERP's API, ready for opening ERP data from the ERP in a
  later release (`docs/desktop-deep-link.md`).
- **Cash Gear ERP (Track E4, in the ERP's repository):** only `https://sheet.cashgearerp.com` may
  call the sheet's routes; launches and sessions are rate-limited per client address, answering
  429 with `Retry-After`; each sheet request is logged once with its correlation id; API keys no
  longer reach the logs; a company taken off `SheetLaunch:Companies` loses its sessions at once;
  Dot is turned on beside Kit; and `Seed_SheetTestVendors.sql` makes the test tenant's vendors.
  `docs/DEPLOYMENT.md` lists what to run before deploying UnifiedAPI.

## [1.9.0] - 2026-11-11

ERP data now copes with other people's edits, long sessions and crashes, speaks Arabic to Arabic
ERP users, and Cash Gear ERP's "Edit in Cash Gear Sheet" button is turned on for Kit. Normal
workbooks are unchanged.

- **Refresh merges** what Cash Gear ERP holds now with your changes, field by field: a field only
  the ERP changed takes its value, one only you changed stays yours, and one you both changed
  differently is a conflict. Records keep the ERP's old version while they're in conflict, so
  nothing you send can overwrite a change you haven't seen. The status bar says how many changed,
  merged and conflict. A record no longer in the ERP is hatched grey as Not in ERP and left out
  of commits.
- **A commit refused because someone changed a record** merges the same way: records that don't
  clash are ready to commit again at once.
- **Resolve Conflicts** lists every field you and the ERP both changed, with the original, yours
  and the ERP's now and who changed it. Keep Mine or Take ERP's for the fields you select, or for
  all of them; **Next Conflict** goes through them on the sheet.
- **Sessions** renew while you work, so they end only after 30 idle minutes or 10 hours, and end
  when the tab closes. When one ends, your changes stay in the sheet and **Reconnect** waits for
  your next "Edit in Cash Gear Sheet": the new tab hands its connection over and closes. A
  connection for another company or user is refused.
- **A commit whose answer was lost** is looked up with Cash Gear ERP before anything sends it
  again, after Try again or a Reconnect, so it's never saved twice.
- **Recovery:** a bound sheet keeps a recovery copy of its changes on this device, up to 16 MB. After
  a crash it's offered back, opens disconnected and never commits by itself; reconnect, then
  commit.
- **Arabic:** the ERP Data tab, its dialogs, the status badge, hover details and messages are in
  Arabic for Arabic ERP users, right to left.
- Discard Selected in Review Changes now discards the fields its checkboxes select.
- **Cash Gear ERP:** UnifiedAPI's session Renew and End and the commit lookup, CommitStatus; Kit is
  listed in `SheetLaunch:Companies` (Track E3, in the ERP's repository). Run
  `Create_SheetSessions.sql` in CGService and `Add_VendorsRowVersion.sql` and
  `Create_SheetCommit.sql` in ASTDataTest before deploying UnifiedAPI there.

## [1.8.0] - 2026-11-10

Changes to Cash Gear ERP's vendors now reach the ERP: they can be validated, reviewed and
committed from the sheet, all of them or none, and a commit that loses its answer is sent again
without being saved twice. Normal workbooks are unchanged, and the ERP's button stays hidden for
every company until Phase 18.

- **Validate** on the ERP Data tab asks Cash Gear ERP to check every change without saving
  anything. What it refuses goes on its cell with the red border, counts in the status bar and is
  reached by Next Error, with the reason in hover details; editing the cell clears it.
- **Review Changes** lists every changed field with its record, original and new value, status
  and message, filtered to the invalid, the conflicting, those in rows a filter hides, or
  formulas. Go to Cell selects one, and Discard Selected puts the ERP's values back.
- **Commit** sends every change, or with Commit Changes in Visible Rows only those the filter
  shows, after a confirmation that states the counts. Up to 500 vendors go at once; past that it
  asks to commit in smaller filtered groups. Edits wait while it runs. Cells that still hold what
  was sent take the values the ERP saved and show a green bar until they change, read-only
  columns such as Modified by follow, and undo clears.
- **Refused commits write nothing:** the ERP's errors go on their cells, and vendors someone else
  changed since they were opened show an orange hatch with "!", with who changed them and when.
  Commit waits until those changes are discarded or refreshed.
- **A lost answer:** a commit with no answer is sent again under the same id, which Cash Gear ERP
  saves only once; if the answer still doesn't come, Try again sends the same commit.
- **Cash Gear ERP:** UnifiedAPI's vendor Commit, with dry runs, strict row versions, one
  transaction per commit, idempotency by commit id, an audit row per changed field and a SystemLog
  summary (Track E2, in the ERP's repository). `Create_SheetCommit.sql` runs in each tenant
  database before Commit is used there.

## [1.7.0] - 2026-11-10

Cash Gear ERP's vendors can now be edited in the sheet, with every change tracked against what the
ERP sent and shown on its cell. Nothing leaves the sheet yet: Validate, Review and Commit come in
1.8.0. Normal workbooks are unchanged, and the ERP's button stays hidden for every company until
Phase 18.

- **Editable fields:** the columns the ERP lets the user edit (for Vendors: name, group, mobile,
  payment method, address and tax file) take typing, paste, fill, clear, cut and paste, Replace and
  dropdown picks. Keys and read-only columns still refuse with "This column is read-only in Cash
  Gear ERP.", now with a lock in their heading and a faint tint, and sorting still moves whole
  records.
- **Text stays text:** mobiles, codes and tax files keep their leading zeros on every way in, and a
  code typed in another case takes its list's spelling. The green number-stored-as-text triangles
  no longer show on those columns.
- **Every change tracked:** each edited field shows an amber bar and tint until it holds the ERP's
  value again, by undo or by typing it back. Local rules (required, length, type, decimals, range
  and list codes) mark a broken value with a red border as it is typed, and Next Error goes through
  them. Formulas are allowed, shown with a dotted violet bar, and left out of what will be sent
  until converted to values. The colours hold in light, dark and right-to-left sheets, and never
  print or copy.
- **Hover details and dropdowns:** hovering over a changed cell shows its original and new value
  and any rule it breaks; over a read-only one, why it is read-only. List fields drop down their
  codes with the ERP's labels, with no validation rule written to the file.
- **The ERP Data tab:** Discard Changes puts the ERP's values back, everywhere or in the selected
  cells, as one step undo takes back. The status bar counts changed, invalid and formula fields.
  Refresh asks before replacing changes, and closing the tab asks while there are any.
- **Protection:** a paste that grows to its source's size is now checked against every cell it
  writes, so it can no longer reach locked cells on a protected sheet.

## [1.6.0] - 2026-11-09

Cash Gear ERP's vendors open in the sheet, read-only: the first end-to-end slice of editing ERP
data, proving the handoff, cross-origin requests and the guard that keeps each record whole before
any editing exists. Normal workbooks are unchanged, and the ERP's button stays hidden for every
company until Phase 18.

- **Opening from the ERP:** "Edit in Cash Gear Sheet" on the ERP's Vendors list opens a new tab
  with a one-time ticket in the link's fragment, which the sheet reads and removes from the address
  bar and history before anything else runs. A load panel shows progress with Cancel, and says
  plainly when a link has expired or was already used, when the ERP can't be reached (with Try
  again), or when the list is over the 10,000-vendor cap.
- **A bound table:** the vendors open in a new workbook, "Vendors – Kit", as one table with the
  ERP's own headings and column order, typed values (leading zeros kept, decimals at their scale,
  dates as dates), filter buttons, a frozen heading row and fitted columns. Arabic users get Arabic
  headings and a right-to-left sheet.
- **Sorting and filtering, with every record kept whole:** every bound cell is read-only in this
  version. A guard in the engine checks every change to cells, the table and the sheet's shape:
  sorting and its undo pass, while typing, paste, fill, moves, Replace, Remove Duplicates, inserting
  or deleting rows in the table, and reshaping it are refused with a clear message. Formatting is
  never limited.
- **The ERP Data tab:** Refresh replaces the records with what the ERP has now, found by id
  wherever a sort moved them, and Connection Info says where the data came from. The status bar
  and the window's title name the data, the record count, the user and the company.
- **Nothing ERP in saved files:** Save As writes an ordinary workbook. Record ids, versions and the
  connection stay in the engine's memory, and a bound workbook keeps no recovery copy on the device
  yet.
- **The ERP side (Track E1):** UnifiedAPI gained launch tickets, sheet sessions in their own
  authentication scheme, and the Vendors schema and query; the contract suite passes against it on
  LocalDB.

## [1.5.0] - 2026-11-09

The ERP contract: the first step towards editing Cash Gear ERP records, Vendors first, in the
sheet. Nothing in the app changes yet. This release fixes what the sheet and the ERP will
exchange, and proves it with a test suite before any code depends on it.

- **The contract:** `@cgs/erp-connector` holds the type of every request and response, for
  launching from the ERP with a one-time ticket, the short-lived sheet session, the Vendors
  schema, paged queries, validation and commits. Each response has a hand-written guard, and every
  error carries a code from a fixed list. `docs/erp-contract.md` gives the ERP side its routes,
  status codes, C# DTOs and table scripts, and the order for adding RowVersion to every tenant
  database before either ERP host depends on it.
- **A mock ERP:** `tests/erp-mock` serves the contract from Node with no dependencies. It has two
  companies sharing one database, seeded vendors with Arabic names, leading-zero mobiles and
  decimals, tickets and sessions on a clock tests can move, versions that move on every write,
  idempotent commits and failure injection. `pnpm erp:mock` starts it.
- **The contract suite:** runs in `pnpm test` against the mock, and against UnifiedAPI with
  `ERP_BASE_URL`. It covers single-use and expiring tickets, idle and maximum session expiry, the
  schema and who may edit what, keyset paging, dry runs that write nothing, all-or-nothing
  commits, 409 listing every conflict, retries that write once, a reused commit id refused,
  another company's records kept out, read-only and AdvEdit fields refused, and 413 past the batch
  limit.

## [1.4.0] - 2026-11-09

Excel parity polish: the daily-use gaps that 1.0 left, each measured against Excel 2016 first.

- **Hiding sheets, and zoom per sheet:** Hide and Unhide… on a tab's menu and in Home › Format,
  as Excel 2016 has them, the active sheet moving to its neighbour. Each sheet keeps its own
  zoom, saved with the file.
- **Protection:** Protect Sheet with Excel's fifteen options and Protect Workbook's structure,
  with passwords hashed as Excel 2016 hashes them, so a file protected in Excel unprotects here
  with its password and the other way round. Format Cells has a Protection tab for Locked and
  Hidden, and a protected sheet refuses what Excel refuses, with Excel's message.
- **Notes and links:** a note is written in a yellow box beside its cell, by the user name set in
  File › Options. Show/Hide Note and Show All Notes, as in Excel; Remove Hyperlink takes the
  link's look with it.
- **Sort and filter by colour and icon:** fills, font colours and conditional formatting's icons,
  in the filter menu and the Sort dialog.
- **Error checking:** green triangles for Excel's rules, each with its menu (Convert to Number,
  Ignore Error and more), Formulas › Error Checking, and the status bar naming a circular
  reference's cell.
- **Rich text:** characters selected while editing a cell take their own font, and a cell's runs
  stay with it when it is pasted or moved. Superscript and subscript are drawn, on a cell or on
  characters.
- **Cell styles:** New Cell Style by example and Modify on a style's right-click, with Style
  Includes; a style's cells are linked to it as in Excel, and the book's own styles head the
  gallery.
- **Key tips:** tap Alt for badges on the tabs and File, then letters to reach any command, with
  Excel's own tips on the tabs and the Home tab. Alt and a tab's letter work too, except Alt+F,
  Alt+E and Alt+D in a browser.
- **Pivot tables:** Group Field groups dates by days, months, quarters and years and numbers in
  steps, Group Selection groups items by hand, and Ungroup takes them away, as Excel 2016 does.
  A double-click on a value shows its records on a new sheet, Show Report Filter Pages copies a
  pivot table for each filter item, and a click on a value while typing a formula writes
  GETPIVOTDATA, which now calculates.
- **Charts and pictures:** Add Chart Element adds a trendline (linear, exponential, linear
  forecast or moving average) drawn from its fit, and a Picture Format tab turns, flips, sizes,
  resets, orders and describes a picture.
- **Fixed:** a table made on a sheet added since opening is saved, and so is a second pivot table
  made on one sheet.

## [1.3.0] - 2026-11-09

Large books: saving without holding up the sheet, a book too big refused in seconds, much faster
reading, and the installed web app opening files from Open with.

- **Saving in the background:** a save is written a few milliseconds at a time, so the sheet
  keeps scrolling and answering while it runs; on the 25 MB formula book, cells and tiles came
  back within 35 ms at the 99th percentile during the save. The status bar says how far the save
  and the check after it have come. Anything typed meanwhile waits and lands once the save is
  done, and the next save holds it.
- **Too large, said at once:** a workbook whose cells would clearly need more than the engine's
  4 GB is turned away before it is read, with how much it would need: the 15M-formula book now
  says so in about 4 seconds, where it took 8 minutes to run out. Books near the ceiling open as
  before.
- **Faster reading:** the 10M-formula book is ready in 107 seconds instead of 264, and every
  book with formulas reads faster, the 4M book in about half the time. Memory now grows in large
  steps, and the formula engine no longer copies its whole list of staged formulas for each sheet
  it prepares.
- **Open with for the web app:** the installed web app takes `.xlsx`, `.xlsm`, `.xltx`, `.xltm`
  and `.csv` files from Open with in Windows, each in a window of its own. The browser asks once
  whether the app may open these types.

## [1.2.0] - 2026-11-08

Right-to-left sheets, drawn, edited, saved and printed as Excel does them.

- **Sheet Right-to-Left:** on the Page Layout tab, where Excel 2016 has it, and undone like any
  edit. Column A and the row numbers move to the right, and cells, borders, gridlines, data bars,
  notes' corners, filter and list buttons, frozen panes and the selection are mirrored. Charts and
  pictures are placed from the right with their anchors unchanged. As in Excel, alignment isn't
  mirrored: Latin text stays left and numbers right, and text overflows on its aligned side.
- **Keys and the mouse:** on a right-to-left sheet the arrow keys and Ctrl+arrows go the way they
  point, and Tab, Shift+Tab and Home go by column order, as Excel 2016 does. Clicks, dragging a
  selection, the fill handle at the selection's bottom left, column widths from their left edge,
  the cell editor and the filter menus are mirrored too.
- **A default direction:** File › Options sets whether new workbooks and sheets start left to
  right, the default, or right to left.
- **Saving:** the direction is saved as `sheetView rightToLeft`, written only on sheets whose
  direction changed. A new PivotTable sheet takes its source sheet's direction.
- **Printing:** right-to-left sheets print mirrored against the right margin, with the headings
  on the right and their pages broken as Excel breaks them. Notes' red corners no longer print,
  in either direction, as Excel never prints them.

## [1.1.0] - 2026-11-07

Printing and PDF export, with Excel's page setup and its page breaks.

- **Page setup:** a Page Layout tab (margins, orientation, paper size, print area, breaks, print
  titles, scale to fit, and gridlines and headings to view and print) and Excel's Page Setup
  dialog with its Page, Margins, Header/Footer and Sheet tabs. Page setup is read from the file
  and saved by editing the file's own elements, so the printer settings Excel keeps stay.
- **Page breaks:** pages break where Excel 2016 breaks them, as measured against Excel with
  Microsoft Print to PDF: scaling and Fit To, print titles, manual breaks, hidden rows and columns,
  headings, centring, page order and print areas. With no print area, what prints runs from A1
  to the last cell in use, with the cells text overflows into and the pictures that print. Normal
  view shows dashed lines where pages break once page setup has been used. Breaks move with
  inserted and deleted rows and columns.
- **Headers and footers:** text with page numbers, page count, date, time, file name, path and
  sheet name, fonts, sizes, bold, italic, underline and colour, a different first page and
  different odd and even pages. Pictures already in a header or footer are kept and printed.
- **File › Print:** a preview of the pages as they will print, for the active sheet, the whole
  workbook or the selection, and printing through Windows' print dialog on the desktop and the
  browser's on the web.
- **Export as PDF:** vector PDFs drawn by the same code as the screen, in the file's own colours.
  Text can be searched and copied, Arabic included, and fonts are embedded as subsets: the
  file's own fonts where they are installed, else fonts with the same widths (Carlito, Caladea,
  Liberation and Noto Sans Arabic), which are fetched the first time a PDF needs them.
  Gridlines and headings print black, as Excel prints them.
- **Fixed:** a cell in the workbook's normal font no longer makes its row taller than the
  default, so rows of Arial 10 workbooks are 12.75 points as in Excel. Cells in fonts under 12
  points, such as Calibri 10, make rows as tall as Excel does. A cell that starts with a number,
  such as "42 رقم", keeps its number on the left while staying right-aligned, as Excel draws it.

## [1.0.0] - 2026-11-07

The first release: an Excel-like spreadsheet for the browser and for Windows, free to use, that
opens real `.xlsx` files and saves them without damaging what it can't edit.

- **Workbooks:** opens `.xlsx`, `.xlsm`, `.xltx`, `.xltm` and CSV files, and saves `.xlsx` and
  `.xlsm`. Charts, pivot tables, macros and anything else it doesn't edit are kept byte for byte.
  Macros are never run.
- **Editing:** typing, formulas with Excel's functions, fill, copy and paste with Excel, undo and
  redo, inserting and deleting rows, columns, cells and sheets, manual calculation, and recovery
  after a crash.
- **Formatting:** Excel's Home tab: fonts, fills, borders, alignment, number formats, merges, cell
  styles, the format painter, sizes, hiding, freeze panes, and find and replace.
- **Data:** sort, filters, tables with structured references, conditional formatting, data
  validation, names, notes and hyperlinks.
- **Charts:** column, bar, line, pie and doughnut, area and scatter charts with their stacked
  variants and combinations, made, edited, moved and sized as in Excel 2016. Pictures too.
- **Pivot tables:** refreshed, rearranged and made from a range or table, with all of Excel's
  summaries, Show Values As, layouts and its 84 built-in pivot styles.
- **Light and dark:** both, following the system or chosen by hand. Dark mode changes only what is
  on screen, never the file.
- **Arabic:** Arabic and mixed text show correctly.
- **Desktop:** a per-user Windows installer that updates itself, and opens `.xlsx`, `.xlsm`,
  `.xltx`, `.xltm` and `.csv` files from Explorer once it is chosen as their app.
- **Web:** works offline once it has loaded, and can be installed as an app.
- **Large workbooks:** about 10 million formulas or 100 million values, within the engine's 4 GB.
  Past 3 GB the status bar asks for a save, and running out starts a new engine that offers your
  edits back.
