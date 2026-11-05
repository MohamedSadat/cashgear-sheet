// Measures Phase 4's input target in the real app, in Chrome: how long a key takes to show while
// a million formulas recalculate, and how long the engine keeps a question waiting. Also how long
// saving takes in WASM, with the page's longest frame while it does. Runs against `pnpm preview`,
// so build first. Timings mean something only on the reference machine.
// Usage: node bench/measure-input.mjs [--out=file] [--no-save]
import { spawn, spawnSync } from 'node:child_process';
import { statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const speedFolder = join(root, 'tests/workbooks/generated/speed');
const out = option('out') ?? join(root, 'bench/results/input-speed.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';

async function serverIsUp() {
  try {
    return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}

const server = (await serverIsUp())
  ? null
  : spawn('pnpm', ['preview'], { cwd: root, stdio: 'ignore', shell: true });
for (let tries = 0; !(await serverIsUp()); tries++) {
  if (tries > 60) throw new Error('The preview server did not start.');
  await new Promise((resolve) => setTimeout(resolve, 500));
}

const browser = await chromium.launch({ channel: 'chrome', headless: false });

async function openFile(path) {
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();
  // A page that has fallen over gives timings that mean nothing.
  page.on('pageerror', (error) => {
    console.error('Page error:', error.stack ?? error.message);
    process.exit(1);
  });
  page.on('console', (message) => {
    if (message.type() === 'error') console.error('Console:', message.text().slice(0, 400));
  });
  await page.addInitScript(() => {
    delete window.showOpenFilePicker;
  });
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Open a workbook…' }).click();
  await (await chooser).setFiles(path);
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 600000,
    polling: 200,
  });
  // Nothing should be left calculating from opening.
  await page
    .getByTestId('calc-status')
    .waitFor({ state: 'detached', timeout: 600000 })
    .catch(() => undefined);
  return { context, page };
}

const summary = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (share) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))];
  return {
    count: sorted.length,
    p50Ms: Number((at(0.5) ?? 0).toFixed(1)),
    p95Ms: Number((at(0.95) ?? 0).toFixed(1)),
    p99Ms: Number((at(0.99) ?? 0).toFixed(1)),
    maxMs: Number((sorted[sorted.length - 1] ?? 0).toFixed(1)),
  };
};

async function goTo(page, name) {
  await page.getByTestId('name-box').click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await page.waitForFunction(
    (name) => document.querySelector('[data-testid="name-box"]').value === name,
    name,
  );
}

/** Keys during a recalculation: each from when the browser got it to the frame that shows it. */
async function measureInput(path) {
  const { context, page } = await openFile(path);
  await page.evaluate(() => {
    window.keyDelays = [];
    window.engineWaits = [];
    window.frameGaps = [];
    window.recording = true;
    document.addEventListener(
      'keydown',
      (event) => {
        const pressed = event.timeStamp;
        requestAnimationFrame(() => {
          // A message runs after the frame has gone to the screen.
          const channel = new MessageChannel();
          channel.port1.onmessage = () => window.keyDelays.push(performance.now() - pressed);
          channel.port2.postMessage(0);
        });
      },
      { capture: true },
    );
    const ask = async () => {
      while (window.recording) {
        const started = performance.now();
        await window.cgsTest.cellText(1, 1);
        window.engineWaits.push(performance.now() - started);
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    };
    void ask();
    let last = performance.now();
    const tick = () => {
      const now = performance.now();
      window.frameGaps.push(now - last);
      last = now;
      if (window.recording) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  // The first edit's graph build happened on opening, so this only starts the calculation.
  const calculating = page.getByTestId('calc-status');
  const result = { file: path.split(/[\\/]/).pop(), calculations: [] };

  await goTo(page, 'H2');
  for (let press = 0; press < 60; press++) {
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
  }
  await page.waitForTimeout(200);
  result.keysRightAfterOpening = summary(await page.evaluate(() => window.keyDelays.splice(0)));
  await page.evaluate(() => window.engineWaits.splice(0));
  await page.evaluate(() => window.frameGaps.splice(0));

  let spare = 20;
  const commits = [];
  const keysDuring = [];
  const engineDuring = [];
  const framesDuring = [];
  for (let round = 0; round < 5; round++) {
    await goTo(page, 'G1');
    const started = Date.now();
    await page.keyboard.type(String(round + 2));
    await page.keyboard.press('Enter');
    await page.evaluate(() => window.cgsTest.settled());
    await calculating.waitFor({ state: 'visible', timeout: 60000 });
    let typed = 0;
    await goTo(page, 'H' + (round * 100 + 2));
    while (await calculating.isVisible()) {
      // Moving around and typing, as someone carrying on with their work.
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowLeft');
      await page.keyboard.type(String(spare++));
      await page.keyboard.press('Enter');
      typed += 5;
      // Five keys in about a third of a second, which is someone typing very fast. A command runs
      // once those before it have, so this says how long an edit takes to reach the engine.
      const sent = Date.now();
      await page.evaluate(() => window.cgsTest.settled());
      commits.push(Date.now() - sent);
      await page.waitForTimeout(250);
    }
    await page.evaluate(() => window.cgsTest.settled());
    const took = Date.now() - started;
    keysDuring.push(...(await page.evaluate(() => window.keyDelays.splice(0))));
    engineDuring.push(...(await page.evaluate(() => window.engineWaits.splice(0))));
    framesDuring.push(...(await page.evaluate(() => window.frameGaps.splice(0))));
    // A calculation that ended early, or a refused edit, would make these timings meaningless.
    const shown = await page.evaluate(async (value) => {
      const grid = window.cgsTest;
      return {
        input: await grid.cellText(1, 7),
        expected: value + 1,
        first: await grid.cellText(1, 2),
        status: document.querySelector('[data-testid="engine-status"]')?.textContent,
        memoryMiB: Math.round((await grid.memoryBytes()) / 1048576),
      };
    }, round + 2);
    result.calculations.push({ calculationMs: took, keysPressed: typed, ...shown });
    console.log(JSON.stringify(result.calculations.at(-1)));
  }
  // The same keys with nothing left to calculate, for comparison.
  await page.evaluate(() => window.keyDelays.splice(0));
  for (let press = 0; press < 60; press++) {
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(30);
  }
  await page.waitForTimeout(200);
  result.keysOnceSettled = summary(await page.evaluate(() => window.keyDelays.splice(0)));
  result.commitsDuringRecalculation = summary(commits);
  result.keysDuringRecalculation = summary(keysDuring);
  result.engineWaitDuringRecalculation = summary(engineDuring);
  result.framesDuringRecalculation = summary(framesDuring);
  result.valueCheck = await page.evaluate(() => window.cgsTest.cellText(1, 2));
  await context.close();
  console.log(JSON.stringify(result));
  return result;
}

/** One edit and then a save, which is all the worker does until it is done. */
async function measureSave(path) {
  const { context, page } = await openFile(path);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 123 }));
  const timing = await page.evaluate(async () => {
    let worst = 0;
    let last = performance.now();
    let going = true;
    const tick = () => {
      const now = performance.now();
      worst = Math.max(worst, now - last);
      last = now;
      if (going) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    let size = 0;
    const started = performance.now();
    await window.cgsTest.writeTo({
      async write(blob) {
        size = blob.size;
        return { name: 'saved.xlsx', source: { kind: 'blob', blob } };
      },
    });
    const took = performance.now() - started;
    going = false;
    return { saveMs: Math.round(took), longestFrameMs: Math.round(worst), savedBytes: size };
  });
  const problem = await page.evaluate(() => window.cgsTest.problem());
  const memory = await page.evaluate(() => window.cgsTest.memoryBytes());
  const result = {
    file: path.split(/[\\/]/).pop(),
    megabytes: Number((statSync(path).size / 1048576).toFixed(1)),
    ...timing,
    wasmMemoryMiB: Math.round(memory / 1048576),
    problem,
  };
  await context.close();
  console.log(JSON.stringify(result));
  return result;
}

const output = { chrome: browser.version() };
output.input = await measureInput(join(speedFolder, 'recalc-1m.xlsx'));
if (!process.argv.includes('--no-save')) {
  output.save = [];
  for (const name of ['values-25mb', 'formulas-25mb', 'rows-1m', 'values-100mb']) {
    output.save.push(await measureSave(join(speedFolder, name + '.xlsx')));
  }
}
writeFileSync(out, JSON.stringify(output, null, 2) + '\n');
await browser.close();
// The server is a shell's grandchild, which killing the shell would leave running and holding dist.
if (server) spawnSync('taskkill', ['/pid', String(server.pid), '/t', '/f'], { stdio: 'ignore' });
process.exit(0);
