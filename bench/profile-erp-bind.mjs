// Phase 19: where binding ERP records goes in Chrome. Binds `--records` (1,000 by default) of
// erp-pages.mjs's 20 fields as measure-erp-bind.mjs does, with Chrome's sampling profiler in the
// main thread, the engine worker and the tile worker from just before the pages are handed over
// until the first paint. Prints each thread's functions with the most time of their own and under
// them. Function names need a build that keeps them: `node scripts/build-wasm.mjs --release
// --profiling`, then `pnpm --filter @cgs/web build`.
// Usage: node bench/profile-erp-bind.mjs [--records=1000] [--out=file]
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { pages, schema, session } from './erp-pages.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const records = Number(option('records') ?? 1000);
const folder = join(root, 'bench/results/erp-bind-profile');
mkdirSync(folder, { recursive: true });
const out = option('out') ?? join(folder, `bind-${records}.json`);
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

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
await page.goto(origin);
await page.waitForFunction(
  () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
);

/** A profiler in one thread: the page's own, or a worker's through the browser's session. */
async function profiler(target) {
  if (target === 'main') {
    const cdp = await page.context().newCDPSession(page);
    return { send: (method, params = {}) => cdp.send(method, params) };
  }
  const cdp = await browser.newBrowserCDPSession();
  const { targetInfos } = await cdp.send('Target.getTargets');
  const worker = targetInfos.find((info) => info.type === 'worker' && target.test(info.url));
  if (!worker) throw new Error(`No worker ${target} among ${targetInfos.map((t) => t.url)}`);
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
  return {
    send: (method, params = {}) =>
      new Promise((resolve, reject) => {
        const id = nextId++;
        waiting.set(id, { resolve, reject });
        void cdp.send('Target.sendMessageToTarget', {
          sessionId,
          message: JSON.stringify({ id, method, params }),
        });
      }),
  };
}

const threads = {
  main: await profiler('main'),
  engine: await profiler(/engine-worker/),
  tiles: await profiler(/tile-worker/),
};
for (const thread of Object.values(threads)) {
  await thread.send('Profiler.enable');
  await thread.send('Profiler.setSamplingInterval', { interval: 200 });
  await thread.send('Profiler.start');
}
const timing = await page.evaluate(
  async ({ schema, encoded, session }) => {
    const bytes = encoded.map((text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
    const steps = await window.cgsTest.bindPages(schema, bytes, session);
    const started = steps.startedAt;
    await new Promise((done) => {
      const check = () => {
        const canvas = document.querySelector('[data-testid="sheet-canvas"]');
        if (canvas?.dataset.ready === 'true') requestAnimationFrame(() => done());
        else setTimeout(check, 1);
      };
      // A frame first, so the flag says what the bound sheet's tiles are, not the blank's.
      requestAnimationFrame(check);
    });
    return { ...steps, totalMs: performance.now() - started };
  },
  { schema, encoded: pages(records), session },
);
const profiles = {};
for (const [name, thread] of Object.entries(threads)) {
  profiles[name] = (await thread.send('Profiler.stop')).profile;
}
await browser.close();
// The server is a shell's grandchild, which killing the shell would leave running.
if (server) spawnSync('taskkill', ['/pid', String(server.pid), '/t', '/f'], { stdio: 'ignore' });

function summarize(profile) {
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
    const node = byId.get(id);
    if (name(node) === '(idle)') return;
    sampled += ms;
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
      .map(([label, ms]) => ({ label, ms: Math.round(ms * 10) / 10 }));
  return { busyMs: Math.round(sampled), self: top(self, 40), total: top(total, 60) };
}

const result = { records, timing, threads: {} };
for (const [name, profile] of Object.entries(profiles)) {
  result.threads[name] = summarize(profile);
  writeFileSync(out.replace(/\.json$/, `.${name}.cpuprofile`), JSON.stringify(profile));
}
writeFileSync(out, JSON.stringify(result, null, 2) + '\n');
console.log(`${records} records: ${timing.totalMs.toFixed(0)} ms`);
for (const [name, summary] of Object.entries(result.threads)) {
  console.log(`\n${name}: busy ${summary.busyMs} ms`);
  for (const entry of summary.total.slice(0, 25)) console.log(`  ${entry.ms} ms  ${entry.label}`);
}
