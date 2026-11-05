// Pictures for reviewing the grid against Excel by eye: Excel draws a range over COM, and the app draws
// the same range of the same file in Chrome, in light and dark. Only generated workbooks go through it.
// Needs the app served at E2E_BASE_URL, such as `pnpm dev:web` or `pnpm preview`.
// Usage: node bench/run-visual-review.mjs <out-folder>
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { SHOWCASE_RANGE, showcaseCommands } from '../tests/fixtures/showcase.mjs';

const folder = resolve(process.argv[2] ?? join(root, 'bench/results/visual'));
const base = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:5173';
mkdirSync(folder, { recursive: true });
const scratch = mkdtempSync(join(tmpdir(), 'cgs-visual-'));
const tool = join(root, 'target/fast/savecheck.exe');
const picture = readFileSync(join(root, 'bench/excel-picture.ps1'), 'utf8');
const fixtures = join(root, 'tests/workbooks/fixtures');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

const books = [];
try {
  const showcase = join(scratch, 'showcase.xlsx');
  const commands = join(scratch, 'commands.json');
  writeFileSync(commands, JSON.stringify(showcaseCommands()));
  const made = spawnSync(tool, ['--edit', 'blank', showcase, '--command-file=' + commands], {
    encoding: 'utf8',
  });
  if (made.status !== 0) throw new Error('savecheck: ' + made.stderr);
  books.push({ name: 'showcase', path: showcase, sheet: 0, range: SHOWCASE_RANGE });
  books.push({
    name: 'formatting',
    path: join(fixtures, 'formatting.xlsx'),
    sheet: 0,
    range: 'A1:K16',
  });
  books.push({ name: 'arabic', path: join(fixtures, 'arabic.xlsx'), sheet: 0, range: 'A1:F12' });

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
        const left = canvas.x + box.corner.x;
        const top = canvas.y + box.corner.y;
        const width = Math.min(
          canvas.width - box.corner.x,
          box.far.x + box.far.width - box.corner.x,
        );
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
