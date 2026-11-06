// Charts reviewed against Excel 2016, and Excel's own chart XML edited and checked. Excel makes a
// workbook of charts over COM and exports each as a picture; the app draws the same file in Chrome,
// light and dark, for a side-by-side page. Then the app edits every chart and saves: Excel must
// open the result without repair, and every extension element in the chart parts must still be
// there. Only the generated workbook goes through Excel.
// Needs the app served at E2E_BASE_URL, such as `pnpm dev:web` or `pnpm preview`.
// Usage: node bench/run-chart-review.mjs <out-folder>
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { readZip } from '../tests/fixtures/workbooks.mjs';

const folder = resolve(process.argv[2] ?? join(root, 'bench/results/charts'));
const base = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:5173';
mkdirSync(folder, { recursive: true });
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const powershell = (script, parameters) => {
  const args = Object.entries(parameters)
    .map(([key, value]) => `-${key} ${quoted(value)}`)
    .join(' ');
  const run = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-STA', '-Command', `& {${script}} ${args}`],
    { encoding: 'utf8', timeout: 600000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel failed: ' + run.stderr.slice(0, 500));
};

const NAMES = [
  'Clustered column',
  'Clustered bar',
  'Line with markers',
  'Pie',
  'Scatter',
  'Combo (line on a secondary axis)',
  'Stacked area',
  'Doughnut',
  'Stacked column',
];

// 1. Excel makes the charts and draws each one.
const book = join(folder, 'excel-charts.xlsx');
rmSync(book, { force: true });
powershell(readFileSync(join(root, 'bench/excel-make-charts.ps1'), 'utf8'), {
  Out: book,
  Pictures: folder,
});
console.log('Excel made and drew', NAMES.length, 'charts');

// 2. The app edits every chart, as Chart Design and Format do, and saves.
const results = { charts: NAMES.length, opened: null, extensions: [], edits: 0 };
const tool = join(root, 'target/fast/savecheck.exe');
const built = spawnSync(
  'cargo',
  ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', 'savecheck'],
  {
    stdio: 'inherit',
    cwd: root,
  },
);
if (built.status !== 0) throw new Error('The check tool did not build.');
const edit = (index, change) => ({ kind: 'editChart', sheet: 0, index, edit: change });
const commands = [];
NAMES.forEach((_, index) => {
  commands.push(edit(index, { kind: 'title', text: 'Edited ' + (index + 1) }));
  commands.push(edit(index, { kind: 'legend', position: index % 2 ? 'right' : 'top' }));
  commands.push(edit(index, { kind: 'dataLabels', show: true }));
  commands.push(
    edit(index, {
      kind: 'fill',
      element: { kind: 'chartArea' },
      color: { theme: 'bg1', lumMod: 95000 },
    }),
  );
  commands.push(edit(index, { kind: 'font', element: { kind: 'legend' }, size: 10, bold: true }));
});
commands.push(edit(0, { kind: 'switchRowCol' }));
commands.push(edit(1, { kind: 'type', chart: 'lineMarkers' }));
commands.push(edit(2, { kind: 'quickLayout', layout: 9 }));
commands.push(edit(6, { kind: 'colors', palette: 6 }));
commands.push(edit(8, { kind: 'gapWidth', group: 0, gap: 60 }));
commands.push({
  kind: 'setDrawingBox',
  sheet: 0,
  index: 7,
  x: 40,
  y: 400,
  width: 360,
  height: 300,
});
// The chart sheet's chart too.
commands.push({
  kind: 'editChart',
  sheet: 1,
  index: 0,
  edit: { kind: 'title', text: 'Chart sheet' },
});
commands.push({
  kind: 'editChart',
  sheet: 1,
  index: 0,
  edit: { kind: 'type', chart: 'lineMarkers' },
});
results.edits = commands.length;
const commandFile = join(folder, 'edits.json');
writeFileSync(commandFile, JSON.stringify(commands));
const edited = join(folder, 'excel-charts-edited.xlsx');
const saved = spawnSync(tool, ['--edit', book, edited, '--command-file=' + commandFile], {
  encoding: 'utf8',
});
if (saved.status !== 0) throw new Error('Saving failed: ' + saved.stderr.slice(0, 500));
const openResult = join(folder, 'opened.json');
powershell(readFileSync(join(root, 'bench/excel-open.ps1'), 'utf8'), {
  Workbook: edited,
  Out: openResult,
});
results.opened = JSON.parse(readFileSync(openResult, 'utf8'));
console.log(
  'Excel opened the edited file:',
  results.opened.opened && !results.opened.repaired ? 'clean' : 'REPAIR',
);

// Every extension element Excel wrote in a chart part is still there after the edits.
const MARKS = [
  '<c:extLst>',
  '<c:ext ',
  'c16:uniqueId',
  'c14:style',
  'mc:AlternateContent',
  'c15:',
  'c16r2:',
  'c16r3:',
];
const parts = (path) =>
  new Map(readZip(readFileSync(path)).map(([name, body]) => [name, body.toString('utf8')]));
const before = parts(book);
const after = parts(edited);
for (const [name, text] of before) {
  if (!/^xl\/charts\/chart\d+\.xml$/.test(name)) continue;
  const now = after.get(name) ?? '';
  const count = (body, mark) => body.split(mark).length - 1;
  for (const mark of MARKS) {
    const was = count(text, mark);
    if (!was) continue;
    // A series' uniqueId extension lives on; a chart can gain extension lists, never lose them.
    results.extensions.push({
      part: name,
      mark,
      before: was,
      after: count(now, mark),
      kept: count(now, mark) >= was,
    });
  }
  results.extensions.push({
    part: name,
    mark: 'changed',
    before: 1,
    after: Number(now !== text),
    kept: true,
  });
  // Excel's own style and colour parts stay byte for byte.
}
for (const [name, text] of before) {
  if (/^xl\/charts\/(style|colors)\d+\.xml$/.test(name)) {
    results.extensions.push({
      part: name,
      mark: 'bytes',
      before: 1,
      after: Number(after.get(name) === text),
      kept: after.get(name) === text,
    });
  }
}
const lost = results.extensions.filter((entry) => !entry.kept);
console.log('Extension elements kept:', lost.length ? 'NO ' + JSON.stringify(lost) : 'all');

// 3. The app draws each chart of Excel's file, light and dark.
const browser = await chromium.launch({ channel: 'chrome' });
try {
  for (const scheme of ['light', 'dark']) {
    const page = await browser.newPage({
      viewport: { width: 1600, height: 1000 },
      colorScheme: scheme,
      deviceScaleFactor: 1,
    });
    await page.goto(base);
    await page.waitForFunction(
      () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
      null,
      {
        timeout: 120000,
      },
    );
    const base64 = readFileSync(book).toString('base64');
    await page.evaluate(async (data) => {
      const binary = atob(data);
      const bytes = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) bytes[at] = binary.charCodeAt(at);
      await window.cgsTest.open({
        name: 'excel-charts.xlsx',
        source: { kind: 'blob', blob: new File([bytes], 'excel-charts.xlsx') },
      });
    }, base64);
    await page.waitForFunction(
      () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
      null,
      {
        timeout: 120000,
      },
    );
    for (let index = 0; index < NAMES.length; index++) {
      // Scroll the chart to the top left of the grid, then let it draw.
      await page.evaluate(async (at) => {
        const grid = window.cgsTest.grid();
        const drawings = await window.cgsTest.drawings();
        const found = drawings.find((d) => d.index === at);
        if (found) grid.scrollTo(Math.max(0, found.x - 20), Math.max(0, found.y - 20));
      }, index);
      const host = page.locator(`[data-testid="chart"][data-index="${index}"]`);
      await host.waitFor({ state: 'visible', timeout: 30000 });
      await page.waitForTimeout(500);
      await host.screenshot({ path: join(folder, `chart-${index}-app-${scheme}.png`) });
    }
    // The chart sheet, filling its tab.
    await page.getByTestId('sheet-tab-1').click();
    const sheetChart = page
      .locator('[data-testid="chart-sheet"] [data-testid="chart"] svg')
      .first();
    await sheetChart.waitFor({ state: 'visible', timeout: 30000 });
    await page.waitForTimeout(500);
    await page
      .locator('[data-testid="chart-sheet"]')
      .screenshot({ path: join(folder, `chart-sheet-app-${scheme}.png`) });
    console.log('The app drew the charts in', scheme);
    await page.close();
  }
} finally {
  await browser.close();
}

// 4. The page to review them on.
const rows = NAMES.map(
  (name, index) =>
    `<tr><th>${name}</th><td><img src="chart-${index}-excel.png"></td><td><img src="chart-${index}-app-light.png"></td><td class="dark"><img src="chart-${index}-app-dark.png"></td></tr>`,
).join('\n');
const sheetRow = `<tr><th>Chart sheet</th><td><img src="chart-sheet-excel.png"></td><td><img src="chart-sheet-app-light.png"></td><td class="dark"><img src="chart-sheet-app-dark.png"></td></tr>`;
writeFileSync(
  join(folder, 'review.html'),
  `<!doctype html><meta charset="utf-8"><title>Charts against Excel 2016</title>
<style>body{font:14px system-ui;margin:16px}table{border-collapse:collapse}th{text-align:left;padding:8px;vertical-align:top;width:140px}td{padding:8px}td.dark{background:#1e1e1e}img{display:block;max-width:480px}</style>
<h1>Charts: Excel 2016 beside the app</h1><table><tr><th></th><th>Excel 2016</th><th>App, light</th><th>App, dark</th></tr>${rows}${sheetRow}</table>`,
);
writeFileSync(join(folder, 'results.json'), JSON.stringify(results, null, 1) + '\n');
console.log('Review page:', join(folder, 'review.html'));
