// Has the app export workbooks as PDFs, through File › Print's own path, and checks each PDF:
// `qpdf --check` finds nothing wrong, and pdf.js reads its text back in reading order.
// Needs the app served at E2E_BASE_URL, such as `pnpm dev:web` or `pnpm preview`.
// Usage: node bench/export-pdfs.mjs <out-folder> <workbook>... [--workbook-wide] [--png]
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { qpdfPath } from '../scripts/fetch-qpdf.mjs';
import { pdfText } from './pdf-text.mjs';
import { renderPdf } from './pdf-render.mjs';

const args = process.argv.slice(2).filter((arg) => !arg.startsWith('--'));
const folder = resolve(args[0] ?? join(root, 'bench/results/pdf'));
const books = args.slice(1);
const what = process.argv.includes('--workbook-wide') ? 'workbook' : 'active';
const base = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:5173';
mkdirSync(folder, { recursive: true });

const qpdf = await qpdfPath();
const results = [];
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
    const name = basename(path);
    const bytes = readFileSync(path).toString('base64');
    await page.evaluate(
      async ({ name, base64 }) => {
        const binary = atob(base64);
        const data = new Uint8Array(binary.length);
        for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
        await window.cgsTest.open({ name, source: { kind: 'blob', blob: new File([data], name) } });
      },
      { name, base64: bytes },
    );
    await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', null, {
      timeout: 300000,
    });
    const started = Date.now();
    const out = await page.evaluate(async (what) => {
      const { bytes, pages } = await window.cgsTest.pdf({ what });
      let text = '';
      for (let at = 0; at < bytes.length; at += 32768) {
        text += String.fromCharCode(...bytes.subarray(at, at + 32768));
      }
      return { base64: btoa(text), pages };
    }, what);
    const pdf = Buffer.from(out.base64, 'base64');
    const target = join(folder, name.replace(/\.[^.]+$/, '') + '.pdf');
    writeFileSync(target, pdf);
    const check = spawnSync(qpdf, ['--check', target], { encoding: 'utf8' });
    // 0 is clean; 2 is errors and 3 warnings.
    const clean = check.status === 0 && !/WARNING/.test(check.stdout + check.stderr);
    const text = await pdfText(pdf);
    if (process.argv.includes('--png')) await renderPdf(page, pdf, target.replace(/\.pdf$/, ''));
    results.push({
      book: name,
      pages: out.pages,
      bytes: pdf.length,
      ms: Date.now() - started,
      qpdf: clean ? 'clean' : (check.stdout + check.stderr).trim().slice(0, 400),
      text,
    });
    console.log(
      `${name}: ${out.pages} pages, ${pdf.length} bytes, qpdf ${clean ? 'clean' : 'FAILED'}`,
    );
  }
} finally {
  await browser.close();
}
writeFileSync(join(folder, 'report.json'), JSON.stringify(results, null, 2));
if (results.some((r) => r.qpdf !== 'clean')) process.exitCode = 1;
