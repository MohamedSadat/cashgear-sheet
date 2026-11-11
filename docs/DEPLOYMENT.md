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
