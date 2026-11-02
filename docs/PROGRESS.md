# Progress

What is done, and the evidence for it. The plan is in `PLAN.md`, and decisions are in `decisions.md`.

| Work | Status | Tag |
| --- | --- | --- |
| Planning | Done 2026-11-01 | |
| Track U: components in `CashGear.React.UI` | Not started | |
| Phase 0: Scaffold, toolchain, CI | Done 2026-11-01 | `phase-0` |
| Phase 1: Formula engine bake-off | Done 2026-11-02: Formualizer, with the accuracy target moved to Phase 4 | `phase-1` |
| Phase 2: Open xlsx | Not started | |
| Phase 3: Preserving save | Not started | |
| Phase 4: Editing, formulas, undo | Not started | |
| Phase 5: Ribbon and formatting | Not started | |
| Phase 6: Data tools | Not started | |
| Phase 7: Charts | Not started | |
| Phase 8: Pivot tables | Not started | |
| Phase 9: Scale and release | Not started | |

## Planning

- The plan was approved on 2026-11-01 and saved as `docs/PLAN.md`.
- Engine and library facts were read from published packages and source. Nothing has been compiled yet, so Phase 1 tests them.

## Phase 0: Scaffold, toolchain, CI

A canvas tile drawn in a worker from WASM engine data shows in the browser and in Tauri, and CI enforces quality and licences. The data is a test pattern served by `cgs-engine` through the real `ViewBlock` format.

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| `pnpm dev:web` shows the pattern and a CashGear UI control, in light and dark | Pass | 12 of 12 Playwright tests against the running dev server (`E2E_BASE_URL=http://127.0.0.1:5173 pnpm test:e2e`), in Chrome 156.0.8078.79 and Edge 156.0.4314.58. Screenshots checked by eye |
| `pnpm dev:desktop` shows the same, in light and dark | Pass | Launched on the reference machine and attached over CDP (WebView2 156.0.4314.58). The shared `tests/e2e/pattern.mjs` checks passed in both themes, with the same first-tile hash as the web, 2069359672. The real window was captured with its title bar, which follows the theme |
| CI is green | Pass | [Run 41313708990](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41313708990) at `379d3a7`: Windows job 14 min, licence job 1 min. CI's artifact shows 12 of 12 browser tests with none flaky, and the desktop smoke test passing on WebView2 156.0.4314.50 |
| Adding a GPL dependency makes CI fail | Pass | Every CI run does this with fixtures: `pnpm licenses:selftest` logged "cargo-deny rejected the GPL-3.0 fixture" and "npm license check rejected the GPL-3.0 fixture". Real packages were also tried locally and reverted: `shakmaty` 0.30.2 (GPL-3.0-or-later) made `cargo deny` exit 4, and `tinymce` 7.9.1 (GPL-2.0-or-later) made `pnpm licenses:check` exit 1 |

**Other evidence**

- **Release desktop build:** `pnpm desktop:build && pnpm desktop:smoke` passes on this machine and in CI. It covers `crossOriginIsolated`, shared memory with the engine worker, both worker scripts loading, the native command, the pattern in both themes, the window theme, the choice surviving a restart, going back to the system theme, and no CSP refusals.
- **Theme:** browser tests cover following the system, an override beating the system, the override surviving a reload, and going back to System. In dark mode the bars stay as stored, and background, gridlines and automatic text come from the palette.
- **Every commit:** each of the 8 commits passed the checks that exist at that commit, run one by one in a clean clone.
- **WASM size:** 17,899 bytes, 8,072 gzipped, against the 4 MB budget (CI's `wasm-size.json`).
- **Unit tests:** 30 in Vitest and 7 in Rust.
- **Advisories:** `cargo deny check advisories` is clean. It is informational and doesn't fail CI.

**Known gaps**

- macOS was neither built nor run: there is no Mac here, and CI builds on Windows only, as decided.
- Safari and Firefox weren't run; both are best-effort.
- Only Vite and Tauri set COOP/COEP. Whichever static host Phase 9 picks must set them too.
- A real GPL package never reached remote CI, because that would turn `main` red and branches aren't allowed. Remote CI shows the rejection through fixtures, and real packages were tried locally.
- The grid is only the test tile: no headers, selection, scrolling or real data until Phase 2. `TileCache` never evicts tiles yet.
- CI warns that `actions/checkout@v4`, `actions/setup-node@v4` and others target Node 20 and are being run on Node 24. It still passes.

**Findings**

- Playwright over CDP doesn't list workers that started before it attached, so the desktop smoke test reads worker scripts from the page's resource entries.
- Tiles composite in `requestAnimationFrame`, so a hidden page shows an empty grid until it is shown. This appeared in the app's hidden browser pane, where the canvas had the right pixels by the next read.

## Phase 1: Formula engine bake-off

Measured 2026-11-02. Neither engine reached the 99.5% target, so the phase stopped for a decision. You chose Formualizer and moved the target to Phase 4, for real documents including your private workbooks. The full results and the per-function report are in [phase1-report.md](phase1-report.md).

**Exit criteria**

| Criterion | Result | Evidence |
| --- | --- | --- |
| 200+ files run, with a per-function report | Pass | 2,989 public and 10 generated workbooks ran in Chrome 156 WASM. 696 files have scored cells on Formualizer and 690 on IronCalc. The report covers 524 and 525 functions |
| Winner matches 99.5% of formula cells | **Not met; moved to Phase 4 by your decision** | All Excel-saved files, 10% cap: Formualizer 86.05%, IronCalc 94.67%. Real documents only: Formualizer 98.22% (pooled 98.45%), IronCalc 98.51% (pooled 98.75%) |
| Winner has slice p99 of 50 ms or less | Pass | Every slice measured: Formualizer 13.9 ms, longest 1,343 ms. IronCalc failed at 99.9 ms, longest 15,318 ms, and 2 of 10 scale workbooks timed out at 5 minutes |
| Decision logged | Pass | Formualizer, in `decisions.md` |

**Other evidence**

- **Structural edits against Excel over COM:** 2 seeds, 1,000 edits each, compared with Excel after every edit. Formualizer matched 2,000 of 2,000, comparing about 2.36M cells. IronCalc matched 484 of 2,000.
- **Cached values without calculating:** both engines read them back with nothing calculated. Formualizer in every file, IronCalc except 28 cells in one file. Formualizer then recalculates only what an edit touches, and a contract test proves it with a deliberately wrong cached value.
- **`calc_slice`:** proven with evaluation that stops and carries on, which both forks added. Contract tests ran a 2,000-formula chain at a zero budget on both engines, with visible cells first, and still run it on Formualizer.
- **WASM size:** engine and adapter, 2.21 MB gzipped for Formualizer and 0.62 MB for IronCalc, against the 5 MB budget.
- **Licences:** both engines and all their dependencies pass `pnpm deny`.
- **CI:** [run 41369440155](https://github.com/MohamedSadat/cashgear-sheet/actions/runs/41369440155) at `30c7281`, green with Formualizer alone, on the fork pin.
- **Track U:** no new commits on `claude/cashgear-react-ui-library-xq8tsz`, so the `@cashgear/ui` pin stays at `96d2499`.

**Known gaps**

- Your private workbooks aren't in this run. The harness is ready for them (`CGS_PRIVATE_WORKBOOKS`, `--set=private`), and Phase 4 needs them.
- The fork stays pinned until the upstream PRs land: [#556](https://github.com/PSU3D0/formualizer/pull/556), [#557](https://github.com/PSU3D0/formualizer/pull/557) and [#558](https://github.com/PSU3D0/formualizer/pull/558). The last one's public API snapshot can't be generated on Windows, so that PR's CI check may need it from Linux.
- Spilled arrays after structural edits go wrong in Formualizer, and the structural test's seed has no arrays. See the report.
- Formualizer's graph build on the first edit can't be divided: 4 s at 1,000,000 formulas.
- Timings were measured with another project's benchmark running on this PC, so they are on the pessimistic side.

**Findings**

- Formualizer's own importer scores spilled cells by leaving the saved values in place, so comparing against it makes the engine look better than it is at arrays. Our adapter clears them.
- A published crate can stop building against its own dependency range: `formualizer-workbook` 0.13.0's `xlsx-recalc` feature doesn't compile with `zip` 7.4. We don't enable that feature, but it matters if Phase 2 ever does.
- `SheetJS/test_files` is blocked on GitHub, so it isn't in the corpus.
