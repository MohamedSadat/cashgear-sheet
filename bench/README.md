# Phase 1 bake-off

Runs test workbooks through the formula engines and scores them against the values Excel stored. Nothing here ships.

- `harness/`: the `cgs-bench` crate. `book.rs` is a neutral xlsx reader, so neither engine's own importer decides the result. `run.rs` loads a workbook, reads cached values, makes a first edit, calculates in slices, scores, makes random edits and inserts a top row. `bin/structural.rs` compares an engine with Excel after each random structural edit.
- `size-probe/`: what one engine and its adapter add to the WASM.
- `chrome/`, `run-chrome.mjs`, `build-wasm.mjs`: the WASM harness in Chrome, one fresh worker per workbook. The bake-off's numbers come from here.
- `run-native.mjs`: the native build over the corpus, for quick iteration.
- `run-structural.mjs`, `excel-fuzz.ps1`: random structural edits against Excel over COM.
- `report.mjs`: turns result files into the per-function report.

```bash
node scripts/fetch-test-workbooks.mjs --generated
node bench/build-wasm.mjs formualizer
node bench/run-chrome.mjs formualizer bench/results/formualizer-public.jsonl --jobs=4 --edits=20
node bench/run-chrome.mjs formualizer bench/results/formualizer-generated.jsonl --set=generated
node bench/report.mjs bench/results/formualizer-public.jsonl bench/results/formualizer-generated.jsonl
cargo build -p cgs-bench --profile fast --bin structural
node bench/run-structural.mjs
```

Set `CGS_PRIVATE_WORKBOOKS` and pass `--set=private` to include a private folder. Those results hold no file names or cell values.

Build with `--features own-loaders` and add `--own` to `run-native.mjs` to run each engine through its own xlsx importer. That helps tell adapter bugs from engine gaps, with one catch: an importer that keeps saved spill values can match without calculating them.
