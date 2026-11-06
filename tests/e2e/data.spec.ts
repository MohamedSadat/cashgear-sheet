// Phase 6's data tools through the UI: filter buttons and their menu, sorting from the Data tab,
// Format as Table and the Table Design tab, and validation's message.
import { expect, test, type Page } from '@playwright/test';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const nameBox = (page: Page) => page.getByTestId('name-box');
const ribbon = (page: Page) => page.getByTestId('ribbon');

const shows = (page: Page, row: number, col: number) =>
  expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]));

/** How tall a row draws, 1-based: 0 when it's hidden. */
const rowHeight = (page: Page, row: number) =>
  page.evaluate(async (r) => {
    await window.cgsTest.settled();
    return window.cgsTest.grid()!.boxForCell(r - 1, 0).height;
  }, row);

async function goTo(page: Page, name: string) {
  await nameBox(page).click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await expect(nameBox(page)).toHaveValue(name);
}

async function tab(page: Page, name: string) {
  await ribbon(page).getByRole('tab', { name, exact: true }).click();
}

/** Puts rows in from A1, as typing them would. */
async function typeRows(page: Page, rows: string[][]) {
  await page.evaluate(async (list) => {
    for (const [r, row] of list.entries()) {
      for (const [c, text] of row.entries()) {
        await window.cgsTest.command({ kind: 'input', sheet: 0, row: r + 1, col: c + 1, text });
      }
    }
    await window.cgsTest.settled();
  }, rows);
}

const PEOPLE = [
  ['Name', 'Qty'],
  ['cy', '3'],
  ['ann', '1'],
  ['bob', '2'],
  ['ann', '5'],
];

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(status(page)).toHaveText('Ready');
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
  await expect(page.getByTestId('sheet-canvas')).toBeFocused();
});

test('a filter button opens Excel’s menu, and unticking a value hides its rows', async ({
  page,
}) => {
  await typeRows(page, PEOPLE);
  await goTo(page, 'A1');
  await tab(page, 'Data');
  await ribbon(page).getByRole('button', { name: 'Filter', exact: true }).click();
  // The button sits at the right of A1's box.
  const box = await page.evaluate(() => window.cgsTest.grid()!.boxForCell(0, 0));
  const canvas = (await page.getByTestId('sheet-canvas').boundingBox())!;
  await page.mouse.click(canvas.x + box.x + box.width - 6, canvas.y + box.y + box.height - 6);
  const menu = page.getByTestId('filter-menu');
  await expect(menu).toBeVisible();
  await expect(menu.getByTestId('filter-value-ann')).toBeVisible();
  await menu.getByTestId('filter-value-ann').click();
  await menu.getByTestId('filter-ok').click();
  await expect(menu).toBeHidden();
  await expect.poll(() => rowHeight(page, 3)).toBe(0);
  await expect.poll(() => rowHeight(page, 5)).toBe(0);
  expect(await rowHeight(page, 2)).toBeGreaterThan(0);
  await ribbon(page).getByRole('button', { name: 'Clear', exact: true }).click();
  await expect.poll(() => rowHeight(page, 3)).toBeGreaterThan(0);
});

test('Sort A to Z sorts the region under its heading', async ({ page }) => {
  await typeRows(page, PEOPLE);
  await goTo(page, 'A3');
  await tab(page, 'Data');
  await ribbon(page).getByRole('button', { name: 'Sort A to Z' }).click();
  await shows(page, 1, 1).toBe('Name');
  await shows(page, 2, 1).toBe('ann');
  await shows(page, 3, 1).toBe('ann');
  await shows(page, 4, 1).toBe('bob');
  await shows(page, 5, 1).toBe('cy');
  await shows(page, 5, 2).toBe('3');
});

test('Format as Table makes a table, and Table Design shows while inside it', async ({ page }) => {
  await typeRows(page, PEOPLE);
  await goTo(page, 'B2');
  await tab(page, 'Home');
  await page.getByTestId('table-style-gallery').getByRole('button').first().click();
  await page.getByRole('gridcell', { name: 'Medium 9' }).click();
  await expect(page.getByTestId('table-name')).toBeHidden();
  await page.getByRole('button', { name: 'OK' }).click();
  await expect(ribbon(page).getByRole('tab', { name: 'Table Design' })).toBeVisible();
  await tab(page, 'Table Design');
  await expect(page.getByTestId('table-name')).toHaveValue('Table1');
  await goTo(page, 'E9');
  await expect(ribbon(page).getByRole('tab', { name: 'Table Design' })).toBeHidden();
});

test('a list refuses what isn’t in it with its own message', async ({ page }) => {
  await goTo(page, 'A1');
  await page.evaluate(() =>
    window.cgsTest.command({
      kind: 'setValidation',
      sheet: 0,
      areas: [{ top: 1, left: 1, bottom: 1, right: 1 }],
      rule: {
        kind: 'list',
        formula1: '"Red,Green"',
        errorTitle: 'Colours',
        errorText: 'Pick one.',
      },
    }),
  );
  await page.evaluate(() => window.cgsTest.settled());
  await page.keyboard.type('Purple\n');
  const message = page.getByTestId('data-message');
  await expect(message).toContainText('Pick one.');
  await message.getByTestId('refused-cancel').click();
  await shows(page, 1, 1).toBe('');
  await page.keyboard.type('Green\n');
  await shows(page, 1, 1).toBe('Green');
});
