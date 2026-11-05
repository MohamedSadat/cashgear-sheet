import { expect, test, type Page } from '@playwright/test';
import { differenceShare, FONT_OVERRIDE, gridPicture, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const editor = (page: Page) => page.getByTestId('cell-editor');
const bar = (page: Page) => page.getByTestId('formula-bar');
const nameBox = (page: Page) => page.getByTestId('name-box');

/** What a cell shows, 1-based, once commands sent so far have run. */
function shows(page: Page, row: number, col: number) {
  return expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]));
}

/** Clicks a cell, 0-based, by where the grid says it is. */
async function clickCell(page: Page, row: number, col: number) {
  const point = await page.evaluate(
    ([r, c]) => window.cgsTest.grid()!.pointForCell(r!, c!),
    [row, col],
  );
  const box = (await page.getByTestId('sheet-canvas').boundingBox())!;
  await page.mouse.click(box.x + point.x, box.y + point.y);
}

async function goTo(page: Page, name: string) {
  await nameBox(page).click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await expect(nameBox(page)).toHaveValue(name);
}

async function menu(page: Page, top: string, item: RegExp) {
  await page.getByRole('menuitem', { name: top, exact: true }).click();
  await page.getByRole('menuitem', { name: item }).click();
}

test.beforeEach(async ({ page, context, browserName }) => {
  void browserName;
  page.on('pageerror', (error) => {
    throw error;
  });
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(status(page)).toHaveText('Ready');
  await page.keyboard.press('Control+Alt+n');
  await waitForGrid(page);
  await expect(page.getByTestId('sheet-canvas')).toBeFocused();
});

test.describe('typing', () => {
  test('a typed value and a formula calculate, and the sheet shows it', async ({ page }) => {
    const before = await gridPicture(page);
    await page.keyboard.type('41');
    await expect(status(page)).toHaveText('Enter');
    await expect(editor(page)).toHaveValue('41');
    await expect(bar(page)).toHaveValue('41');
    await page.keyboard.press('Enter');
    await expect(nameBox(page)).toHaveValue('A2');
    await expect(status(page)).toHaveText('Ready');
    await page.keyboard.type('=A1+1');
    await page.keyboard.press('Tab');
    await expect(nameBox(page)).toHaveValue('B2');
    await shows(page, 1, 1).toBe('41');
    await shows(page, 2, 1).toBe('42');
    await waitForGrid(page);
    await expect
      .poll(async () => differenceShare(before, await gridPicture(page)))
      .toBeGreaterThan(0);
    await goTo(page, 'A2');
    await expect(bar(page)).toHaveValue('=A1+1');
  });

  test('a change reaches the cells that depend on it, on the picture too', async ({ page }) => {
    await page.keyboard.type('1\n=A1*2\n');
    await shows(page, 2, 1).toBe('2');
    await waitForGrid(page);
    const before = await gridPicture(page);
    await goTo(page, 'A1');
    await page.keyboard.type('500\n');
    await shows(page, 2, 1).toBe('1000');
    await expect
      .poll(async () => {
        await waitForGrid(page);
        return differenceShare(before, await gridPicture(page));
      })
      .toBeGreaterThan(0);
  });

  test('keys typed quickly all land where they should', async ({ page }) => {
    await page.keyboard.type('1\n2\n3\n4\n5\n=SUM(A1:A5)\n');
    await shows(page, 6, 1).toBe('15');
    await expect(nameBox(page)).toHaveValue('A7');
  });

  test('Esc leaves the cell as it was', async ({ page }) => {
    await page.keyboard.type('7\n');
    await goTo(page, 'A1');
    await page.keyboard.type('99');
    await page.keyboard.press('Escape');
    await expect(status(page)).toHaveText('Ready');
    await expect(page.getByTestId('sheet-canvas')).toBeFocused();
    await shows(page, 1, 1).toBe('7');
  });

  test('F2 and the formula bar edit what the cell holds', async ({ page }) => {
    await page.keyboard.type('=1+1\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('F2');
    await expect(status(page)).toHaveText('Edit');
    await expect(editor(page)).toHaveValue('=1+1');
    await page.keyboard.type('0');
    await page.keyboard.press('Enter');
    await shows(page, 1, 1).toBe('11');
    await goTo(page, 'A1');
    await bar(page).click();
    await expect(status(page)).toHaveText('Edit');
    await page.keyboard.press('Control+a');
    await page.keyboard.type('=2+2');
    await page.keyboard.press('Enter');
    await shows(page, 1, 1).toBe('4');
  });

  test('a formula that makes no sense stays open, with the reason', async ({ page }) => {
    await page.keyboard.type('=1+*');
    await page.keyboard.press('Enter');
    await expect(status(page)).not.toHaveText('Ready');
    await expect(editor(page)).toBeFocused();
    await expect(editor(page)).toHaveValue('=1+*');
    await expect(nameBox(page)).toHaveValue('A1');
  });

  test('Ctrl+Enter fills the selection', async ({ page }) => {
    await goTo(page, 'B2:C3');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('5');
    await page.keyboard.press('Control+Enter');
    await shows(page, 3, 3).toBe('5');
    await shows(page, 2, 2).toBe('5');
    await shows(page, 1, 1).toBe('');
  });

  test('Backspace starts over, and Delete clears', async ({ page }) => {
    await page.keyboard.type('12\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Delete');
    await shows(page, 1, 1).toBe('');
    await page.keyboard.press('Control+z');
    await shows(page, 1, 1).toBe('12');
    await page.keyboard.press('Backspace');
    await expect(editor(page)).toHaveValue('');
    await page.keyboard.type('3');
    await page.keyboard.press('Enter');
    await shows(page, 1, 1).toBe('3');
  });
});

test.describe('pointing at cells', () => {
  test('a click puts a reference in, and a drag makes a range', async ({ page }) => {
    await page.keyboard.type('1\n2\n3\n');
    await goTo(page, 'C1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('=SUM(');
    await clickCell(page, 0, 0);
    await expect(editor(page)).toHaveValue('=SUM(A1');
    await expect(status(page)).toHaveText('Point');
    const first = await page.evaluate(() => window.cgsTest.grid()!.pointForCell(0, 0));
    const last = await page.evaluate(() => window.cgsTest.grid()!.pointForCell(2, 0));
    const box = (await page.getByTestId('sheet-canvas').boundingBox())!;
    await page.mouse.move(box.x + first.x, box.y + first.y);
    await page.mouse.down();
    await page.mouse.move(box.x + last.x, box.y + last.y);
    await page.mouse.up();
    await expect(editor(page)).toHaveValue('=SUM(A1:A3');
    await page.keyboard.press('Enter');
    await shows(page, 1, 3).toBe('6');
  });

  test('arrow keys point too, and F4 makes it absolute', async ({ page }) => {
    await goTo(page, 'B2');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('=');
    await page.keyboard.press('ArrowUp');
    await expect(editor(page)).toHaveValue('=B1');
    await page.keyboard.press('ArrowLeft');
    await expect(editor(page)).toHaveValue('=A1');
    await page.keyboard.press('Shift+ArrowDown');
    await expect(editor(page)).toHaveValue('=A1:A2');
    await page.keyboard.press('F4');
    await expect(editor(page)).toHaveValue('=$A$1:$A$2');
  });

  test('a click with nothing to point at leaves the cell', async ({ page }) => {
    await page.keyboard.type('hello');
    await clickCell(page, 4, 4);
    await expect(nameBox(page)).toHaveValue('E5');
    await shows(page, 1, 1).toBe('hello');
  });

  test('a formula can point at another sheet', async ({ page }) => {
    await page.keyboard.type('5\n');
    await page.getByTestId('add-sheet').click();
    await expect(page.getByTestId('sheet-tab-1')).toBeVisible();
    await expect(nameBox(page)).toHaveValue('A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('=');
    await page.getByTestId('sheet-tab-0').click();
    await clickCell(page, 0, 0);
    await expect(editor(page)).toHaveValue('=Sheet1!A1');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('sheet-tab-1')).toHaveAttribute('aria-selected', 'true');
    await expect
      .poll(() =>
        page.evaluate(() =>
          window.cgsTest.command({ kind: 'undo' }).then(() => window.cgsTest.problem()),
        ),
      )
      .toBeNull();
  });

  test('function names complete', async ({ page }) => {
    await page.keyboard.type('=SU');
    await expect(page.getByTestId('completions')).toBeVisible();
    await expect(page.getByRole('option', { name: 'SUM', exact: true })).toBeVisible();
    await page.keyboard.type('MI');
    await expect(page.getByRole('option', { name: 'SUMIF', exact: true })).toBeVisible();
    await page.keyboard.press('Tab');
    await expect(editor(page)).toHaveValue(/^=SUMIF[A-Z]*\($/);
    await expect(page.getByTestId('completions')).toHaveCount(0);
  });
});

test.describe('undo', () => {
  test('goes back and forward, from the keyboard and the menu', async ({ page }) => {
    await page.keyboard.type('1\n2\n');
    await page.keyboard.press('Control+z');
    await shows(page, 2, 1).toBe('');
    await page.keyboard.press('Control+z');
    await shows(page, 1, 1).toBe('');
    await page.keyboard.press('Control+y');
    await shows(page, 1, 1).toBe('1');
    await page.getByRole('menuitem', { name: 'Edit', exact: true }).click();
    await expect(page.getByRole('menuitem', { name: /^Redo/ })).toBeEnabled();
    await page.getByRole('menuitem', { name: /^Redo/ }).click();
    await shows(page, 2, 1).toBe('2');
  });

  test('brings back formulas that a delete turned into #REF!', async ({ page }) => {
    await page.keyboard.type('10\n=A1*2\n');
    await goTo(page, '1:1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+-');
    await shows(page, 1, 1).toBe('#REF!');
    await page.keyboard.press('Control+z');
    await shows(page, 2, 1).toBe('20');
    await goTo(page, 'A2');
    await expect(bar(page)).toHaveValue('=A1*2');
  });

  test('undoing every edit leaves nothing to save', async ({ page }) => {
    await page.evaluate(() => window.cgsTest.hasChanges());
    await page.keyboard.type('1\n');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
    await page.keyboard.press('Control+z');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
  });
});

test.describe('fill', () => {
  test('dragging the handle carries on a series', async ({ page }) => {
    await page.keyboard.type('1\n2\n');
    await goTo(page, 'A1:A2');
    const corner = await page.evaluate(() => window.cgsTest.grid()!.pointForCell(2, 0));
    const end = await page.evaluate(() => window.cgsTest.grid()!.pointForCell(4, 0));
    const box = (await page.getByTestId('sheet-canvas').boundingBox())!;
    // `pointForCell` is a cell's middle, so the handle is half a cell above and to the left.
    const cell = await page.evaluate(() => {
      const grid = window.cgsTest.grid()!;
      const a = grid.pointForCell(0, 0);
      const b = grid.pointForCell(1, 1);
      return { width: b.x - a.x, height: b.y - a.y };
    });
    await page.mouse.move(box.x + corner.x + cell.width / 2, box.y + corner.y - cell.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + end.x, box.y + end.y, { steps: 4 });
    await page.mouse.up();
    await shows(page, 5, 1).toBe('5');
    await shows(page, 3, 1).toBe('3');
    await expect(nameBox(page)).toHaveValue('A1:A5');
    await page.keyboard.press('Control+z');
    await shows(page, 5, 1).toBe('');
  });

  test('Ctrl+D and Ctrl+R copy', async ({ page }) => {
    await page.keyboard.type('=ROW()\n');
    await goTo(page, 'A1:A3');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+d');
    await shows(page, 3, 1).toBe('3');
    await goTo(page, 'B1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+r');
    await shows(page, 1, 2).toBe('1');
  });
});

test.describe('clipboard', () => {
  test('copy and paste within the sheet moves references along', async ({ page }) => {
    await page.keyboard.type('1\n=A1+1\n');
    await goTo(page, 'A2');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+c');
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain('2');
    await goTo(page, 'A3');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+v');
    await shows(page, 3, 1).toBe('3');
    await expect(bar(page)).toHaveValue('=A2+1');
  });

  test('cut moves, once', async ({ page }) => {
    await page.keyboard.type('8\n=A1\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+x');
    await goTo(page, 'C3');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+v');
    await shows(page, 3, 3).toBe('8');
    await shows(page, 1, 1).toBe('');
    await goTo(page, 'A2');
    await expect(bar(page)).toHaveValue('=C3');
  });

  /** What another program's copy looks like: a paste event with its own data. */
  async function pasteFromElsewhere(page: Page, text: string, html?: string) {
    await page.evaluate(
      ({ text, html }) => {
        const data = new DataTransfer();
        data.setData('text/plain', text);
        if (html) data.setData('text/html', html);
        const target = document.querySelector('[data-testid="sheet-canvas"]')!;
        target.dispatchEvent(
          new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
        );
      },
      { text, html },
    );
  }

  test('tab-separated text from elsewhere', async ({ page }) => {
    await page.getByTestId('sheet-canvas').focus();
    await pasteFromElsewhere(page, 'a\t1\r\nb\t2\r\n');
    await shows(page, 2, 1).toBe('b');
    await shows(page, 2, 2).toBe('2');
  });

  test('a table from Excel, with its formats', async ({ page }) => {
    await page.getByTestId('sheet-canvas').focus();
    const html = `<html><head><style>.x1{font-weight:700;mso-number-format:"0\\.00";}</style></head>
      <body><table><!--StartFragment--><tr><td class=x1 x:num="1.5">1.50</td><td>text</td></tr>
      <!--EndFragment--></table></body></html>`;
    await pasteFromElsewhere(page, '1.50\ttext\r\n', html);
    await shows(page, 1, 1).toBe('1.50');
    await shows(page, 1, 2).toBe('text');
    await goTo(page, 'A1');
    await expect(bar(page)).toHaveValue('1.5');
  });
});

test.describe('structure', () => {
  test('inserting and deleting rows and columns moves formulas', async ({ page }) => {
    await page.keyboard.type('5\n=A1*3\n');
    await goTo(page, '1:1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+Shift+=');
    await shows(page, 3, 1).toBe('15');
    await goTo(page, 'A3');
    await expect(bar(page)).toHaveValue('=A2*3');
    await goTo(page, 'A:A');
    await menu(page, 'Insert', /^Columns/);
    await shows(page, 3, 2).toBe('15');
    await goTo(page, 'B3');
    await expect(bar(page)).toHaveValue('=B2*3');
    await goTo(page, 'A:A');
    await menu(page, 'Insert', /^Delete columns/);
    await shows(page, 3, 1).toBe('15');
    await goTo(page, '1:1');
    await menu(page, 'Insert', /^Delete rows/);
    await shows(page, 2, 1).toBe('15');
  });

  test('a part of a selection that is not whole rows says so', async ({ page }) => {
    await page.keyboard.press('Control+-');
    await expect(status(page)).toHaveText(/whole rows or columns/);
    await page.keyboard.press('Escape');
    await expect(status(page)).toHaveText('Ready');
  });

  test('sheets are added, renamed and deleted, and formulas follow', async ({ page }) => {
    await page.keyboard.type('6\n');
    await page.getByTestId('add-sheet').click();
    await expect(page.getByTestId('sheet-tab-1')).toHaveText(/Sheet2/);
    await expect(page.getByTestId('sheet-tab-1')).toHaveAttribute('aria-selected', 'true');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('=Sheet1!A1*2\n');
    await shows(page, 1, 1).toBe('12');
    await page.getByTestId('sheet-tab-0').dblclick();
    await page.getByTestId('sheet-name').fill('My Data');
    await page.getByRole('button', { name: 'Rename' }).click();
    await expect(page.getByTestId('sheet-tab-0')).toHaveText(/My Data/);
    await page.getByTestId('sheet-tab-1').click();
    await goTo(page, 'A1');
    await expect(bar(page)).toHaveValue("='My Data'!A1*2");
    await page.getByTestId('sheet-tab-0').click();
    await menu(page, 'Sheet', /^Delete sheet/);
    await expect(page.getByTestId('sheet-tab-0')).toHaveCount(0);
    await shows(page, 1, 1).toBe('#REF!');
    await page.keyboard.press('Control+z');
    await expect(page.getByTestId('sheet-tab-0')).toHaveText(/My Data/);
    await page.getByTestId('sheet-canvas').focus();
    await page.getByTestId('sheet-tab-1').click();
    await shows(page, 1, 1).toBe('12');
  });

  test('the last sheet stays', async ({ page }) => {
    await menu(page, 'Sheet', /^Delete sheet/);
    await expect(status(page)).not.toHaveText('Ready');
    await expect(page.getByTestId('sheet-tab-0')).toBeVisible();
  });
});

test.describe('moving around', () => {
  test('the name box goes to cells and names', async ({ page }) => {
    await goTo(page, 'C5');
    await nameBox(page).click();
    await page.keyboard.type('not a place');
    await page.keyboard.press('Enter');
    await expect(status(page)).toHaveText(/isn.t a cell/);
    await expect(nameBox(page)).toHaveValue('C5');
  });

  test('Ctrl+arrow stops at the edges of what has cells', async ({ page }) => {
    await page.keyboard.type('1\n2\n3\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+ArrowDown');
    await expect(nameBox(page)).toHaveValue('A3');
    await page.keyboard.press('Control+Shift+ArrowUp');
    await expect(nameBox(page)).toHaveValue('A1:A3');
    await page.keyboard.press('Control+Space');
    await expect(nameBox(page)).toHaveValue('A:A');
    await page.keyboard.press('Shift+Space');
    await expect(nameBox(page)).toHaveValue('A1:XFD1048576');
  });

  test('Enter goes round a selection', async ({ page }) => {
    await goTo(page, 'A1:B2');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter');
    await page.keyboard.type('x');
    await page.keyboard.press('Enter');
    await shows(page, 1, 2).toBe('x');
  });

  test('Alt+= sums what is above', async ({ page }) => {
    await page.keyboard.type('2\n3\n');
    await page.keyboard.press('Alt+=');
    await expect(editor(page)).toHaveValue('=SUM(A1:A2)');
    await page.keyboard.press('Enter');
    await shows(page, 3, 1).toBe('5');
  });
});

test.describe('calculation', () => {
  test('manual calculation waits for F9', async ({ page }) => {
    await page.keyboard.type('1\n=A1+1\n');
    await shows(page, 2, 1).toBe('2');
    await page.getByRole('menuitem', { name: 'Formulas', exact: true }).click();
    await page.getByRole('menuitemcheckbox', { name: /Manual/ }).click();
    await goTo(page, 'A1');
    await page.keyboard.type('10\n');
    await shows(page, 1, 1).toBe('10');
    await expect(page.getByTestId('calc-status')).toHaveText('Calculate');
    await shows(page, 2, 1).toBe('2');
    await page.keyboard.press('F9');
    await shows(page, 2, 1).toBe('11');
    await expect(page.getByTestId('calc-status')).toHaveCount(0);
  });
});

test.describe('CSV', () => {
  test('opens as a new workbook, and exports again', async ({ page }) => {
    await page.evaluate(async () => {
      const text = 'name,amount\r\nali,12\r\n"a,b",=1+1\r\n';
      await window.cgsTest.open({
        name: 'money.csv',
        source: { kind: 'blob', blob: new Blob([text]) },
      });
    });
    await expect(page.getByRole('heading', { name: 'money.xlsx' })).toBeVisible();
    await shows(page, 3, 1).toBe('a,b');
    await shows(page, 2, 2).toBe('12');
    await shows(page, 3, 2).toBe('2');
    // What the file brought isn't an edit that undo would take away, but it does need saving.
    expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+z');
    await shows(page, 2, 2).toBe('12');
    await page.evaluate(async () => {
      const root = await navigator.storage.getDirectory();
      const handle = await root.getFileHandle('out.csv', { create: true });
      (window as unknown as { showSaveFilePicker: () => Promise<unknown> }).showSaveFilePicker =
        async () => handle;
    });
    await menu(page, 'File', /^Export sheet as CSV/);
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const root = await navigator.storage.getDirectory();
          const handle = await root.getFileHandle('out.csv');
          return (await handle.getFile()).text();
        }),
      )
      .toContain('"a,b"');

    // Saving asks where, and the cells are in what it writes.
    await page.evaluate(async () => {
      const root = await navigator.storage.getDirectory();
      const handle = await root.getFileHandle('money.xlsx', { create: true });
      (window as unknown as { showSaveFilePicker: () => Promise<unknown> }).showSaveFilePicker =
        async () => handle;
    });
    await page.keyboard.press('Control+s');
    await expect(page.getByTestId('save-status')).toBeVisible();
    await page.evaluate(async () => {
      const root = await navigator.storage.getDirectory();
      const handle = await root.getFileHandle('money.xlsx');
      await window.cgsTest.open({
        name: 'money.xlsx',
        source: { kind: 'blob', blob: await handle.getFile() },
      });
    });
    await shows(page, 3, 1).toBe('a,b');
    await shows(page, 3, 2).toBe('2');
  });

  test('an edit on top comes back after a crash, along with the file', async ({ page }) => {
    await page.evaluate(async () => {
      await window.cgsTest.open({
        name: 'crash.csv',
        source: { kind: 'blob', blob: new Blob(['x,y\r\n1,2\r\n']) },
      });
    });
    await shows(page, 2, 2).toBe('2');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.type('edited\n');
    await shows(page, 1, 1).toBe('edited');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
    page.on('dialog', (dialog) => void dialog.accept());
    await page.reload();
    await page.getByRole('button', { name: 'Recover' }).click();
    await expect(page.getByRole('heading', { name: /recovered/ })).toBeVisible();
    await shows(page, 1, 1).toBe('edited');
    await shows(page, 2, 2).toBe('2');
  });
});

test.describe('recovery', () => {
  test('edits come back after the page goes away without saving', async ({ page }) => {
    await page.keyboard.type('123\n=A1*2\n');
    await shows(page, 2, 1).toBe('246');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
    // The worker's journal is written as commands run, so a reload loses nothing.
    await page.evaluate(() => window.removeEventListener('beforeunload', () => undefined));
    page.on('dialog', (dialog) => void dialog.accept());
    await page.reload();
    await expect(page.getByTestId('recover')).toBeVisible();
    await page.getByRole('button', { name: 'Recover' }).click();
    await expect(page.getByRole('heading', { name: /recovered/ })).toBeVisible();
    await shows(page, 1, 1).toBe('123');
    await shows(page, 2, 1).toBe('246');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
  });

  test('what was discarded stays gone', async ({ page }) => {
    await page.keyboard.type('5\n');
    await expect.poll(() => page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
    page.on('dialog', (dialog) => void dialog.accept());
    await page.reload();
    await expect(page.getByTestId('recover')).toBeVisible();
    await page.getByTestId('recover-discard').click();
    await expect(page.getByTestId('recover')).toHaveCount(0);
    await page.reload();
    await expect(status(page)).toHaveText('Ready');
    await page.waitForTimeout(300);
    await expect(page.getByTestId('recover')).toHaveCount(0);
  });
});
