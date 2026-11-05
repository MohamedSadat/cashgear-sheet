import { expect, test, type Page } from '@playwright/test';
import {
  arabicBook,
  defaultStyledBook,
  formattingBook,
  manyRowsBook,
  zip,
} from '../fixtures/workbooks.mjs';
import { controlColor, expectGolden, FONT_OVERRIDE, openBytes, waitForGrid } from './helpers.mjs';

const canvas = (page: Page) => page.getByTestId('sheet-canvas');

/** Where a cell is drawn on the canvas, from the grid's own geometry. */
async function clickCell(page: Page, row: number, col: number) {
  const point = await page.evaluate(
    ({ row, col }) => {
      const grid = window.cgsTest.grid()!;
      return grid.pointForCell(row, col);
    },
    { row, col },
  );
  await canvas(page).click({ position: point });
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
});

test.describe('the shell', () => {
  test('starts empty, in the system theme, with a CashGear control', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    await expect(page.getByTestId('welcome')).toBeVisible();
    await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
    await expect(page.getByRole('radio', { name: 'System' })).toBeChecked();
    await expect(page.locator('html')).toHaveAttribute('data-cg-theme', 'dark');
  });

  test('is cross-origin isolated, and its workers share memory', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.headers()['cross-origin-opener-policy']).toBe('same-origin');
    expect(response?.headers()['cross-origin-embedder-policy']).toBe('require-corp');
    expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
    await expect(page.locator('.cgs-shell')).toHaveAttribute('data-shared-memory', 'true');
    await expect(page.getByTestId('host-info')).toHaveText('Web');
  });

  test('runs the engine and the tiles in workers started from the page', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    const scripts = page.workers().map((worker) => worker.url());
    expect(scripts.some((url) => url.includes('engine-worker'))).toBe(true);
    expect(scripts.some((url) => url.includes('tile-worker'))).toBe(true);
  });

  test('a manual theme beats the system, restyles the grid, and is remembered', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/');
    await openBytes(page, 'defaults.xlsx', defaultStyledBook());
    await waitForGrid(page, 'light');
    const lightControl = await controlColor(page);

    await page.getByRole('radio', { name: 'Dark' }).click();
    await waitForGrid(page, 'dark');
    expect(await controlColor(page)).not.toBe(lightControl);
    expect(await page.evaluate(() => localStorage.getItem('cgs-theme'))).toBe('dark');

    await page.reload();
    await expect(page.getByRole('radio', { name: 'Dark' })).toBeChecked();

    // The system changing doesn't move a manual choice.
    await page.emulateMedia({ colorScheme: 'light' });
    await expect(page.locator('html')).toHaveAttribute('data-cg-theme', 'dark');

    await page.getByRole('radio', { name: 'System' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-cg-theme', 'light');
    expect(await page.evaluate(() => localStorage.getItem('cgs-theme'))).toBeNull();
    await page.emulateMedia({ colorScheme: 'dark' });
    await expect(page.locator('html')).toHaveAttribute('data-cg-theme', 'dark');
  });
});

test.describe('opening', () => {
  test('through the file picker', async ({ page }) => {
    // Without the File System Access API, as in Safari and Firefox.
    await page.addInitScript(() => {
      delete (window as { showOpenFilePicker?: unknown }).showOpenFilePicker;
    });
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Open a workbook…' }).click();
    await (
      await chooser
    ).setFiles({
      name: 'picked.xlsx',
      mimeType: 'application/octet-stream',
      buffer: formattingBook(),
    });
    await waitForGrid(page);
    await expect(page.getByRole('heading', { name: 'picked.xlsx' })).toBeVisible();
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  });

  test('by dropping a file', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    const base64 = formattingBook().toString('base64');
    await page.evaluate((base64) => {
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      const transfer = new DataTransfer();
      transfer.items.add(new File([data], 'dropped.xlsx'));
      document
        .querySelector('.cgs-shell')!
        .dispatchEvent(new DragEvent('drop', { dataTransfer: transfer, bubbles: true }));
    }, base64);
    await waitForGrid(page);
    await expect(page.getByRole('heading', { name: 'dropped.xlsx' })).toBeVisible();
  });

  test('remembers recent files through file handles', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    // A real handle the picker can hand back without a native dialog.
    const base64 = defaultStyledBook().toString('base64');
    await page.evaluate(async (base64) => {
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      const root = await navigator.storage.getDirectory();
      const handle = await root.getFileHandle('recent-test.xlsx', { create: true });
      const writable = await handle.createWritable();
      await writable.write(data);
      await writable.close();
      (window as unknown as { showOpenFilePicker: () => Promise<unknown[]> }).showOpenFilePicker =
        async () => [handle];
    }, base64);
    await page.getByRole('button', { name: 'Open a workbook…' }).click();
    await waitForGrid(page);
    await expect(page.getByRole('heading', { name: 'recent-test.xlsx' })).toBeVisible();

    await page.reload();
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    await page.getByRole('menuitem', { name: 'File' }).click();
    await page.getByRole('menuitem', { name: 'Recent' }).click();
    await page.getByRole('menuitem', { name: 'recent-test.xlsx' }).click();
    await waitForGrid(page);
    await expect(page.getByRole('heading', { name: 'recent-test.xlsx' })).toBeVisible();
  });

  for (const [what, bytes, expected] of [
    ['text', Buffer.from('this is not a workbook'), /isn't an Excel workbook/],
    ['a zip with no workbook', zip([['readme.txt', 'hello']]), /no workbook/],
    [
      'an old .xls',
      Buffer.concat([
        Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]),
        Buffer.alloc(4096),
      ]),
      /old-format/,
    ],
    [
      'a password-protected file',
      Buffer.concat([
        Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]),
        Buffer.alloc(1024),
        Buffer.from('EncryptedPackage', 'utf16le'),
      ]),
      /password protected/,
    ],
  ] as const) {
    test(`says why it can't open ${what}, and still opens the next file`, async ({ page }) => {
      await page.goto('/');
      await expect(page.getByTestId('engine-status')).toHaveText('Ready');
      await openBytes(page, 'bad.xlsx', bytes);
      await expect(page.getByTestId('engine-status')).toHaveText(expected);
      await expect(page.getByTestId('welcome')).toBeVisible();
      await openBytes(page, 'good.xlsx', defaultStyledBook());
      await waitForGrid(page);
      await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    });
  }
});

test.describe('a sheet', () => {
  test.beforeEach(async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/');
    await expect(page.getByTestId('engine-status')).toHaveText('Ready');
    await openBytes(page, 'formats.xlsx', formattingBook());
    await waitForGrid(page, 'light');
  });

  test('shows a cell in the name box and formula bar', async ({ page }) => {
    await clickCell(page, 5, 4);
    await expect(page.getByTestId('name-box')).toHaveValue('E6');
    await expect(page.getByTestId('formula-bar')).toHaveValue('="a"&"b"');
    await clickCell(page, 5, 3);
    await expect(page.getByTestId('formula-bar')).toHaveValue('=1+1');
    await clickCell(page, 1, 1);
    await expect(page.getByTestId('name-box')).toHaveValue('B2');
    await expect(page.getByTestId('formula-bar')).toHaveValue('italic');
    await page.keyboard.press('ArrowDown');
    await expect(page.getByTestId('name-box')).toHaveValue('B3');
    await page.keyboard.press('Shift+ArrowRight');
    await expect(page.getByTestId('name-box')).toHaveValue('B3:C3');
    await page.keyboard.press('Control+Home');
    await expect(page.getByTestId('name-box')).toHaveValue('A1');
  });

  test('selects a merged area whole, and leaves it by one key press', async ({ page }) => {
    await clickCell(page, 10, 2);
    await expect(page.getByTestId('name-box')).toHaveValue('A11');
    await expect(page.getByTestId('formula-bar')).toHaveValue('merged across three');
    await page.keyboard.press('ArrowRight');
    await expect(page.getByTestId('name-box')).toHaveValue('D11');
  });

  test('keeps frozen panes in place while the rest scrolls', async ({ page }) => {
    await canvas(page).hover({ position: { x: 300, y: 300 } });
    await page.mouse.wheel(0, 2000);
    await expect
      .poll(() => page.evaluate(() => window.cgsTest.grid()!.state().topRow))
      .toBeGreaterThan(20);
    await waitForGrid(page);
    await expectGolden(page, 'formatting-scrolled-light');
  });

  test('switches sheets, leaves hidden ones out, and comes back to where it was', async ({
    page,
  }) => {
    await expect(page.getByTestId('sheet-tab-2')).toHaveCount(0);
    await clickCell(page, 3, 2);
    await expect(page.getByTestId('name-box')).toHaveValue('C4');
    await page.getByTestId('sheet-tab-1').click();
    await waitForGrid(page);
    await expect(page.getByTestId('name-box')).toHaveValue('A1');
    await expect(page.getByTestId('formula-bar')).toHaveValue('On the second sheet');
    await page.getByTestId('sheet-tab-0').click();
    await waitForGrid(page);
    await expect(page.getByTestId('name-box')).toHaveValue('C4');
  });

  test('zooms by the slider and by the wheel', async ({ page }) => {
    await expect(page.getByTestId('zoom-percent')).toHaveText('100%');
    await canvas(page).hover({ position: { x: 300, y: 300 } });
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
    await expect(page.getByTestId('zoom-percent')).toHaveText('110%');
    await waitForGrid(page);
    await page.getByRole('button', { name: 'Zoom out' }).click();
    await expect(page.getByTestId('zoom-percent')).not.toHaveText('110%');
    await waitForGrid(page);
  });

  test('shows placeholders for charts and images', async ({ page }) => {
    const drawings = await page.evaluate(() =>
      window.cgsTest
        .grid()!
        .state()
        .drawings.map((box: { name: string }) => box.name),
    );
    expect(drawings).toEqual(['Sales chart', 'Logo']);
    await expectGolden(page, 'formatting-light');
  });
});

test.describe('golden images', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`formatting in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(page.getByTestId('engine-status')).toHaveText('Ready');
      await openBytes(page, 'formats.xlsx', formattingBook());
      await waitForGrid(page, theme);
      await expectGolden(page, 'formatting-' + theme);
    });

    test(`Arabic and mixed text in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(page.getByTestId('engine-status')).toHaveText('Ready');
      await openBytes(page, 'arabic.xlsx', arabicBook());
      await waitForGrid(page, theme);
      await expectGolden(page, 'arabic-' + theme);
    });

    test(`a default-styled workbook in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(page.getByTestId('engine-status')).toHaveText('Ready');
      await openBytes(page, 'defaults.xlsx', defaultStyledBook());
      await waitForGrid(page, theme);
      await expectGolden(page, 'defaults-' + theme);
    });
  }
});

test('scrolls a sheet of 1,048,576 rows to its end', async ({ page }) => {
  test.setTimeout(180000);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  await openBytes(page, 'rows.xlsx', manyRowsBook(1_048_576, 2));
  await waitForGrid(page, 'light');
  await page.evaluate(() => window.cgsTest.grid()!.scrollTo(0, 1e12));
  await waitForGrid(page);
  expect(await page.evaluate(() => window.cgsTest.grid()!.state().topRow)).toBeGreaterThan(
    1_048_000,
  );
  expect(await page.evaluate(() => window.cgsTest.cellText(1_048_576, 1))).toBe('Row 1048576');
});
