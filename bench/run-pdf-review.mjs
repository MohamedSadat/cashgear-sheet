// Phase 10's PDF review: Excel exports each book with ExportAsFixedFormat and the app exports the
// same book through File › Print's own path; both are drawn page by page with pdf.js into one
// side-by-side page to review by eye. The app's PDFs must pass `qpdf --check` and read back their
// text, Arabic included. Generated workbooks and fixtures only.
// Needs the app served at E2E_BASE_URL, such as `pnpm dev:web` or `pnpm preview`.
// With `--rtl`, Phase 11's right-to-left books go through instead.
// Usage: node bench/run-pdf-review.mjs [out-folder] [--no-excel] [--rtl]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { qpdfPath } from '../scripts/fetch-qpdf.mjs';
import { pdfText } from './pdf-text.mjs';
import { renderPdf } from './pdf-render.mjs';
import { printBooks } from './print-books.mjs';

const rightToLeft = process.argv.includes('--rtl');
const folder = resolve(
  process.argv.slice(2).find((arg) => !arg.startsWith('--')) ??
    join(root, rightToLeft ? 'bench/results/pdf-review-rtl' : 'bench/results/pdf-review'),
);
const withExcel = !process.argv.includes('--no-excel');
const base = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:5173';
const script = readFileSync(join(root, 'bench/excel-export-pdf.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
mkdirSync(folder, { recursive: true });

spawnSync('node', [join(root, 'scripts/write-fixtures.mjs')], { stdio: 'inherit' });
const fixtures = join(root, 'tests/workbooks/fixtures');
const generated = join(folder, 'books');
mkdirSync(generated, { recursive: true });
const picks = ['letter-portrait.xlsx', 'titles-both.xlsx', 'everything.xlsx', 'headings.xlsx'];
const books = [
  ...(rightToLeft
    ? [join(fixtures, 'rtl-print.xlsx'), join(fixtures, 'rtl.xlsx')]
    : [
        join(fixtures, 'print.xlsx'),
        join(fixtures, 'formatting.xlsx'),
        join(fixtures, 'arabic.xlsx'),
        join(fixtures, 'pivot.xlsx'),
      ]),
  ...Object.entries(printBooks({ rightToLeft }))
    .filter(([name]) => picks.includes(name))
    .map(([name, bytes]) => {
      const path = join(generated, name);
      writeFileSync(path, bytes);
      return path;
    }),
];

const qpdf = await qpdfPath();
const report = [];
const browser = await chromium.launch({ channel: 'chrome' });
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  await page.goto(base);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
    null,
    { timeout: 120000 },
  );
  for (const path of books) {
    const name = basename(path).replace(/\.[^.]+$/, '');
    const entry = { book: name, excel: null, app: null };
    if (withExcel) {
      const excelPdf = join(folder, name + '-excel.pdf');
      const run = spawnSync(
        'powershell',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `& {${script}} -Workbook ${quoted(resolve(path))} -Pdf ${quoted(excelPdf)}`,
        ],
        { encoding: 'utf8', timeout: 600000, windowsHide: true },
      );
      if (run.status === 0 && existsSync(excelPdf)) {
        const bytes = readFileSync(excelPdf);
        const pngs = await renderPdf(page, bytes, join(folder, name + '-excel'));
        entry.excel = { pages: pngs.length, pngs: pngs.map((p) => basename(p)) };
      } else {
        entry.excel = { error: run.stderr.slice(0, 300) };
      }
    }
    const bytes = readFileSync(path).toString('base64');
    await page.evaluate(
      async ({ name, base64 }) => {
        const binary = atob(base64);
        const data = new Uint8Array(binary.length);
        for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
        await window.cgsTest.open({ name, source: { kind: 'blob', blob: new File([data], name) } });
      },
      { name: basename(path), base64: bytes },
    );
    await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', null, {
      timeout: 300000,
    });
    const out = await page.evaluate(async () => {
      const { bytes } = await window.cgsTest.pdf({ what: 'workbook' });
      let text = '';
      for (let at = 0; at < bytes.length; at += 32768) {
        text += String.fromCharCode(...bytes.subarray(at, at + 32768));
      }
      return btoa(text);
    });
    const pdf = Buffer.from(out, 'base64');
    const appPdf = join(folder, name + '-app.pdf');
    writeFileSync(appPdf, pdf);
    const check = spawnSync(qpdf, ['--check', appPdf], { encoding: 'utf8' });
    const pngs = await renderPdf(page, pdf, join(folder, name + '-app'));
    entry.app = {
      pages: pngs.length,
      pngs: pngs.map((p) => basename(p)),
      bytes: pdf.length,
      qpdf:
        check.status === 0 && !/WARNING/.test(check.stdout + check.stderr)
          ? 'clean'
          : check.stdout + check.stderr,
      text: await pdfText(pdf),
    };
    report.push(entry);
    console.log(
      name,
      'Excel',
      entry.excel?.pages ?? '-',
      'pages, the app',
      entry.app.pages,
      'pages, qpdf',
      entry.app.qpdf === 'clean' ? 'clean' : 'FAILED',
    );
  }
} finally {
  await browser.close();
}
writeFileSync(join(folder, 'report.json'), JSON.stringify(report, null, 2));

// The side-by-side page.
const rows = report
  .map((entry) => {
    const count = Math.max(entry.excel?.pages ?? 0, entry.app?.pages ?? 0);
    const pages = Array.from({ length: count }, (_, n) => {
      const excel = entry.excel?.pngs?.[n];
      const app = entry.app?.pngs?.[n];
      return `<div class="pair"><figure>${excel ? `<img src="${excel}" alt="Excel page ${n + 1}">` : '<p>No page</p>'}<figcaption>Excel ${n + 1}</figcaption></figure><figure>${app ? `<img src="${app}" alt="App page ${n + 1}">` : '<p>No page</p>'}<figcaption>CashGear Sheet ${n + 1}</figcaption></figure></div>`;
    }).join('');
    return `<section><h2>${entry.book}</h2><p>Excel: ${entry.excel?.pages ?? entry.excel?.error ?? 'not run'} pages. The app: ${entry.app?.pages} pages, ${entry.app?.bytes} bytes, qpdf ${entry.app?.qpdf === 'clean' ? 'clean' : 'not clean'}.</p>${pages}</section>`;
  })
  .join('');
writeFileSync(
  join(folder, 'index.html'),
  `<!doctype html><meta charset="utf-8"><title>PDF review</title><style>body{font:14px system-ui;margin:16px;background:#eee}section{margin-bottom:32px}.pair{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin-bottom:12px}figure{margin:0;background:#fff;padding:6px}img{width:100%;border:1px solid #ccc}figcaption{color:#555}</style><h1>Excel's PDFs beside the app's</h1>${rows}`,
);
console.log('Wrote', join(folder, 'index.html'));
