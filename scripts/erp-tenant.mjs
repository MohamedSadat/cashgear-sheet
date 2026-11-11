#!/usr/bin/env node
// Runs the ERP contract suite or the scripted e2e against Cash Gear ERP's test tenant (Phase 19):
//
//   pnpm erp:tenant suite [vitest args]       packages/erp-connector/src/contract.test.ts
//   pnpm erp:tenant e2e [playwright args]     tests/tenant, with playwright.tenant.config.ts
//
// The test users' API keys come from `%USERPROFILE%\.cashgear\sheet-erp-test.env` (or the file
// `CGS_ERP_TEST_ENV` names), one `NAME=value` a line: ERP_TEST_KEY_EDITOR, ERP_TEST_KEY_ADV,
// ERP_TEST_KEY_VIEWER, ERP_TEST_KEY_NONE and ERP_TEST_KEY_OTHER, and optionally ERP_BASE_URL,
// E2E_BASE_URL, ERP_TEST_STAGE and ERP_TEST_RATE_LIMITS. The keys are passed to the run's
// environment only: this script never prints them, and blanks them out of everything the run
// prints. Real commits stay on the tenant's own test vendors (`SHT-…`), and the run puts back
// every value it changed.

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export const KEY_NAMES = [
  'ERP_TEST_KEY_EDITOR',
  'ERP_TEST_KEY_ADV',
  'ERP_TEST_KEY_VIEWER',
  'ERP_TEST_KEY_NONE',
  'ERP_TEST_KEY_OTHER',
];
const OPTIONAL = ['ERP_BASE_URL', 'E2E_BASE_URL', 'ERP_TEST_STAGE', 'ERP_TEST_RATE_LIMITS'];

/** `NAME=value` lines, with `#` comments, blank lines and optional quotes; other names refused. */
export function parseEnvFile(text) {
  const vars = {};
  for (const [i, raw] of text.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) continue;
    const match = /^([A-Z0-9_]+)\s*=\s*(.*)$/.exec(line);
    if (!match) throw new Error(`Line ${i + 1} isn't NAME=value.`);
    const [, name, value] = match;
    if (!KEY_NAMES.includes(name) && !OPTIONAL.includes(name)) {
      throw new Error(`Line ${i + 1} names ${name}, which this script doesn't take.`);
    }
    vars[name] = value.replace(/^(['"])(.*)\1$/, '$2');
  }
  return vars;
}

/** A filter over text the run prints that blanks out every secret in it, across chunks. */
export function redactor(secrets) {
  const values = secrets.filter((s) => s && s.length >= 6).sort((a, b) => b.length - a.length);
  let held = '';
  const blank = (text) => values.reduce((out, value) => out.split(value).join('[key]'), text);
  return {
    push(chunk) {
      const text = held + chunk;
      const cut = text.lastIndexOf('\n') + 1;
      held = text.slice(cut);
      return blank(text.slice(0, cut));
    },
    end() {
      const rest = blank(held);
      held = '';
      return rest;
    },
  };
}

function main() {
  const [what, ...rest] = process.argv.slice(2);
  if (what !== 'suite' && what !== 'e2e') {
    console.error('Usage: pnpm erp:tenant suite|e2e [args]');
    process.exit(2);
  }
  const file = process.env.CGS_ERP_TEST_ENV ?? join(homedir(), '.cashgear', 'sheet-erp-test.env');
  if (!existsSync(file)) {
    console.error(
      `No keys file at ${file}. Put the test users' keys there: ${KEY_NAMES.join(', ')}.`,
    );
    process.exit(2);
  }
  const vars = parseEnvFile(readFileSync(file, 'utf8'));
  const missing = KEY_NAMES.filter((name) => !vars[name]);
  if (
    what === 'e2e' &&
    (missing.includes('ERP_TEST_KEY_EDITOR') || missing.includes('ERP_TEST_KEY_ADV'))
  ) {
    console.error('The e2e needs ERP_TEST_KEY_EDITOR and ERP_TEST_KEY_ADV.');
    process.exit(2);
  }
  const base = (vars.ERP_BASE_URL ?? 'https://api.cashgearerp.com').replace(/\/+$/, '');
  const sheet = (vars.E2E_BASE_URL ?? 'https://sheet.cashgearerp.com').replace(/\/+$/, '');
  const env = {
    ...process.env,
    ...vars,
    ERP_BASE_URL: base,
    E2E_BASE_URL: sheet,
    ERP_ALLOW_WRITES: '1',
    ERP_TEST_WRITE_SEARCH: 'SHT-',
    ERP_TEST_ORIGIN: new URL(sheet).origin,
    ERP_TEST_STAGE: vars.ERP_TEST_STAGE ?? 'E4',
    ERP_TEST_RATE_LIMITS: vars.ERP_TEST_RATE_LIMITS ?? '1',
  };
  delete env.ERP_BASE_IS_MOCK;
  console.log(
    `${what === 'suite' ? 'The contract suite' : 'The tenant e2e'} against ${base}` +
      (what === 'e2e' ? `, through ${sheet}` : '') +
      `, with ${KEY_NAMES.length - missing.length} of ${KEY_NAMES.length} keys from ${file}` +
      (missing.length ? ` (missing: ${missing.join(', ')}, whose cases skip)` : '') +
      `, stage ${env.ERP_TEST_STAGE}, real commits on SHT- vendors only.`,
  );
  const args =
    what === 'suite'
      ? [
          'exec',
          'vitest',
          'run',
          'packages/erp-connector/src/contract.test.ts',
          '--testTimeout=300000',
          '--hookTimeout=300000',
          ...rest,
        ]
      : ['exec', 'playwright', 'test', '-c', 'playwright.tenant.config.ts', ...rest];
  const child = spawn('pnpm', args, {
    env,
    shell: process.platform === 'win32',
    stdio: ['inherit', 'pipe', 'pipe'],
  });
  const secrets = KEY_NAMES.map((name) => vars[name]);
  for (const [from, to] of [
    [child.stdout, process.stdout],
    [child.stderr, process.stderr],
  ]) {
    const filter = redactor(secrets);
    from.setEncoding('utf8');
    from.on('data', (chunk) => to.write(filter.push(chunk)));
    from.on('end', () => to.write(filter.end()));
  }
  child.on('exit', (code) => process.exit(code ?? 1));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main();
