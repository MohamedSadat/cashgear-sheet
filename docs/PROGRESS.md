# Progress

What is done, and the evidence for it. The plan is in `PLAN.md`, and decisions are in `decisions.md`.

| Work | Status | Tag |
| --- | --- | --- |
| Planning | Done 2026-11-01 | |
| Track U: components in `CashGear.React.UI` | Not started | |
| Phase 0: Scaffold, toolchain, CI | Done 2026-11-01 | `phase-0` |
| Phase 1: Formula engine bake-off | Not started | |
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
