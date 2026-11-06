// Phase 7's charts and pictures through the UI: a chart made from the selection on the Insert tab,
// changed from Chart Design, moved and sized with the mouse, deleted and brought back, a picture
// inserted, and a chart's golden images in light and dark.
import { expect, test, type Page } from '@playwright/test';
import { FONT_OVERRIDE, expectGolden, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const ribbon = (page: Page) => page.getByTestId('ribbon');
const chart = (page: Page) => page.locator('[data-testid="chart"]').first();

const SALES = [
  ['', 'Sales', 'Costs'],
  ['Jan', '120', '80'],
  ['Feb', '150', '90'],
  ['Mar', '170', '120'],
  ['Apr', '140', '100'],
];

async function typeSales(page: Page) {
  await page.evaluate(async (rows) => {
    for (let row = 0; row < rows.length; row++) {
      for (let col = 0; col < rows[row]!.length; col++) {
        const text = rows[row]![col]!;
        if (text) {
          await window.cgsTest.command({
            kind: 'input',
            sheet: 0,
            row: row + 1,
            col: col + 1,
            text,
          });
        }
      }
    }
    await window.cgsTest.settled();
  }, SALES);
}

async function tab(page: Page, name: string) {
  await ribbon(page).getByRole('tab', { name, exact: true }).click();
}

const drawings = (page: Page) => page.evaluate(() => window.cgsTest.drawings());

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

test('Insert makes a chart of the selection, and Chart Design changes it', async ({ page }) => {
  await typeSales(page);
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 0, left: 0, bottom: 4, right: 2 }),
  );
  await tab(page, 'Insert');
  await ribbon(page).getByRole('button', { name: 'Column', exact: true }).click();
  await expect(chart(page).locator('svg').first()).toBeVisible();
  // The new chart is chosen, so its tabs show.
  await expect(ribbon(page).getByRole('tab', { name: 'Chart Design' })).toBeVisible();
  const made = await page.evaluate(() => window.cgsTest.chart(0));
  expect(made?.groups[0]?.kind).toBe('bar');

  await tab(page, 'Chart Design');
  await ribbon(page).getByRole('button', { name: 'Change Chart Type' }).click();
  const dialog = page.getByTestId('chart-type-dialog');
  await dialog.getByTestId('chart-family-line').click();
  await dialog.getByTestId('chart-kind-lineMarkers').click();
  await dialog.getByTestId('chart-type-ok').click();
  await expect
    .poll(async () => (await page.evaluate(() => window.cgsTest.chart(0)))?.groups[0]?.kind)
    .toBe('line');
  // Still chosen after the change.
  await expect(chart(page)).toHaveClass(/cgs-drawing-selected/);
});

test('a chart moves and sizes with the mouse, and Delete and undo take it away and back', async ({
  page,
}) => {
  await typeSales(page);
  await page.evaluate(() =>
    window.cgsTest.command({
      kind: 'insertChart',
      sheet: 0,
      area: { top: 1, left: 1, bottom: 5, right: 3 },
      chart: 'columnClustered',
      x: 260,
      y: 40,
    }),
  );
  await expect(chart(page).locator('svg').first()).toBeVisible();
  const box = (await chart(page).boundingBox())!;
  // Dragging the chart's body moves it by as far as the pointer went.
  await page.mouse.move(box.x + box.width / 2, box.y + 30);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 40, box.y + 60, { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await drawings(page))[0]).toMatchObject({ x: 300, y: 70 });
  // Its corner handle sizes it.
  const moved = (await chart(page).boundingBox())!;
  await page.mouse.move(moved.x + moved.width, moved.y + moved.height);
  await page.mouse.down();
  await page.mouse.move(moved.x + moved.width + 50, moved.y + moved.height + 20, { steps: 4 });
  await page.mouse.up();
  await expect
    .poll(async () => (await drawings(page))[0])
    .toMatchObject({ width: 530, height: 308 });
  // Delete takes it off; undo puts it back where it was.
  await chart(page).click({ position: { x: 20, y: 20 } });
  await page.keyboard.press('Delete');
  await expect.poll(async () => (await drawings(page)).length).toBe(0);
  await page.keyboard.press('Control+z');
  await expect
    .poll(async () => (await drawings(page))[0])
    .toMatchObject({ x: 300, y: 70, width: 530 });
});

test('Insert Pictures puts a picture at the active cell', async ({ page }) => {
  // A 2x2 PNG.
  const png = Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFklEQVR4nGP8z8DwnwEIGBkZGBgYAAAoBAMBr2kkTgAAAABJRU5ErkJggg==',
    'base64',
  );
  await tab(page, 'Insert');
  const chooser = page.waitForEvent('filechooser');
  await ribbon(page).getByRole('button', { name: 'Pictures' }).click();
  await (await chooser).setFiles({ name: 'dot.png', mimeType: 'image/png', buffer: png });
  await expect(page.locator('[data-testid="picture"] img')).toBeVisible();
  const list = await drawings(page);
  expect(list).toHaveLength(1);
  expect(list[0]).toMatchObject({ kind: 'picture', x: 0, y: 0, width: 2, height: 2 });
});

test.describe('golden images', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`a chart in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto('/');
      await expect(status(page)).toHaveText('Ready');
      await page.keyboard.press('Control+Alt+n');
      await waitForGrid(page, theme);
      await typeSales(page);
      await page.evaluate(async () => {
        await window.cgsTest.command({
          kind: 'insertChart',
          sheet: 0,
          area: { top: 1, left: 1, bottom: 5, right: 3 },
          chart: 'columnClustered',
          x: 200,
          y: 20,
        });
        await window.cgsTest.command({
          kind: 'insertChart',
          sheet: 0,
          area: { top: 1, left: 1, bottom: 5, right: 2 },
          chart: 'pie',
          x: 200,
          y: 330,
        });
        // A click in the grid leaves the new chart, so no handles show.
        window.cgsTest.grid()!.selectArea({ top: 20, left: 0, bottom: 20, right: 0 });
      });
      await waitForGrid(page, theme);
      await expectGolden(page, 'charts-' + theme, 0.004);
    });
  }
});
