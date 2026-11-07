# Changelog

Every release of CashGear Sheet, newest first. Versions follow semantic versioning, with one version
across the app, its crates and its packages. The release workflow takes each release's notes from
its section here.

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
