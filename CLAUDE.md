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
  2. Commit and push, then tag `phase-N` once remote CI is green. Phase 5 is `v0.1.0-mvp` and Phase 9 is `v1.0.0`.
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

Carried over from CashGear Video, where they were measured:

- WebView2 never asks Tauri for the script of a worker started by a worker, so such a worker never loads on desktop. Start workers from the main thread, or copy `nested-worker.ts`.
- Tauri IPC carries about 11 MB/s and a custom protocol about 45 MB/s. Read files through a protocol, never through IPC.
- The CG library's overlays copy `data-cg-theme` from `<html>`, and some of its styles read `--cg-color-*` tokens.
- `@cashgear/ui` is also the name of a private package in `cashgear-archive`. Ours is the git dependency on `MohamedSadat/CashGear.React.UI`.
- `CgGrid` is record-based with no cell-range mode, so it can't be the sheet grid.
