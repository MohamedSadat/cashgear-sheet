import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { serviceWorker } from './pwa';
import {
  contentSecurityPolicy,
  isEmbedPath,
  LIVE_APIS,
  LIVE_FRAME_PARENTS,
  LOCAL_FRAME_PARENTS,
  MOCK_APIS,
} from './src/csp';

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
// policy, with the mocks and a build's own ERP and Workspace APIs among the origins it may reach,
// and Workspace's Blob Storage when a local run names it (`VITE_WORKSPACE_BLOB_URL`, Azurite's in
// Workspace's dev stack). `/embed` may be framed by Workspace's live and local origins; the rest
// never. The dev server sends none, since its hot reload needs more.
const configured = [
  process.env.VITE_ERP_API_URL,
  process.env.VITE_WORKSPACE_API_URL,
  process.env.VITE_WORKSPACE_BLOB_URL,
]
  .filter((url): url is string => Boolean(url))
  .map((url) => new URL(url).origin);
const previewApis = [...LIVE_APIS, ...MOCK_APIS, ...configured];
const previewHeaders = { ...headers, 'X-Content-Type-Options': 'nosniff' };
const appPolicy = contentSecurityPolicy(previewApis);
const embedPolicy = contentSecurityPolicy(previewApis, [
  ...LIVE_FRAME_PARENTS,
  ...LOCAL_FRAME_PARENTS,
]);

/** Sets each preview answer's policy by its path, before Vite sends the file. */
function pagePolicies(): Plugin {
  return {
    name: 'cgs-page-policies',
    configurePreviewServer(server) {
      server.middlewares.use((req, res, next) => {
        const policy = isEmbedPath(req.url ?? '/') ? embedPolicy : appPolicy;
        res.setHeader('Content-Security-Policy', policy);
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), serviceWorker(), pagePolicies()],
  define: { __CGS_VERSION__: JSON.stringify(version) },
  server: { host: '127.0.0.1', port: 5173, strictPort: true, headers },
  preview: { host: '127.0.0.1', port: 4173, strictPort: true, headers: previewHeaders },
  worker: { format: 'es' },
  build: {
    target: 'es2022',
    // `/embed` is a page of its own (Track S3), served for `/embed` as Vite's preview and Azure
    // Static Web Apps rewrite it.
    rolldownOptions: {
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        embed: fileURLToPath(new URL('./embed.html', import.meta.url)),
      },
    },
  },
});
