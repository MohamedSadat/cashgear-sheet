// The desktop's content security policy (apps/desktop/src-tauri/tauri.conf.json): the ERP
// connector may reach Cash Gear ERP's API and nothing else off the device (Phase 19, plan B.4).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { root } from './lib.mjs';

const conf = JSON.parse(readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
const directives = Object.fromEntries(
  conf.app.security.csp
    .split(';')
    .map((part) => part.trim().split(/\s+/))
    .filter((words) => words[0])
    .map(([name, ...sources]) => [name, sources]),
);

describe('the desktop CSP', () => {
  it('lets the ERP connector reach Cash Gear ERP’s API, and no other remote origin', () => {
    const remote = directives['connect-src'].filter((source) => /^(https?:|\*|wss?:)/.test(source));
    expect(remote.filter((source) => !/^http:\/\/[a-z-]+\.localhost$/.test(source))).toEqual([
      'https://api.cashgearerp.com',
    ]);
  });

  it('keeps everything else to the app itself', () => {
    expect(directives['default-src']).toEqual(["'self'"]);
    expect(directives['script-src']).toEqual(["'self'", "'wasm-unsafe-eval'"]);
  });
});
