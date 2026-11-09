// Phase 13's Excel parity polish through the UI, each as Excel 2016 measured over COM does it:
// hiding and showing sheets with the zoom saved per sheet, sheet and workbook protection,
// notes in their box beside the cell with Remove Hyperlink, sorting and filtering by colour, and
// error checking with circular references, and rich text edited in the cell.
import { expect, test, type Page } from '@playwright/test';
import { readZip } from '../fixtures/workbooks.mjs';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const canvas = (page: Page) => page.getByTestId('sheet-canvas');
const ribbon = (page: Page) => page.getByTestId('ribbon');
const tab = (page: Page, index: number) => page.getByTestId('sheet-tab-' + index);
const nameBox = (page: Page) => page.getByTestId('name-box');

/** What a cell shows, 1-based, once commands sent so far have run. */
const shows = (page: Page, row: number, col: number) =>
  expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]));

async function goTo(page: Page, name: string) {
  await nameBox(page).click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await expect(nameBox(page)).toHaveValue(name);
}

/** A part of what Save would write now, once queued commands have run. */
async function savedPart(page: Page, name: string | RegExp) {
  const bytes = Buffer.from(
    await page.evaluate(async () => {
      await window.cgsTest.settled();
      return window.cgsTest.saveBytes();
    }),
  );
  return readZip(bytes)
    .find(([part]) => (typeof name === 'string' ? part === name : name.test(part)))![1]
    .toString('utf8');
}

/** Opens a command's menu on the Home tab and picks from it, through a submenu if given. */
async function homeMenu(page: Page, command: string, ...items: string[]) {
  await ribbon(page).getByRole('tab', { name: 'Home', exact: true }).click();
  await ribbon(page)
    .getByRole('button', { name: new RegExp('^' + command + '( ?▾)?$') })
    .first()
    .click();
  for (const item of items) await page.getByRole('menuitem', { name: item, exact: true }).click();
}

async function tabMenu(page: Page, index: number, item: string) {
  await tab(page, index).click({ button: 'right' });
  await page.getByRole('menuitem', { name: item, exact: true }).click();
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
  await expect(canvas(page)).toBeFocused();
});

test.describe('hiding sheets and zoom', () => {
  test('Hide moves to the next sheet, Unhide brings it back, and both save and undo', async ({
    page,
  }) => {
    await page.getByTestId('add-sheet').click();
    await expect(tab(page, 1)).toHaveAttribute('aria-selected', 'true');
    await page.getByTestId('add-sheet').click();
    await expect(tab(page, 2)).toHaveAttribute('aria-selected', 'true');
    await tab(page, 1).click();
    await expect(tab(page, 1)).toHaveAttribute('aria-selected', 'true');
    // Hiding the sheet showing goes to the next visible one to its right, as in Excel.
    await tabMenu(page, 1, 'Hide');
    await expect(tab(page, 1)).toHaveCount(0);
    await expect(tab(page, 2)).toHaveAttribute('aria-selected', 'true');
    const book = await savedPart(page, 'xl/workbook.xml');
    expect(book).toMatch(/<sheet name="Sheet2" [^>]*state="hidden"/);
    expect(book).toContain('activeTab="2"');
    // Unhide lists it and shows it again.
    await tabMenu(page, 2, 'Unhide…');
    const dialog = page.getByTestId('unhide-dialog');
    await expect(dialog.getByRole('option', { name: 'Sheet2' })).toBeVisible();
    await dialog.getByTestId('unhide-ok').click();
    await expect(tab(page, 1)).toHaveAttribute('aria-selected', 'true');
    expect(await savedPart(page, 'xl/workbook.xml')).not.toContain('state="hidden"');
    // From the ribbon, and undone.
    await homeMenu(page, 'Format', 'Hide & Unhide', 'Hide Sheet');
    await expect(tab(page, 1)).toHaveCount(0);
    await page.keyboard.press('Control+z');
    await expect(tab(page, 1)).toBeVisible();
    await expect(tab(page, 1)).toHaveAttribute('aria-selected', 'true');
    // The last visible sheet stays.
    await tabMenu(page, 0, 'Hide');
    await tabMenu(page, 1, 'Hide');
    await tabMenu(page, 2, 'Hide');
    await expect(page.getByTestId('engine-status')).toContainText('at least one visible');
    await expect(tab(page, 2)).toBeVisible();
  });

  test('each sheet keeps its own zoom in the saved file, without being a change', async ({
    page,
  }) => {
    await canvas(page).hover({ position: { x: 300, y: 300 } });
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
    await expect(page.getByTestId('zoom-percent')).toHaveText('110%');
    // As in Excel, zoom alone leaves nothing to save.
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
    expect(await savedPart(page, 'xl/worksheets/sheet1.xml')).toContain(
      'zoomScale="110" zoomScaleNormal="110"',
    );
    await page.getByTestId('add-sheet').click();
    await expect(page.getByTestId('zoom-percent')).toHaveText('100%');
    expect(await savedPart(page, 'xl/worksheets/sheet2.xml')).not.toContain('zoomScale');
    await tab(page, 0).click();
    await expect(page.getByTestId('zoom-percent')).toHaveText('110%');
    expect(await savedPart(page, 'xl/worksheets/sheet1.xml')).toContain('zoomScale="110"');
  });
});

test.describe('protection', () => {
  const review = async (page: Page, name: string) => {
    await ribbon(page).getByRole('tab', { name: 'Review', exact: true }).click();
    await ribbon(page).getByRole('button', { name, exact: true }).click();
  };
  const message = (page: Page) => page.getByTestId('protect-message-text');
  const closeMessage = async (page: Page) => {
    await page.getByTestId('protect-message').getByRole('button', { name: 'OK' }).click();
    await expect(message(page)).toHaveCount(0);
  };

  test('Protect Sheet keeps locked cells, lets unlocked ones change, and takes its password', async ({
    page,
  }) => {
    await page.evaluate(() =>
      window.cgsTest.command({ kind: 'input', sheet: 0, row: 1, col: 1, text: '1' }),
    );
    // B1 unlocked through Format Cells' Protection tab.
    await goTo(page, 'B1');
    await page.keyboard.press('Control+1');
    const format = page.getByTestId('format-cells');
    await format.getByRole('tab', { name: 'Protection' }).click();
    await expect(format.getByTestId('format-locked')).toBeChecked();
    await format.getByTestId('format-locked').uncheck();
    await format.getByTestId('format-ok').click();
    await review(page, 'Protect Sheet');
    const dialog = page.getByTestId('protect-sheet-dialog');
    await expect(dialog.getByTestId('allow-selectLocked')).toBeChecked();
    await expect(dialog.getByTestId('allow-formatCells')).not.toBeChecked();
    await dialog.getByTestId('protect-password').fill('pw');
    await dialog.getByTestId('protect-ok').click();
    const confirm = page.getByTestId('confirm-password-dialog');
    await confirm.getByTestId('confirm-password').fill('pw');
    await confirm.getByTestId('confirm-password-ok').click();
    await expect(ribbon(page).getByRole('button', { name: 'Unprotect Sheet' })).toBeVisible();
    // Typing into a locked cell is refused at the first key, as in Excel.
    await goTo(page, 'A1');
    await page.keyboard.type('x');
    await expect(message(page)).toContainText('on a protected sheet');
    await closeMessage(page);
    await shows(page, 1, 1).toBe('1');
    // An unlocked cell takes it.
    await goTo(page, 'B1');
    await page.keyboard.type('5');
    await page.keyboard.press('Enter');
    await shows(page, 1, 2).toBe('5');
    // Formatting is greyed out, as the sheet doesn't allow it.
    await ribbon(page).getByRole('tab', { name: 'Home', exact: true }).click();
    await expect(ribbon(page).getByRole('button', { name: 'Bold' })).toBeDisabled();
    const sheet = await savedPart(page, 'xl/worksheets/sheet1.xml');
    expect(sheet).toMatch(
      /<sheetProtection algorithmName="SHA-512" hashValue="[^"]+" saltValue="[^"]+" spinCount="100000" sheet="1" objects="1" scenarios="1"\/>/,
    );
    // A wrong password is refused, and the right one unprotects.
    await review(page, 'Unprotect Sheet');
    await page.getByTestId('unprotect-password').fill('PW');
    await page.getByTestId('unprotect-ok').click();
    await expect(message(page)).toContainText('not correct');
    await closeMessage(page);
    await review(page, 'Unprotect Sheet');
    await page.getByTestId('unprotect-password').fill('pw');
    await page.getByTestId('unprotect-ok').click();
    await expect(ribbon(page).getByRole('button', { name: 'Protect Sheet' })).toBeVisible();
    await goTo(page, 'A1');
    await page.keyboard.type('2');
    await page.keyboard.press('Enter');
    await shows(page, 1, 1).toBe('2');
  });

  test('Protect Workbook locks the sheets, and a sheet can keep its cells from being selected', async ({
    page,
  }) => {
    await review(page, 'Protect Workbook');
    const dialog = page.getByTestId('protect-workbook-dialog');
    await expect(dialog.getByTestId('protect-structure')).toBeChecked();
    await expect(dialog.getByTestId('protect-windows')).toBeDisabled();
    await dialog.getByTestId('protect-workbook-ok').click();
    // Greyed, as Excel greys it; the ribbon's Insert Sheet is refused as Excel refuses it.
    await expect(page.getByTestId('add-sheet')).toBeDisabled();
    await homeMenu(page, 'Insert', 'Insert Sheet');
    await expect(message(page)).toContainText('Workbook is protected');
    await closeMessage(page);
    await expect(tab(page, 1)).toHaveCount(0);
    expect(await savedPart(page, 'xl/workbook.xml')).toContain(
      '<workbookProtection lockStructure="1"/>',
    );
    await review(page, 'Protect Workbook');
    await page.getByTestId('add-sheet').click();
    await expect(tab(page, 1)).toBeVisible();
    // Only unlocked cells selectable: Tab goes from one to the next, as in Excel.
    await tab(page, 0).click();
    for (const col of [3, 5]) {
      await page.evaluate(
        (c) =>
          window.cgsTest.command({
            kind: 'format',
            sheet: 0,
            areas: [{ top: 1, left: c, bottom: 1, right: c }],
            change: { locked: false },
          }),
        col,
      );
    }
    await goTo(page, 'C1');
    await page.evaluate(() =>
      window.cgsTest.command({ kind: 'protectSheet', sheet: 0, allow: { selectLocked: false } }),
    );
    await canvas(page).focus();
    await page.keyboard.press('Tab');
    await expect(nameBox(page)).toHaveValue('E1');
    await page.keyboard.press('Tab');
    await expect(nameBox(page)).toHaveValue('C1');
  });
});

test.describe('notes and links', () => {
  const COMMENTS = /^xl\/comments\d*\.xml$/;
  const VML = /\.vml$/;
  const reviewButton = async (page: Page, name: string) => {
    await ribbon(page).getByRole('tab', { name: 'Review', exact: true }).click();
    await ribbon(page).getByRole('button', { name, exact: true }).click();
  };

  test('a note is written in a box beside its cell, by the user name, and can show all the time', async ({
    page,
  }) => {
    await page.getByRole('menuitem', { name: 'File', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Options…' }).click();
    await page.getByTestId('options-user-name').fill('Ann Lee');
    await page.getByTestId('options-dialog').getByRole('button', { name: 'OK' }).click();
    await goTo(page, 'B2');
    await reviewButton(page, 'New Note');
    const editor = page.getByTestId('note-editor');
    await expect(editor).toBeFocused();
    await expect(editor).toHaveValue('Ann Lee:\n');
    // Beside the cell, to its right, as Excel puts it.
    const cell = await page.evaluate(() => window.cgsTest.grid()!.boxForCell(1, 1));
    const canvasBox = (await canvas(page).boundingBox())!;
    const box = (await editor.boundingBox())!;
    expect(box.x).toBeGreaterThan(canvasBox.x + cell.x + cell.width);
    expect(Math.round(box.width)).toBe(144);
    expect(Math.round(box.height)).toBe(79);
    await page.keyboard.type('check this');
    await page.keyboard.press('Escape');
    await expect(editor).toHaveCount(0);
    await expect.poll(() => savedPart(page, COMMENTS)).toContain('Ann Lee');
    expect(await savedPart(page, COMMENTS)).toContain('check this');
    // Hidden until hovered; Show/Hide Note keeps it showing, and saves it so.
    await expect(page.getByTestId('note-box-2-2')).toHaveCount(0);
    await reviewButton(page, 'Show/Hide Note');
    await expect(page.getByTestId('note-box-2-2')).toContainText('check this');
    const vml = await savedPart(page, VML);
    expect(vml).toContain('<x:Visible/>');
    expect(vml).toContain('<x:Anchor>2, 15, 0, 10, 4, 31, 4, 9</x:Anchor>');
    // A click in the box edits it.
    await page.getByTestId('note-box-2-2').click();
    await expect(page.getByTestId('note-editor')).toBeFocused();
    await page.keyboard.type('!');
    await canvas(page).click({ position: { x: 600, y: 400 } });
    await expect(page.getByTestId('note-box-2-2')).toContainText('check this!');
    await reviewButton(page, 'Show/Hide Note');
    // Show All Notes shows every note, without saving that, as in Excel.
    await goTo(page, 'D6');
    await reviewButton(page, 'New Note');
    await expect(editor).toBeFocused();
    await expect(editor).toHaveValue('Ann Lee:\n');
    await page.keyboard.type('second');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('note-box-6-4')).toHaveCount(0);
    await reviewButton(page, 'Show All Notes');
    await expect(page.getByTestId('note-box-2-2')).toBeVisible();
    await expect(page.getByTestId('note-box-6-4')).toContainText('second');
    expect(await savedPart(page, VML)).not.toContain('<x:Visible/>');
    await reviewButton(page, 'Show All Notes');
    await expect(page.getByTestId('note-box-6-4')).toHaveCount(0);
  });

  test('Remove Hyperlink takes the link and its look off, from the cell menu', async ({ page }) => {
    await page.evaluate(() =>
      window.cgsTest.command({
        kind: 'setLink',
        sheet: 0,
        area: { top: 1, left: 1, bottom: 1, right: 1 },
        link: { address: 'https://example.com/', text: 'site' },
      }),
    );
    await goTo(page, 'A1');
    await expect
      .poll(() => page.evaluate(() => window.cgsTest.cellFormat(1, 1)))
      .toMatchObject({
        underline: 'single',
      });
    const point = await page.evaluate(() => window.cgsTest.grid()!.pointForCell(0, 0));
    const bounds = (await canvas(page).boundingBox())!;
    await page.mouse.click(bounds.x + point.x, bounds.y + point.y, { button: 'right' });
    await page.getByRole('menuitem', { name: 'Remove Hyperlink' }).click();
    await expect
      .poll(() => page.evaluate(() => window.cgsTest.cellFormat(1, 1)))
      .toMatchObject({
        underline: 'none',
      });
    await shows(page, 1, 1).toBe('site');
    expect(await savedPart(page, 'xl/worksheets/sheet1.xml')).not.toContain('hyperlink');
  });
});

test.describe('sort and filter by colour', () => {
  /** N with yellow and red fills and an id beside each row, as Excel 2016 was measured on. */
  async function colored(page: Page) {
    await page.evaluate(async () => {
      const values = [5, 1, 8, 3, 9, 2, 7, 4];
      const input = (row: number, col: number, text: string) =>
        window.cgsTest.command({ kind: 'input', sheet: 0, row, col, text });
      await input(1, 1, 'N');
      await input(1, 2, 'Id');
      for (const [at, value] of values.entries()) {
        await input(at + 2, 1, String(value));
        await input(at + 2, 2, String(at + 1));
      }
      const fill = (row: number, rgb: string) =>
        window.cgsTest.command({
          kind: 'format',
          sheet: 0,
          areas: [{ top: row, left: 1, bottom: row, right: 1 }],
          change: { fill: { pattern: 'solid', color: { kind: 'rgb', rgb } } },
        });
      await fill(3, 'FFFF00');
      await fill(6, 'FFFF00');
      await fill(8, 'FF0000');
      await window.cgsTest.settled();
    });
  }

  const ids = (page: Page) =>
    page.evaluate(async () => {
      const out: string[] = [];
      for (let row = 2; row <= 9; row++) out.push(await window.cgsTest.cellText(row, 2));
      return out.join(',');
    });

  async function openFilterMenu(page: Page) {
    // A click on the button just after its menu closed would only close it, as in Excel.
    await expect(page.getByTestId('filter-menu')).toHaveCount(0);
    await page.waitForTimeout(400);
    const box = await page.evaluate(() => window.cgsTest.grid()!.boxForCell(0, 0));
    const bounds = (await canvas(page).boundingBox())!;
    await page.mouse.click(bounds.x + box.x + box.width - 6, bounds.y + box.y + box.height - 6);
    await expect(page.getByTestId('filter-menu')).toBeVisible();
  }

  test('the filter menu sorts and filters by the colours its column has', async ({ page }) => {
    await colored(page);
    await goTo(page, 'A1');
    await ribbon(page).getByRole('tab', { name: 'Data', exact: true }).click();
    await ribbon(page).getByRole('button', { name: 'Filter', exact: true }).click();
    // Sort by Color: red on top, the rest as they were, as Excel 2016 sorted them.
    await openFilterMenu(page);
    await page.getByTestId('filter-sort-color').hover();
    const sortList = page.getByTestId('filter-sort-colors');
    await expect(sortList.getByTestId('color-fill-none')).toBeVisible();
    await sortList.getByTestId('color-fill-FF0000').click();
    await expect.poll(() => ids(page)).toBe('7,1,2,3,4,5,6,8');
    // Filter by Color: only the yellow rows, ids 2 and 5 as Excel showed them.
    await openFilterMenu(page);
    await page.getByTestId('filter-by-color').hover();
    await page.getByTestId('filter-filter-colors').getByTestId('color-fill-FFFF00').click();
    const shownRows = async () => {
      const out: string[] = [];
      for (let row = 2; row <= 9; row++) {
        const height = await page.evaluate(async (r) => {
          await window.cgsTest.settled();
          return window.cgsTest.grid()!.boxForCell(r - 1, 0).height;
        }, row);
        if (height > 0) out.push(await page.evaluate((r) => window.cgsTest.cellText(r, 2), row));
      }
      return out.join(',');
    };
    await expect.poll(shownRows).toBe('2,5');
    expect(await savedPart(page, 'xl/worksheets/sheet1.xml')).toMatch(
      /<filterColumn colId="0"><colorFilter dxfId="\d+"\/><\/filterColumn>/,
    );
  });
});

test.describe('error checking', () => {
  const typeIn = async (page: Page, cell: string, text: string) => {
    await goTo(page, cell);
    await page.keyboard.type(text);
    await page.keyboard.press('Enter');
  };

  test('a smart tag offers the fix, and Error Checking goes from cell to cell', async ({
    page,
  }) => {
    await typeIn(page, 'A1', "'456");
    await typeIn(page, 'C3', '=1/0');
    await goTo(page, 'A1');
    const tag = page.getByTestId('error-tag');
    await expect(tag).toHaveAttribute('title', 'Number Stored as Text');
    await tag.click();
    await page.getByTestId('error-tag-convert-to-number').click();
    await expect(tag).toHaveCount(0);
    await shows(page, 1, 1).toBe('456');
    // Error Checking finds C3 next, names its error as Excel does, and ends.
    await ribbon(page).getByRole('tab', { name: 'Formulas', exact: true }).click();
    await ribbon(page).getByRole('button', { name: 'Error Checking', exact: true }).click();
    const dialog = page.getByTestId('error-checking-dialog');
    await expect(dialog.getByTestId('error-checking-cell')).toHaveText('Error in cell C3');
    await expect(dialog.getByTestId('error-checking-title')).toHaveText('Divide by Zero Error');
    await expect(nameBox(page)).toHaveValue('C3');
    await dialog.getByTestId('error-checking-ignore').click();
    await expect(dialog.getByTestId('error-checking-done')).toBeVisible();
    await dialog.getByTestId('error-checking-ok').click();
    expect(await savedPart(page, 'xl/worksheets/sheet1.xml')).toContain(
      '<ignoredErrors><ignoredError sqref="C3" evalError="1"/></ignoredErrors>',
    );
    // Turned off in Options, nothing is flagged.
    await typeIn(page, 'B5', "'789");
    await goTo(page, 'B5');
    await expect(tag).toBeVisible();
    await page.getByRole('menuitem', { name: 'File', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Options…' }).click();
    await page.getByTestId('options-background-checking').uncheck();
    await page.getByTestId('options-dialog').getByRole('button', { name: 'OK' }).click();
    await goTo(page, 'B4');
    await goTo(page, 'B5');
    await expect(tag).toHaveCount(0);
  });

  test('a formula that closes a loop gets Excel’s warning, and the status bar names it', async ({
    page,
  }) => {
    await typeIn(page, 'A1', '=B1+1');
    await expect(page.getByTestId('circular-status')).toHaveCount(0);
    await typeIn(page, 'B1', '=A1+1');
    await expect(page.getByTestId('protect-message-text')).toContainText(
      'There are one or more circular references',
    );
    await page.getByTestId('protect-message').getByRole('button', { name: 'OK' }).click();
    await expect(page.getByTestId('circular-status')).toHaveText('Circular References: A1');
    await typeIn(page, 'B1', '7');
    await expect(page.getByTestId('circular-status')).toHaveCount(0);
  });
});

test.describe('rich text in the cell', () => {
  test('characters selected while editing take a font of their own, and keep it when copied', async ({
    page,
    context,
  }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await goTo(page, 'A1');
    await page.keyboard.type('Bold plain');
    // F2 edits in the cell, where Shift and the arrows select characters.
    await page.keyboard.press('F2');
    await page.keyboard.press('Home');
    for (let at = 0; at < 4; at++) await page.keyboard.press('Shift+ArrowRight');
    await page.keyboard.press('Control+b');
    const rich = page.getByTestId('cell-editor-rich');
    await expect(rich).toBeVisible();
    await expect(rich.locator('span').first()).toHaveText('Bold');
    await expect(rich.locator('span').first()).toHaveCSS('font-weight', '700');
    await page.keyboard.press('Enter');
    await shows(page, 1, 1).toBe('Bold plain');
    const strings = await savedPart(page, 'xl/sharedStrings.xml');
    expect(strings).toContain(
      '<si><r><rPr><b/></rPr><t xml:space="preserve">Bold</t></r><r><t xml:space="preserve"> plain</t></r></si>',
    );
    // Edited again, the cell opens with its runs.
    await goTo(page, 'A1');
    await page.keyboard.press('F2');
    await expect(rich.locator('span')).toHaveCount(2);
    await page.keyboard.press('Escape');
    // Copied and pasted, the copy is rich too.
    await goTo(page, 'A1');
    await page.keyboard.press('Control+c');
    await goTo(page, 'C3');
    await page.keyboard.press('Control+v');
    await shows(page, 3, 3).toBe('Bold plain');
    // Its runs went with it: the saved strings hold the bold run twice.
    const copied = await savedPart(page, 'xl/sharedStrings.xml');
    expect(copied.match(/<rPr><b\/><\/rPr><t xml:space="preserve">Bold<\/t>/g)).toHaveLength(2);
  });
});

test.describe('cell styles', () => {
  const styles = async (page: Page) => {
    await ribbon(page).getByRole('tab', { name: 'Home', exact: true }).click();
    await page.getByTestId('cell-style-gallery').getByRole('button').first().click();
  };
  const format = (page: Page, row: number, col: number) =>
    expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellFormat(r!, c!), [row, col]));

  test('New Cell Style takes a cell by example, and Modify changes the cells linked to it', async ({
    page,
  }) => {
    await goTo(page, 'B2');
    await page.keyboard.type('1.5');
    await page.keyboard.press('Enter');
    await goTo(page, 'B2');
    await page.keyboard.press('Control+b');
    await format(page, 2, 2).toMatchObject({ bold: true });
    await styles(page);
    await page.getByTestId('cell-style-new').click();
    await expect(page.getByTestId('style-name')).toHaveValue('Style 1');
    await page.getByTestId('style-name').fill('Mine');
    await page.getByTestId('style-includes-number').uncheck();
    await page.getByTestId('style-ok').click();
    await expect(page.getByTestId('style-dialog')).toBeHidden();
    // The example keeps the Normal style, as in Excel; D4 takes Mine and keeps its own number.
    await goTo(page, 'D4');
    await page.keyboard.type('25%');
    await page.keyboard.press('Enter');
    await goTo(page, 'D4');
    await styles(page);
    await page.getByRole('gridcell', { name: 'Mine' }).click();
    await format(page, 4, 4).toMatchObject({ style: 'Mine', bold: true, numberFormat: '0%' });
    await format(page, 2, 2).toMatchObject({ style: 'Normal' });
    // Modify, from the style's right-click: Format… makes it italic, and D4 follows.
    await styles(page);
    await page.getByRole('gridcell', { name: 'Mine' }).click({ button: 'right' });
    await expect(page.getByTestId('style-name')).toHaveValue('Mine');
    await expect(page.getByTestId('style-includes-number')).not.toBeChecked();
    await page.getByTestId('style-format').click();
    await page.getByRole('tab', { name: 'Font', exact: true }).click();
    await page.getByTestId('font-dialog-style').selectOption('bi');
    await page.getByTestId('format-ok').click();
    await page.getByTestId('style-ok').click();
    await format(page, 4, 4).toMatchObject({ style: 'Mine', bold: true, italic: true });
    await format(page, 2, 2).toMatchObject({ italic: false });
    const saved = await savedPart(page, 'xl/styles.xml');
    expect(saved).toMatch(/<cellStyle name="Mine" xfId="\d+"\/>/);
    expect(saved).toMatch(/applyNumberFormat="0"[^>]*\/><\/cellStyleXfs>/);
  });
});
