import { defineConfig } from '@playwright/test';

// The scripted e2e against Cash Gear ERP's test tenant (tests/tenant), run by
// `pnpm erp:tenant e2e`: the deployed sheet against the live API, with no web server of its own.
// Never part of `pnpm test:e2e`, since it needs the test users' keys. No traces, since a trace
// would keep the sheet's session tokens with its requests.
const baseURL = process.env.E2E_BASE_URL ?? 'https://sheet.cashgearerp.com';
export default defineConfig({
  testDir: './tests/tenant',
  timeout: 240_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['json', { outputFile: 'bench/results/tenant-tests.json' }]],
  use: {
    baseURL,
    serviceWorkers: 'block',
    viewport: { width: 1280, height: 800 },
    trace: 'off',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chrome', use: { channel: 'chrome' } },
    { name: 'edge', use: { channel: 'msedge' } },
  ],
  // ERP_TENANT_ON_MOCK=1 tries the spec itself on this machine: the preview and the mock, with
  // the mock's keys, before a tenant exists.
  webServer: process.env.ERP_TENANT_ON_MOCK
    ? [
        { command: 'pnpm preview', url: baseURL, reuseExistingServer: true, timeout: 30000 },
        {
          command: `node tests/erp-mock/server.mjs --port 4180 --sheet-url ${baseURL}`,
          url: 'http://127.0.0.1:4180/__mock/state',
          reuseExistingServer: true,
          timeout: 30000,
        },
      ]
    : [],
});
