// Phase 8's pivot tables through the UI: Insert › PivotTable on a new sheet, fields checked in the
// field list, the Design tab's report layout, a refresh after the source changes, the filter
// button on Row Labels, Value Field Settings, and a pivot table's golden images in light and dark.
import { expect, test, type Page } from '@playwright/test';
import { FONT_OVERRIDE, expectGolden, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const ribbon = (page: Page) => page.getByTestId('ribbon');
const pane = (page: Page) => page.getByTestId('pivot-field-list');

const SALES = [
  ['Region', 'Product', 'Sales'],
  ['North', 'Apple', '10'],
  ['South', 'Apple', '20'],
  ['North', 'Pear', '30'],
  ['South', 'Pear', '40'],
];

async function typeSales(page: Page) {
  await page.evaluate(async (rows) => {
    for (let row = 0; row < rows.length; row++) {
      for (let col = 0; col < rows[row]!.length; col++) {
        await window.cgsTest.command({
          kind: 'input',
          sheet: 0,
          row: row + 1,
          col: col + 1,
          text: rows[row]![col]!,
        });
      }
    }
    await window.cgsTest.settled();
  }, SALES);
}

async function tab(page: Page, name: string) {
  await ribbon(page).getByRole('tab', { name, exact: true }).click();
}

const column = (page: Page, col: number, top: number, bottom: number) =>
  page.evaluate(
    async ({ col, top, bottom }) => {
      const out: string[] = [];
      for (let row = top; row <= bottom; row++) out.push(await window.cgsTest.cellText(row, col));
      return out;
    },
    { col, top, bottom },
  );

/** A pivot table made on a sheet of its own, with Region in Rows and Sales in Values. */
async function makePivot(page: Page) {
  await typeSales(page);
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 0, left: 0, bottom: 0, right: 0 }),
  );
  await tab(page, 'Insert');
  await ribbon(page).getByRole('button', { name: 'PivotTable', exact: true }).click();
  const dialog = page.getByTestId('pivot-create-dialog');
  await expect(dialog.getByTestId('pivot-range')).toHaveValue('Sheet1!$A$1:$C$5');
  await dialog.getByTestId('pivot-ok').click();
  await expect(pane(page)).toBeVisible();
  await expect(ribbon(page).getByRole('tab', { name: 'PivotTable Analyze' })).toBeVisible();
  await pane(page).getByRole('checkbox', { name: 'Region' }).click();
  await pane(page).getByRole('checkbox', { name: 'Sales' }).click();
  await page.evaluate(() => window.cgsTest.settled());
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(status(page)).toHaveText('Ready');
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
});

test('Insert makes a pivot table, and its field list lays it out', async ({ page }) => {
  await makePivot(page);
  // A new sheet, with the table at A3 as Excel puts it.
  await expect
    .poll(() => column(page, 1, 3, 6))
    .toEqual(['Row Labels', 'North', 'South', 'Grand Total']);
  expect(await column(page, 2, 3, 6)).toEqual(['Sum of Sales', '40', '60', '100']);
  // A text field goes to Rows when it's checked, as in Excel.
  await pane(page).getByRole('checkbox', { name: 'Product' }).click();
  await page.evaluate(() => window.cgsTest.settled());
  await expect.poll(() => column(page, 1, 3, 5)).toEqual(['Row Labels', 'North', 'Apple']);
});

test('the Design tab changes the report layout, and a refresh reads the source again', async ({
  page,
}) => {
  await makePivot(page);
  await expect.poll(() => column(page, 1, 4, 4)).toEqual(['North']);
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 3, left: 0, bottom: 3, right: 0 }),
  );
  await tab(page, 'Design');
  await ribbon(page).getByRole('button', { name: 'Report Layout' }).click();
  await page.getByText('Show in Tabular Form').click();
  await expect.poll(() => column(page, 1, 3, 3)).toEqual(['Region']);
  // The source changes; the pivot keeps what it read until a refresh.
  await page.evaluate(async () => {
    await window.cgsTest.command({ kind: 'input', sheet: 0, row: 2, col: 3, text: '15' });
    await window.cgsTest.settled();
  });
  expect(await column(page, 2, 4, 4)).toEqual(['40']);
  await tab(page, 'PivotTable Analyze');
  await ribbon(page).getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect.poll(() => column(page, 2, 4, 4)).toEqual(['45']);
});

test('the filter button hides items, and Value Field Settings changes the summary', async ({
  page,
}) => {
  await makePivot(page);
  await expect.poll(() => column(page, 1, 4, 4)).toEqual(['North']);
  // The button on Row Labels.
  const box = await page.evaluate(() => window.cgsTest.grid()!.boxForCell(2, 0));
  const canvas = (await page.getByTestId('sheet-canvas').boundingBox())!;
  await page.mouse.click(canvas.x + box.x + box.width - 6, canvas.y + box.y + box.height - 6);
  const menu = page.getByTestId('pivot-filter-menu');
  await expect(menu).toBeVisible();
  await menu.getByTestId('pivot-filter-item-1').uncheck();
  await menu.getByTestId('pivot-filter-ok').click();
  await expect.poll(() => column(page, 1, 4, 5)).toEqual(['North', 'Grand Total']);
  // Average instead of Sum.
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 3, left: 1, bottom: 3, right: 1 }),
  );
  await tab(page, 'PivotTable Analyze');
  await ribbon(page).getByRole('button', { name: 'Field Settings' }).click();
  const dialog = page.getByTestId('pivot-value-dialog');
  await dialog.getByTestId('pivot-summary').selectOption('average');
  await dialog.getByTestId('pivot-value-ok').click();
  await expect.poll(() => column(page, 2, 3, 4)).toEqual(['Average of Sales', '20']);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`a pivot table in its style, ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await makePivot(page);
    await pane(page).getByRole('checkbox', { name: 'Product' }).click();
    await page.evaluate(() => window.cgsTest.settled());
    await expect.poll(() => column(page, 1, 4, 5)).toEqual(['North', 'Apple']);
    await page.evaluate(() =>
      window.cgsTest.grid()!.selectArea({ top: 20, left: 8, bottom: 20, right: 8 }),
    );
    await expectGolden(page, `pivot-${scheme}`);
  });
}
