// A stand-in for Cash Gear Workspace's page, on its test origin, for tests/e2e/embed.spec.ts
// (Track S5): it frames the sheet's real `/embed` as Workspace does (sandboxed, no referrer, no
// COEP) and drives `sheet-embed.1` from `parent.js`. The same pages are served on a second port,
// an origin Workspace doesn't have, which may not frame the embed or speak to it.
//
//   node tests/embed-parent/server.mjs [--port 4273] [--other-port 4274] [--host 127.0.0.1]
// Node only, with no dependencies.

import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = (name) => new URL(name, import.meta.url);

const FILES = {
  '/parent.html': ['parent.html', 'text/html; charset=utf-8'],
  '/parent.js': ['parent.js', 'text/javascript; charset=utf-8'],
  '/rogue.html': ['rogue.html', 'text/html; charset=utf-8'],
};

function handle(req, res) {
  const url = new URL(req.url ?? '/', 'http://parent');
  if (url.pathname === '/__parent/state') {
    res.writeHead(200, { 'Content-Type': 'application/json' }).end('{"parent":true}');
    return;
  }
  const file = FILES[url.pathname];
  if (!file || req.method !== 'GET') {
    res.writeHead(404, { 'Content-Type': 'text/plain' }).end('Not found');
    return;
  }
  res.writeHead(200, {
    'Content-Type': file[1],
    'Cache-Control': 'no-store',
    // As Workspace: no COEP, so the frame isn't cross-origin isolated.
    'Referrer-Policy': 'no-referrer',
  });
  res.end(readFileSync(here(file[0])));
}

function listen(port, host) {
  const server = createServer(handle);
  return new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(port, host, () => done(server));
  });
}

export async function startParent({ port = 4273, otherPort = 4274, host = '127.0.0.1' } = {}) {
  const servers = [await listen(port, host), await listen(otherPort, host)];
  return {
    url: `http://${host}:${servers[0].address().port}`,
    otherUrl: `http://${host}:${servers[1].address().port}`,
    close: () => Promise.all(servers.map((server) => new Promise((done) => server.close(done)))),
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const at = args.indexOf(`--${name}`);
    return at >= 0 && args[at + 1] ? args[at + 1] : fallback;
  };
  const { url, otherUrl } = await startParent({
    port: Number(option('port', '4273')),
    otherPort: Number(option('other-port', '4274')),
    host: option('host', '127.0.0.1'),
  });
  console.log(`Embed parent on ${url}, and an origin Workspace doesn't have on ${otherUrl}`);
}
