# Phase 1 bake-off

Runs test workbooks through the formula engine and scores them against the values Excel stored. Nothing here ships. It picked Formualizer over IronCalc: see `docs/phase1-report.md`. Commit `720a89e` is the last one that can run IronCalc.

- `harness/`: the `cgs-bench` crate. `book.rs` is a neutral xlsx reader, so the engine's own importer doesn't decide the result. `run.rs` loads a workbook, reads cached values, makes a first edit, calculates in slices, scores, makes random edits and inserts a top row. `bin/structural.rs` compares an engine with Excel after each random structural edit.
- `size-probe/`: what the engine and its adapter add to the WASM.
- `chrome/`, `run-chrome.mjs`, `build-wasm.mjs`: the WASM harness in Chrome, one fresh worker per workbook. The bake-off's numbers come from here.
- `run-native.mjs`: the native build over the corpus, for quick iteration.
- `run-structural.mjs`, `excel-fuzz.ps1`: random structural edits against Excel over COM.
- `report.mjs`: turns result files into the per-function report.

```bash
node scripts/fetch-test-workbooks.mjs --generated
node bench/build-wasm.mjs
node bench/run-chrome.mjs formualizer bench/results/formualizer-public.jsonl --jobs=4 --edits=20
node bench/run-chrome.mjs formualizer bench/results/formualizer-generated.jsonl --set=generated
node bench/report.mjs bench/results/formualizer-public.jsonl bench/results/formualizer-generated.jsonl
cargo build -p cgs-bench --profile fast --bin structural
node bench/run-structural.mjs
```

Set `CGS_PRIVATE_WORKBOOKS` and pass `--set=private` to include a private folder. Those results hold no file names or cell values.

# Phase 2 checks

What the shipped reader and grid are held to. Nothing here ships either.

- `harness/src/bin/fidelity.rs`: opens each file through the shipped reader and engine, and compares every cell with `calamine`.
- `run-excel-text.mjs`, `excel-text.ps1`, `harness/src/bin/exceltext.rs`: compares what we show with what Excel shows over COM, on generated and private workbooks only. Public files never open in Excel.
- `generate-speed-books.mjs`, `measure-speed.mjs`: the size and scrolling targets, in the real app in Chrome. Timings mean something only on the reference machine.

```bash
cargo build -p cgs-bench --profile fast --bin fidelity --bin exceltext
./target/fast/fidelity bench/results/fidelity-public.jsonl tests/workbooks/public
node bench/run-excel-text.mjs tests/workbooks/generated bench/results/excel-text-generated.json
node bench/generate-speed-books.mjs
pnpm build
node bench/measure-speed.mjs
```

Add `--private` to `fidelity` and `run-excel-text.mjs` for a private folder: results then hold no file names or cell values.

# Phase 3 checks

What saving is held to. Nothing here ships either, and saved files only ever go in a temporary folder that is deleted afterwards.

- `harness/src/bin/savecheck.rs`: saves each file through the shipped engine, with no edits and then with test edits through the Rust API. Clean entries are compared by SHA-256 of their raw spans and through the `zip` crate, which shares no code with the writer, and every cell of the saved file is compared with what the edited engine holds.
- `run-save-check.mjs` and `../tests/validator`: runs that over folders of workbooks, then has the Open XML SDK compare each saved file's errors with its source's. `pnpm test:save` runs it on the fixtures, and CI runs that.
- `run-excel-matrix.mjs`, `excel-open.ps1`, `excel-features.ps1`: the COM-open matrix on generated and private workbooks. Excel refuses a workbook over COM where a person would see a repair prompt, and the matrix starts with deliberately broken files (`tests/workbooks/broken`) to prove it can see that. Then come edits chosen to provoke a repair, and the chart, pivot and macro checks. Macros run only in `excel-features.ps1 -Mode macro`, on the fixture whose VBA project `tests/fixtures/vba.mjs` builds. Everything else stays macros-off.

```bash
node scripts/write-fixtures.mjs
node bench/run-save-check.mjs bench/results/save-public.json tests/workbooks/public
node bench/run-excel-matrix.mjs bench/results/excel-matrix.json
```

Add `--private` to `run-save-check.mjs`, or `--private=<folder>` to `run-excel-matrix.mjs`, for a private folder: results then hold counts only. Excel opens the originals read-only and never saves them.

# Phase 4 checks

What editing is held to. Only generated workbooks and what we saved from them go through Excel here.

- `run-edit-fuzz.mjs`, `excel-edits.ps1`, `harness/src/bin/editfuzz.rs`: random edits on a generated seed through the shipped command path and through Excel over COM, compared cell by cell after every step. An undo is judged by Excel's earlier state. Saved files then open in Excel, and are compared again.
- `run-excel-input.mjs`, `excel-input.ps1`, `harness/src/bin/inputcheck.rs`: what typed text means, against Excel.
- `measure-input.mjs`: key to painted frame while 1,000,000 formulas recalculate in Chrome, the longest wait for the engine, and how long saving takes in WASM. Needs `node bench/generate-speed-books.mjs --only=recalc-1m` and a release build.
- `run-excel-matrix.mjs`: now also runs each structural edit over each fixture. `moving-parts.xlsx` holds one of each thing in a file that keeps a cell address.

```bash
cargo build -p cgs-bench --profile fast --bin editfuzz --bin inputcheck
node bench/run-edit-fuzz.mjs --seed=1 --episodes=250 --steps=5 --fresh
node bench/run-excel-input.mjs
node bench/measure-input.mjs
```

# Phase 5 checks

What formatting is held to. Only generated workbooks and private ones go through Excel, as before.

- `run-excel-format.mjs`, `excel-format.ps1`: formats go on through the engine and are saved, Excel reads each one back over COM and saves its own copy, and our reader reads that copy. All three have to agree, and a file's existing style indices and untouched cells have to stay as they were. With `--private=<folder>`, temporary copies of private files get formats too, and only counts reach the output.
- `run-visual-review.mjs`, `excel-picture.ps1`: pictures for reviewing the grid against Excel by eye. Excel draws a range with `CopyPicture`, and the app draws the same range of the same file in Chrome, in light and dark. `tests/fixtures/showcase.mjs` makes the showcase workbook from engine commands, and the e2e goldens draw it too.
- `run-excel-matrix.mjs`: now also formats, merges, sizes, freezes, shifts cells and replaces text on each fixture. `run-edit-fuzz.mjs` shifts cells sideways and down as well.

```bash
cargo build -p cgs-bench --profile fast --bin savecheck --bin editfuzz
node bench/run-excel-format.mjs bench/results/excel-format.json
node bench/run-excel-format.mjs bench/results/excel-format-private.json --private=<folder>
pnpm dev:web
node bench/run-visual-review.mjs bench/results/visual
```

# Phase 10 checks

What printing is held to. Excel measures with Microsoft Print to PDF as its active printer, which each script sets for its own COM session and puts back afterwards. Generated workbooks and fixtures go through Excel; public files only with `--public`, as temporary copies that open read-only with macros forced off, for their page counts.

- `print-books.mjs`: generated books covering every page setup option: paper, orientation, scale and fitting, margins, print titles, manual breaks, hidden and resized lines, headings, centring, page order, print areas, and where the printed range starts and ends.
- `run-page-breaks.mjs`, `excel-page-breaks.ps1`, `harness/src/bin/pagebreaks.rs`: every sheet's `HPageBreaks`, `VPageBreaks` and page count from Excel, against the engine's.
- `run-page-setup-roundtrip.mjs`, `excel-page-setup.ps1`: page setup made in the app opens in Excel with no repair and reads as meant, and Excel's own copy reads back the same.
- `run-pdf-review.mjs`, `excel-export-pdf.ps1`: Excel's `ExportAsFixedFormat` beside the app's PDFs, page by page, as `index.html` to review by eye. `export-pdfs.mjs` exports any workbooks. Both check the app's PDFs with `qpdf --check` (a test tool, fetched by `scripts/fetch-qpdf.mjs` and never shipped) and read their text back with pdf.js (`pdf-text.mjs`), Arabic included.
- `run-excel-matrix.mjs` adds page setup, breaks, print areas and titles to its edits, and the print fixture to its books.

```bash
node scripts/write-fixtures.mjs
node bench/run-page-breaks.mjs bench/results/page-breaks.json --public
node bench/run-page-setup-roundtrip.mjs
pnpm dev:web
node bench/run-pdf-review.mjs
```
