# Phase 1 bake-off

Runs test workbooks through the formula engines and scores them against the values Excel stored. Nothing here ships.

- `harness/`: the `cgs-bench` crate. `book.rs` is a neutral xlsx reader, so neither engine's own importer decides the result. `run.rs` loads a workbook, reads cached values, calculates in slices, scores, makes random edits and inserts a top row.
- `run-native.mjs`: runs the native build over the corpus, for quick iteration.
- `report.mjs`: turns result files into the per-function report.

```bash
node scripts/fetch-test-workbooks.mjs
cargo build -p cgs-bench --profile fast --bin run
node bench/run-native.mjs ironcalc bench/results/ironcalc.jsonl --edits=3
node bench/report.mjs bench/results/ironcalc.jsonl --detail
```

Add `--own` to `run-native.mjs`, after building with `--features own-loaders`, to run each engine through its own xlsx importer. That helps tell adapter bugs from engine gaps, with one catch: an importer that keeps saved spill values can match without calculating them.
