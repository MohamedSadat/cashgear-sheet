// Shared by the Playwright specs and scripts/desktop-smoke.mjs, so web and desktop are held
// to the same pictures. Works on web and desktop pages alike.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';

const here = dirname(fileURLToPath(import.meta.url));

/** The part of the canvas a golden image covers, so window sizes needn't match. */
const GOLDEN_WIDTH = 900;
const GOLDEN_HEIGHT = 380;

/** Opens workbook bytes through the app's own path, as a dropped file would be. */
export async function openBytes(page, name, bytes) {
  await page.evaluate(
    async ({ name, base64 }) => {
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      const file = new File([data], name);
      await window.cgsTest.open({ name, source: { kind: 'blob', blob: file } });
    },
    { name, base64: bytes.toString('base64') },
  );
}

/** Waits for every tile in view to be drawn in the current theme. */
export async function waitForGrid(page, theme) {
  await page.waitForFunction(
    (theme) => {
      const root = document.documentElement;
      const canvas = document.querySelector('[data-testid="sheet-canvas"]');
      return (!theme || root.dataset.cgTheme === theme) && canvas?.dataset.ready === 'true';
    },
    theme,
    { timeout: 60000 },
  );
  // Charts and pictures over the grid have drawn, where there are any.
  await page.waitForFunction(
    () => {
      const layer = document.querySelector('[data-testid="drawing-layer"]');
      if (!layer) return true;
      const images = [...layer.querySelectorAll('img')];
      return (
        layer.dataset.ready === 'true' &&
        images.every((image) => image.complete && image.naturalWidth > 0) &&
        !layer.querySelector('.cgs-drawing-placeholder:empty')
      );
    },
    null,
    { timeout: 60000 },
  );
  // The picture goes up on the next frame.
  await page.evaluate(
    () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
  );
}

/**
 * The grid's picture as raw pixels, cropped to what a golden image covers: from the canvas's
 * left, or from its right with `fromRight`, where a right-to-left sheet starts.
 */
export async function gridPicture(page, { fromRight = false } = {}) {
  const buffer = await page.locator('[data-testid="sheet-canvas"]').screenshot();
  const full = PNG.sync.read(buffer);
  const width = Math.min(GOLDEN_WIDTH, full.width);
  const height = Math.min(GOLDEN_HEIGHT, full.height);
  const cropped = new PNG({ width, height });
  PNG.bitblt(full, cropped, fromRight ? full.width - width : 0, 0, width, height, 0, 0);
  return cropped;
}

/** The share of pixels that differ by more than a soft edge can explain. */
export function differenceShare(a, b) {
  if (a.width !== b.width || a.height !== b.height) return 1;
  let different = 0;
  for (let at = 0; at < a.data.length; at += 4) {
    const delta =
      Math.abs(a.data[at] - b.data[at]) +
      Math.abs(a.data[at + 1] - b.data[at + 1]) +
      Math.abs(a.data[at + 2] - b.data[at + 2]);
    if (delta > 96) different++;
  }
  return different / (a.width * a.height);
}

/**
 * Compares the grid with a golden image in tests/e2e/goldens. `CGS_UPDATE_GOLDENS=1` writes
 * the image instead. A failure leaves what it saw in test-results for reading by eye.
 */
export async function expectGolden(page, name, limit = 0.002, { fromRight = false } = {}) {
  const seen = await gridPicture(page, { fromRight });
  const path = join(here, 'goldens', name + '.png');
  if (process.env.CGS_UPDATE_GOLDENS) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, PNG.sync.write(seen));
    return 0;
  }
  if (!existsSync(path)) throw new Error('No golden image at ' + path);
  const share = differenceShare(seen, PNG.sync.read(readFileSync(path)));
  if (share > limit) {
    const out = join(here, '../../test-results/goldens');
    mkdirSync(out, { recursive: true });
    writeFileSync(join(out, name + '-seen.png'), PNG.sync.write(seen));
    throw new Error(`${name}: ${(share * 100).toFixed(3)}% of pixels differ from the golden image`);
  }
  return share;
}

/** Sets the font every cell draws in, before the page starts. */
export const FONT_OVERRIDE = () => localStorage.setItem('cgs-font-override', 'Carlito');

/** The colour of the CashGear control, to prove it follows the theme. */
export function controlColor(page) {
  return page
    .getByRole('radio', { name: 'System' })
    .evaluate((element) => getComputedStyle(element).color);
}
