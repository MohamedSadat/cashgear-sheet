// The web app's content security policy (Phase 19's security review): scripts, styles, workers
// and fonts from the app itself, WebAssembly compiled, never framed, and Cash Gear ERP's and Cash
// Gear Workspace's APIs the only other origins it reaches (Track S1). Workspace's Blob Storage
// joins them by its exact origin once its account exists (decisions, 2026-11-12).
// public/staticwebapp.config.json carries it for the live site, and `pnpm preview` sends it too,
// with the mocks' origins, so the browser tests run under it. The desktop's is in
// apps/desktop/src-tauri/tauri.conf.json.

/** The ERP API the live sheet talks to (`erpApiBase`). */
export const LIVE_API = 'https://api.cashgearerp.com';

/** Cash Gear Workspace's API the live sheet opens workbooks from (`workspaceApiBase`). */
export const LIVE_WORKSPACE_API = 'https://workspace-api.cashgearerp.com';

/** Every other origin the live sheet reaches. */
export const LIVE_APIS = [LIVE_API, LIVE_WORKSPACE_API];

/**
 * The mocks the preview's browser tests talk to: the ERP (`pnpm erp:mock`), and Workspace's API
 * and its Blob Storage one port up (`pnpm workspace:mock`).
 */
export const MOCK_APIS = [
  'http://127.0.0.1:4180',
  'http://localhost:4180',
  'http://127.0.0.1:4280',
  'http://localhost:4280',
  'http://127.0.0.1:4281',
  'http://localhost:4281',
];

export function contentSecurityPolicy(apis: readonly string[]): string {
  return [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "worker-src 'self' blob:",
    ['connect-src', "'self'", 'blob:', 'data:', ...apis].join(' '),
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ');
}
