// The content security policy the live site and the preview send (apps/web/src/csp.ts, Phase 19):
// nothing the app does is refused by it. The rest of the suite runs under the same policy; this
// spec fails on a violation that only logs, in the page or its workers, while the app draws a
// chart, prints Arabic to PDF, opens ERP data from the mock and reconnects.
import { expect, test, type Page } from '@playwright/test';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const MOCK = 'http://127.0.0.1:4180';

/** Every violation the page or its workers report, as the browser words it. */
function watch(page: Page) {
  const seen: string[] = [];
  page.on('console', (message) => {
    if (/Content Security Policy/i.test(message.text())) seen.push(message.text());
  });
  return seen;
}

test.beforeEach(async () => {
  await fetch(`${MOCK}/__mock/reset`, { method: 'POST' });
});

test('the app draws, prints and exports under its content security policy', async ({ page }) => {
  const seen = watch(page);
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  const response = await page.goto('/');
  expect(response?.headers()['content-security-policy']).toContain("frame-ancestors 'none'");
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
  await page.evaluate(async () => {
    const rows = [
      ['', 'مبيعات', 'Costs'],
      ['يناير', '120', '80'],
      ['Feb', '150', '90'],
    ];
    for (const [r, row] of rows.entries()) {
      for (const [c, text] of row.entries()) {
        if (text)
          await window.cgsTest.command({ kind: 'input', sheet: 0, row: r + 1, col: c + 1, text });
      }
    }
    await window.cgsTest.settled();
    window.cgsTest.grid()!.selectArea({ top: 0, left: 0, bottom: 2, right: 2 });
  });
  const ribbon = page.getByTestId('ribbon');
  await ribbon.getByRole('tab', { name: 'Insert', exact: true }).click();
  await ribbon.getByRole('button', { name: 'Column', exact: true }).click();
  await expect(page.locator('[data-testid="chart"] svg').first()).toBeVisible();
  const pdf = await page.evaluate(async () => (await window.cgsTest.pdf()).pages);
  expect(pdf).toBeGreaterThan(0);
  expect(seen).toEqual([]);
});

test('ERP data opens and commits under the policy, its API the only other origin', async ({
  page,
}) => {
  const seen = watch(page);
  const created = await fetch(`${MOCK}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': 'mock-kit-editor' },
    body: JSON.stringify({ entity: 'vendors', filter: { search: 'V-100' }, language: 'ar' }),
  });
  const { url } = (await created.json()) as { url: string };
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto(url);
  await expect
    .poll(async () => (await page.getByTestId('erp-status').textContent()) ?? '', {
      timeout: 60_000,
    })
    .toContain('8');
  await waitForGrid(page);
  await page.evaluate(() =>
    window.cgsTest.command({ kind: 'input', sheet: 0, row: 2, col: 7, text: 'CSP St' }),
  );
  const changes = await page.evaluate(() => window.cgsTest.erpChanges());
  expect(changes.records.length).toBe(1);
  expect(seen).toEqual([]);
});
