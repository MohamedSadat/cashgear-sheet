# Decisions

Every answer and default, with its date. Newest section last.

## 2026-11-01: Planning

**Answered by you**

| Question | Answer |
| --- | --- |
| Which UI repo? | `CashGear.React.UI` (`@cashgear/ui`) |
| Saving an opened xlsx with content the app can't edit | Keep it intact, byte for byte |
| Formula engine | Wrap an existing engine behind a trait; Phase 1 benchmark picks |
| v1 features beyond editing, formulas, formatting and sheets | Sort, filter, tables, conditional formatting, data validation, charts, pivot tables. Print and PDF come after v1 |
| Arabic and RTL | Arabic and mixed text display correctly; grid stays left to right |
| Desktop core | Same WASM as web; a native host only if Phase 9 benchmarks call for it |
| Collaboration | None; local files only, no server |
| Missing UI components | Create them in `CashGear.React.UI` |
| Git in this repo | Push everything to `main` |
| Git in `CashGear.React.UI` | Use the existing branch; create no branches |
| Theme | Light and dark mode are both supported |

**Defaults picked (say if any should change)**

| Choice | Default | Reason |
| --- | --- | --- |
| Name | CashGear Sheet, crate prefix `cgs-` | Matches the sibling apps |
| File format | `.xlsx` is the save format; no private format | Crash-recovery data is kept separately |
| Sheet grid location | Canvas grid, formula bar and cell editor stay in this repo | They are tied to the engine's data protocol |
| Saving method | Patch source byte spans instead of re-serializing parts | Unknown elements and attributes survive automatically |
| Cell storage | The formula engine is the only cell store | Avoids two copies of every cell |
| Undo | Our command stack owns it, not the engine | Formualizer's engine undo has open bugs (#303, #412) |
| Grid drawing | Tile worker draws; main thread composites | Drawing and recalculation never block input |
| Dark mode colours | `dk1`/`lt1` theme colours and automatic colours swap for display only | Default black text stays readable, and the file never changes |
| Targets until Phase 0 confirms | Windows 10/11 x64 first, macOS second; Chrome and Edge first | Same as CashGear Video |
| Licences | No GPL, AGPL or LGPL in anything shipped | Same as CashGear Video |

**Evidence read for the choices above (nothing compiled yet)**

| Item | Finding |
| --- | --- |
| Formualizer 0.13.0 | Dirty-graph recalculation; row and column insert with names; published WASM 10.0 MB raw, 3.6 MB compressed; moved from 0.10.1 to 0.13.0 in 25 days |
| IronCalc 0.9.1 | `evaluate()` recomputes every formula; row and column insert; published WASM 2.5 MB raw, about 1 MB compressed |
| Both engines | Neither can yield during a calculation in browser WASM today |
| `umya-spreadsheet` 3.1.1 | Reads and writes xlsx, with WASM through a `js` feature; its docs don't mention keeping unknown parts |
| `rxls` 0.1.x | Keeps untouched parts byte for byte, but can't insert or delete rows or columns |
| `CgChart` | SVG, with bar, line, area, pie and donut; no scatter |
