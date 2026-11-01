// Reads Phase 0's test tile back from a screenshot of the grid canvas. Keep in sync
// with crates/cgs-engine/src/test_pattern.rs. Shared by the Playwright specs and
// scripts/desktop-smoke.mjs, so web and desktop must show the same pattern.
import { PNG } from 'pngjs';

/** FNV-1a of the first tile's bytes from the engine. The Rust test holds the same value. */
export const FIRST_TILE_HASH = 2069359672;

/** Left to right: white, yellow, cyan, green, magenta, red, blue, black. */
const BAR_COLORS = [
  [255, 255, 255],
  [255, 255, 0],
  [0, 255, 255],
  [0, 255, 0],
  [255, 0, 255],
  [255, 0, 0],
  [0, 0, 255],
  [0, 0, 0],
];
const BAR_ROWS = 4;
const CELL_WIDTH = 64;
const ROW_HEIGHT = 20;
const TOLERANCE = 2;

/** `#rgb` or `#rrggbb` as numbers. */
export function parseColor(hex) {
  const digits = hex.replace('#', '');
  const full = digits.length === 3 ? [...digits].map((d) => d + d).join('') : digits;
  return [0, 2, 4].map((at) => parseInt(full.slice(at, at + 2), 16));
}

function pixel(png, x, y) {
  const index = (Math.floor(y) * png.width + Math.floor(x)) * 4;
  return [png.data[index], png.data[index + 1], png.data[index + 2]];
}

const near = (actual, expected) =>
  actual.every((value, channel) => Math.abs(value - expected[channel]) <= TOLERANCE);

/**
 * Compares a screenshot of the canvas with the pattern. Returns one message per
 * difference. `colors` are the theme's tokens; `scale` is device pixels per CSS pixel.
 */
export function patternProblems(png, colors, scale) {
  const problems = [];
  const width = Math.round(CELL_WIDTH * scale);
  const height = Math.round(ROW_HEIGHT * scale);
  const expect = (label, x, y, expected) => {
    const actual = pixel(png, x, y);
    if (!near(actual, expected))
      problems.push(`${label} at ${x},${y}: expected ${expected}, got ${actual}`);
  };
  // The bars have explicit fills, so they are the same in either theme.
  BAR_COLORS.forEach((color, bar) =>
    expect('bar ' + bar, (bar * 2 + 1) * width, (BAR_ROWS * height) / 2, color),
  );
  const background = parseColor(colors.background);
  const gridline = parseColor(colors.gridline);
  const text = parseColor(colors.text);
  // Row 9, column 2 shows "R9C2": its text sits in the cell's left half.
  const row = 8;
  const col = 1;
  const left = col * width;
  const top = row * height;
  expect('background', left + width - 4 * scale, top + height / 2, background);
  expect('vertical gridline', left + width - 1, top + height / 2, gridline);
  expect('horizontal gridline', left + width / 2, top + height - 1, gridline);
  // Antialiasing softens most text pixels, but some must lie close to the text colour.
  const distance = (a, b) => a.reduce((sum, value, i) => sum + Math.abs(value - b[i]), 0);
  const full = distance(background, text);
  let inked = 0;
  for (let y = top; y < top + height - 1; y++)
    for (let x = left; x < left + width / 2; x++)
      if (distance(pixel(png, x, y), text) < full / 3) inked++;
  if (inked < 8) problems.push(`the cell's text is missing: ${inked} pixels near ${text}`);
  return problems;
}

/**
 * Waits for the page to finish drawing `theme`, then checks what it shows. Returns
 * the screenshot and what was read from the page. Works on web and desktop pages alike.
 */
export async function checkPattern(page, theme) {
  await page.waitForFunction(
    (theme) => {
      const root = document.documentElement;
      const canvas = document.querySelector('[data-testid="sheet-canvas"]');
      const surface = getComputedStyle(root).getPropertyValue('--cg-surface').trim();
      return (
        root.dataset.cgTheme === theme &&
        canvas?.dataset.ready === 'true' &&
        canvas.dataset.background === surface
      );
    },
    theme,
    { timeout: 30000 },
  );
  const seen = await page.evaluate(() => {
    const style = getComputedStyle(document.documentElement);
    const token = (name) => style.getPropertyValue(name).trim();
    const canvas = document.querySelector('[data-testid="sheet-canvas"]');
    return {
      colors: {
        background: token('--cg-surface'),
        gridline: token('--cg-border'),
        text: token('--cg-text'),
      },
      scale: window.devicePixelRatio,
      hash: Number(canvas.dataset.firstTileHash),
      tiles: Number(canvas.dataset.tiles),
    };
  });
  const canvas = page.locator('[data-testid="sheet-canvas"]');
  const image = await canvas.screenshot();
  const problems = patternProblems(PNG.sync.read(image), seen.colors, seen.scale);
  if (seen.hash !== FIRST_TILE_HASH)
    problems.push(`the first tile's hash is ${seen.hash}, not ${FIRST_TILE_HASH}`);
  if (problems.length) throw new Error(theme + ' pattern: ' + problems.join('; '));
  return { image, ...seen };
}

/** The computed colour of a CashGear control, which its stylesheet sets per theme. */
export function controlColor(page) {
  return page
    .getByRole('radio', { name: 'System' })
    .evaluate((button) => getComputedStyle(button).color);
}
