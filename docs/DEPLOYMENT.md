# Deployment and releases

How CashGear Sheet reaches people: the web app on Azure Static Web Apps, and the Windows installer
on GitHub Releases. Decisions behind it are in `decisions.md` (2026-11-07).

## Web

`pnpm build` writes a static site to `apps/web/dist`, with `sw.js` (the service worker) and
`third-party-notices.txt`. Every response needs these headers, or the page isn't cross-origin
isolated and the engine has no shared memory:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Resource-Policy: same-origin
```

- **Azure Static Web Apps** (`proud-desert-008137810`): `.github/workflows/azure-static-web-apps-proud-desert-008137810.yml` builds on every push to `main` and uploads `apps/web/dist`. `apps/web/public/staticwebapp.config.json` sets the headers, the `.wasm` and `.webmanifest` types, `no-cache` for the page and `sw.js`, and a year's caching for `/assets/*`, whose names carry their hashes.
- **Custom domain:** `sheet.cashgearerp.com`. Add a `CNAME` record for `sheet` pointing at `proud-desert-008137810.4.azurestaticapps.net`, then add the domain in the Azure portal under the Static Web App's Custom domains. Azure issues the certificate.
- **Offline:** the service worker caches the app shell when it installs, under a name of its build, and drops older builds' caches. Pages come from the network first, so a deploy shows on the next load. Workbooks never pass through it.
- **Installing:** `manifest.webmanifest` with 192 and 512 px icons and a maskable one, drawn by `pnpm make:icons` from `apps/web/public/favicon.svg`.

## Desktop

- `pnpm desktop:installer` (after `pnpm build`) builds `target/release/bundle/nsis/CashGear Sheet_<version>_x64-setup.exe`: NSIS, per user, no admin rights, with `licences/THIRD-PARTY-NOTICES.txt` beside the app.
- **File types:** `.xlsx`, `.xlsm`, `.xltx`, `.xltm` and `.csv` (`apps/desktop/src-tauri/windows/hooks.nsh`). The installer adds a ProgID for each (`com.cashgear.sheet.xlsx`), lists it under the type's Open with, and registers the app in Settings › Default apps. It never takes the default: a person picks CashGear Sheet once, from Open with › Always or from Default apps. Uninstalling removes exactly these keys.
- **Double-click:** Explorer runs `cgs-desktop.exe "<file>"`. Each window is its own process, and the app opens the workbook as soon as its engine is ready.
- **Updates:** Tauri's updater plugin reads `latest.json` from the latest GitHub release, 10 s after start and every 6 hours. The status bar then offers the update, and the Update dialog saves the workbook first if it has changes. Packages are checked against the public key in `tauri.conf.json`.
- **Signing:** `scripts/sign-windows.mjs` signs the app, the uninstaller and the installer, with Azure Artifact Signing when its settings exist and a throwaway certificate otherwise.

## Releasing

1. Put the new version in every `package.json`, the Cargo workspace and `tauri.conf.json`, and add its section to `CHANGELOG.md`. `pnpm release:check` checks them.
2. Push a tag `v<version>`. `.github/workflows/release.yml` builds, runs the installer test (install, double-click, update, uninstall) with throwaway keys, builds the signed installer, and drafts the GitHub release with the installer, its `.sig`, `latest.json` and the notices.
3. Publish the draft. Installed copies see the update from then on.

**Settings**

| Setting | Kind | What it is |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Secrets | The updater's private key and its password. Set on 2026-11-07. A copy is outside the repo, at `%USERPROFILE%\.cashgear\cashgear-sheet-updater` on the reference machine: losing both means installed copies can never update themselves |
| `CGS_SIGN_ENDPOINT`, `CGS_SIGN_ACCOUNT`, `CGS_SIGN_PROFILE` | Variables | Azure Artifact Signing. Not set yet |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | Secrets | The service principal that signs. Not set yet |

## Testing a release locally

- `pnpm desktop:installer-test` (Windows, after `pnpm build`): installs as a product of its own (`CGS Installer Test`) in a temp folder, opens an `.xlsx` through the shell, updates from a feed on 127.0.0.1, uninstalls, and checks that nothing is left and that the user's own keys for the file types are unchanged.
- An installer built here without the release workflow's settings is a test build: share the release's installer instead.

## Cash Gear ERP data

Bound sheets (Phases 14 to 19) talk to UnifiedAPI. The design is `ERP_SHEET_INTEGRATION_PLAN.md`,
the wire is `erp-contract.md`, and the ERP side is Track E in `D:\CashGear\Projects\CashGear`.

**The sheet**
- **The API's origin** comes from the build, never the link: `VITE_ERP_API_URL` when a build names
  one, else `https://api.cashgearerp.com` for every page not on localhost (the mock,
  `http://127.0.0.1:4180`, for one that is). The Azure workflow names none, so
  `sheet.cashgearerp.com` talks to `api.cashgearerp.com`.
- **Headers:** ERP calls are CORS requests, which COEP `require-corp` allows, so
  `staticwebapp.config.json` needs nothing more. The service worker leaves cross-origin requests to
  the network.
- **The desktop app** may reach `https://api.cashgearerp.com` (`connect-src`), but has no ERP
  launch until the deep link (`desktop-deep-link.md`), and the API doesn't allow its origin yet.

**UnifiedAPI's `SheetLaunch` settings**

| Setting | Live value | What it is |
| --- | --- | --- |
| `Companies` | `["Kit", "Dot"]` | Companies whose Vendors list shows "Edit in Cash Gear Sheet"; `CreateTicket` refuses the rest |
| `SheetUrl` | `https://sheet.cashgearerp.com` | Where the button sends the launch |
| `Origins` | `["https://sheet.cashgearerp.com"]` | Who may redeem tickets and call the sheet's routes. `appsettings.Development.json` adds the localhost dev and preview servers. Lists merge by index, so an override must start with the live origin |
| `TicketSeconds`, `IdleMinutes`, `MaxHours` | 120, 30, 10 | A ticket's life; a session's idle and longest life |
| `KeyCheckSeconds` | 60 | How long a session trusts its API key's and company's state before reading them again |
| `RateLimits:LaunchPerMinute`, `SessionPerMinute` | 20, 120 | Per client address over one-minute windows: CreateTicket with Redeem, Renew with End. Past one: 429 `rate.limited` with `Retry-After`. The address is the connection's own, so behind a proxy or load balancer the limiter's key must change |

**Turning a company on**, in this order (`erp-contract.md` has the RowVersion rollout):
1. `CashGear.IdentitySystem/DbScripts/Create_SheetSessions.sql` in CGService, once.
2. `DbScripts/Add_VendorsRowVersion.sql`, then `DbScripts/Create_SheetCommit.sql`, in the
   company's database. The first script's last query must show `RowVersionBytes` 8.
3. The company in `SheetLaunch:Companies`, then deploy UnifiedAPI. `ERPClient_React` needs no
   deploy: its button asks Status.

**The test tenant** is Kit in ASTDataTest (decisions, 2026-11-11, Phase 19).
- `DbScripts/Seed_SheetTestVendors.sql` adds Kit's `SHT-0001` to `SHT-0012` and Dot's `SHT-0001`
  and `SHT-0002`, and changes nothing on a second run.
- The five test users' keys go in `%USERPROFILE%\.cashgear\sheet-erp-test.env`, one `NAME=value`
  a line: `ERP_TEST_KEY_EDITOR`, `ERP_TEST_KEY_ADV`, `ERP_TEST_KEY_VIEWER`, `ERP_TEST_KEY_NONE`
  and `ERP_TEST_KEY_OTHER` (a Dot editor). Never in the repo.
- `pnpm erp:tenant suite` runs the contract suite against the live API, and `pnpm erp:tenant e2e`
  runs `tests/tenant` in Chrome and Edge, driving the deployed sheet. Both send the live sheet's
  Origin, make real commits only to the `SHT-` vendors, put back every value they changed, and
  never print the keys. The launch limit counts this machine's every session, so the suite waits
  out 429s and takes several minutes.

**Monitoring.** UnifiedAPI logs each sheet request once with its route, status, time, company,
user and correlation id, and never a token, ticket, key or body. 403s, 429s and failures log at
Warning or above, which reaches the Windows Event Log. The sheet's Connection Info shows the last
failed request's reference, the same correlation id, so a user can quote it.

## Cash Gear Workspace's workbooks

Workbooks that Cash Gear Workspace keeps open here from its "Open in Cash Gear Sheet" (Track S1
and S2): `https://sheet.cashgearerp.com/cloud#launch=<ticket>`. The wire is
`workspace-contract.md`; Workspace's side is its Phase 5, in `D:\CashGear\Projects\cashgear-workspace`.

**The sheet**
- **The API's origin** comes from the build, never the link: `VITE_WORKSPACE_API_URL` when a build
  names one, else `https://workspace-api.cashgearerp.com` for every page not on localhost (the
  mock, `http://127.0.0.1:4280`, for one that is). The Azure workflow names none.
- **Headers:** `connect-src` lists Workspace's API (`apps/web/src/csp.ts`, `LIVE_APIS`, which
  `csp.test.ts` checks against `staticwebapp.config.json`). Workbooks are read from Workspace's
  Blob Storage by signed URL, so its account's exact origin
  (`https://<account>.blob.core.windows.net`) goes into both once the account exists; until then
  the live sheet can't read a Workspace workbook. Nothing else changes: the reads and saves are
  CORS requests, which COEP `require-corp` allows.
- **The desktop app** has no Workspace launch until the deep link (`desktop-deep-link.md`).

**What Workspace's deployment needs for the sheet**
- Its sheet-session routes (`/api/v1/sheet-sessions/*`) allow `https://sheet.cashgearerp.com`
  only, without credentials, with the headers in `workspace-contract.md`; Redeem refuses any
  other `Origin` with 403 `origin.denied`.
- Its storage account's CORS rule allows `GET` from `https://sheet.cashgearerp.com`, so the
  signed read URLs can be read from the sheet's cross-origin isolated page.
- Its launch route opens `https://sheet.cashgearerp.com/cloud#launch=<ticket>`.

**The mock:** `pnpm workspace:mock` (its API on 4280, its Blob Storage on 4281). Playwright starts
it for `tests/e2e/cloud.spec.ts`.

### The embed (Track S3 to S5)

Workspace's pages frame `https://sheet.cashgearerp.com/embed` (`sheet-embed.1`, Workspace's
`docs/INTEGRATIONS.md`), sandboxed (`allow-scripts allow-same-origin`), with no referrer.

- **Headers:** `/embed` (rewritten to `/embed.html`) and `/embed.html` get the site's policy with
  `frame-ancestors https://workspace.cashgearerp.com tauri://localhost http://tauri.localhost`;
  every other path keeps `frame-ancestors 'none'` (`staticwebapp.config.json`'s routes, checked by
  `csp.test.ts`). COOP, COEP and CORP stay as everywhere: in a frame from a parent with no COEP the
  page isn't cross-origin isolated, and the engine runs without shared memory.
- **What it reads:** the signed URL in `init.source`, so Workspace's storage account's exact origin
  must be in `connect-src` (as for `/cloud`), and its CORS rule must allow `GET` from the sheet.
- **What it keeps:** nothing. The engine starts with `inMemoryOnly`, the page registers no service
  worker, and the installed app's worker answers `/embed` offline with `embed.html`.
- **The desktop app** isn't involved: Workspace's desktop frames the web page.

### Running the sheet locally against Workspace's development stack

For Workspace's end-to-end tests against the real `/embed` and `/cloud`. Workspace's stack
(`pnpm dev:server` there) serves its API on `http://127.0.0.1:5280` and Azurite's Blob Storage on
`http://127.0.0.1:10000`; its web app is on `5273` (dev) or `4273` (e2e).

1. Once: `pnpm install` and `pnpm setup:wasm`.
2. Build with Workspace's API named, so `/cloud` redeems there, not at the mock:

   ```bash
   VITE_WORKSPACE_API_URL=http://127.0.0.1:5280 pnpm build
   ```

   In PowerShell: `$env:VITE_WORKSPACE_API_URL='http://127.0.0.1:5280'; pnpm build`. `pnpm build`
   spends about 17 minutes in binaryen; for runs that only need the pages,
   `node scripts/build-wasm.mjs --release --profiling`, then
   `pnpm --filter @cgs/web build` and `pnpm notices` (never ship that build).
3. Serve it, naming the API and Azurite for the preview's `connect-src`:

   ```bash
   VITE_WORKSPACE_API_URL=http://127.0.0.1:5280 VITE_WORKSPACE_BLOB_URL=http://127.0.0.1:10000 pnpm preview
   ```

   The sheet is then on `http://127.0.0.1:4173` (fixed port): `http://127.0.0.1:4173/embed` and
   `http://127.0.0.1:4173/cloud#launch=<ticket>`. Its `/embed` may be framed by
   `http://127.0.0.1:5273`, `http://127.0.0.1:4273`, `http://localhost:5273` and
   `http://localhost:4273`, and takes `init` from them, because the page itself is served locally.

What Workspace's side needs: its `{sheet}` origin `http://127.0.0.1:4173`, in `frame-src`; its
sheet-session routes' CORS allowing `http://127.0.0.1:4173` (and `http://localhost:4173`) without
credentials; Azurite's CORS allowing `GET` from the same.

**The suites:** `pnpm workspace:contract` runs `sheet-session.1`'s suite against the mock, or with
`WORKSPACE_API_URL=http://127.0.0.1:5280` and `WORKSPACE_LAUNCHER_URL` (Workspace's test launcher,
which answers `POST {user, workbook}` with `{ticket}`) against Workspace's API. `tests/e2e/embed.spec.ts`
runs in `pnpm test:e2e` with its stand-in parent on `127.0.0.1:4273` and `4274`
(`pnpm embed:parent`), so it can't run while Workspace's e2e stack holds 4273.
