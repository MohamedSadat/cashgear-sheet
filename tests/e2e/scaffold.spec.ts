import { mkdir, writeFile } from 'node:fs/promises';
import { expect, test, type Page } from '@playwright/test';
import { checkPattern, controlColor } from './pattern.mjs';

async function saveResult(page: Page, name: string, image: Buffer) {
  await mkdir('bench/results', { recursive: true });
  await writeFile(`bench/results/${name}.png`, image);
  await page.screenshot({ path: `bench/results/${name}-page.png` });
}

test.describe('scaffold', () => {
  test.beforeEach(async ({ page }) => {
    page.on('pageerror', (error) => {
      throw error;
    });
  });

  for (const colorScheme of ['light', 'dark'] as const) {
    test(`shows the pattern and a CashGear control in the system's ${colorScheme} theme`, async ({
      page,
    }, info) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/');
      await expect(page.getByTestId('engine-status')).toHaveText('Engine ready');
      const { image } = await checkPattern(page, colorScheme);
      await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
      await expect(page.getByRole('radio', { name: 'System' })).toBeChecked();
      await saveResult(page, `web-${info.project.name}-${colorScheme}`, image);
    });
  }

  test('is cross-origin isolated, and its workers share memory', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.headers()['cross-origin-opener-policy']).toBe('same-origin');
    expect(response?.headers()['cross-origin-embedder-policy']).toBe('require-corp');
    expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
    await expect(page.getByTestId('shared-memory')).toHaveText('Shared memory OK');
    await expect(page.getByTestId('host-info')).toHaveText('Web');
  });

  test('runs the engine and the tiles in workers started from the page', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Engine ready');
    await checkPattern(page, await page.evaluate(() => document.documentElement.dataset.cgTheme!));
    const scripts = page.workers().map((worker) => worker.url());
    expect(scripts.some((url) => url.includes('engine-worker'))).toBe(true);
    expect(scripts.some((url) => url.includes('tile-worker'))).toBe(true);
  });

  test('a manual theme beats the system, restyles the control, and is remembered', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/');
    await checkPattern(page, 'light');
    const lightControl = await controlColor(page);

    await page.getByRole('radio', { name: 'Dark' }).click();
    await checkPattern(page, 'dark');
    expect(await controlColor(page)).not.toBe(lightControl);
    expect(await page.evaluate(() => localStorage.getItem('cgs-theme'))).toBe('dark');

    await page.reload();
    await expect(page.getByRole('radio', { name: 'Dark' })).toBeChecked();
    await checkPattern(page, 'dark');

    // The system changing doesn't move a manual choice.
    await page.emulateMedia({ colorScheme: 'light' });
    await checkPattern(page, 'dark');

    await page.getByRole('radio', { name: 'Light' }).click();
    await checkPattern(page, 'light');
  });

  test('going back to System follows the system again, as it changes', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto('/');
    await page.getByRole('radio', { name: 'Light' }).click();
    await checkPattern(page, 'light');

    await page.getByRole('radio', { name: 'System' }).click();
    await checkPattern(page, 'dark');
    expect(await page.evaluate(() => localStorage.getItem('cgs-theme'))).toBeNull();

    await page.emulateMedia({ colorScheme: 'light' });
    await checkPattern(page, 'light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await checkPattern(page, 'dark');
  });
});
