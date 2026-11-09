import { defineConfig } from '@playwright/test';

// Chrome and Edge are the first-class browsers. E2E_BASE_URL points the specs at a
// server that is already running, such as `pnpm dev:web`.
const baseURL = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4173';
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 60000,
  expect: { timeout: 30000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI
    ? [
        ['github'],
        ['html', { open: 'never' }],
        ['json', { outputFile: 'bench/results/browser-tests.json' }],
      ]
    : [['list'], ['json', { outputFile: 'bench/results/browser-tests.json' }]],
  use: {
    baseURL,
    // Every spec runs without the service worker, so each load is the build itself.
    // tests/e2e/pwa.spec.ts allows it in a profile of its own.
    serviceWorkers: 'block',
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chrome', use: { channel: 'chrome' } },
    { name: 'edge', use: { channel: 'msedge' } },
  ],
  // The sheet's preview, and the mock ERP on a second origin for tests/e2e/erp.spec.ts, as the
  // real API is on another origin than the sheet.
  webServer: [
    ...(process.env.E2E_BASE_URL
      ? []
      : [
          {
            command: 'pnpm preview',
            url: baseURL,
            reuseExistingServer: !process.env.CI,
            timeout: 30000,
          },
        ]),
    {
      command: 'node tests/erp-mock/server.mjs --port 4180 --sheet-url http://127.0.0.1:4173',
      url: 'http://127.0.0.1:4180/__mock/state',
      reuseExistingServer: !process.env.CI,
      timeout: 30000,
    },
  ],
});
