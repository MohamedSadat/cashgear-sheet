# Shared contract vectors

Files another repository owns, copied here as they are so this repo's validators run the same
cases. Never edit one by hand: copy the new version from its source and update its line below.

| File                       | Source                                                                  | Commit                                                                                                                              | Run by                                                                                                                                             |
| -------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sheet-embed-vectors.json` | Cash Gear Workspace, `packages/editor/vectors/sheet-embed-vectors.json` | `3e4d7606c93142e33044b7f736eb8cec51e8b9c3` (2026-11-14), SHA-256 `7f18b78dd2e3ce10138dd94c865b2a6e6998be943bffccbc0f7f282d6f1b2f9c` | `packages/ui/src/embed/vectors.test.ts` (Track S5): every message through Sheet's `sheet-embed.1` validators, as sent and after a structured clone |

To take a newer version, from `D:\CashGear\Projects\cashgear-workspace`:

```bash
git show <commit>:packages/editor/vectors/sheet-embed-vectors.json > ../cashgear-sheet/tests/contracts/sheet-embed-vectors.json
```
