# Changelog

Every release of CashGear Sheet, newest first. Versions follow semantic versioning, with one version
across the app, its crates and its packages. The release workflow takes each release's notes from
its section here.

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
