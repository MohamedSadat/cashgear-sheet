import { expect, test, type Page } from '@playwright/test';
import { differenceShare, FONT_OVERRIDE, gridPicture, waitForGrid } from './helpers.mjs';

const status = (page: Page) => page.getByTestId('engine-status');
const nameBox = (page: Page) => page.getByTestId('name-box');
const ribbon = (page: Page) => page.getByTestId('ribbon');

/** How a cell is formatted, 1-based, once commands sent so far have run. */
function format(page: Page, row: number, col: number) {
  return page.evaluate(
    async ([r, c]) => {
      await window.cgsTest.settled();
      return window.cgsTest.cellFormat(r!, c!);
    },
    [row, col],
  );
}

const shows = (page: Page, row: number, col: number) =>
  expect.poll(() => page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]));

async function goTo(page: Page, name: string) {
  await nameBox(page).click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await expect(nameBox(page)).toHaveValue(name);
}

async function pointFor(page: Page, row: number, col: number) {
  const point = await page.evaluate(
    ([r, c]) => window.cgsTest.grid()!.pointForCell(r!, c!),
    [row, col],
  );
  const box = (await page.getByTestId('sheet-canvas').boundingBox())!;
  return { x: box.x + point.x, y: box.y + point.y, box };
}

/** Opens a ribbon tab, if it isn't showing. */
async function tab(page: Page, name: string) {
  await ribbon(page).getByRole('tab', { name, exact: true }).click();
}

async function press(page: Page, name: string | RegExp) {
  await ribbon(page).getByRole('button', { name }).first().click();
}

/** Opens a command's menu and picks from it. */
async function pick(page: Page, name: string, item: string) {
  await ribbon(page)
    .getByRole('button', { name: new RegExp('^' + name + '( ?▾)?$') })
    .first()
    .click();
  await page.getByRole('menuitem', { name: item, exact: true }).click();
}

test.beforeEach(async ({ page, context }) => {
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

test.describe('the ribbon', () => {
  test('shows only tabs with working commands', async ({ page }) => {
    const tabs = ribbon(page).getByRole('tab');
    await expect(tabs).toHaveText([
      'Home',
      'Insert',
      'Page Layout',
      'Formulas',
      'Data',
      'Review',
      'View',
    ]);
    await expect(ribbon(page).getByRole('menuitem', { name: 'File' })).toBeVisible();
  });

  test('bold, italic and underline toggle by button and key, and show on the ribbon', async ({
    page,
  }) => {
    await page.keyboard.type('text\n');
    await goTo(page, 'A1');
    const bold = ribbon(page).getByRole('button', { name: 'Bold' });
    await expect(bold).toHaveAttribute('aria-pressed', 'false');
    await bold.click();
    await expect.poll(async () => (await format(page, 1, 1))?.bold).toBe(true);
    await expect(bold).toHaveAttribute('aria-pressed', 'true');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+b');
    await expect.poll(async () => (await format(page, 1, 1))?.bold).toBe(false);
    await page.keyboard.press('Control+i');
    await expect.poll(async () => (await format(page, 1, 1))?.italic).toBe(true);
    await page.keyboard.press('Control+u');
    await expect.poll(async () => (await format(page, 1, 1))?.underline).toBe('single');
    await page.keyboard.press('Control+5');
    await expect.poll(async () => (await format(page, 1, 1))?.strike).toBe(true);
    await page.keyboard.press('Control+z');
    await expect.poll(async () => (await format(page, 1, 1))?.strike).toBe(false);
  });

  test('the font boxes take what is typed, and a bigger font makes the row taller', async ({
    page,
  }) => {
    await page.keyboard.type('big\n');
    await goTo(page, 'A1');
    await page.getByTestId('font-name').click();
    await page.keyboard.type('Arial');
    await page.keyboard.press('Enter');
    await expect.poll(async () => (await format(page, 1, 1))?.fontName).toBe('Arial');
    await page.getByTestId('font-size').click();
    await page.keyboard.type('24');
    await page.keyboard.press('Enter');
    await expect.poll(async () => (await format(page, 1, 1))?.fontSize).toBe(24);
    const first = await pointFor(page, 0, 0);
    const second = await pointFor(page, 1, 0);
    expect(second.y - first.y).toBeGreaterThan(25);
    await press(page, 'Increase Font Size');
    await expect.poll(async () => (await format(page, 1, 1))?.fontSize).toBe(26);
  });

  test('fill and font colours go on from their pickers, and again from the button', async ({
    page,
  }) => {
    await goTo(page, 'B2');
    const before = await gridPicture(page);
    await ribbon(page).getByRole('button', { name: 'Fill Color: more options' }).click();
    await page.getByRole('gridcell', { name: 'Accent 1', exact: true }).click();
    await expect
      .poll(async () => (await format(page, 2, 2))?.fill.color)
      .toMatchObject({ kind: 'theme', theme: 4 });
    await goTo(page, 'B3');
    await ribbon(page)
      .getByRole('button', { name: /Fill Color/ })
      .first()
      .click();
    await expect.poll(async () => (await format(page, 3, 2))?.fill.pattern).toBe('solid');
    await waitForGrid(page);
    await expect
      .poll(async () => differenceShare(before, await gridPicture(page)))
      .toBeGreaterThan(0);
  });

  test('borders go by where each cell sits in the selection', async ({ page }) => {
    await nameBox(page).click();
    await page.keyboard.type('B2:C3');
    await page.keyboard.press('Enter');
    await ribbon(page).getByRole('button', { name: 'Borders: more options' }).click();
    await page.getByRole('gridcell', { name: 'Outside borders', exact: true }).click();
    await expect.poll(async () => (await format(page, 2, 2))?.border.top.style).toBe('thin');
    const corner = await format(page, 2, 2);
    expect(corner?.border.left.style).toBe('thin');
    expect(corner?.border.right.style).toBe('none');
    const other = await format(page, 3, 3);
    expect(other?.border.bottom.style).toBe('thin');
    expect(other?.border.right.style).toBe('thin');
    expect(other?.border.top.style).toBe('none');
  });

  test('alignment, wrap, indent and number buttons', async ({ page }) => {
    await page.keyboard.type('1234.5\n');
    await goTo(page, 'A1');
    await press(page, 'Center');
    await expect.poll(async () => (await format(page, 1, 1))?.horizontal).toBe('center');
    await press(page, 'Center');
    await expect.poll(async () => (await format(page, 1, 1))?.horizontal).toBe('general');
    await press(page, 'Top Align');
    await expect.poll(async () => (await format(page, 1, 1))?.vertical).toBe('top');
    await press(page, 'Increase Indent');
    await expect.poll(async () => (await format(page, 1, 1))?.indent).toBe(1);
    await press(page, 'Percent Style');
    await shows(page, 1, 1).toBe('123450%');
    await press(page, 'Increase Decimal');
    await shows(page, 1, 1).toBe('123450.0%');
    await page.getByTestId('number-format').selectOption('Text');
    await expect.poll(async () => (await format(page, 1, 1))?.numberFormat).toBe('@');
    await page.getByTestId('number-format').selectOption('General');
    await press(page, 'Comma Style');
    await expect.poll(async () => (await format(page, 1, 1))?.numberFormatId).toBe(43);
    await press(page, 'Wrap Text');
    await expect.poll(async () => (await format(page, 1, 1))?.wrap).toBe(true);
  });

  test('Ctrl+Shift number formats', async ({ page }) => {
    await page.keyboard.type('0.5\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+Shift+%');
    await shows(page, 1, 1).toBe('50%');
    await page.keyboard.press('Control+Shift+~');
    await shows(page, 1, 1).toBe('0.5');
    await page.keyboard.press('Control+Shift+!');
    await shows(page, 1, 1).toBe('0.50');
    await page.keyboard.press('Control+Shift+^');
    await shows(page, 1, 1).toBe('5.00E-01');
  });
});

test.describe('Format Cells', () => {
  test('a custom number format shows its sample, and a bad one can not be used', async ({
    page,
  }) => {
    await page.keyboard.type('5\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+1');
    const dialog = page.getByTestId('format-cells');
    await expect(dialog).toBeVisible();
    await page.getByTestId('number-category').selectOption('Custom');
    await page.getByTestId('number-custom').fill('0.000" kg"');
    await expect(page.getByTestId('number-sample')).toContainText('5.000 kg');
    await page.getByTestId('number-custom').fill('0.0.0[[');
    await expect(page.getByTestId('number-sample')).toContainText('can’t be used');
    await expect(page.getByTestId('format-ok')).toBeDisabled();
    await page.getByTestId('number-custom').fill('0.000" kg"');
    await page.getByTestId('format-ok').click();
    await shows(page, 1, 1).toBe('5.000 kg');
  });

  test('each tab changes only what was changed on it', async ({ page }) => {
    await nameBox(page).click();
    await page.keyboard.type('A1:A2');
    await page.keyboard.press('Enter');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+b');
    await expect.poll(async () => (await format(page, 2, 1))?.bold).toBe(true);
    await page.keyboard.press('Control+1');
    await page.getByRole('tab', { name: 'Alignment' }).click();
    await page.getByTestId('align-horizontal').selectOption('right');
    await page.getByTestId('align-indent').fill('2');
    await page.getByTestId('align-rotation').fill('45');
    await page.getByRole('tab', { name: 'Font', exact: true }).click();
    await page.getByTestId('font-dialog-size').fill('14');
    await page.getByTestId('font-dialog-underline').selectOption('double');
    await page.getByTestId('font-dialog-strike').check();
    await page.getByRole('tab', { name: 'Border' }).click();
    await page.getByTestId('border-style').selectOption('double');
    await page.getByTestId('border-outline').click();
    await page.getByTestId('border-diagonalDown').click();
    await page.getByRole('tab', { name: 'Fill' }).click();
    await page.getByTestId('fill-pattern').selectOption('darkUp');
    await page.getByTestId('format-ok').click();
    await expect.poll(async () => (await format(page, 1, 1))?.fontSize).toBe(14);
    const cell = (await format(page, 1, 1))!;
    expect(cell.bold).toBe(true);
    expect(cell.horizontal).toBe('right');
    expect(cell.indent).toBe(2);
    expect(cell.rotation).toBe(45);
    expect(cell.underline).toBe('double');
    expect(cell.strike).toBe(true);
    expect(cell.border.top.style).toBe('double');
    expect(cell.border.diagonal.style).toBe('double');
    expect(cell.border.diagonalDown).toBe(true);
    expect(cell.fill.pattern).toBe('darkUp');
  });
});

test.describe('the format painter', () => {
  test('paints once, and leaves values alone', async ({ page }) => {
    await page.keyboard.type('one\ntwo\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+b');
    await expect.poll(async () => (await format(page, 1, 1))?.bold).toBe(true);
    const painter = ribbon(page).getByRole('button', { name: 'Format Painter' });
    await painter.click();
    await expect(painter).toHaveAttribute('aria-pressed', 'true');
    const target = await pointFor(page, 1, 0);
    await page.mouse.click(target.x, target.y);
    await expect.poll(async () => (await format(page, 2, 1))?.bold).toBe(true);
    await shows(page, 2, 1).toBe('two');
    await expect(painter).toHaveAttribute('aria-pressed', 'false');
    const later = await pointFor(page, 2, 0);
    await page.mouse.click(later.x, later.y);
    await page.waitForTimeout(100);
    expect((await format(page, 3, 1))?.bold).toBe(false);
  });

  test('Escape puts it down', async ({ page }) => {
    await page.keyboard.press('Control+b');
    const painter = ribbon(page).getByRole('button', { name: 'Format Painter' });
    await painter.click();
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Escape');
    await expect(painter).toHaveAttribute('aria-pressed', 'false');
  });
});

test.describe('merging', () => {
  test('warns before throwing cells away, centres, and comes apart', async ({ page }) => {
    await page.keyboard.type('a');
    await page.keyboard.press('Tab');
    await page.keyboard.type('b');
    await page.keyboard.press('Enter');
    await nameBox(page).click();
    await page.keyboard.type('A1:C1');
    await page.keyboard.press('Enter');
    await press(page, 'Merge & Center');
    await expect(page.getByTestId('merge-dialog')).toBeVisible();
    await page.getByRole('button', { name: 'OK' }).click();
    await expect.poll(async () => (await format(page, 1, 1))?.merged).toBe(true);
    await shows(page, 1, 2).toBe('');
    expect((await format(page, 1, 1))?.horizontal).toBe('center');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+z');
    await expect.poll(async () => (await format(page, 1, 1))?.merged).toBe(false);
    await shows(page, 1, 2).toBe('b');
    await page.keyboard.press('Control+y');
    await expect.poll(async () => (await format(page, 1, 1))?.merged).toBe(true);
    await goTo(page, 'A1');
    await press(page, 'Merge & Center');
    await expect.poll(async () => (await format(page, 1, 1))?.merged).toBe(false);
  });
});

test.describe('sizes', () => {
  test('dragging a column edge resizes it, and a double click fits it', async ({ page }) => {
    await page.keyboard.type('a long piece of text here\n');
    const box = (await page.getByTestId('sheet-canvas').boundingBox())!;
    const a = await pointFor(page, 0, 0);
    const b = await pointFor(page, 0, 1);
    const edge = (a.x + b.x) / 2;
    const header = box.y + 10;
    await page.mouse.move(edge, header);
    await page.mouse.down();
    await page.mouse.move(edge + 40, header, { steps: 3 });
    await expect(page.getByTestId('resize-hint')).toContainText('Width');
    await page.mouse.up();
    await expect.poll(async () => (await pointFor(page, 0, 1)).x - b.x).toBeGreaterThan(35);
    const dragged = (await pointFor(page, 0, 1)).x;
    await page.mouse.dblclick(edge + 40, header);
    await expect.poll(async () => (await pointFor(page, 0, 1)).x - dragged).toBeGreaterThan(20);
    await page.keyboard.press('Control+z');
    await expect.poll(async () => (await pointFor(page, 0, 1)).x).toBe(dragged);
  });

  test('a row with wrapped text refits by itself, unless someone set its height', async ({
    page,
  }) => {
    await page.keyboard.type('some words that will have to wrap onto several lines here\n');
    await goTo(page, 'A1');
    const row2 = (await pointFor(page, 1, 0)).y;
    await press(page, 'Wrap Text');
    await expect.poll(async () => (await pointFor(page, 1, 0)).y - row2).toBeGreaterThan(30);
    await page.keyboard.press('Control+z');
    await expect.poll(async () => (await pointFor(page, 1, 0)).y).toBe(row2);
    await pick(page, 'Format', 'Row Height…');
    await page.getByTestId('size-value').fill('30');
    await page.getByRole('button', { name: 'OK' }).click();
    await expect.poll(async () => (await pointFor(page, 1, 0)).y - row2).toBe(10);
    await press(page, 'Wrap Text');
    await page.waitForTimeout(200);
    expect((await pointFor(page, 1, 0)).y - row2).toBe(10);
  });

  test('rows and columns hide and come back, by key and by menu', async ({ page }) => {
    await page.keyboard.type('1\n2\n3\n');
    await goTo(page, 'A2');
    const below = (await pointFor(page, 2, 0)).y;
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+9');
    await expect.poll(async () => (await pointFor(page, 2, 0)).y).toBeLessThan(below);
    await nameBox(page).click();
    await page.keyboard.type('A1:A3');
    await page.keyboard.press('Enter');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+Shift+(');
    await expect.poll(async () => (await pointFor(page, 2, 0)).y).toBe(below);
    const right = (await pointFor(page, 0, 2)).x;
    const b = await pointFor(page, 0, 1);
    const box = b.box;
    await page.mouse.click(b.x, box.y + 10, { button: 'right' });
    await page.getByRole('menuitem', { name: /^Hide Columns/ }).click();
    await expect.poll(async () => (await pointFor(page, 0, 2)).x).toBeLessThan(right);
  });
});

test.describe('freezing', () => {
  test('panes hold still while the rest scrolls, and let go', async ({ page }) => {
    await goTo(page, 'B3');
    await tab(page, 'View');
    await pick(page, 'Freeze Panes', 'Freeze Panes');
    await expect
      .poll(() => page.evaluate(() => window.cgsTest.grid()!.state().scrollY))
      .toBeGreaterThan(0);
    const low = await page.evaluate(() => window.cgsTest.grid()!.state().scrollY);
    const before = await pointFor(page, 0, 0);
    const moving = await pointFor(page, 5, 0);
    await page.evaluate(() => window.cgsTest.grid()!.scrollTo(0, 100000));
    // The frozen row stays exactly where it was, and a row below the panes went away.
    await expect.poll(async () => (await pointFor(page, 5, 0)).y).not.toBe(moving.y);
    expect((await pointFor(page, 0, 0)).y).toBe(before.y);
    await pick(page, 'Freeze Panes', 'Unfreeze Panes');
    // The panes go when the engine answers, and until then the top stops at their edge.
    await expect
      .poll(() =>
        page.evaluate(() => {
          window.cgsTest.grid()!.scrollTo(0, 0);
          return window.cgsTest.grid()!.state().scrollY;
        }),
      )
      .toBeLessThan(low);
  });

  test('gridlines and headings turn off', async ({ page }) => {
    await waitForGrid(page);
    const canvas = page.getByTestId('sheet-canvas');
    const before = await canvas.screenshot();
    await tab(page, 'View');
    await press(page, 'Gridlines');
    await expect.poll(async () => before.equals(await canvas.screenshot())).toBe(false);
    const a = await pointFor(page, 0, 0);
    await press(page, 'Headings');
    await expect.poll(async () => (await pointFor(page, 0, 0)).y).toBeLessThan(a.y);
  });
});

test.describe('find and replace', () => {
  test('finds, goes round, lists, replaces one and replaces all', async ({ page }) => {
    await page.keyboard.type('apple\nbanana\nApple pie\n=1+1\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+f');
    await page.getByTestId('find-text').fill('apple');
    await page.getByTestId('find-next').click();
    await expect(nameBox(page)).toHaveValue('A3');
    await page.getByTestId('find-next').click();
    await expect(nameBox(page)).toHaveValue('A1');
    await page.getByTestId('find-case').check();
    await page.getByTestId('find-all').click();
    await expect(page.getByTestId('find-list').getByRole('button')).toHaveCount(1);
    await page.getByTestId('find-case').uncheck();
    await page.getByTestId('find-text').fill('nothing here');
    await page.getByTestId('find-next').click();
    await expect(page.getByTestId('find-message')).toContainText('couldn’t find');
    await page.getByRole('tab', { name: 'Replace' }).click();
    await page.getByTestId('find-text').fill('apple');
    await page.getByTestId('replace-text').fill('pear');
    await page.getByTestId('replace-all').click();
    await expect(page.getByTestId('find-message')).toContainText('2 replacements');
    await shows(page, 1, 1).toBe('pear');
    await shows(page, 3, 1).toBe('pear pie');
    await page.getByTestId('find-close').click();
    await expect(page.getByTestId('find-dialog')).toHaveCount(0);
    await page.keyboard.press('Control+z');
    await shows(page, 1, 1).toBe('apple');
  });

  test('values and formulas are looked in separately', async ({ page }) => {
    await page.keyboard.type('=1+1\n');
    await goTo(page, 'B5');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+f');
    await page.getByTestId('find-text').fill('1+1');
    await page.getByTestId('find-all').click();
    await expect(page.getByTestId('find-message')).toHaveText('1 cell found');
    await page.getByTestId('find-look-in').selectOption('values');
    await page.getByTestId('find-all').click();
    await expect(page.getByTestId('find-message')).toHaveText('0 cells found');
    await page.getByTestId('find-text').fill('2');
    await page.getByTestId('find-all').click();
    await expect(page.getByTestId('find-message')).toHaveText('1 cell found');
  });
});

test.describe('context menus and inserting cells', () => {
  test('the sheet has its menu, and inserting cells asks which way', async ({ page }) => {
    await page.keyboard.type('1\n2\n=A1+A2\n');
    await goTo(page, 'A1');
    const cell = await pointFor(page, 0, 0);
    await page.mouse.click(cell.x, cell.y, { button: 'right' });
    await page.getByRole('menuitem', { name: 'Insert…' }).click();
    await page.getByTestId('shift-down').check();
    await page.getByRole('button', { name: 'OK' }).click();
    await shows(page, 2, 1).toBe('1');
    await shows(page, 4, 1).toBe('3');
    await goTo(page, 'A4');
    await expect(page.getByTestId('formula-bar')).toHaveValue('=A2+A3');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+z');
    await shows(page, 1, 1).toBe('1');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+-');
    await page.getByTestId('shift-down').check();
    await page.getByRole('button', { name: 'OK' }).click();
    await shows(page, 1, 1).toBe('2');
  });

  test('a sheet tab has its own menu', async ({ page }) => {
    await page.getByTestId('sheet-tab-0').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Insert Sheet' }).click();
    await expect(page.getByTestId('sheet-tab-1')).toBeVisible();
  });
});

test.describe('saving formats', () => {
  test('come back after a save, and a font name goes through', async ({ page }) => {
    await page.keyboard.type('x\n');
    await goTo(page, 'A1');
    await page.getByTestId('sheet-canvas').focus();
    await page.keyboard.press('Control+b');
    await page.evaluate(async () => {
      await window.cgsTest.settled();
      const bytes = await window.cgsTest.saveBytes();
      await window.cgsTest.open({
        name: 'again.xlsx',
        source: { kind: 'blob', blob: new Blob([bytes]) },
      });
    });
    await waitForGrid(page);
    expect((await format(page, 1, 1))?.bold).toBe(true);
    await shows(page, 1, 1).toBe('x');
  });
});
