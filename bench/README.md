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
