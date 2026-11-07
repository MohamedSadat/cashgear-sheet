import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { serviceWorker } from './pwa';

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
export default defineConfig({
  plugins: [react(), serviceWorker()],
  define: { __CGS_VERSION__: JSON.stringify(version) },
  server: { host: '127.0.0.1', port: 5173, strictPort: true, headers },
  preview: { host: '127.0.0.1', port: 4173, strictPort: true, headers },
  worker: { format: 'es' },
  build: { target: 'es2022' },
});
