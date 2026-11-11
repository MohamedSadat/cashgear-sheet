// The web app's content security policy (Phase 19's security review): scripts, styles, workers
// and fonts from the app itself, WebAssembly compiled, never framed, and Cash Gear ERP's API the
// only other origin it reaches. public/staticwebapp.config.json carries it for the live site,
// and `pnpm preview` sends it too, with the mock ERP's origins, so the browser tests run under it.
// The desktop's is in apps/desktop/src-tauri/tauri.conf.json.

/** The ERP API the live sheet talks to (`erpApiBase`). */
export const LIVE_API = 'https://api.cashgearerp.com';

/** The mock ERP the preview's browser tests talk to (`pnpm erp:mock`). */
export const MOCK_APIS = ['http://127.0.0.1:4180', 'http://localhost:4180'];

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
