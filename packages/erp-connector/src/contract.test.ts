// The ERP sheet contract suite (`docs/erp-contract.md`). It speaks raw HTTP, so it tests the
// wire rather than the connector, and runs against any server:
//
//   pnpm test                                     starts the mock in tests/erp-mock
//   ERP_BASE_URL=http://localhost:5000 pnpm test  another server, such as UnifiedAPI on LocalDB
//
// Against another server, the API keys come from ERP_TEST_KEY_EDITOR (Edit), ERP_TEST_KEY_ADV
// (AdvEdit), ERP_TEST_KEY_VIEWER (View only), ERP_TEST_KEY_NONE (no Vendors privilege) and
// ERP_TEST_KEY_OTHER (an editor in another company in the same database). A case whose key is
// missing skips, and so do the cases that need the mock's clock or failure injection, unless
// ERP_BASE_IS_MOCK=1 says the server is the mock. The suite writes to the records it reads, so
// it refuses a server that isn't on this machine unless ERP_ALLOW_WRITES=1.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { LIMITS, startMockErp } from '../../../tests/erp-mock/server.mjs';
import {
  API_KEY_HEADER,
  CORRELATION_HEADER,
  ROUTES,
  SESSION_HEADER,
  isErrorCode,
  type CommitRequest,
  type CreateTicketRequest,
  type EntitySchema,
  type QueryRequest,
  type QueryResponse,
  type RecordChange,
  type RecordSnapshot,
  type SheetSession,
  type WireValue,
} from './contract.ts';
import {
  readCommitResponse,
  readCommitStatus,
  readConflictBody,
  readCreateTicketResponse,
  readEntitySchema,
  readErrorBody,
  readQueryResponse,
  readRenewResponse,
  readSheetSession,
  recordProblems,
} from './validate.ts';

const env =
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env ?? {};
const external = env.ERP_BASE_URL?.replace(/\/+$/, '');
const onMock = !external || env.ERP_BASE_IS_MOCK === '1';
const ORIGIN = env.ERP_TEST_ORIGIN ?? 'http://localhost:4173';

type Role = 'editor' | 'adv' | 'viewer' | 'none' | 'other';
const KEYS: Record<Role, string | undefined> = onMock
  ? {
      editor: 'mock-kit-editor',
      adv: 'mock-kit-adv',
      viewer: 'mock-kit-viewer',
      none: 'mock-kit-none',
      other: 'mock-dot-editor',
    }
  : {
      editor: env.ERP_TEST_KEY_EDITOR,
      adv: env.ERP_TEST_KEY_ADV,
      viewer: env.ERP_TEST_KEY_VIEWER,
      none: env.ERP_TEST_KEY_NONE,
      other: env.ERP_TEST_KEY_OTHER,
    };
const lacks = (...roles: Role[]) => roles.some((role) => !KEYS[role]);
/** Records the suite leaves alone: the mock's locked vendor, or `ERP_TEST_SKIP_KEYS`. */
const SKIP = new Set(
  onMock ? ['V-1005'] : (env.ERP_TEST_SKIP_KEYS ?? '').split(',').filter(Boolean),
);

/** The Vendors fields and who may edit them (decisions, 2026-11-09, Phase 14). */
const VENDOR_FIELDS = {
  edit: ['vendorName', 'taxFile', 'mobile', 'address', 'payMethod', 'vendorGroup'],
  advEdit: ['blocked', 'currency'],
  readOnly: ['vendorAccount', 'bankAccount', 'debitLimit', 'modifiedBy', 'modifiedDate'],
};

let base = external ?? '';
let stopMock: (() => Promise<void>) | undefined;

beforeAll(async () => {
  if (!external) {
    const server = await startMockErp();
    base = server.url;
    stopMock = server.close;
  } else if (
    !/^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/.test(base) &&
    env.ERP_ALLOW_WRITES !== '1'
  ) {
    throw new Error(
      `The contract suite writes to ${base}'s records. Set ERP_ALLOW_WRITES=1 if it is a test server.`,
    );
  }
});

afterAll(async () => {
  await stopMock?.();
});

beforeEach(async () => {
  if (onMock) await control('reset');
});

// ---------------------------------------------------------------------------------------------
// HTTP

interface Answer {
  readonly status: number;
  readonly headers: Headers;
  readonly json: unknown;
}

interface SendOptions {
  readonly body?: unknown;
  readonly session?: string;
  readonly apiKey?: string;
  /** The Origin sent; the sheet's by default, and none with `null`. */
  readonly origin?: string | null;
  readonly headers?: Record<string, string>;
}

/**
 * Sends one request and checks what every response must carry: a correlation id, `no-store`
 * on the sheet's routes, and an error body (repeating the id, with known codes) on errors.
 */
async function send(
  method: 'GET' | 'POST' | 'OPTIONS',
  route: string,
  options: SendOptions = {},
): Promise<Answer> {
  const headers: Record<string, string> = { ...options.headers };
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';
  if (options.session !== undefined) headers[SESSION_HEADER] = options.session;
  if (options.apiKey !== undefined) headers[API_KEY_HEADER] = options.apiKey;
  const origin = options.origin === undefined ? ORIGIN : options.origin;
  if (origin) headers.Origin = origin;
  const response = await fetch(`${base}/${route}`, {
    method,
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });
  const raw = await response.text();
  const json: unknown = raw === '' ? null : JSON.parse(raw);
  if (method !== 'OPTIONS') {
    const correlationId = response.headers.get(CORRELATION_HEADER);
    expect(correlationId, `${route} sends ${CORRELATION_HEADER}`).toBeTruthy();
    if (route !== ROUTES.createTicket) {
      expect(response.headers.get('Cache-Control'), `${route} is no-store`).toContain('no-store');
    }
    if (response.status === 409) {
      expect(readConflictBody(json).correlationId).toBe(correlationId);
    } else if (response.status >= 400 && json !== null) {
      const body = readErrorBody(json);
      expect(body.correlationId).toBe(correlationId);
      for (const error of body.errors)
        expect(isErrorCode(error.code), `known code ${error.code}`).toBe(true);
    }
  }
  return { status: response.status, headers: response.headers, json };
}

/** The error codes of an error answer, in order. */
function codes(answer: Answer): string[] {
  return readErrorBody(answer.json).errors.map((e) => e.code);
}

async function control(route: string, body?: unknown): Promise<unknown> {
  const response = await fetch(`${base}/__mock/${route}`, {
    method: body === undefined && route.startsWith('state') ? 'GET' : 'POST',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  expect(response.status, `/__mock/${route}`).toBeLessThan(300);
  const raw = await response.text();
  return raw === '' ? null : JSON.parse(raw);
}

const advance = (ms: number) => control('clock', { advanceMs: ms });
const mockState = async (company = 'Kit') =>
  (await control(`state?company=${company}`)) as { writes: number; commits: number; audit: number };

// ---------------------------------------------------------------------------------------------
// Flow

async function ticket(role: Role = 'editor', request: Partial<CreateTicketRequest> = {}) {
  const answer = await send('POST', ROUTES.createTicket, {
    apiKey: KEYS[role],
    body: { entity: 'vendors', filter: null, language: 'en', ...request },
  });
  expect(answer.status).toBe(200);
  return readCreateTicketResponse(answer.json);
}

const redeem = (value: string, origin?: string | null) =>
  send('POST', ROUTES.redeem, { body: { ticket: value }, origin });

async function open(
  role: Role = 'editor',
  request: Partial<CreateTicketRequest> = {},
): Promise<SheetSession> {
  const answer = await redeem((await ticket(role, request)).ticket);
  expect(answer.status).toBe(200);
  return readSheetSession(answer.json);
}

const at = (session: SheetSession, name: string) => `${session.entityRoute}/${name}`;

async function schemaOf(session: SheetSession): Promise<EntitySchema> {
  const answer = await send('GET', at(session, 'Schema'), { session: session.token });
  expect(answer.status).toBe(200);
  return readEntitySchema(answer.json);
}

function query(session: SheetSession, request: Partial<QueryRequest>): Promise<Answer> {
  return send('POST', at(session, 'Query'), {
    session: session.token,
    body: { filter: null, ids: null, afterId: 0, pageSize: 100, ...request },
  });
}

async function page(session: SheetSession, request: Partial<QueryRequest>): Promise<QueryResponse> {
  const answer = await query(session, request);
  expect(answer.status).toBe(200);
  return readQueryResponse(answer.json);
}

async function byIds(
  session: SheetSession,
  records: readonly { id: number }[],
): Promise<Map<number, RecordSnapshot>> {
  const found = await page(session, { ids: records.map((r) => r.id), pageSize: 1000 });
  return new Map(found.records.map((r) => [r.id, r]));
}

/** `n` records the suite may write to, in id order. */
async function pick(session: SheetSession, n: number): Promise<RecordSnapshot[]> {
  const found = await page(session, {
    filter: { blocked: false },
    pageSize: Math.min(1000, n + 20),
  });
  const records = found.records.filter((r) => !SKIP.has(r.key)).slice(0, n);
  expect(records.length, `the server has ${n} vendors to write to`).toBe(n);
  return records;
}

let schemaVersion = 'vendors.1';

function commit(
  session: SheetSession,
  records: readonly RecordChange[],
  request: Partial<Omit<CommitRequest, 'records'>> = {},
): Promise<Answer> {
  return send('POST', at(session, 'Commit'), {
    session: session.token,
    body: {
      commitId: crypto.randomUUID(),
      company: session.company,
      schemaVersion,
      dryRun: false,
      records,
      ...request,
    },
  });
}

/** A change to some of a record's fields, from the values it was read with. */
function change(
  record: RecordSnapshot,
  to: Record<string, WireValue>,
  version = record.version,
): RecordChange {
  return {
    id: record.id,
    key: record.key,
    version,
    changes: Object.fromEntries(
      Object.entries(to).map(([name, value]) => [
        name,
        { from: record.values[name] ?? null, to: value },
      ]),
    ),
  };
}

let mobiles = 0;
/** A mobile no record has yet, with the leading zero the ERP's mobiles have. */
const freshMobile = () =>
  `01${String(Date.now() % 1e7).padStart(7, '0')}${String(mobiles++ % 100).padStart(2, '0')}`;

/** An active code in `list` other than `from`. */
const differentCode = (schema: EntitySchema, list: string, from: WireValue) =>
  schema.lookups[list]!.filter((item) => item.active !== false)
    .map((item) => item.code)
    .find((code) => code !== from)!;

// ---------------------------------------------------------------------------------------------

describe('tickets', () => {
  it('are 256 random bits that expire within minutes, with the ticket in the URL fragment', async () => {
    const before = Date.now();
    const a = await ticket();
    const b = await ticket();
    expect(a.ticket).not.toBe(b.ticket);
    const expires = Date.parse(a.expiresAt);
    expect(expires).toBeGreaterThan(before);
    expect(expires - before).toBeLessThanOrEqual(10 * 60_000);
    expect(new URL(a.url).hash).toBe(`#launch=${a.ticket}`);
    expect(new URL(a.url).search).toBe('');
  });

  it.skipIf(lacks('editor'))(
    'carry the user, company, entity, filter and language into the session',
    async () => {
      const filter = { blocked: false, search: 'noor' };
      const session = await open('editor', { filter, language: 'ar' });
      expect(session.entity).toBe('vendors');
      expect(session.entityRoute).toBe('api/payable/VendorSheet');
      expect(session.filter).toEqual(filter);
      expect(session.language).toBe('ar');
      if (onMock)
        expect([session.company, session.userId, session.userName]).toEqual(['Kit', 'u17', 'Sara']);
    },
  );

  it('redeem once: a second redeem is refused', async () => {
    const { ticket: value } = await ticket();
    expect((await redeem(value)).status).toBe(200);
    const again = await redeem(value);
    expect(again.status).toBe(401);
    expect(codes(again)).toEqual(['ticket.invalid']);
  });

  it('that are unknown are refused like used ones', async () => {
    const answer = await redeem('A'.repeat(43));
    expect(answer.status).toBe(401);
    expect(codes(answer)).toEqual(['ticket.invalid']);
  });

  it.skipIf(!onMock)('expire after 120 seconds', async () => {
    const early = await ticket();
    const late = await ticket();
    await advance(LIMITS.ticketMs - 1000);
    expect((await redeem(early.ticket)).status).toBe(200);
    await advance(2000);
    const answer = await redeem(late.ticket);
    expect(answer.status).toBe(401);
    expect(codes(answer)).toEqual(['ticket.invalid']);
  });

  it('redeem only from an allow-listed Origin, and a refused Origin does not use the ticket up', async () => {
    const { ticket: value } = await ticket();
    const elsewhere = await redeem(value, 'https://evil.example');
    expect(elsewhere.status).toBe(403);
    expect(codes(elsewhere)).toEqual(['origin.denied']);
    const none = await redeem(value, null);
    expect(none.status).toBe(403);
    expect((await redeem(value)).status).toBe(200);
  });

  it('call nothing on their own', async () => {
    const { ticket: value } = await ticket();
    const answer = await send('GET', 'api/payable/VendorSheet/Schema', { session: value });
    expect(answer.status).toBe(401);
    expect(codes(answer)).toEqual(['session.expired']);
  });

  it('need an API key, not a sheet session', async () => {
    const session = await open();
    const answer = await send('POST', ROUTES.createTicket, {
      session: session.token,
      body: { entity: 'vendors', filter: null, language: 'en' },
    });
    expect(answer.status).toBe(401);
  });

  it.skipIf(lacks('none'))('need the Vendors View privilege', async () => {
    const answer = await send('POST', ROUTES.createTicket, {
      apiKey: KEYS.none,
      body: { entity: 'vendors', filter: null, language: 'en' },
    });
    expect(answer.status).toBe(403);
    expect(codes(answer)).toEqual(['privilege.denied']);
  });
});

describe('sessions', () => {
  it('last 30 minutes idle and 10 hours at most', async () => {
    const before = Date.now();
    const session = await open();
    const idle = Date.parse(session.idleExpiresAt) - before;
    const max = Date.parse(session.maxExpiresAt) - before;
    expect(idle).toBeGreaterThan(LIMITS.idleMs - 60_000);
    expect(idle).toBeLessThanOrEqual(LIMITS.idleMs + 60_000);
    expect(max).toBeGreaterThan(LIMITS.maxMs - 60_000);
    expect(max).toBeLessThanOrEqual(LIMITS.maxMs + 60_000);
  });

  it.skipIf(!onMock)('expire when idle, and only Renew slides the idle time', async () => {
    const session = await open();
    await advance(LIMITS.idleMs - 1000);
    expect((await send('GET', at(session, 'Schema'), { session: session.token })).status).toBe(200);
    await advance(2000);
    const answer = await send('GET', at(session, 'Schema'), { session: session.token });
    expect(answer.status).toBe(401);
    expect(codes(answer)).toEqual(['session.expired']);
    expect((await send('POST', ROUTES.renew, { session: session.token })).status).toBe(401);
  });

  it.skipIf(!onMock)('slide on Renew', async () => {
    const session = await open();
    await advance(20 * 60_000);
    const renewed = await send('POST', ROUTES.renew, { session: session.token });
    expect(renewed.status).toBe(200);
    const { idleExpiresAt } = readRenewResponse(renewed.json);
    expect(Date.parse(idleExpiresAt)).toBeGreaterThan(Date.parse(session.idleExpiresAt));
    await advance(20 * 60_000);
    expect((await send('GET', at(session, 'Schema'), { session: session.token })).status).toBe(200);
  });

  it.skipIf(!onMock)('end at their maximum however often they renew', async () => {
    const session = await open();
    const max = Date.parse(session.maxExpiresAt);
    for (let t = 0; t < LIMITS.maxMs - 25 * 60_000; t += 25 * 60_000) {
      await advance(25 * 60_000);
      const renewed = await send('POST', ROUTES.renew, { session: session.token });
      expect(renewed.status).toBe(200);
      expect(Date.parse(readRenewResponse(renewed.json).idleExpiresAt)).toBeLessThanOrEqual(max);
    }
    await advance(max - Date.parse(((await control('clock', {})) as { now: string }).now) + 1000);
    const answer = await send('GET', at(session, 'Schema'), { session: session.token });
    expect(answer.status).toBe(401);
    expect(codes(answer)).toEqual(['session.expired']);
  });

  it('end on End', async () => {
    const session = await open();
    const ended = await send('POST', ROUTES.end, { session: session.token });
    expect(ended.status).toBe(204);
    expect((await send('GET', at(session, 'Schema'), { session: session.token })).status).toBe(401);
    expect((await send('POST', ROUTES.renew, { session: session.token })).status).toBe(401);
  });

  it.skipIf(!onMock)('end when their API key is revoked', async () => {
    const session = await open();
    await control('key', { key: KEYS.editor, active: false });
    expect((await send('GET', at(session, 'Schema'), { session: session.token })).status).toBe(401);
  });

  it('are the only way into the entity routes: an API key is refused there', async () => {
    const session = await open();
    const answer = await send('GET', at(session, 'Schema'), { apiKey: KEYS.editor });
    expect(answer.status).toBe(401);
  });
});

describe('schema', () => {
  it('describes the vendors with their key, limits, labels and lookups', async () => {
    const schema = await schemaOf(await open());
    expect(schema.entity).toBe('vendors');
    expect(schema.keyField).toBe('vendorAccount');
    expect(schema.idField).toBe('recId');
    expect([schema.maxPageSize, schema.maxRecords, schema.maxCommitRecords]).toEqual([
      1000, 10_000, 500,
    ]);
    const names = schema.fields.map((f) => f.name).sort();
    expect(names).toEqual(
      [...VENDOR_FIELDS.edit, ...VENDOR_FIELDS.advEdit, ...VENDOR_FIELDS.readOnly].sort(),
    );
    for (const secret of ['token', 'secRule', 'recId', 'company'])
      expect(names).not.toContain(secret);
    const field = (name: string) => schema.fields.find((f) => f.name === name)!;
    expect(field('vendorName')).toMatchObject({ type: 'text', maxLength: 150, required: true });
    expect(field('payMethod')).toMatchObject({
      type: 'lookup',
      lookup: 'paymentModes',
      required: true,
    });
    expect(field('vendorGroup')).toMatchObject({
      type: 'lookup',
      lookup: 'vendorGroups',
      required: true,
    });
    expect(field('currency')).toMatchObject({
      type: 'lookup',
      lookup: 'currencies',
      required: true,
    });
    expect(field('blocked').type).toBe('boolean');
    expect(field('debitLimit')).toMatchObject({ type: 'decimal', scale: 2 });
    expect(field('modifiedDate').type).toBe('dateTime');
  });

  const editability = async (role: Role) =>
    Object.fromEntries(
      (await schemaOf(await open(role))).fields.map((f) => [f.name, f.editReason ?? 'editable']),
    );

  it.skipIf(lacks('editor'))(
    'says what an editor may edit, and why the rest is read-only',
    async () => {
      const fields = await editability('editor');
      for (const name of VENDOR_FIELDS.edit) expect(fields[name], name).toBe('editable');
      for (const name of VENDOR_FIELDS.advEdit) expect(fields[name], name).toBe('needsAdvEdit');
      for (const name of VENDOR_FIELDS.readOnly) expect(fields[name], name).toBe('notEditable');
    },
  );

  it.skipIf(lacks('adv'))('lets AdvEdit edit Blocked and Currency', async () => {
    const fields = await editability('adv');
    for (const name of [...VENDOR_FIELDS.edit, ...VENDOR_FIELDS.advEdit])
      expect(fields[name], name).toBe('editable');
    for (const name of VENDOR_FIELDS.readOnly) expect(fields[name], name).toBe('notEditable');
  });

  it.skipIf(lacks('viewer'))('makes everything read-only for a viewer', async () => {
    const fields = await editability('viewer');
    for (const name of VENDOR_FIELDS.edit) expect(fields[name], name).toBe('needsEdit');
    for (const name of VENDOR_FIELDS.advEdit) expect(fields[name], name).toBe('needsAdvEdit');
  });
});

describe('query', () => {
  it('pages by id to the end, each record in the schema’s wire formats', async () => {
    const session = await open();
    const schema = await schemaOf(session);
    const seen: RecordSnapshot[] = [];
    let afterId = 0;
    let total = -1;
    for (let pages = 0; pages < 20; pages++) {
      const found = await page(session, { afterId, pageSize: schema.maxPageSize });
      if (total < 0) total = found.total;
      expect(found.total).toBe(total);
      seen.push(...found.records);
      if (found.nextAfterId === null) break;
      expect(found.records.length).toBe(schema.maxPageSize);
      afterId = found.nextAfterId;
    }
    expect(seen.length).toBe(total);
    for (let i = 1; i < seen.length; i++) expect(seen[i]!.id).toBeGreaterThan(seen[i - 1]!.id);
    expect(new Set(seen.map((r) => r.key)).size).toBe(seen.length);
    for (const record of seen) expect(recordProblems(schema, record), record.key).toEqual([]);

    const small = [];
    afterId = 0;
    for (let pages = 0; pages < 3; pages++) {
      const found = await page(session, { afterId, pageSize: 7 });
      small.push(...found.records);
      afterId = found.nextAfterId!;
    }
    expect(small).toEqual(seen.slice(0, 21));

    if (onMock) {
      expect(total).toBe(2345);
      expect(seen.filter((r) => r.key === 'V-1001').map((r) => r.values.vendorName)).toEqual([
        'Al Noor Trading',
      ]);
      expect(seen.some((r) => r.key.startsWith('D-'))).toBe(false);
    }
  });

  it.skipIf(!onMock)('carries text, decimals, Arabic and empty values as they are', async () => {
    const found = await page(await open(), { pageSize: 10 });
    const vendor = (key: string) => found.records.find((r) => r.key === key)!.values;
    expect(vendor('V-1001')).toMatchObject({
      mobile: '01001234567',
      taxFile: '001-234-567',
      debitLimit: '15000.00',
    });
    expect(vendor('V-1002')).toMatchObject({
      vendorName: 'مؤسسة النور للتوريدات',
      taxFile: '0045',
      currency: 'USD',
    });
    expect(vendor('V-1003')).toMatchObject({
      blocked: true,
      address: null,
      mobile: null,
      debitLimit: '2500.50',
    });
    expect(vendor('V-1004').taxFile).toBe('001');
    expect(vendor('V-1007').currency).toBeNull();
    expect(found.records.every((r) => !('token' in r.values))).toBe(true);
  });

  it('filters by search and by field', async () => {
    const session = await open();
    const noor = await page(session, { filter: { search: 'NOOR' }, pageSize: 1000 });
    for (const r of noor.records) {
      expect(`${r.key} ${r.values.vendorName}`.toLowerCase()).toContain('noor');
    }
    expect(noor.total).toBe(noor.records.length);
    const unblocked = await page(session, { filter: { blocked: false }, pageSize: 1000 });
    expect(unblocked.records.every((r) => r.values.blocked === false)).toBe(true);
    if (onMock) {
      expect(noor.records.map((r) => r.key)).toEqual(['V-1001', 'V-1006']);
      expect(unblocked.total).toBeLessThan(2345);
    }
  });

  it('by ids returns those still there, in id order, whatever the filter', async () => {
    const session = await open();
    const [a, , c] = await pick(session, 3);
    const found = await page(session, {
      ids: [c!.id, 2_000_000_000, a!.id],
      filter: { search: 'no vendor is called this' },
    });
    expect(found.records.map((r) => r.id)).toEqual([a!.id, c!.id]);
    expect(found.total).toBe(2);
  });

  it.skipIf(lacks('other'))(
    'never shows another company’s records, though they share the database',
    async () => {
      const kit = await open('editor');
      const dot = await open('other');
      const kitRecords = await pick(kit, 3);
      const found = await page(dot, { ids: kitRecords.map((r) => r.id) });
      expect(found.records).toEqual([]);
      const dotAll = await page(dot, { pageSize: 1000 });
      const kitIds = new Set((await page(kit, { pageSize: 1000 })).records.map((r) => r.id));
      expect(dotAll.records.some((r) => kitIds.has(r.id))).toBe(false);
      if (onMock)
        expect(dotAll.records.find((r) => r.key === 'V-1001')?.values.vendorName).toBe(
          'Dot Al Noor',
        );
    },
  );

  it('refuses a page size past the limit, and more ids than a sheet holds', async () => {
    const session = await open();
    for (const pageSize of [0, 1001]) {
      const answer = await query(session, { pageSize });
      expect(answer.status, `pageSize ${pageSize}`).toBe(400);
      expect(codes(answer)).toEqual(['request.invalid']);
    }
    const ids = Array.from({ length: 10_001 }, (_, i) => i + 1);
    const answer = await query(session, { ids });
    expect(answer.status).toBe(413);
    expect(codes(answer)).toEqual(['batch.tooLarge']);
  });
});

describe('commit', () => {
  beforeEach(async () => {
    schemaVersion = (await schemaOf(await open())).schemaVersion;
  });

  it('writes the changes, moves the versions and stamps who changed them', async () => {
    const session = await open();
    const schema = await schemaOf(session);
    const [a, b] = await pick(session, 2);
    const mobile = freshMobile();
    const payMethod = differentCode(schema, 'paymentModes', b!.values.payMethod!);
    const answer = await commit(session, [
      change(a!, { mobile }),
      change(b!, { payMethod, taxFile: '007' }),
    ]);
    expect(answer.status).toBe(200);
    const result = readCommitResponse(answer.json);
    expect(result.dryRun).toBe(false);
    expect(result.committedAt).not.toBeNull();
    const byId = new Map(result.records.map((r) => [r.id, r]));
    expect(byId.get(a!.id)!.values.mobile).toBe(mobile);
    expect(byId.get(b!.id)!.values).toMatchObject({ payMethod, taxFile: '007' });
    for (const r of result.records) {
      expect(r.version).not.toBe([a, b].find((x) => x!.id === r.id)!.version);
      expect(r.values.modifiedBy).toBe(session.userId);
      expect(recordProblems(schema, r)).toEqual([]);
    }
    const now = await byIds(session, [a!, b!]);
    expect(now.get(a!.id)).toEqual(byId.get(a!.id));
    expect(now.get(b!.id)).toEqual(byId.get(b!.id));
  });

  it('keeps text as text, and takes "" as empty', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const answer = await commit(session, [change(a!, { mobile: '0012', address: '' })]);
    expect(answer.status).toBe(200);
    expect(readCommitResponse(answer.json).records[0]!.values).toMatchObject({
      mobile: '0012',
      address: null,
    });
  });

  it('on a dry run checks everything and writes nothing', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const before = onMock ? await mockState() : null;
    const mobile = freshMobile();
    const commitId = crypto.randomUUID();
    const answer = await commit(session, [change(a!, { mobile })], { dryRun: true, commitId });
    expect(answer.status).toBe(200);
    const result = readCommitResponse(answer.json);
    expect(result).toMatchObject({ dryRun: true, committedAt: null });
    expect(result.records[0]!.version).toBe(a!.version);
    expect(result.records[0]!.values.mobile).toBe(mobile);
    expect((await byIds(session, [a!])).get(a!.id)).toEqual(a);
    const status = await send('GET', at(session, `CommitStatus/${commitId}`), {
      session: session.token,
    });
    expect(readCommitStatus(status.json).status).toBe('unknown');
    if (before) expect(await mockState()).toEqual(before);

    const invalid = await commit(session, [change(a!, { vendorName: null })], { dryRun: true });
    expect(invalid.status).toBe(400);
    expect(codes(invalid)).toEqual(['required']);
  });

  it('is all or nothing: one bad record writes none', async () => {
    const session = await open();
    const [a, b, c] = await pick(session, 3);
    const answer = await commit(session, [
      change(a!, { mobile: freshMobile() }),
      change(b!, { vendorName: 'x'.repeat(151) }),
      change(c!, { mobile: freshMobile() }),
    ]);
    expect(answer.status).toBe(400);
    expect(readErrorBody(answer.json).errors).toMatchObject([
      { id: b!.id, key: b!.key, field: 'vendorName', code: 'maxLength' },
    ]);
    const now = await byIds(session, [a!, b!, c!]);
    for (const r of [a!, b!, c!]) expect(now.get(r.id)).toEqual(r);
  });

  it('lists every field error before writing anything', async () => {
    const session = await open();
    const [a, b, c] = await pick(session, 3);
    const answer = await commit(session, [
      change(a!, { vendorName: null }),
      change(b!, { payMethod: 'NO-SUCH-METHOD', mobile: '0'.repeat(51) }),
      change(c!, { vendorGroup: 7 }),
    ]);
    expect(answer.status).toBe(400);
    const errors = readErrorBody(answer.json)
      .errors.map((e) => `${e.id} ${e.field} ${e.code}`)
      .sort();
    expect(errors).toEqual(
      [
        `${a!.id} vendorName required`,
        `${b!.id} payMethod lookup.notFound`,
        `${b!.id} mobile maxLength`,
        `${c!.id} vendorGroup type`,
      ].sort(),
    );
  });

  it('answers 409 listing every record changed since it was read, and writes nothing', async () => {
    const mine = await open();
    const theirs = await open();
    const schema = await schemaOf(mine);
    const [a, b, c] = await pick(mine, 3);
    const theirMobile = freshMobile();
    const theirMethod = differentCode(schema, 'paymentModes', b!.values.payMethod!);
    expect(
      (
        await commit(theirs, [
          change(a!, { mobile: theirMobile }),
          change(b!, { payMethod: theirMethod }),
        ])
      ).status,
    ).toBe(200);
    const myMethod =
      schema.lookups
        .paymentModes!.map((m) => m.code)
        .find((m) => m !== theirMethod && m !== b!.values.payMethod) ?? b!.values.payMethod;

    const answer = await commit(mine, [
      change(a!, { mobile: freshMobile() }),
      change(b!, { payMethod: myMethod, taxFile: '0099' }),
      change(c!, { mobile: freshMobile() }),
    ]);
    expect(answer.status).toBe(409);
    const body = readConflictBody(answer.json);
    const conflicts = new Map(body.conflicts.map((x) => [x.id, x]));
    expect([...conflicts.keys()].sort()).toEqual([a!.id, b!.id].sort());
    expect(conflicts.get(a!.id)).toMatchObject({
      key: a!.key,
      fields: ['mobile'],
      modifiedBy: theirs.userId,
    });
    expect(conflicts.get(a!.id)!.values.mobile).toBe(theirMobile);
    expect(conflicts.get(b!.id)!.fields).toEqual(['payMethod']);
    expect(conflicts.get(b!.id)!.values.payMethod).toBe(theirMethod);
    const now = await byIds(mine, [a!, b!, c!]);
    expect(now.get(a!.id)!.version).toBe(conflicts.get(a!.id)!.version);
    expect(now.get(c!.id)).toEqual(c);
  });

  it('is strict: a change to other fields since reading is still a conflict', async () => {
    const mine = await open();
    const theirs = await open();
    const [a] = await pick(mine, 1);
    expect((await commit(theirs, [change(a!, { address: 'Somewhere else' })])).status).toBe(200);
    const answer = await commit(mine, [change(a!, { mobile: freshMobile() })]);
    expect(answer.status).toBe(409);
    expect(readConflictBody(answer.json).conflicts).toMatchObject([{ id: a!.id, fields: [] }]);
    const dryRun = await commit(mine, [change(a!, { mobile: freshMobile() })], { dryRun: true });
    expect(dryRun.status).toBe(409);
  });

  it.skipIf(!onMock)('sees an edit made elsewhere in the ERP as a conflict', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    await control('edit', {
      company: 'Kit',
      key: a!.key,
      values: { mobile: '01234567890' },
      by: 'omar',
    });
    const answer = await commit(session, [change(a!, { mobile: freshMobile() })]);
    expect(answer.status).toBe(409);
    expect(readConflictBody(answer.json).conflicts).toMatchObject([
      { id: a!.id, fields: ['mobile'], modifiedBy: 'omar' },
    ]);
  });

  it('refuses a record sent without its version', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const { version, ...unversioned } = change(a!, { mobile: freshMobile() });
    void version;
    const answer = await send('POST', at(session, 'Commit'), {
      session: session.token,
      body: {
        commitId: crypto.randomUUID(),
        company: session.company,
        schemaVersion,
        dryRun: false,
        records: [unversioned],
      },
    });
    expect(answer.status).toBe(400);
    expect(codes(answer)).toEqual(['version.missing']);
  });

  it('retried with the same commitId returns the stored result and writes once', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const before = onMock ? await mockState() : null;
    const commitId = crypto.randomUUID();
    const records = [change(a!, { mobile: freshMobile() })];
    const first = await commit(session, records, { commitId });
    expect(first.status).toBe(200);
    const again = await commit(session, records, { commitId });
    expect(again.status).toBe(200);
    expect(again.json).toEqual(first.json);
    const written = readCommitResponse(first.json).records[0]!;
    expect((await byIds(session, [a!])).get(a!.id)!.version).toBe(written.version);
    const status = await send('GET', at(session, `CommitStatus/${commitId}`), {
      session: session.token,
    });
    expect(readCommitStatus(status.json)).toEqual({
      status: 'committed',
      ...readCommitResponse(first.json),
    });
    if (before) expect((await mockState()).writes).toBe(before.writes + 1);
  });

  it.skipIf(!onMock)(
    'whose response is lost is found by CommitStatus, and its retry writes once',
    async () => {
      const session = await open();
      const [a] = await pick(session, 1);
      const before = await mockState();
      const commitId = crypto.randomUUID();
      const records = [change(a!, { mobile: freshMobile() })];
      await control('fault', { match: 'VendorSheet/Commit$', drop: 'after' });
      await expect(commit(session, records, { commitId })).rejects.toThrow();
      const status = await send('GET', at(session, `CommitStatus/${commitId}`), {
        session: session.token,
      });
      const found = readCommitStatus(status.json);
      expect(found.status).toBe('committed');
      const retry = await commit(session, records, { commitId });
      expect(retry.status).toBe(200);
      expect({ status: 'committed', ...readCommitResponse(retry.json) }).toEqual(found);
      expect((await mockState()).writes).toBe(before.writes + 1);
    },
  );

  it.skipIf(!onMock)(
    'lost before it arrives is unknown to CommitStatus, and its retry writes once',
    async () => {
      const session = await open();
      const [a] = await pick(session, 1);
      const before = await mockState();
      const commitId = crypto.randomUUID();
      const records = [change(a!, { mobile: freshMobile() })];
      await control('fault', { match: 'VendorSheet/Commit$', drop: 'before' });
      await expect(commit(session, records, { commitId })).rejects.toThrow();
      const status = await send('GET', at(session, `CommitStatus/${commitId}`), {
        session: session.token,
      });
      expect(readCommitStatus(status.json)).toEqual({ status: 'unknown' });
      expect((await commit(session, records, { commitId })).status).toBe(200);
      expect((await mockState()).writes).toBe(before.writes + 1);
    },
  );

  it('refuses a commitId reused for other changes', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const commitId = crypto.randomUUID();
    const first = await commit(session, [change(a!, { mobile: freshMobile() })], { commitId });
    expect(first.status).toBe(200);
    const other = await commit(session, [change(a!, { mobile: freshMobile() })], { commitId });
    expect(other.status).toBe(400);
    expect(codes(other)).toEqual(['commit.reused']);
    expect((await byIds(session, [a!])).get(a!.id)!.values.mobile).toBe(
      readCommitResponse(first.json).records[0]!.values.mobile,
    );
  });

  it.skipIf(lacks('other'))('keeps commitIds per company', async () => {
    const kit = await open('editor');
    const dot = await open('other');
    const commitId = crypto.randomUUID();
    const [k] = await pick(kit, 1);
    const [d] = await pick(dot, 1);
    expect((await commit(kit, [change(k!, { mobile: freshMobile() })], { commitId })).status).toBe(
      200,
    );
    const answer = await commit(dot, [change(d!, { mobile: freshMobile() })], { commitId });
    expect(answer.status).toBe(200);
    expect(readCommitResponse(answer.json).records[0]!.id).toBe(d!.id);
  });

  it.skipIf(lacks('other'))(
    'refuses another company’s ids as missing, and their company’s name',
    async () => {
      const kit = await open('editor');
      const dot = await open('other');
      const [k] = await pick(kit, 1);
      const answer = await commit(dot, [change(k!, { mobile: freshMobile() })]);
      expect(answer.status).toBe(400);
      expect(readErrorBody(answer.json).errors).toMatchObject([
        { id: k!.id, code: 'record.notFound' },
      ]);
      expect((await byIds(kit, [k!])).get(k!.id)).toEqual(k);
      const mismatch = await commit(kit, [change(k!, { mobile: freshMobile() })], {
        company: dot.company,
      });
      expect(mismatch.status).toBe(400);
      expect(codes(mismatch)).toEqual(['company.mismatch']);
    },
  );

  it('refuses a company the session isn’t in', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const answer = await commit(session, [change(a!, { mobile: freshMobile() })], {
      company: `${session.company}-other`,
    });
    expect(answer.status).toBe(400);
    expect(codes(answer)).toEqual(['company.mismatch']);
  });

  it('refuses a commit made for another schema version', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const answer = await commit(session, [change(a!, { mobile: freshMobile() })], {
      schemaVersion: 'vendors.0',
    });
    expect(answer.status).toBe(400);
    expect(codes(answer)).toEqual(['schema.changed']);
  });

  it('refuses read-only fields, and fields the schema doesn’t have', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const answer = await commit(session, [
      change(a!, {
        vendorAccount: 'V-NEW',
        bankAccount: 'EG00',
        debitLimit: '1.00',
        modifiedBy: 'me',
        modifiedDate: '2026-11-09T10:00:00Z',
        token: 'x',
      }),
    ]);
    expect(answer.status).toBe(400);
    const errors = readErrorBody(answer.json).errors;
    expect(errors.map((e) => `${e.field} ${e.code}`).sort()).toEqual(
      ['vendorAccount', 'bankAccount', 'debitLimit', 'modifiedBy', 'modifiedDate', 'token']
        .map((f) => `${f} field.readOnly`)
        .sort(),
    );
    expect((await byIds(session, [a!])).get(a!.id)).toEqual(a);
  });

  it.skipIf(lacks('editor', 'adv'))(
    'refuses AdvEdit fields without AdvEdit, and takes them with it',
    async () => {
      const editor = await open('editor');
      const schema = await schemaOf(editor);
      const [a] = await pick(editor, 1);
      const currency = differentCode(schema, 'currencies', a!.values.currency ?? null);
      const refused = await commit(editor, [change(a!, { blocked: true, currency })]);
      expect(refused.status).toBe(400);
      expect(
        readErrorBody(refused.json)
          .errors.map((e) => `${e.field} ${e.code}`)
          .sort(),
      ).toEqual(['blocked privilege.denied', 'currency privilege.denied']);
      const adv = await open('adv');
      const taken = await commit(adv, [change(a!, { blocked: true, currency })]);
      expect(taken.status).toBe(200);
      expect(readCommitResponse(taken.json).records[0]!.values).toMatchObject({
        blocked: true,
        currency,
      });
      const cleared = await commit(adv, [change(a!, { currency: null })]);
      expect(cleared.status).toBe(400);
      expect(codes(cleared)).toEqual(['required']);
    },
  );

  it('refuses a change to an inactive lookup item, and keeps one a record already holds', async (test) => {
    const session = await open();
    const schema = await schemaOf(session);
    const inactive = Object.entries(schema.lookups)
      .flatMap(([list, items]) => items.map((item) => ({ list, item })))
      .find(({ item }) => item.active === false);
    const field = schema.fields.find((f) => f.lookup === inactive?.list && f.editable);
    if (!inactive || !field) return test.skip();
    const [a] = await pick(session, 1);
    const answer = await commit(session, [change(a!, { [field.name]: inactive.item.code })]);
    expect(answer.status).toBe(400);
    expect(readErrorBody(answer.json).errors).toMatchObject([
      { id: a!.id, field: field.name, code: 'lookup.notFound' },
    ]);
    if (onMock) {
      const old = (await page(session, { filter: { search: 'V-1008' } })).records[0]!;
      expect(old.values.vendorGroup).toBe('OLD');
      const kept = await commit(session, [change(old, { mobile: freshMobile() })]);
      expect(kept.status).toBe(200);
      expect(readCommitResponse(kept.json).records[0]!.values.vendorGroup).toBe('OLD');
    }
  });

  it.skipIf(lacks('viewer'))('refuses a viewer, dry run or not', async () => {
    const viewer = await open('viewer');
    const [a] = await pick(viewer, 1);
    for (const dryRun of [true, false]) {
      const answer = await commit(viewer, [change(a!, { mobile: freshMobile() })], { dryRun });
      expect(answer.status).toBe(403);
      expect(codes(answer)).toEqual(['privilege.denied']);
    }
  });

  it.skipIf(!onMock)('refuses a locked record', async () => {
    const session = await open();
    const locked = (await page(session, { pageSize: 10 })).records.find((r) => r.key === 'V-1005')!;
    const answer = await commit(session, [change(locked, { mobile: freshMobile() })]);
    expect(answer.status).toBe(400);
    expect(codes(answer)).toEqual(['record.locked']);
  });

  it('answers 413 past the batch limit, and takes the limit itself', async () => {
    const session = await open();
    const schema = await schemaOf(session);
    const tooMany = Array.from({ length: schema.maxCommitRecords + 1 }, (_, i) => ({
      id: i + 1,
      key: `K${i}`,
      version: 'AAAAAAAAAAE=',
      changes: { mobile: { from: null, to: '01000000000' } },
    }));
    const answer = await commit(session, tooMany);
    expect(answer.status).toBe(413);
    expect(codes(answer)).toEqual(['batch.tooLarge']);

    const all = await pick(session, schema.maxCommitRecords);
    const full = await commit(
      session,
      all.map((r) => change(r, { mobile: freshMobile() })),
      { dryRun: true },
    );
    expect(full.status).toBe(200);
    expect(readCommitResponse(full.json).records.length).toBe(schema.maxCommitRecords);
  });
});

describe('cross-origin requests', () => {
  it('are preflighted for the sheet’s Origin, with its session header', async () => {
    const answer = await send('OPTIONS', 'api/payable/VendorSheet/Query', {
      headers: {
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'content-type,x-sheet-session',
      },
    });
    expect(answer.status).toBeLessThan(300);
    expect(answer.headers.get('Access-Control-Allow-Origin')).toBe(ORIGIN);
    expect(answer.headers.get('Access-Control-Allow-Methods') ?? '').toMatch(/POST/i);
    expect((answer.headers.get('Access-Control-Allow-Headers') ?? '').toLowerCase()).toContain(
      'x-sheet-session',
    );
  });

  it('are refused for another Origin', async () => {
    const answer = await send('OPTIONS', 'api/payable/VendorSheet/Query', {
      origin: 'https://evil.example',
      headers: {
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'x-sheet-session',
      },
    });
    expect(answer.headers.get('Access-Control-Allow-Origin')).toBeNull();
  });

  it('let the sheet read the answer and its correlation id', async () => {
    const session = await open();
    const answer = await send('GET', at(session, 'Schema'), { session: session.token });
    expect(answer.headers.get('Access-Control-Allow-Origin')).toBe(ORIGIN);
    expect((answer.headers.get('Access-Control-Expose-Headers') ?? '').toLowerCase()).toContain(
      'x-correlation-id',
    );
  });
});

describe.skipIf(!onMock)('the mock’s failure injection', () => {
  it('answers 429 and 503 with Retry-After, then recovers', async () => {
    const session = await open();
    for (const status of [429, 503]) {
      await control('fault', { match: 'VendorSheet/Schema$', status, retryAfter: 2 });
      const answer = await send('GET', at(session, 'Schema'), { session: session.token });
      expect(answer.status).toBe(status);
      expect(answer.headers.get('Retry-After')).toBe('2');
      expect((await send('GET', at(session, 'Schema'), { session: session.token })).status).toBe(
        200,
      );
    }
  });

  it('answers 401 mid-session, and delays answers', async () => {
    const session = await open();
    await control('fault', { match: 'VendorSheet/Query$', status: 401 });
    const refused = await query(session, {});
    expect(refused.status).toBe(401);
    expect(codes(refused)).toEqual(['session.expired']);
    await control('fault', { match: 'VendorSheet/Query$', latencyMs: 150 });
    const start = Date.now();
    expect((await query(session, {})).status).toBe(200);
    expect(Date.now() - start).toBeGreaterThanOrEqual(140);
  });

  it('keeps an audit row per changed field', async () => {
    const session = await open();
    const [a] = await pick(session, 1);
    const before = await mockState();
    expect(
      (await commit(session, [change(a!, { mobile: freshMobile(), taxFile: '0101' })])).status,
    ).toBe(200);
    const after = await mockState();
    expect(after.audit - before.audit).toBe(2);
    expect(after.commits - before.commits).toBe(1);
  });
});
