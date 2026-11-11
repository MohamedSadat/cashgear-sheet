// The connector against the mock ERP: launch, HTTP, session and record loading (Phase 15).

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { startMockErp } from '../../../tests/erp-mock/server.mjs';
import { openRecords, refreshRecords, type RecordSink, type StagedPage } from './connector.ts';
import { ErpError, ErpHttp, kindOf, retryAfterMs } from './http.ts';
import { erpApiBase, readErpLaunch } from './launch.ts';
import { ErpSession } from './session.ts';
import { readQueryResponse } from './validate.ts';

let base = '';
let stop: (() => Promise<void>) | undefined;
// Node's fetch sends no Origin of its own, and Redeem wants the sheet's.
const ORIGIN = 'http://localhost:4173';
const withOrigin: typeof fetch = (input, init) =>
  fetch(input, { ...init, headers: { ...(init?.headers as object), Origin: ORIGIN } });

beforeAll(async () => {
  const server = await startMockErp();
  base = server.url;
  stop = server.close;
});
afterAll(async () => stop?.());
beforeEach(async () => {
  await fetch(`${base}/__mock/reset`, { method: 'POST' });
});

const http = () => new ErpHttp({ baseUrl: base, fetch: withOrigin, sleep: async () => {} });

async function ticket(filter: unknown = null, key = 'mock-kit-editor'): Promise<string> {
  const response = await fetch(`${base}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': key },
    body: JSON.stringify({ entity: 'vendors', filter, language: 'en' }),
  });
  return ((await response.json()) as { ticket: string }).ticket;
}

/** A sink that reads pages as the engine would, in the test only. */
class TestSink implements RecordSink {
  schema: string | null = null;
  refreshing = false;
  keys: string[] = [];
  ids: number[] = [];
  cancelled = 0;
  refuseOver = Infinity;

  async begin(schema: string) {
    this.schema = schema;
    this.keys = [];
  }
  async beginRefresh() {
    this.refreshing = true;
    this.keys = [];
  }
  async page(bytes: Uint8Array): Promise<StagedPage> {
    const page = readQueryResponse(JSON.parse(new TextDecoder().decode(bytes)));
    if (page.total > this.refuseOver) throw new Error(`This list has ${page.total} vendors.`);
    this.keys.push(...page.records.map((r) => r.key));
    this.ids.push(...page.records.map((r) => r.id));
    return {
      records: page.records.length,
      staged: this.keys.length,
      nextAfterId: page.nextAfterId,
      total: page.total,
    };
  }
  async cancel() {
    this.cancelled++;
  }
}

describe('the launch fragment', () => {
  const history = () => {
    const calls: string[] = [];
    return {
      calls,
      state: { kept: true },
      replaceState(_: unknown, __: string, url?: string | URL | null) {
        calls.push(String(url));
      },
    };
  };
  const ticketText = 'k3J'.padEnd(43, 'x');

  it('is read once and removed from the address bar and history', () => {
    const h = history();
    const launch = readErpLaunch(
      { hash: `#launch=${ticketText}`, pathname: '/erp', search: '' },
      h,
      'https://api.cashgearerp.com/',
    );
    expect(launch).toEqual({ ticket: ticketText, apiBase: 'https://api.cashgearerp.com' });
    expect(h.calls).toEqual(['/']);
  });

  it('is removed even when its ticket is malformed, and nothing else is touched', () => {
    const h = history();
    expect(readErpLaunch({ hash: '#launch=short', pathname: '/erp', search: '' }, h, 'x')).toBe(
      null,
    );
    expect(h.calls).toEqual(['/']);
    const other = history();
    expect(readErpLaunch({ hash: '#other', pathname: '/', search: '?a=1' }, other, 'x')).toBe(null);
    expect(other.calls).toEqual([]);
  });

  it('never takes the ERP’s origin from the link', () => {
    expect(erpApiBase('https://sheet.cashgearerp.com')).toBe('https://api.cashgearerp.com');
    expect(erpApiBase('http://127.0.0.1:4173')).toBe('http://127.0.0.1:4180');
    expect(erpApiBase('http://localhost:5173')).toBe('http://127.0.0.1:4180');
    expect(erpApiBase('https://sheet.cashgearerp.com', 'http://localhost:5206/')).toBe(
      'http://localhost:5206',
    );
  });
});

describe('HTTP', () => {
  it('maps statuses and codes to what the sheet says', () => {
    expect(kindOf(401, 'ticket.invalid')).toBe('ticket');
    expect(kindOf(401, 'session.expired')).toBe('session');
    expect(kindOf(401, null)).toBe('session');
    expect(kindOf(403, 'origin.denied')).toBe('origin');
    expect(kindOf(403, 'privilege.denied')).toBe('privilege');
    expect(kindOf(413, null)).toBe('tooMany');
    expect(kindOf(429, null)).toBe('busy');
    expect(kindOf(400, 'request.invalid')).toBe('invalid');
    expect(kindOf(500, null)).toBe('server');
    expect(retryAfterMs('3')).toBe(3000);
    expect(retryAfterMs(new Date(10_000).toUTCString(), 4_000)).toBe(6000);
    expect(retryAfterMs(null)).toBe(null);
  });

  it('waits as 429 and 503 ask, then goes on', async () => {
    await fetch(`${base}/__mock/fault`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ match: 'VendorSheet/Schema$', status: 503, retryAfter: 2, times: 2 }),
    });
    const waits: number[] = [];
    const client = new ErpHttp({
      baseUrl: base,
      fetch: withOrigin,
      sleep: async (ms) => void waits.push(ms),
    });
    const session = await ErpSession.redeem(client, await ticket());
    const schema = await session.json('GET', session.route('Schema'));
    expect((schema as { entity: string }).entity).toBe('vendors');
    expect(waits).toEqual([2000, 2000]);
  });

  it('gives up as busy, and calls a failure to connect a network error', async () => {
    await fetch(`${base}/__mock/fault`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ match: 'VendorSheet/Schema$', status: 429, retryAfter: 1, times: 0 }),
    });
    const client = http();
    const session = await ErpSession.redeem(client, await ticket());
    const busy = await session.json('GET', session.route('Schema')).catch((e: unknown) => e);
    expect(busy).toBeInstanceOf(ErpError);
    expect((busy as ErpError).kind).toBe('busy');
    const offline = new ErpHttp({ baseUrl: 'http://127.0.0.1:9', retries: 0 });
    const error = (await offline.json('GET', 'x').catch((e: unknown) => e)) as ErpError;
    expect(error.kind).toBe('network');
  });

  it('refuses a redirect, so the session header never follows one', async () => {
    const { createServer } = await import('node:http');
    const seen: (string | undefined)[] = [];
    const target = createServer((req, res) => {
      seen.push(req.headers['x-sheet-session'] as string | undefined);
      res.writeHead(200, { 'Content-Type': 'application/json' }).end('{}');
    });
    await new Promise<void>((done) => target.listen(0, '127.0.0.1', done));
    const to = `http://127.0.0.1:${(target.address() as { port: number }).port}/stolen`;
    const redirecting = createServer((_req, res) => res.writeHead(307, { Location: to }).end());
    await new Promise<void>((done) => redirecting.listen(0, '127.0.0.1', done));
    try {
      const client = new ErpHttp({
        baseUrl: `http://127.0.0.1:${(redirecting.address() as { port: number }).port}`,
        retries: 0,
      });
      const error = (await client
        .json('GET', 'api/payable/VendorSheet/Schema', { session: 'secret-token' })
        .catch((e: unknown) => e)) as ErpError;
      expect(error).toBeInstanceOf(ErpError);
      expect(error.kind).toBe('network');
      expect(seen).toEqual([]);
    } finally {
      redirecting.close();
      target.close();
    }
  });

  it('times out and cancels', async () => {
    await fetch(`${base}/__mock/fault`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ match: 'Session/Redeem$', latencyMs: 400, times: 0 }),
    });
    const slow = new ErpHttp({ baseUrl: base, fetch: withOrigin, timeoutMs: 50 });
    const timedOut = (await ErpSession.redeem(slow, await ticket()).catch((e) => e)) as ErpError;
    expect(timedOut.kind).toBe('network');
    const controller = new AbortController();
    const pending = ErpSession.redeem(http(), await ticket(), { signal: controller.signal });
    controller.abort();
    expect(((await pending.catch((e) => e)) as ErpError).kind).toBe('cancelled');
  });
});

describe('the session', () => {
  it('keeps its token to itself', async () => {
    const session = await ErpSession.redeem(http(), await ticket({ blocked: false }));
    expect(session.company).toBe('Kit');
    expect(session.filter).toEqual({ blocked: false });
    expect(Object.keys(session)).not.toContain('token');
    expect(JSON.stringify(session)).not.toMatch(/token/i);
    expect(JSON.parse(JSON.stringify(session))).toMatchObject({
      company: 'Kit',
      userName: 'Sara',
      entityRoute: 'api/payable/VendorSheet',
    });
    expect(String(session)).not.toMatch(/[A-Za-z0-9_-]{43}/);
  });

  it('refuses a ticket used twice, as a used ticket', async () => {
    const value = await ticket();
    await ErpSession.redeem(http(), value);
    const again = (await ErpSession.redeem(http(), value).catch((e) => e)) as ErpError;
    expect(again.kind).toBe('ticket');
    expect(again.code).toBe('ticket.invalid');
    expect(again.correlationId).toBeTruthy();
  });

  it('ends when the ERP answers 401, and when its time runs out', async () => {
    const session = await ErpSession.redeem(http(), await ticket());
    const states: string[] = [];
    session.onState((state) => states.push(state));
    await fetch(`${base}/__mock/key`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ key: 'mock-kit-editor', active: false }),
    });
    const ended = (await session.json('GET', session.route('Schema')).catch((e) => e)) as ErpError;
    expect(ended.kind).toBe('session');
    expect(session.state).toBe('expired');
    expect(states).toEqual(['expired']);
    // A request after the end isn't sent.
    const after = (await session.json('GET', session.route('Schema')).catch((e) => e)) as ErpError;
    expect(after.kind).toBe('session');

    let now = Date.now();
    const clocked = await ErpSession.redeem(http(), await ticket(null, 'mock-kit-adv'), {
      now: () => now,
    });
    expect(clocked.state).toBe('open');
    now += 31 * 60_000;
    expect(clocked.state).toBe('expired');
  });
});

describe('records', () => {
  it('open page by page with progress, the bytes going to the sink unread', async () => {
    const sink = new TestSink();
    const progress: string[] = [];
    const opened = await openRecords({
      http: http(),
      ticket: await ticket({ blocked: false }),
      sink,
      onProgress: (p) => progress.push(`${p.stage} ${p.loaded}/${p.total ?? '?'}`),
    });
    expect(opened.session.company).toBe('Kit');
    expect(opened.schema.fields.map((f) => f.name).slice(0, 4)).toEqual([
      'vendorAccount',
      'vendorName',
      'vendorGroup',
      'mobile',
    ]);
    expect(JSON.parse(sink.schema!).keyField).toBe('vendorAccount');
    expect(opened.loaded).toBe(sink.keys.length);
    expect(new Set(sink.keys).size).toBe(sink.keys.length);
    const total = Number(progress.at(-1)!.split('/')[1]);
    expect(opened.loaded).toBe(total);
    expect(progress[0]).toBe('connecting 0/?');
    expect(progress.filter((p) => p.startsWith('loading')).length).toBeGreaterThanOrEqual(4);
    expect(sink.cancelled).toBe(0);
  });

  it('open what the list searched: its account, name, group, mobile or phone', async () => {
    const sink = new TestSink();
    await openRecords({
      http: http(),
      ticket: await ticket({ blocked: false, search: '0227654' }),
      sink,
    });
    expect(sink.keys).toEqual(['V-1006']);
  });

  it('stop with the sink’s own words when it refuses the list, and drop what was staged', async () => {
    const sink = new TestSink();
    sink.refuseOver = 100;
    const error = (await openRecords({ http: http(), ticket: await ticket(), sink }).catch(
      (e) => e,
    )) as ErpError;
    expect(error.kind).toBe('contract');
    expect(error.message).toBe('This list has 2345 vendors.');
    expect(sink.cancelled).toBe(1);
  });

  it('cancel part of the way, dropping what was staged', async () => {
    const sink = new TestSink();
    const controller = new AbortController();
    const error = (await openRecords({
      http: http(),
      ticket: await ticket(),
      sink,
      signal: controller.signal,
      onProgress: (p) => {
        if (p.loaded > 0) controller.abort();
      },
    }).catch((e) => e)) as ErpError;
    expect(error.kind).toBe('cancelled');
    expect(sink.cancelled).toBe(1);
    expect(sink.keys.length).toBeLessThan(2345);
  });

  it('refresh by id, in pages, whatever the filter matches now', async () => {
    const sink = new TestSink();
    const opened = await openRecords({
      http: http(),
      ticket: await ticket({ search: 'noor' }),
      sink,
    });
    expect(sink.keys).toEqual(['V-1001', 'V-1006']);
    await fetch(`${base}/__mock/edit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ company: 'Kit', key: 'V-1001', values: { vendorName: 'Renamed' } }),
    });
    const refreshing = new TestSink();
    const count = await refreshRecords({
      session: opened.session,
      sink: refreshing,
      ids: sink.ids,
      pageSize: 1,
    });
    expect(refreshing.refreshing).toBe(true);
    expect(count).toBe(2);
    expect(refreshing.keys).toEqual(['V-1001', 'V-1006']);
  });
});
