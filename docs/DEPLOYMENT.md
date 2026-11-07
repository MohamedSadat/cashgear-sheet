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
