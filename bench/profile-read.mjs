// Phase 12: where reading a big book goes in WASM. Opens a generated book in the app in Chrome
// against `pnpm preview`, with Chrome's sampling profiler running in the engine worker from the
// moment the file is chosen until the book is ready (read and its graph built). Prints the
// functions with the most time of their own, and the most time under them. Function names need a
// build that keeps them: `node scripts/build-wasm.mjs --release --profiling`.
// Usage: node bench/profile-read.mjs [--book=formulas-2m] [--out=file]
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const bookName = option('book') ?? 'formulas-2m';
const path = join(root, 'tests/workbooks/generated/speed', bookName + '.xlsx');
const folder = join(root, 'bench/results/read-profile');
mkdirSync(folder, { recursive: true });
const out = option('out') ?? join(folder, bookName + '.json');
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
  await new Promise((done) => setTimeout(done, 500));
}

const browser = await chromium.launch({ channel: 'chrome', headless: false });
const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await context.newPage();
await page.addInitScript(() => {
  delete window.showOpenFilePicker;
});
await page.goto(origin);
await page.waitForFunction(
  () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
);

// The engine worker, reached through the browser's own session.
const cdp = await browser.newBrowserCDPSession();
const { targetInfos } = await cdp.send('Target.getTargets');
const worker = targetInfos.find((info) => info.type === 'worker' && /engine-worker/.test(info.url));
if (!worker) throw new Error('No engine worker among ' + targetInfos.map((t) => t.url).join(', '));
const { sessionId } = await cdp.send('Target.attachToTarget', {
  targetId: worker.targetId,
  flatten: false,
});
let nextId = 1;
const waiting = new Map();
cdp.on('Target.receivedMessageFromTarget', (event) => {
  if (event.sessionId !== sessionId) return;
  const message = JSON.parse(event.message);
  const done = waiting.get(message.id);
  if (!done) return;
  waiting.delete(message.id);
  if (message.error) done.reject(new Error(message.error.message));
  else done.resolve(message.result);
});
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    waiting.set(id, { resolve, reject });
    void cdp.send('Target.sendMessageToTarget', {
      sessionId,
      message: JSON.stringify({ id, method, params }),
    });
  });

await send('Profiler.enable');
await send('Profiler.setSamplingInterval', { interval: 500 });
await send('Profiler.start');
const started = Date.now();
const chooser = page.waitForEvent('filechooser');
await page.getByRole('button', { name: 'Open a workbook…' }).click();
await (await chooser).setFiles(path);
await page.waitForFunction(
  () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
  undefined,
  { timeout: 1_800_000, polling: 100 },
);
const shownMs = Date.now() - started;
await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
  timeout: 1_800_000,
  polling: 100,
});
const readyMs = Date.now() - started;
const { profile } = await send('Profiler.stop');
await browser.close();
server?.kill();

// Self time per function, and time under it, from the samples.
const byId = new Map(profile.nodes.map((node) => [node.id, node]));
const parent = new Map();
for (const node of profile.nodes)
  for (const child of node.children ?? []) parent.set(child, node.id);
const name = (node) => node.callFrame.functionName || `(${node.callFrame.url || 'program'})`;
const self = new Map();
const total = new Map();
let sampled = 0;
profile.samples.forEach((id, index) => {
  const ms = (profile.timeDeltas[index] ?? 0) / 1000;
  sampled += ms;
  const node = byId.get(id);
  self.set(name(node), (self.get(name(node)) ?? 0) + ms);
  const seen = new Set();
  for (let at = id; at !== undefined; at = parent.get(at)) {
    const label = name(byId.get(at));
    if (seen.has(label)) continue;
    seen.add(label);
    total.set(label, (total.get(label) ?? 0) + ms);
  }
});
const top = (map, count) =>
  [...map.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, count)
    .map(([label, ms]) => ({
      label,
      ms: Math.round(ms),
      share: Number((ms / sampled).toFixed(3)),
    }));
const result = {
  book: bookName,
  shownMs,
  readyMs,
  sampledMs: Math.round(sampled),
  self: top(self, 60),
  total: top(total, 80),
};
writeFileSync(out, JSON.stringify(result, null, 2) + '\n');
writeFileSync(out.replace(/\.json$/, '.cpuprofile'), JSON.stringify(profile));
console.log(
  `${bookName}: shown ${shownMs} ms, ready ${readyMs} ms, sampled ${result.sampledMs} ms`,
);
for (const entry of result.self.slice(0, 30))
  console.log(`self  ${entry.ms}\t${entry.share}\t${entry.label}`);
for (const entry of result.total.slice(0, 40))
  console.log(`total ${entry.ms}\t${entry.share}\t${entry.label}`);
