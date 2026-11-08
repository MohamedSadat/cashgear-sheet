// Pictures for reviewing the grid against Excel by eye: Excel draws a range over COM, and the app draws
// the same range of the same file in Chrome, in light and dark. Only generated workbooks go through it.
// Needs the app served at E2E_BASE_URL, such as `pnpm dev:web` or `pnpm preview`.
// With `--rtl`, Phase 11's right-to-left sheets go through instead, and an index.html shows them.
// Usage: node bench/run-visual-review.mjs <out-folder> [--rtl]
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { SHOWCASE_RANGE, showcaseCommands } from '../tests/fixtures/showcase.mjs';
import { arabicBook, formattingBook, rightToLeftCopy } from '../tests/fixtures/workbooks.mjs';

const rightToLeft = process.argv.includes('--rtl');
const folder = resolve(
  process.argv.slice(2).find((arg) => !arg.startsWith('--')) ??
    join(root, rightToLeft ? 'bench/results/visual-rtl' : 'bench/results/visual'),
);
const base = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:5173';
mkdirSync(folder, { recursive: true });
const scratch = mkdtempSync(join(tmpdir(), 'cgs-visual-'));
const tool = join(root, 'target/fast/savecheck.exe');
const picture = readFileSync(join(root, 'bench/excel-picture.ps1'), 'utf8');
const fixtures = join(root, 'tests/workbooks/fixtures');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

const books = [];
try {
  if (rightToLeft) {
    spawnSync('node', [join(root, 'scripts/write-fixtures.mjs')], { stdio: 'inherit' });
    const copies = { 'formatting-rtl': formattingBook(), 'arabic-rtl': arabicBook() };
    for (const [name, bytes] of Object.entries(copies)) {
      writeFileSync(join(scratch, name + '.xlsx'), rightToLeftCopy(bytes));
    }
    books.push({ name: 'rtl', path: join(fixtures, 'rtl.xlsx'), sheet: 0, range: 'A1:J12' });
    books.push({
      name: 'formatting-rtl',
      path: join(scratch, 'formatting-rtl.xlsx'),
      sheet: 0,
      range: 'A1:K16',
    });
    books.push({
      name: 'arabic-rtl',
      path: join(scratch, 'arabic-rtl.xlsx'),
      sheet: 0,
      range: 'A1:F12',
    });
  }
  const showcase = join(scratch, 'showcase.xlsx');
  const commands = join(scratch, 'commands.json');
  writeFileSync(commands, JSON.stringify(showcaseCommands()));
  const made = spawnSync(tool, ['--edit', 'blank', showcase, '--command-file=' + commands], {
    encoding: 'utf8',
  });
  if (made.status !== 0) throw new Error('savecheck: ' + made.stderr);
  if (!rightToLeft) {
    books.push({ name: 'showcase', path: showcase, sheet: 0, range: SHOWCASE_RANGE });
    books.push({
      name: 'formatting',
      path: join(fixtures, 'formatting.xlsx'),
      sheet: 0,
      range: 'A1:K16',
    });
    books.push({
      name: 'arabic',
      path: join(fixtures, 'arabic.xlsx'),
      sheet: 0,
      range: 'A1:F12',
    });
  }

  for (const book of books) {
    const png = join(folder, `${book.name}-excel.png`);
    const args = Object.entries({
      Workbook: resolve(book.path),
      Sheet: String(book.sheet),
      Range: book.range,
      Png: png,
    })
      .map(([key, value]) => `-${key} ${quoted(value)}`)
      .join(' ');
    const run = spawnSync(
      'powershell',
      ['-NoProfile', '-NonInteractive', '-STA', '-Command', `& {${picture}} ${args}`],
      { encoding: 'utf8', timeout: 300000, windowsHide: true },
    );
    if (run.status !== 0) throw new Error('Excel picture failed: ' + run.stderr.slice(0, 300));
    console.log('Excel drew', book.name);
  }

  const browser = await chromium.launch({ channel: 'chrome' });
  try {
    for (const scheme of ['light', 'dark']) {
      const page = await browser.newPage({
        viewport: { width: 1500, height: 900 },
        colorScheme: scheme,
      });
      await page.goto(base);
      await page.waitForFunction(
        () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
        null,
        { timeout: 120000 },
      );
      for (const book of books) {
        const bytes = readFileSync(book.path).toString('base64');
        await page.evaluate(
          async ({ name, base64 }) => {
            const binary = atob(base64);
            const data = new Uint8Array(binary.length);
            for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
            await window.cgsTest.open({
              name,
              source: { kind: 'blob', blob: new File([data], name) },
            });
          },
          { name: book.name + '.xlsx', base64: bytes },
        );
        await page.waitForFunction(
          () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
          null,
          { timeout: 120000 },
        );
        await page.evaluate(() =>
          window.cgsTest.grid().selectArea({ top: 200, left: 60, bottom: 200, right: 60 }),
        );
        await page.evaluate(() => window.cgsTest.grid().scrollTo(0, 0));
        await page.waitForTimeout(400);
        // The range's corners, from where the grid says its cells are.
        const [first, last] = book.range.split(':');
        const cell = (name) => {
          const match = /([A-Z]+)(\d+)/.exec(name);
          let col = 0;
          for (const ch of match[1]) col = col * 26 + ch.charCodeAt(0) - 64;
          return [Number(match[2]) - 1, col - 1];
        };
        const box = await page.evaluate(
          ([a, b]) => {
            const grid = window.cgsTest.grid();
            return { corner: grid.boxForCell(a[0], a[1]), far: grid.boxForCell(b[0], b[1]) };
          },
          [cell(first), cell(last)],
        );
        const canvas = await page.getByTestId('sheet-canvas').boundingBox();
        // A right-to-left sheet's first cell is on the right.
        const from = Math.max(0, Math.min(box.corner.x, box.far.x));
        const to = Math.min(
          canvas.width,
          Math.max(box.corner.x + box.corner.width, box.far.x + box.far.width),
        );
        const left = canvas.x + from;
        const top = canvas.y + box.corner.y;
        const width = to - from;
        const height = Math.min(
          canvas.height - box.corner.y,
          box.far.y + box.far.height - box.corner.y,
        );
        await page.screenshot({
          path: join(folder, `${book.name}-app-${scheme}.png`),
          clip: { x: left, y: top, width, height },
        });
        console.log('The app drew', book.name, 'in', scheme);
      }
      await page.close();
    }
  } finally {
    await browser.close();
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// A page to review the pictures side by side.
const figure = (file, caption) =>
  `<figure><img src="${file}" alt="${caption}"><figcaption>${caption}</figcaption></figure>`;
const sections = books
  .map(
    (book) =>
      `<section><h2>${book.name} (${book.range})</h2><div class="row">${figure(`${book.name}-excel.png`, 'Excel 2016 CopyPicture')}${figure(`${book.name}-app-light.png`, 'CashGear Sheet, light')}${figure(`${book.name}-app-dark.png`, 'CashGear Sheet, dark')}</div></section>`,
  )
  .join('');
writeFileSync(
  join(folder, 'index.html'),
  `<!doctype html><meta charset="utf-8"><title>Grid review</title><style>body{font:14px system-ui;margin:16px;background:#eee}.row{display:flex;flex-wrap:wrap;gap:12px}figure{margin:0;background:#fff;padding:6px}figcaption{color:#555}</style><h1>Excel's pictures beside the app's</h1>${sections}`,
);
console.log('Wrote', join(folder, 'index.html'));
