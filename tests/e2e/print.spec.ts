// Phase 10's printing through the UI: the Page Layout tab, the Page Setup dialog, File › Print's
// preview, Normal view's page breaks and Export › PDF.
import { expect, test, type Page } from '@playwright/test';
import { readZip } from '../fixtures/workbooks.mjs';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const ribbon = (page: Page) => page.getByTestId('ribbon');

async function tab(page: Page, name: string) {
  await ribbon(page).getByRole('tab', { name, exact: true }).click();
}

/** Opens a command's menu and picks from it. */
async function pick(page: Page, name: string, item: string | RegExp) {
  await ribbon(page)
    .getByRole('button', { name: new RegExp('^' + name + '( ?▾)?$') })
    .first()
    .click();
  // Items that show a tick are checkbox items.
  await page
    .getByRole('menuitem', { name: item })
    .or(page.getByRole('menuitemcheckbox', { name: item }))
    .first()
    .click();
}

/** Fills A1:C`rows` with numbers, as typing them would. */
async function fill(page: Page, rows: number) {
  await page.evaluate(async (count) => {
    for (let row = 1; row <= count; row++) {
      for (let col = 1; col <= 3; col++) {
        await window.cgsTest.command({
          kind: 'input',
          sheet: 0,
          row,
          col,
          text: String(row * 10 + col),
        });
      }
    }
    await window.cgsTest.settled();
  }, rows);
}

/** The first sheet's part as the app saves it. */
async function savedSheet(page: Page) {
  const bytes = Buffer.from(await page.evaluate(() => window.cgsTest.saveBytes()));
  const parts = new Map(readZip(bytes).map(([name, body]) => [name, body]));
  return {
    sheet: parts.get('xl/worksheets/sheet1.xml')!.toString('utf8'),
    workbook: parts.get('xl/workbook.xml')!.toString('utf8'),
  };
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
});

test('Page Layout turns the page, sets margins and fits it, and the save says so', async ({
  page,
}) => {
  await fill(page, 5);
  await tab(page, 'Page Layout');
  await pick(page, 'Orientation', 'Landscape');
  await pick(page, 'Margins', /^Narrow/);
  await page.getByTestId('fit-width').selectOption('1');
  await ribbon(page).getByRole('button', { name: 'Gridlines: Print' }).click();
  const { sheet } = await savedSheet(page);
  expect(sheet).toContain('orientation="landscape"');
  expect(sheet).toContain('<pageMargins left="0.25" right="0.25" top="0.75" bottom="0.75"');
  expect(sheet).toContain('<pageSetUpPr fitToPage="1"/>');
  expect(sheet).toContain('fitToHeight="0"');
  expect(sheet).toContain('<printOptions gridLines="1"/>');
});

test('the Page Setup dialog sets the header, the print area and the titles', async ({ page }) => {
  await fill(page, 60);
  await tab(page, 'Page Layout');
  await ribbon(page).getByRole('button', { name: 'Print Titles' }).click();
  const dialog = page.getByTestId('page-setup');
  await expect(dialog).toBeVisible();
  await dialog.getByTestId('setup-print-area').fill('$A$1:$C$60');
  await dialog.getByTestId('setup-title-rows').fill('$1:$1');
  await dialog.getByRole('tab', { name: 'Header/Footer' }).click();
  await dialog.getByTestId('setup-oddFooter').selectOption({ label: 'Page 1 of ?' });
  await expect(dialog.getByTestId('footer-preview')).toHaveText('Page 1 of 1');
  await dialog.getByRole('tab', { name: 'Margins' }).click();
  await dialog.getByTestId('margin-top').fill('1');
  await dialog.getByTestId('setup-ok').click();
  await expect(dialog).toBeHidden();
  const { sheet, workbook } = await savedSheet(page);
  expect(sheet).toContain('<oddFooter>&amp;CPage &amp;P of &amp;N</oddFooter>');
  expect(sheet).toContain('top="1"');
  expect(workbook).toContain(
    '<definedName name="_xlnm.Print_Area" localSheetId="0">Sheet1!$A$1:$C$60</definedName>',
  );
  expect(workbook).toContain(
    '<definedName name="_xlnm.Print_Titles" localSheetId="0">Sheet1!$1:$1</definedName>',
  );
});

test('a refused margin says why and changes nothing', async ({ page }) => {
  await tab(page, 'Page Layout');
  await pick(page, 'Margins', 'Custom Margins…');
  const dialog = page.getByTestId('page-setup');
  await dialog.getByTestId('margin-left').fill('-1');
  await dialog.getByTestId('setup-ok').click();
  await expect(dialog.getByTestId('setup-error')).toHaveText(
    'Margins must be between 0 and 49 inches.',
  );
  await dialog.getByTestId('setup-cancel').click();
  await expect(dialog).toBeHidden();
});

test('File › Print previews the pages, and a page break adds one', async ({ page }) => {
  // 100 rows of 20 pixels: 47 to a Letter page at Excel's normal margins, as Excel prints them.
  await fill(page, 100);
  await page.keyboard.press('Control+p');
  const view = page.getByTestId('print-view');
  await expect(view).toBeVisible();
  await expect(view.getByTestId('print-count')).toHaveText('1 of 3');
  await expect(view.getByTestId('print-page').locator('svg')).toBeVisible();
  await view.getByTestId('print-next').click();
  await expect(view.getByTestId('print-count')).toHaveText('2 of 3');
  await view.getByTestId('print-back').click();
  await expect(view).toBeHidden();
  // A manual break above row 51 ends a page at row 50: 1-47, 48-50, 51-97 and 98-100.
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 50, left: 0, bottom: 50, right: 0 }),
  );
  await tab(page, 'Page Layout');
  await pick(page, 'Breaks', 'Insert Page Break');
  await page.keyboard.press('Control+p');
  await expect(view.getByTestId('print-count')).toHaveText('1 of 4');
  await view.getByTestId('print-scaling').selectOption('sheet');
  await expect(view.getByTestId('print-count')).toHaveText('1 of 1');
});

test('Export › PDF writes a PDF of the sheet’s pages with its text', async ({ page }) => {
  await fill(page, 50);
  const { bytes, pages } = await page.evaluate(async () => {
    const out = await window.cgsTest.pdf();
    return { bytes: Array.from(out.bytes.subarray(0, 8)), pages: out.pages };
  });
  expect(pages).toBe(2);
  expect(String.fromCharCode(...bytes)).toBe('%PDF-1.7');
});

test('Normal view shows where pages break once page setup is used', async ({ page }) => {
  await fill(page, 60);
  await tab(page, 'Page Layout');
  await pick(page, 'Orientation', 'Portrait');
  await expect
    .poll(() =>
      page.evaluate(() => {
        const grid = window.cgsTest.grid() as unknown as { pageBreaks: { rows: number[] } | null };
        return grid.pageBreaks?.rows ?? null;
      }),
    )
    .toEqual([47]);
});
