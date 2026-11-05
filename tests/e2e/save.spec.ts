import { expect, test, type Page } from '@playwright/test';
import { pivotBook, readZip, structuresBook } from '../fixtures/workbooks.mjs';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

/** Puts bytes in a private file, and hands it back through the page's own open picker. */
async function openThroughPicker(page: Page, name: string, bytes: Buffer) {
  await page.evaluate(
    async ({ name, base64 }) => {
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      const root = await navigator.storage.getDirectory();
      const handle = await root.getFileHandle(name, { create: true });
      const writable = await handle.createWritable();
      await writable.write(data);
      await writable.close();
      (window as unknown as { showOpenFilePicker: () => Promise<unknown[]> }).showOpenFilePicker =
        async () => [handle];
    },
    { name, base64: bytes.toString('base64') },
  );
  await page.keyboard.press('Control+o');
  await waitForGrid(page);
  await expect(page.getByRole('heading', { name })).toBeVisible();
}

/** Makes Save As choose this private file, and no dialog appear. */
async function answerSaveAs(page: Page, name: string) {
  await page.evaluate(async (name) => {
    const root = await navigator.storage.getDirectory();
    const handle = await root.getFileHandle(name, { create: true });
    (window as unknown as { showSaveFilePicker: () => Promise<unknown> }).showSaveFilePicker =
      async () => handle;
  }, name);
}

async function privateFile(page: Page, name: string) {
  const base64 = await page.evaluate(async (name) => {
    const root = await navigator.storage.getDirectory();
    // While a save closes its stream, the browser swaps the new file in, and for a moment there's
    // none by that name or one that can't be read yet.
    let bytes: Uint8Array | undefined;
    for (let tries = 0; !bytes; tries++) {
      try {
        const handle = await root.getFileHandle(name);
        bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
      } catch (error) {
        const kind = (error as DOMException).name;
        if (tries > 50 || (kind !== 'NotFoundError' && kind !== 'NotReadableError')) throw error;
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
    }
    let binary = '';
    for (let at = 0; at < bytes.length; at += 32768) {
      binary += String.fromCharCode(...bytes.subarray(at, at + 32768));
    }
    return btoa(binary);
  }, name);
  return Buffer.from(base64, 'base64');
}

/** What is left in the folder working copies live in. */
const workingCopies = (page: Page) =>
  page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const folder = await root.getDirectoryHandle('cgs-working', { create: true });
    const names: string[] = [];
    const keys = (folder as unknown as { keys(): AsyncIterable<string> }).keys();
    for await (const name of keys) names.push(name);
    return names;
  });

const parts = (bytes: Buffer) => new Map(readZip(bytes).map(([name, body]) => [name, body]));

/** A menu item's name includes its shortcut, so this matches how it starts. */
function clickFile(page: Page, item: RegExp) {
  return page
    .getByRole('menuitem', { name: 'File' })
    .click()
    .then(() => page.getByRole('menuitem', { name: item }));
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto('/');
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  // A clean private folder, as a test's files mustn't reach the next.
  await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const keys = (root as unknown as { keys(): AsyncIterable<string> }).keys();
    const names: string[] = [];
    for await (const name of keys) names.push(name);
    for (const name of names) {
      await root.removeEntry(name, { recursive: true }).catch(() => undefined);
    }
  });
});

test('Save has nothing to do until there is a change', async ({ page }) => {
  const source = structuresBook();
  await openThroughPicker(page, 'nothing.xlsx', source);
  await expect(await clickFile(page, /^Save(?! As)/)).toHaveAttribute('aria-disabled', 'true');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Control+s');
  await page.waitForTimeout(200);
  expect((await privateFile(page, 'nothing.xlsx')).equals(source)).toBe(true);
  await expect(page.getByTestId('save-status')).toHaveCount(0);
});

test('Save overwrites the file, keeps what it did not change, and can do it again', async ({
  page,
}) => {
  const source = structuresBook();
  await openThroughPicker(page, 'book.xlsx', source);
  await page.evaluate(() => window.cgsTest.edit(0, 5, 1, { number: 100 }));
  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('save-status')).toContainText('kept exactly');

  const first = await privateFile(page, 'book.xlsx');
  const before = parts(source);
  const after = parts(first);
  // Only what the edit has to change is different.
  expect(after.has('xl/calcChain.xml')).toBe(false);
  for (const name of ['xl/styles.xml', 'xl/tables/table1.xml', 'xl/sharedStrings.xml']) {
    expect(after.get(name)?.equals(before.get(name)!), name).toBe(true);
  }
  const sheet = after.get('xl/worksheets/sheet1.xml')!.toString();
  expect(sheet).toContain('<c r="A5"><v>100</v></c>');
  // What depends on it was recalculated, and its shared formula is still shared.
  expect(sheet).toContain('<f t="shared" ref="B5:B7" si="0">A5*2</f><v>200</v>');
  expect(after.get('xl/workbook.xml')!.toString()).toContain('fullCalcOnLoad="1"');

  // The report says what happened.
  await page.getByTestId('save-status').click();
  await expect(page.getByTestId('save-report')).toContainText('Removed: xl/calcChain.xml');
  await expect(page.getByTestId('save-report')).toContainText('recalculate');
  await page.getByTestId('save-report').getByRole('button').first().click();

  // A second save reads from the file the first one wrote, as the old read is no longer valid.
  await page.evaluate(() => window.cgsTest.edit(0, 30, 1, { text: 'second <save>' }));
  await page.keyboard.press('Control+s');
  await expect
    .poll(async () => parts(await privateFile(page, 'book.xlsx')).has('xl/calcChain.xml'))
    .toBe(false);
  await expect
    .poll(async () =>
      parts(await privateFile(page, 'book.xlsx'))
        .get('xl/sharedStrings.xml')!
        .toString(),
    )
    .toContain('second &lt;save&gt;');
  const second = parts(await privateFile(page, 'book.xlsx'));
  expect(second.get('xl/worksheets/sheet1.xml')!.toString()).toContain('<c r="A5"><v>100</v></c>');

  // And it opens again, with the values in it.
  await page.reload();
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  await openThroughPicker(page, 'reopened.xlsx', await privateFile(page, 'book.xlsx'));
  expect(await page.evaluate(() => window.cgsTest.cellText(5, 2))).toBe('200');
  expect(await page.evaluate(() => window.cgsTest.cellText(30, 1))).toBe('second <save>');
});

test('Save As writes elsewhere and moves on to the new file', async ({ page }) => {
  const source = pivotBook();
  await openThroughPicker(page, 'original.xlsx', source);
  await answerSaveAs(page, 'copy.xlsx');
  await (await clickFile(page, /^Save As/)).click();
  await expect(page.getByTestId('save-status')).toContainText('every part unchanged');
  await expect(page.getByRole('heading', { name: 'copy.xlsx' })).toBeVisible();
  // With nothing changed, it is the same file.
  expect((await privateFile(page, 'copy.xlsx')).equals(source)).toBe(true);

  await page.getByTestId('save-status').click();
  await expect(page.getByTestId('save-report')).toContainText('pivot tables stayed untouched');
  await page.getByTestId('save-report').getByRole('button').first().click();

  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 1000 }));
  await page.keyboard.press('Control+s');
  await expect
    .poll(async () =>
      parts(await privateFile(page, 'copy.xlsx'))
        .get('xl/worksheets/sheet1.xml')!
        .toString(),
    )
    .toContain('<c r="B2"><v>1000</v></c>');
  // The file it started from is still as it was.
  expect((await privateFile(page, 'original.xlsx')).equals(source)).toBe(true);
});

test('a new workbook has no file yet, so its first save asks where', async ({ page }) => {
  await page.getByRole('menuitem', { name: 'File' }).click();
  await page.getByRole('menuitem', { name: /^New/ }).click();
  await expect(page.getByRole('heading', { name: 'Book1.xlsx' })).toBeVisible();
  await waitForGrid(page);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 41 }));
  await page.evaluate(() => window.cgsTest.edit(0, 2, 3, { formula: 'B2+1' }));
  await answerSaveAs(page, 'made.xlsx');
  await page.keyboard.press('Control+s');
  await expect(page.getByRole('heading', { name: 'made.xlsx' })).toBeVisible();
  await expect(page.getByTestId('save-status')).toBeVisible();

  await openThroughPicker(page, 'made-again.xlsx', await privateFile(page, 'made.xlsx'));
  expect(await page.evaluate(() => window.cgsTest.cellText(2, 3))).toBe('42');
});

test('the first edit copies the source, so a later change to the file costs nothing', async ({
  page,
}) => {
  const source = structuresBook();
  await openThroughPicker(page, 'copied.xlsx', source);
  expect(await workingCopies(page)).toEqual([]);

  await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 9 }));
  expect(await workingCopies(page)).toHaveLength(2);

  // Something else writes to the file, which would stop any read of it.
  await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const handle = await root.getFileHandle('copied.xlsx');
    const writable = await handle.createWritable();
    await writable.write(new Uint8Array([1, 2, 3]));
    await writable.close();
  });

  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('save-status')).toBeVisible();
  const saved = parts(await privateFile(page, 'copied.xlsx'));
  expect(saved.get('xl/worksheets/sheet1.xml')!.toString()).toContain('<c r="A2"><v>9</v></c>');
  expect(
    saved.get('xl/tables/table1.xml')!.equals(parts(source).get('xl/tables/table1.xml')!),
  ).toBe(true);
  // Once saved, the copy has nothing left to do.
  await expect.poll(() => workingCopies(page)).toEqual([]);
});

test('a file that changed before the first edit says so, and nothing is lost', async ({ page }) => {
  await openThroughPicker(page, 'changed.xlsx', structuresBook());
  await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const handle = await root.getFileHandle('changed.xlsx');
    const writable = await handle.createWritable();
    await writable.write(new Uint8Array([1, 2, 3]));
    await writable.close();
  });
  const message = await page.evaluate(() =>
    window.cgsTest.edit(0, 2, 1, { number: 9 }).then(
      () => 'edited',
      (error: Error) => error.message,
    ),
  );
  expect(message).toMatch(/changed since it was opened/);
  expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
  expect((await privateFile(page, 'changed.xlsx')).length).toBe(3);
  expect(await workingCopies(page)).toEqual([]);
});

test('a save that fails leaves the file as it was and the changes in place', async ({ page }) => {
  const source = structuresBook();
  await openThroughPicker(page, 'safe.xlsx', source);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 9 }));
  await page.evaluate(() => {
    const original = FileSystemFileHandle.prototype.createWritable;
    FileSystemFileHandle.prototype.createWritable = async function (...args) {
      const writable = await original.apply(this, args);
      writable.write = () =>
        Promise.reject(new DOMException('The disk is full.', 'QuotaExceededError'));
      return writable;
    };
  });
  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('engine-status')).toHaveText(/disk is full/);
  expect((await privateFile(page, 'safe.xlsx')).equals(source)).toBe(true);
  expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(true);
  await expect(page.getByTestId('save-status')).toHaveCount(0);
});

test('a browser with no file handles saves as a download', async ({ page }) => {
  await page.addInitScript(() => {
    delete (window as { showSaveFilePicker?: unknown }).showSaveFilePicker;
  });
  await page.reload();
  await expect(page.getByTestId('engine-status')).toHaveText('Ready');
  const source = structuresBook();
  await page.evaluate(
    async ({ base64 }) => {
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      await window.cgsTest.open({
        name: 'dropped.xlsx',
        source: { kind: 'blob', blob: new File([data], 'dropped.xlsx') },
      });
    },
    { base64: source.toString('base64') },
  );
  await waitForGrid(page);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 9 }));
  const download = page.waitForEvent('download');
  await page.keyboard.press('Control+s');
  const file = await download;
  expect(file.suggestedFilename()).toBe('dropped.xlsx');
  const stream = await file.createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream) chunks.push(chunk as Buffer);
  const saved = parts(Buffer.concat(chunks));
  expect(saved.get('xl/worksheets/sheet1.xml')!.toString()).toContain('<c r="A2"><v>9</v></c>');
  // The workbook moved on to what it saved, so a later edit is on top of this one.
  await expect(page.getByTestId('save-status')).toBeVisible();
  expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
});
