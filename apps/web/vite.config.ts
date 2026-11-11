import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { serviceWorker } from './pwa';
import { contentSecurityPolicy, LIVE_API, MOCK_APIS } from './src/csp';

// One version for the whole app, from the root package.json (scripts/release-check.mjs).
const { version } = JSON.parse(
  readFileSync(new URL('../../package.json', import.meta.url), 'utf8'),
) as { version: string };

// SharedArrayBuffer needs cross-origin isolation. The Tauri config sets the same
// headers, and public/staticwebapp.config.json sets them on Azure Static Web Apps.
const headers = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
  'Cross-Origin-Resource-Policy': 'same-origin',
};
// The preview, which the browser tests run against, sends the live site's content security
// policy, with the mock ERP and a build's own ERP API among the origins it may reach. The dev
// server sends none, since its hot reload needs more.
const configuredApi = process.env.VITE_ERP_API_URL?.replace(/\/+$/, '');
const previewHeaders = {
  ...headers,
  'Content-Security-Policy': contentSecurityPolicy([
    LIVE_API,
    ...MOCK_APIS,
    ...(configuredApi ? [new URL(configuredApi).origin] : []),
  ]),
  'X-Content-Type-Options': 'nosniff',
};
export default defineConfig({
  plugins: [react(), serviceWorker()],
  define: { __CGS_VERSION__: JSON.stringify(version) },
  server: { host: '127.0.0.1', port: 5173, strictPort: true, headers },
  preview: { host: '127.0.0.1', port: 4173, strictPort: true, headers: previewHeaders },
  worker: { format: 'es' },
  build: { target: 'es2022' },
});
