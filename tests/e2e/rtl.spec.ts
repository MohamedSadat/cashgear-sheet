// Phase 11's right-to-left sheets through the UI: Page Layout › Sheet Right-to-Left, the keyboard
// and mouse on a mirrored grid as Excel 2016 measured over COM has them, saving, the default
// direction, golden images in both themes, and pages printed mirrored.
import { expect, test, type Page } from '@playwright/test';
import { arabicBook, formattingBook, readZip, rightToLeftCopy } from '../fixtures/workbooks.mjs';
import { expectGolden, FONT_OVERRIDE, openBytes, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const nameBox = (page: Page) => page.getByTestId('name-box');
const canvas = (page: Page) => page.getByTestId('sheet-canvas');
const ribbon = (page: Page) => page.getByTestId('ribbon');

/** What a cell shows, 1-based, once commands sent so far have run. */
function shows(page: Page, row: number, col: number) {
  return expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]));
}

async function goTo(page: Page, name: string) {
  await nameBox(page).click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await expect(nameBox(page)).toHaveValue(name);
}

/** Where a cell is on the page, 0-based, from the grid's own geometry. */
async function cellPoint(page: Page, row: number, col: number) {
  const point = await page.evaluate(
    ([r, c]) => window.cgsTest.grid()!.pointForCell(r!, c!),
    [row, col],
  );
  const box = (await canvas(page).boundingBox())!;
  return { x: box.x + point.x, y: box.y + point.y };
}

async function cellBox(page: Page, row: number, col: number) {
  const box = await page.evaluate(
    ([r, c]) => window.cgsTest.grid()!.boxForCell(r!, c!),
    [row, col],
  );
  const bounds = (await canvas(page).boundingBox())!;
  return { ...box, x: bounds.x + box.x, y: bounds.y + box.y };
}

/** Page Layout › Sheet Right-to-Left. */
async function flip(page: Page) {
  await ribbon(page).getByRole('tab', { name: 'Page Layout', exact: true }).click();
  await ribbon(page).getByRole('button', { name: 'Sheet Right-to-Left' }).click();
  await expect.poll(() => page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(true);
  await waitForGrid(page);
  await canvas(page).focus();
}

async function input(page: Page, row: number, col: number, text: string) {
  await page.evaluate(
    async ([r, c, t]) => {
      await window.cgsTest.command({ kind: 'input', sheet: 0, row: r!, col: c!, text: t! });
      await window.cgsTest.settled();
    },
    [row, col, text] as [number, number, string],
  );
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(status(page)).toHaveText('Ready');
});

test.describe('on a mirrored sheet', () => {
  test.beforeEach(async ({ page }) => {
    await page.keyboard.press('Control+Alt+n');
    await waitForGrid(page);
    await flip(page);
  });

  test('arrows go the way they point; Tab, Shift+Tab and Home go by columns', async ({ page }) => {
    await input(page, 3, 1, '1');
    await input(page, 3, 6, '2');
    await goTo(page, 'C3');
    await canvas(page).focus();
    const after = async (key: string, expected: string) => {
      await page.keyboard.press(key);
      await expect(nameBox(page)).toHaveValue(expected);
    };
    // As Excel 2016 does on a right-to-left sheet, measured with SendKeys.
    await after('ArrowRight', 'B3');
    await after('ArrowLeft', 'C3');
    await after('ArrowLeft', 'D3');
    await after('Tab', 'E3');
    await after('Shift+Tab', 'D3');
    await after('Home', 'A3');
    await after('Control+ArrowLeft', 'F3');
    await after('Control+ArrowRight', 'A3');
    await goTo(page, 'C3');
    await canvas(page).focus();
    await after('Shift+ArrowRight', 'B3:C3');
  });

  test('typing then an arrow enters the value and moves the way it points', async ({ page }) => {
    await goTo(page, 'C5');
    await canvas(page).focus();
    await page.keyboard.type('7');
    await page.keyboard.press('ArrowRight');
    await expect(nameBox(page)).toHaveValue('B5');
    await shows(page, 5, 3).toBe('7');
  });

  test('column A is on the right, and clicks land on the cell drawn there', async ({ page }) => {
    const bounds = (await canvas(page).boundingBox())!;
    const a1 = await cellPoint(page, 0, 0);
    const b1 = await cellPoint(page, 0, 1);
    expect(a1.x).toBeGreaterThan(bounds.x + bounds.width / 2);
    expect(b1.x).toBeLessThan(a1.x);
    await page.mouse.click(b1.x, b1.y);
    await expect(nameBox(page)).toHaveValue('B1');
    // The row headings are at the canvas's right edge.
    const row3 = await cellPoint(page, 2, 0);
    await page.mouse.click(bounds.x + bounds.width - 6, row3.y);
    await expect(nameBox(page)).toHaveValue('3:3');
  });

  test('dragging selects towards the left, and the fill handle at the bottom left fills down', async ({
    page,
  }) => {
    const from = await cellPoint(page, 1, 1);
    const to = await cellPoint(page, 3, 3);
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 5 });
    await page.mouse.up();
    await expect(nameBox(page)).toHaveValue('B2:D4');

    await input(page, 2, 2, '5');
    await goTo(page, 'B2');
    const cell = await cellBox(page, 1, 1);
    const down = await cellPoint(page, 4, 1);
    // The handle is at the selection's bottom left on screen, as in Excel.
    await page.mouse.move(cell.x + 1, cell.y + cell.height - 1);
    await page.mouse.down();
    await page.mouse.move(cell.x + 1, down.y, { steps: 5 });
    await page.mouse.up();
    await shows(page, 5, 2).toBe('5');
  });

  test('a column’s far edge is on its left, and dragging it left widens it', async ({ page }) => {
    const before = await cellBox(page, 0, 1);
    const headingY = before.y - 8;
    await page.mouse.move(before.x, headingY);
    await page.mouse.down();
    await page.mouse.move(before.x - 40, headingY, { steps: 5 });
    await page.mouse.up();
    await expect
      .poll(async () => Math.round((await cellBox(page, 0, 1)).width))
      .toBeGreaterThan(Math.round(before.width) + 30);
  });

  test('the editor stays at its cell’s right edge', async ({ page }) => {
    const cell = await cellBox(page, 2, 2);
    await goTo(page, 'C3');
    await canvas(page).focus();
    await page.keyboard.type('a long line of text that grows');
    const editor = (await page.getByTestId('cell-editor').boundingBox())!;
    expect(Math.abs(editor.x + editor.width - (cell.x + cell.width))).toBeLessThan(3);
    expect(editor.x).toBeLessThan(cell.x);
    await page.keyboard.press('Escape');
  });

  test('Sheet Right-to-Left saves rightToLeft, undoes, and reads back', async ({ page }) => {
    const sheetPart = async () => {
      const bytes = Buffer.from(await page.evaluate(() => window.cgsTest.saveBytes()));
      return readZip(bytes)
        .find(([name]) => name === 'xl/worksheets/sheet1.xml')![1]
        .toString('utf8');
    };
    expect(await sheetPart()).toContain('rightToLeft="1"');
    await page.keyboard.press('Control+z');
    await expect.poll(() => page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(false);
    expect(await sheetPart()).not.toContain('rightToLeft');
  });

  test('pages print mirrored, column A on the right', async ({ page }) => {
    for (let col = 1; col <= 3; col++) await input(page, 1, col, String(10 + col));
    await page.keyboard.press('Control+p');
    const preview = page.getByTestId('print-preview');
    const x = async (text: string) => {
      const box = await preview.locator('text', { hasText: new RegExp('^' + text + '$') }).first();
      return (await box.boundingBox())!.x;
    };
    await expect(preview.locator('text', { hasText: /^11$/ }).first()).toBeVisible();
    expect(await x('11')).toBeGreaterThan(await x('12'));
    expect(await x('12')).toBeGreaterThan(await x('13'));
  });
});

test('the default direction starts new workbooks and sheets right to left', async ({ page }) => {
  await page.getByRole('menuitem', { name: 'File', exact: true }).click();
  await page.getByRole('menuitem', { name: 'Options…' }).click();
  await page.getByTestId('direction-rtl').check();
  await page.getByTestId('options-dialog').getByRole('button', { name: 'OK' }).click();
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
  expect(await page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(true);
  await page.getByRole('button', { name: 'Insert a sheet' }).click();
  await expect(page.getByRole('tab', { name: 'Sheet2' })).toBeVisible();
  await waitForGrid(page);
  expect(await page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(true);
});

test.describe('golden images', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`a right-to-left sheet of formatting in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(status(page)).toHaveText('Ready');
      await openBytes(page, 'formats-rtl.xlsx', rightToLeftCopy(formattingBook()));
      await waitForGrid(page, theme);
      await expectGolden(page, 'rtl-formatting-' + theme, 0.002, { fromRight: true });
    });

    test(`a right-to-left sheet of Arabic and mixed text in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(status(page)).toHaveText('Ready');
      await openBytes(page, 'arabic-rtl.xlsx', rightToLeftCopy(arabicBook()));
      await waitForGrid(page, theme);
      await expectGolden(page, 'rtl-arabic-' + theme, 0.002, { fromRight: true });
    });
  }
});
