// The desktop's content security policy (apps/desktop/src-tauri/tauri.conf.json): the ERP
// connector may reach Cash Gear ERP's API (Phase 19, plan B.4), and a window a Workspace link
// opened reaches Cash Gear Workspace's and its storage account (Track S6), and nothing else off the
// device. A build for
// Workspace's development stack adds that stack's origins (scripts/desktop-config.mjs).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { LIVE_APIS } from '../apps/web/src/csp.ts';
import { isOrigin, widenConnectSrc, workspaceOrigins } from './desktop-config.mjs';
import { root } from './lib.mjs';

const conf = JSON.parse(readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
const directivesOf = (policy) =>
  Object.fromEntries(
    policy
      .split(';')
      .map((part) => part.trim().split(/\s+/))
      .filter((words) => words[0])
      .map(([name, ...sources]) => [name, sources]),
  );
const directives = directivesOf(conf.app.security.csp);
const remote = (sources) =>
  sources.filter(
    (source) => /^(https?:|\*|wss?:)/.test(source) && !/^http:\/\/[a-z-]+\.localhost$/.test(source),
  );

describe('the desktop CSP', () => {
  it('reaches Cash Gear ERP’s and Cash Gear Workspace’s APIs and Workspace’s storage, as the live web does, and nothing else', () => {
    expect(remote(directives['connect-src'])).toEqual([
      'https://api.cashgearerp.com',
      'https://workspace-api.cashgearerp.com',
      'https://cashgearworkspace.blob.core.windows.net',
    ]);
    expect(remote(directives['connect-src'])).toEqual(LIVE_APIS);
  });

  it('keeps everything else to the app itself', () => {
    expect(directives['default-src']).toEqual(["'self'"]);
    expect(directives['script-src']).toEqual(["'self'", "'wasm-unsafe-eval'"]);
  });
});

describe('a desktop build for another Workspace (Track S6)', () => {
  it('adds the API and Blob Storage the web build names to connect-src only', () => {
    const origins = workspaceOrigins({
      VITE_WORKSPACE_API_URL: 'http://127.0.0.1:5280/',
      VITE_WORKSPACE_BLOB_URL: 'http://127.0.0.1:10000/devstoreaccount1',
    });
    expect(origins).toEqual(['http://127.0.0.1:5280', 'http://127.0.0.1:10000']);
    const widened = directivesOf(widenConnectSrc(conf.app.security.csp, origins));
    expect(remote(widened['connect-src'])).toEqual([...LIVE_APIS, ...origins]);
    for (const name of Object.keys(directives)) {
      if (name !== 'connect-src') expect(widened[name]).toEqual(directives[name]);
    }
    expect(workspaceOrigins({})).toEqual([]);
  });

  it('refuses what isn’t an origin', () => {
    expect(isOrigin('https://acct.blob.core.windows.net')).toBe(true);
    for (const text of ['http://x; script-src *', 'http://x/path', 'ftp://x', 'http://x *']) {
      expect(isOrigin(text)).toBe(false);
      expect(() => widenConnectSrc(conf.app.security.csp, [text])).toThrow();
    }
  });

  it('answers the cashgear-sheet scheme, which the installer registers', () => {
    expect(conf.plugins['deep-link']).toEqual({ desktop: { schemes: ['cashgear-sheet'] } });
  });
});
