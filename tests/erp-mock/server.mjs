// A mock of Cash Gear ERP's sheet contract (`docs/erp-contract.md`), for the contract suite and,
// from Phase 15, the browser tests. Node only, with no dependencies.
//
// It keeps one database shared by two companies (Kit and Dot), seeded vendors, tickets and
// sessions on a clock the tests can move, versions that move on every write, idempotent commits
// keyed by (company, commitId), and CORS with preflight. `/__mock/*` routes reset it, move its
// clock, inject failures, revoke keys, turn the sheet off for a company and make concurrent
// ERP edits.
//
//   node tests/erp-mock/server.mjs [--port 4180] [--host 127.0.0.1] [--sheet-url http://localhost:4173]

import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { API_KEYS, COMPANIES, VENDOR_FIELDS, lookups, seedVendors } from './seed.mjs';

/** The contract's limits and lifetimes (decisions, 2026-11-09). */
export const LIMITS = {
  ticketMs: 120_000,
  idleMs: 30 * 60_000,
  maxMs: 10 * 60 * 60_000,
  maxPageSize: 1000,
  maxRecords: 10_000,
  maxCommitRecords: 500,
};

/** Origins allowed to redeem tickets and read responses: the sheet's dev, preview and live hosts. */
export const DEFAULT_ORIGINS = [
  'http://localhost:4173',
  'http://127.0.0.1:4173',
  'http://localhost:5173',
  'http://127.0.0.1:5173',
  'https://sheet.cashgearerp.com',
];

export const ENTITY_ROUTE = 'api/payable/VendorSheet';
export const SCHEMA_VERSION = 'vendors.1';
const MAX_BODY = 8 * 1024 * 1024;
const FIELDS = new Map(VENDOR_FIELDS.map((f) => [f.name, f]));

// ---------------------------------------------------------------------------------------------
// Messages, in the session's language. The sheet shows its own text by code; these are the
// server's fallback, as the ERP's will be.

const TEXT = {
  en: {
    required: (f) => `${f} is required.`,
    maxLength: (f, x) => `${f} can be at most ${x.maxLength} characters.`,
    type: (f) => `${f} has the wrong type of value.`,
    range: (f) => `${f} is out of range.`,
    scale: (f, x) => `${f} can have at most ${x.scale} decimal places.`,
    'lookup.notFound': (f, x) => `“${x.value}” isn’t a ${f} in Cash Gear ERP.`,
    'field.readOnly': (f) => `${f} can’t be changed from the sheet.`,
    'privilege.denied': (f) =>
      f
        ? `You don’t have the privilege to change ${f}.`
        : 'You don’t have the privilege to edit vendors.',
    'record.notFound': (_, x) => `Vendor ${x.key} isn’t in Cash Gear ERP.`,
    'record.locked': (_, x) => `Vendor ${x.key} is locked.`,
    'version.missing': (_, x) => `Vendor ${x.key} was sent without its version.`,
    'batch.tooLarge': (_, x) => `Send at most ${x.max} at a time.`,
    'company.mismatch': () => 'This commit is for another company.',
    'schema.changed': () => 'The vendors’ fields have changed. Reopen the vendors to continue.',
    'session.expired': () => 'Your Cash Gear ERP session has ended. Reconnect to continue.',
    'ticket.invalid': () =>
      'This link has expired or has already been used. Open it again from Cash Gear ERP.',
    'origin.denied': () => 'This site can’t connect to Cash Gear ERP.',
    'commit.reused': () => 'This commit id was already used for other changes.',
    'request.invalid': (_, x) => x.why,
    errors: (n) => (n === 1 ? '1 vendor has errors.' : `${n} vendors have errors.`),
    conflicts: (n) =>
      n === 1
        ? '1 vendor was changed by someone else.'
        : `${n} vendors were changed by someone else.`,
  },
  ar: {
    required: (f) => `${f} مطلوب.`,
    maxLength: (f, x) => `${f} يجب ألا يزيد عن ${x.maxLength} حرفًا.`,
    type: (f) => `قيمة ${f} من نوع غير صحيح.`,
    range: (f) => `${f} خارج النطاق المسموح.`,
    scale: (f, x) => `${f} يقبل ${x.scale} أرقام عشرية على الأكثر.`,
    'lookup.notFound': (f, x) => `«${x.value}» غير موجود في ${f}.`,
    'field.readOnly': (f) => `لا يمكن تعديل ${f} من الشيت.`,
    'privilege.denied': (f) =>
      f ? `ليس لديك صلاحية تعديل ${f}.` : 'ليس لديك صلاحية تعديل الموردين.',
    'record.notFound': (_, x) => `المورد ${x.key} غير موجود في Cash Gear ERP.`,
    'record.locked': (_, x) => `المورد ${x.key} مقفل.`,
    'version.missing': (_, x) => `أرسل المورد ${x.key} بدون إصداره.`,
    'batch.tooLarge': (_, x) => `أرسل ${x.max} على الأكثر في المرة الواحدة.`,
    'company.mismatch': () => 'هذا الحفظ لشركة أخرى.',
    'schema.changed': () => 'تغيرت حقول الموردين. افتح الموردين من جديد للمتابعة.',
    'session.expired': () => 'انتهت جلسة Cash Gear ERP. أعد الاتصال للمتابعة.',
    'ticket.invalid': () =>
      'انتهت صلاحية هذا الرابط أو استخدم من قبل. افتحه من جديد من Cash Gear ERP.',
    'origin.denied': () => 'لا يمكن لهذا الموقع الاتصال بـ Cash Gear ERP.',
    'commit.reused': () => 'استخدم رقم هذا الحفظ من قبل لتعديلات أخرى.',
    'request.invalid': (_, x) => x.why,
    errors: (n) => `${n} من الموردين بهم أخطاء.`,
    conflicts: (n) => `عدّل مستخدم آخر ${n} من الموردين.`,
  },
};

function text(language, code, field, extra = {}) {
  const table = TEXT[language] ?? TEXT.en;
  const label = field ? (FIELDS.get(field)?.label[language] ?? field) : undefined;
  return table[code](label, extra);
}

// ---------------------------------------------------------------------------------------------
// Wire helpers

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const iso = (ms) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const blank = (value) => (value === '' ? null : value);

function encodeVersion(n) {
  const bytes = Buffer.alloc(8);
  bytes.writeBigUInt64BE(n);
  return bytes.toString('base64');
}

/** Sorted keys at every level, so equal payloads hash equally whatever their key order. */
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const keys = Object.keys(value).sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

const isWire = (v) =>
  v === null ||
  typeof v === 'string' ||
  typeof v === 'boolean' ||
  (typeof v === 'number' && Number.isFinite(v));
const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const isId = (v) => Number.isSafeInteger(v) && v > 0;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const DECIMAL = /^-?(0|[1-9]\d*)(\.(\d+))?$/;

/** A vendor row as the sheet sees it: the allow-listed fields, with `""` sent as `null`. */
function wire(row) {
  return {
    vendorAccount: row.vendorAccount,
    vendorName: row.vendorName,
    vendorGroup: blank(row.vendorGroup),
    payMethod: blank(row.payMethod),
    currency: blank(row.currency),
    mobile: blank(row.mobile),
    taxFile: blank(row.taxFile),
    address: blank(row.address),
    blocked: row.blocked !== 0,
    bankAccount: blank(row.bankAccount),
    debitLimit: row.debitLimit,
    modifiedBy: blank(row.modifiedBy),
    modifiedDate: row.modifiedDate,
  };
}

/** Writes one wire value into a row, as the ERP's columns hold it. */
function store(row, field, value) {
  value = blank(value);
  if (field === 'blocked') row.blocked = value ? 1 : 0;
  else if (field === 'currency') row.currency = value ?? '';
  else row[field] = value;
}

function compareDecimal(a, b) {
  const [ai, af = ''] = a.replace('-', '').split('.');
  const [bi, bf = ''] = b.replace('-', '').split('.');
  const sa = a.startsWith('-') ? -1 : 1;
  const sb = b.startsWith('-') ? -1 : 1;
  if (sa !== sb) return sa - sb;
  const width = Math.max(af.length, bf.length);
  const x = BigInt(ai + af.padEnd(width, '0'));
  const y = BigInt(bi + bf.padEnd(width, '0'));
  return sa * (x < y ? -1 : x > y ? 1 : 0);
}

/** The first contract rule `value` breaks for `field`, or null. */
function valueError(field, value, list) {
  value = blank(value);
  if (value !== null) {
    switch (field.type) {
      case 'text':
      case 'lookup':
        if (typeof value !== 'string') return 'type';
        break;
      case 'boolean':
        if (typeof value !== 'boolean') return 'type';
        break;
      case 'integer':
        if (!Number.isSafeInteger(value)) return 'type';
        break;
      case 'decimal': {
        const m = typeof value === 'string' ? DECIMAL.exec(value) : null;
        if (!m) return 'type';
        if ((m[3]?.length ?? 0) > (field.scale ?? 0)) return 'scale';
        if (field.min !== undefined && compareDecimal(value, field.min) < 0) return 'range';
        if (field.max !== undefined && compareDecimal(value, field.max) > 0) return 'range';
        break;
      }
      default:
        if (typeof value !== 'string') return 'type';
    }
  }
  if (field.required && value === null) return 'required';
  if (field.maxLength && typeof value === 'string' && value.length > field.maxLength)
    return 'maxLength';
  if (
    field.type === 'lookup' &&
    value !== null &&
    !list.some((item) => item.code === value && item.active !== false)
  ) {
    return 'lookup.notFound';
  }
  return null;
}

class Reply {
  constructor(status, body, headers = {}) {
    this.status = status;
    this.body = body;
    this.headers = headers;
  }
}

// ---------------------------------------------------------------------------------------------
// The mock

export class MockErp {
  constructor({ origins = DEFAULT_ORIGINS, sheetUrl = 'http://localhost:4173' } = {}) {
    this.origins = new Set(origins);
    this.sheetUrl = sheetUrl.replace(/\/+$/, '');
    this.reset();
  }

  /** Back to the seed: records, versions, keys, tickets, sessions, commits, clock and faults. */
  reset() {
    this.offsetMs = 0;
    this.keys = new Map(Object.entries(API_KEYS).map(([key, v]) => [key, { ...v, active: true }]));
    // The companies the sheet is turned on for, as UnifiedAPI's `SheetLaunch:Companies` lists
    // them: every company here, so the suite and the browser tests can launch.
    this.launchCompanies = new Set(Object.keys(COMPANIES));
    this.tickets = new Map();
    this.sessions = new Map();
    this.faults = [];
    this.databases = new Map();
    for (const name of new Set(Object.values(COMPANIES))) {
      this.databases.set(name, {
        counter: 2000n,
        vendors: new Map(),
        commits: new Map(),
        audit: [],
        writes: 0,
      });
    }
    for (const row of seedVendors()) {
      const db = this.database(row.company);
      db.vendors.set(row.recId, { ...row, rowVersion: ++db.counter });
    }
  }

  now() {
    return Date.now() + this.offsetMs;
  }

  database(company) {
    return this.databases.get(COMPANIES[company]);
  }

  /** A company's vendors in id order. */
  vendors(company) {
    return [...this.database(company).vendors.values()]
      .filter((row) => row.company === company)
      .sort((a, b) => a.recId - b.recId);
  }

  snapshot(row) {
    return {
      id: row.recId,
      key: row.vendorAccount,
      version: encodeVersion(row.rowVersion),
      values: wire(row),
    };
  }

  /** Writes values into a record in its own row version, as any ERP save does. */
  write(row, values, by) {
    const db = this.database(row.company);
    for (const [field, value] of Object.entries(values)) store(row, field, value);
    row.rowVersion = ++db.counter;
    row.modifiedBy = by;
    row.modifiedDate = iso(this.now());
    db.writes++;
  }

  // -------------------------------------------------------------------------------------------
  // HTTP

  async handle(req, res) {
    const correlationId = randomUUID();
    const url = new URL(req.url ?? '/', 'http://mock');
    const path = decodeURIComponent(url.pathname).replace(/^\/+|\/+$/g, '');
    const origin = req.headers.origin;
    const allowed = typeof origin === 'string' && this.origins.has(origin);
    res.setHeader('Vary', 'Origin');
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('X-Correlation-Id', correlationId);
    if (allowed) {
      res.setHeader('Access-Control-Allow-Origin', origin);
      res.setHeader('Access-Control-Expose-Headers', 'X-Correlation-Id, X-Company-Id, Retry-After');
    }
    if (req.method === 'OPTIONS') {
      if (allowed) {
        res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
        res.setHeader('Access-Control-Allow-Headers', 'Content-Type, X-Sheet-Session, X-API-Key');
        res.setHeader('Access-Control-Max-Age', '600');
      }
      res.writeHead(204).end();
      return;
    }
    const ctx = {
      req,
      res,
      url,
      path,
      lower: path.toLowerCase(),
      origin,
      allowed,
      correlationId,
      language: 'en',
    };
    let body;
    try {
      body = await readBody(req);
    } catch (error) {
      return send(
        res,
        this.fail(ctx, error.status ?? 400, 'request.invalid', null, { why: error.message }),
      );
    }
    ctx.body = body;
    if (ctx.lower.startsWith('__mock/')) return send(res, this.control(ctx));

    const fault = this.takeFault(path);
    if (fault?.latencyMs) await sleep(fault.latencyMs);
    if (fault?.drop === 'before') return void req.socket.destroy();
    if (fault?.status) {
      const code = { 401: 'session.expired', 403: 'privilege.denied', 413: 'batch.tooLarge' }[
        fault.status
      ];
      const reply = code
        ? this.fail(ctx, fault.status, code, null, { max: LIMITS.maxCommitRecords })
        : new Reply(fault.status, {
            message: `Injected ${fault.status}.`,
            correlationId,
            errors: [],
          });
      if (fault.retryAfter !== undefined) reply.headers['Retry-After'] = String(fault.retryAfter);
      return send(res, reply);
    }
    const reply = this.route(ctx);
    if (fault?.drop === 'after') return void req.socket.destroy();
    send(res, reply);
  }

  takeFault(path) {
    // `match` is a regular expression over the path without its leading slash, ignoring case:
    // `VendorSheet/Commit$` is Commit alone, where `VendorSheet/Commit` is CommitStatus too.
    const index = this.faults.findIndex((f) => f.pattern.test(path));
    if (index < 0) return null;
    const fault = this.faults[index];
    if (fault.times > 0 && --fault.times === 0) this.faults.splice(index, 1);
    return fault;
  }

  fail(ctx, status, code, field, extra = {}) {
    const message = text(ctx.language, code, field, extra);
    const error = { code, message };
    if (field) error.field = field;
    return new Reply(status, { message, correlationId: ctx.correlationId, errors: [error] });
  }

  route(ctx) {
    const { req, lower } = ctx;
    const post = req.method === 'POST';
    const get = req.method === 'GET';
    const entity = ENTITY_ROUTE.toLowerCase();
    const is = (route, method) => {
      if (lower !== route.toLowerCase()) return false;
      if (!method) ctx.wrongMethod = true;
      return method;
    };
    if (is('api/system/SheetLaunch/CreateTicket', post)) return this.createTicket(ctx);
    if (is('api/system/SheetLaunch/Status', get)) return this.launchStatus(ctx);
    if (is('api/sheet/Session/Redeem', post)) return this.redeem(ctx);
    if (is('api/sheet/Session/Renew', post)) return this.withSession(ctx, () => this.renew(ctx));
    if (is('api/sheet/Session/End', post)) return this.withSession(ctx, () => this.end(ctx));
    if (is(`${entity}/schema`, get)) return this.withSession(ctx, () => this.schema(ctx));
    if (is(`${entity}/query`, post)) return this.withSession(ctx, () => this.query(ctx));
    if (is(`${entity}/commit`, post)) return this.withSession(ctx, () => this.commit(ctx));
    if (get && lower.startsWith(`${entity}/commitstatus/`)) {
      return this.withSession(ctx, () =>
        this.commitStatus(ctx, ctx.path.slice(entity.length + 14)),
      );
    }
    const status = ctx.wrongMethod ? 405 : 404;
    return new Reply(status, {
      message: status === 405 ? 'Method not allowed.' : 'Not found.',
      correlationId: ctx.correlationId,
      errors: [],
    });
  }

  // -------------------------------------------------------------------------------------------
  // C.1 Launch and session

  /** The caller of an API-key route, or the API-key handler's empty-bodied 401. */
  apiKeyCaller(ctx) {
    const key = ctx.req.headers['x-api-key'];
    const caller = typeof key === 'string' ? this.keys.get(key) : undefined;
    if (!caller || !caller.active) {
      return new Reply(401, {
        message: 'A valid API key is required.',
        correlationId: ctx.correlationId,
        errors: [],
      });
    }
    return { key, caller };
  }

  /** Whether the ERP's button shows: the sheet is on for the company, and the caller may view. */
  launchStatus(ctx) {
    const found = this.apiKeyCaller(ctx);
    if (found instanceof Reply) return found;
    const entity = ctx.url.searchParams.get('entity') ?? 'vendors';
    if (entity !== 'vendors') {
      return this.fail(ctx, 400, 'request.invalid', null, { why: 'entity must be "vendors".' });
    }
    const { caller } = found;
    return new Reply(200, {
      enabled: this.launchCompanies.has(caller.company) && caller.privileges.includes('View'),
    });
  }

  createTicket(ctx) {
    const found = this.apiKeyCaller(ctx);
    if (found instanceof Reply) return found;
    const { key, caller } = found;
    const body = ctx.body;
    if (!isObject(body) || body.entity !== 'vendors') {
      return this.fail(ctx, 400, 'request.invalid', null, { why: 'entity must be "vendors".' });
    }
    if (body.language !== 'en' && body.language !== 'ar') {
      return this.fail(ctx, 400, 'request.invalid', null, {
        why: 'language must be "en" or "ar".',
      });
    }
    const why = filterProblem(body.filter);
    if (why) return this.fail(ctx, 400, 'request.invalid', null, { why });
    if (!caller.privileges.includes('View') || !this.launchCompanies.has(caller.company)) {
      return this.fail(ctx, 403, 'privilege.denied', null);
    }
    const ticket = randomBytes(32).toString('base64url');
    const expiresAt = this.now() + LIMITS.ticketMs;
    this.tickets.set(sha256(ticket), {
      apiKey: key,
      entity: body.entity,
      filter: body.filter ?? null,
      language: body.language,
      expiresAt,
      used: false,
    });
    return new Reply(200, {
      ticket,
      expiresAt: iso(expiresAt),
      url: `${this.sheetUrl}/erp#launch=${ticket}`,
    });
  }

  redeem(ctx) {
    // The Origin is checked before the ticket is looked at, so a refused site can't use it up.
    if (!ctx.allowed) return this.fail(ctx, 403, 'origin.denied', null);
    const ticket = isObject(ctx.body) ? ctx.body.ticket : undefined;
    if (typeof ticket !== 'string' || ticket.length === 0) {
      return this.fail(ctx, 400, 'request.invalid', null, { why: 'ticket is required.' });
    }
    const row = this.tickets.get(sha256(ticket));
    const now = this.now();
    const caller = row && this.keys.get(row.apiKey);
    if (!row || row.used || now >= row.expiresAt || !caller?.active) {
      return this.fail(ctx, 401, 'ticket.invalid', null);
    }
    row.used = true;
    const token = randomBytes(32).toString('base64url');
    const session = {
      apiKey: row.apiKey,
      company: caller.company,
      userId: caller.userId,
      userName: caller.userName,
      language: row.language,
      entity: row.entity,
      entityRoute: ENTITY_ROUTE,
      filter: row.filter,
      idleExpiresAt: Math.min(now + LIMITS.idleMs, now + LIMITS.maxMs),
      maxExpiresAt: now + LIMITS.maxMs,
      ended: false,
    };
    this.sessions.set(sha256(token), session);
    return new Reply(200, {
      token,
      idleExpiresAt: iso(session.idleExpiresAt),
      maxExpiresAt: iso(session.maxExpiresAt),
      company: session.company,
      userId: session.userId,
      userName: session.userName,
      language: session.language,
      entity: session.entity,
      entityRoute: session.entityRoute,
      filter: session.filter,
    });
  }

  /** The live session for `X-Sheet-Session`, or null. Its key and company are checked each time. */
  session(ctx) {
    const token = ctx.req.headers['x-sheet-session'];
    if (typeof token !== 'string' || token.length === 0) return null;
    const session = this.sessions.get(sha256(token));
    if (!session || session.ended) return null;
    const now = this.now();
    if (now >= session.idleExpiresAt || now >= session.maxExpiresAt) return null;
    const caller = this.keys.get(session.apiKey);
    if (!caller?.active || caller.company !== session.company) return null;
    return { ...session, privileges: caller.privileges, row: session };
  }

  withSession(ctx, next) {
    const session = this.session(ctx);
    if (!session) return this.fail(ctx, 401, 'session.expired', null);
    ctx.session = session;
    ctx.language = session.language;
    ctx.res.setHeader('X-Company-Id', session.company);
    return next();
  }

  renew(ctx) {
    const { row } = ctx.session;
    row.idleExpiresAt = Math.min(this.now() + LIMITS.idleMs, row.maxExpiresAt);
    return new Reply(200, { idleExpiresAt: iso(row.idleExpiresAt) });
  }

  end(ctx) {
    ctx.session.row.ended = true;
    return new Reply(204, null);
  }

  // -------------------------------------------------------------------------------------------
  // C.2 Schema

  schema(ctx) {
    const { privileges, company } = ctx.session;
    const fields = VENDOR_FIELDS.map((f) => {
      const editable = f.needs !== null && privileges.includes(f.needs);
      const field = { name: f.name, label: f.label, type: f.type, editable };
      if (!editable)
        field.editReason =
          f.needs === null ? 'notEditable' : f.needs === 'Edit' ? 'needsEdit' : 'needsAdvEdit';
      for (const name of ['required', 'maxLength', 'min', 'max', 'scale', 'lookup']) {
        if (f[name] !== undefined) field[name] = f[name];
      }
      return field;
    });
    return new Reply(200, {
      entity: 'vendors',
      schemaVersion: SCHEMA_VERSION,
      label: { en: 'Vendors', ar: 'الموردون' },
      keyField: 'vendorAccount',
      idField: 'recId',
      maxPageSize: LIMITS.maxPageSize,
      maxRecords: LIMITS.maxRecords,
      maxCommitRecords: LIMITS.maxCommitRecords,
      fields,
      lookups: lookups(company),
    });
  }

  // -------------------------------------------------------------------------------------------
  // C.3 Query

  query(ctx) {
    const body = ctx.body;
    if (!isObject(body))
      return this.fail(ctx, 400, 'request.invalid', null, { why: 'A JSON body is required.' });
    const { ids, afterId = 0, pageSize } = body;
    const why =
      filterProblem(body.filter) ??
      (ids !== null && ids !== undefined && (!Array.isArray(ids) || !ids.every(isId))
        ? 'ids must be null or a list of record ids.'
        : null) ??
      (!Number.isSafeInteger(afterId) || afterId < 0
        ? 'afterId must be a whole number from 0.'
        : null) ??
      (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > LIMITS.maxPageSize
        ? `pageSize must be 1 to ${LIMITS.maxPageSize}.`
        : null);
    if (why) return this.fail(ctx, 400, 'request.invalid', null, { why });
    if (Array.isArray(ids) && ids.length > LIMITS.maxRecords) {
      return this.fail(ctx, 413, 'batch.tooLarge', null, { max: LIMITS.maxRecords });
    }
    const wanted = Array.isArray(ids) ? new Set(ids) : null;
    const filter = body.filter ?? {};
    const search = typeof filter.search === 'string' ? filter.search.toLocaleLowerCase() : '';
    const matches = this.vendors(ctx.session.company).filter((row) => {
      if (wanted) return wanted.has(row.recId);
      // The fields the ERP's Vendors list searches: account, name, group code, mobile and phone.
      const searched = [row.vendorAccount, row.vendorName, row.vendorGroup, row.mobile, row.phone];
      if (search && !searched.some((v) => (v ?? '').toLocaleLowerCase().includes(search)))
        return false;
      const values = wire(row);
      return Object.entries(filter).every(
        ([name, value]) => name === 'search' || blank(values[name]) === blank(value),
      );
    });
    const after = matches.filter((row) => row.recId > afterId);
    const page = after.slice(0, pageSize);
    return new Reply(200, {
      records: page.map((row) => this.snapshot(row)),
      nextAfterId: after.length > page.length ? page[page.length - 1].recId : null,
      total: matches.length,
      asOf: iso(this.now()),
    });
  }

  // -------------------------------------------------------------------------------------------
  // C.4 to C.7 Commit

  commit(ctx) {
    const session = ctx.session;
    const { language, company } = session;
    if (!session.privileges.includes('Edit')) return this.fail(ctx, 403, 'privilege.denied', null);
    const body = ctx.body;
    const why = commitProblem(body);
    if (why) return this.fail(ctx, 400, 'request.invalid', null, { why });
    if (body.records.length > LIMITS.maxCommitRecords) {
      return this.fail(ctx, 413, 'batch.tooLarge', null, { max: LIMITS.maxCommitRecords });
    }
    const db = this.database(company);
    const hash = sha256(
      canonical({
        company: body.company,
        schemaVersion: body.schemaVersion,
        records: [...body.records].sort((a, b) => a.id - b.id),
      }),
    );
    const slot = `${company}\u0000${body.commitId.toLowerCase()}`;
    if (!body.dryRun && db.commits.has(slot)) {
      const stored = db.commits.get(slot);
      if (stored.hash !== hash) return this.fail(ctx, 400, 'commit.reused', null);
      return new Reply(200, stored.body);
    }
    if (body.company !== company) return this.fail(ctx, 400, 'company.mismatch', null);
    if (body.schemaVersion !== SCHEMA_VERSION) return this.fail(ctx, 400, 'schema.changed', null);

    // Every error in the batch is collected before anything is written.
    const lists = lookups(company);
    const errors = [];
    const found = [];
    for (const record of body.records) {
      const row = db.vendors.get(record.id);
      const at = { id: record.id, key: record.key };
      const add = (code, field, extra = {}) =>
        errors.push({
          ...at,
          ...(field ? { field } : {}),
          code,
          message: text(language, code, field, { ...at, ...extra }),
        });
      if (!row || row.company !== company) {
        add('record.notFound');
        continue;
      }
      if (typeof record.version !== 'string' || record.version.length === 0) add('version.missing');
      if (row.locked) add('record.locked');
      for (const [name, change] of Object.entries(record.changes)) {
        const field = FIELDS.get(name);
        if (!field || field.needs === null) add('field.readOnly', name);
        else if (!session.privileges.includes(field.needs)) add('privilege.denied', name);
        else {
          const code = valueError(field, change.to, field.lookup ? lists[field.lookup] : []);
          if (code) add(code, name, { ...field, value: change.to });
        }
      }
      found.push([record, row]);
    }
    if (errors.length > 0) {
      const records = new Set(errors.map((e) => e.id)).size;
      return new Reply(400, {
        message: TEXT[language].errors(records),
        correlationId: ctx.correlationId,
        errors,
      });
    }

    // Versions are compared strictly: any write since the record was read is a conflict.
    const conflicts = [];
    for (const [record, row] of found) {
      if (record.version === encodeVersion(row.rowVersion)) continue;
      const now = wire(row);
      conflicts.push({
        id: row.recId,
        key: row.vendorAccount,
        version: encodeVersion(row.rowVersion),
        values: now,
        fields: Object.entries(record.changes)
          .filter(([name, change]) => blank(now[name]) !== blank(change.from))
          .map(([name]) => name),
        modifiedBy: blank(row.modifiedBy),
        modifiedAt: row.modifiedDate,
      });
    }
    if (conflicts.length > 0) {
      return new Reply(409, {
        message: TEXT[language].conflicts(conflicts.length),
        correlationId: ctx.correlationId,
        conflicts,
      });
    }

    if (body.dryRun) {
      return new Reply(200, {
        commitId: body.commitId,
        dryRun: true,
        committedAt: null,
        records: found.map(([record, row]) => {
          const preview = { ...row };
          for (const [name, change] of Object.entries(record.changes))
            store(preview, name, change.to);
          return { ...this.snapshot(row), values: wire(preview) };
        }),
      });
    }

    const at = iso(this.now());
    for (const [record, row] of found) {
      const before = wire(row);
      this.write(
        row,
        Object.fromEntries(Object.entries(record.changes).map(([n, c]) => [n, c.to])),
        session.userId,
      );
      const after = wire(row);
      for (const name of Object.keys(record.changes)) {
        db.audit.push({
          company,
          commitId: body.commitId,
          entity: 'vendors',
          recId: row.recId,
          field: name,
          oldValue: before[name],
          newValue: after[name],
          userId: session.userId,
          at,
          correlationId: ctx.correlationId,
        });
      }
    }
    const result = {
      commitId: body.commitId,
      dryRun: false,
      committedAt: at,
      records: found.map(([, row]) => this.snapshot(row)),
    };
    db.commits.set(slot, { hash, body: result, userId: session.userId, at });
    return new Reply(200, result);
  }

  commitStatus(ctx, commitId) {
    const db = this.database(ctx.session.company);
    const stored = db.commits.get(`${ctx.session.company}\u0000${commitId.toLowerCase()}`);
    return new Reply(200, stored ? { status: 'committed', ...stored.body } : { status: 'unknown' });
  }

  // -------------------------------------------------------------------------------------------
  // /__mock/*: test control

  control(ctx) {
    const route = ctx.lower.slice('__mock/'.length);
    const body = isObject(ctx.body) ? ctx.body : {};
    const ok = (value) => new Reply(value === undefined ? 204 : 200, value ?? null);
    const bad = (why) =>
      new Reply(400, { message: why, correlationId: ctx.correlationId, errors: [] });
    switch (route) {
      case 'reset':
        this.reset();
        return ok();
      case 'clock':
        if (Number.isFinite(body.advanceMs)) this.offsetMs += body.advanceMs;
        if (typeof body.set === 'string') this.offsetMs = Date.parse(body.set) - Date.now();
        return ok({ now: iso(this.now()) });
      case 'fault': {
        // `{clear: true}` forgets every fault, such as one that lasts until then.
        if (body.clear === true) {
          this.faults = [];
          return ok();
        }
        if (typeof body.match !== 'string' || body.match.length === 0)
          return bad('match is required.');
        let pattern;
        try {
          pattern = new RegExp(body.match, 'i');
        } catch {
          return bad('match must be a regular expression.');
        }
        const times = Number.isSafeInteger(body.times) && body.times >= 0 ? body.times : 1;
        this.faults.push({ ...body, pattern, times });
        return ok();
      }
      case 'key': {
        const caller = this.keys.get(body.key);
        if (!caller) return bad('Unknown key.');
        if (typeof body.active === 'boolean') caller.active = body.active;
        if (Array.isArray(body.privileges)) caller.privileges = body.privileges;
        return ok();
      }
      case 'launch':
        if (!Array.isArray(body.companies)) return bad('companies must be a list.');
        this.launchCompanies = new Set(body.companies);
        return ok();
      case 'edit': {
        const row = this.vendors(body.company).find((r) => r.vendorAccount === body.key);
        if (!row || !isObject(body.values)) return bad('Unknown vendor, or no values.');
        this.write(row, body.values, typeof body.by === 'string' ? body.by : 'erp-user');
        return ok(this.snapshot(row));
      }
      case 'state': {
        const company = ctx.url.searchParams.get('company') ?? 'Kit';
        const db = this.database(company);
        if (!db) return bad('Unknown company.');
        return ok({
          now: iso(this.now()),
          vendors: this.vendors(company).length,
          writes: db.writes,
          commits: [...db.commits.keys()].filter((k) => k.startsWith(`${company}\u0000`)).length,
          audit: db.audit.filter((a) => a.company === company).length,
        });
      }
      case 'vendor': {
        const company = ctx.url.searchParams.get('company') ?? 'Kit';
        const row = this.vendors(company).find(
          (r) => r.vendorAccount === ctx.url.searchParams.get('key'),
        );
        if (!row)
          return new Reply(404, {
            message: 'Not found.',
            correlationId: ctx.correlationId,
            errors: [],
          });
        return ok({ ...row, rowVersion: encodeVersion(row.rowVersion) });
      }
      default:
        return new Reply(404, {
          message: 'Not found.',
          correlationId: ctx.correlationId,
          errors: [],
        });
    }
  }
}

function filterProblem(filter) {
  if (filter === null || filter === undefined) return null;
  if (!isObject(filter)) return 'filter must be null or an object.';
  for (const [name, value] of Object.entries(filter)) {
    if (name !== 'search' && !FIELDS.has(name)) return `filter can't use ${name}.`;
    if (!isWire(value)) return `filter.${name} must be a value.`;
  }
  return null;
}

function commitProblem(body) {
  if (!isObject(body)) return 'A JSON body is required.';
  if (typeof body.commitId !== 'string' || !UUID.test(body.commitId))
    return 'commitId must be a UUID.';
  if (typeof body.company !== 'string') return 'company is required.';
  if (typeof body.schemaVersion !== 'string') return 'schemaVersion is required.';
  if (typeof body.dryRun !== 'boolean') return 'dryRun must be true or false.';
  if (!Array.isArray(body.records) || body.records.length === 0)
    return 'records must list at least one record.';
  const ids = new Set();
  for (const record of body.records) {
    if (!isObject(record) || !isId(record.id) || typeof record.key !== 'string') {
      return 'Each record needs an id and a key.';
    }
    if (ids.has(record.id)) return `Record ${record.id} is listed twice.`;
    ids.add(record.id);
    if (
      record.version !== undefined &&
      record.version !== null &&
      typeof record.version !== 'string'
    ) {
      return 'version must be a string.';
    }
    if (!isObject(record.changes) || Object.keys(record.changes).length === 0) {
      return `Record ${record.id} has no changes.`;
    }
    for (const change of Object.values(record.changes)) {
      if (
        !isObject(change) ||
        !('to' in change) ||
        !isWire(change.to) ||
        !isWire(change.from ?? null)
      ) {
        return `Record ${record.id} has a change without a value.`;
      }
    }
  }
  return null;
}

function readBody(req) {
  return new Promise((done, failed) => {
    const chunks = [];
    let size = 0;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY) {
        failed(Object.assign(new Error('The request is too large.'), { status: 413 }));
        req.destroy();
      } else chunks.push(chunk);
    });
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf8');
      if (raw.trim() === '') return done(undefined);
      try {
        done(JSON.parse(raw));
      } catch {
        failed(new Error('The body isn’t JSON.'));
      }
    });
    req.on('error', failed);
  });
}

function send(res, reply) {
  if (res.destroyed) return;
  for (const [name, value] of Object.entries(reply.headers)) res.setHeader(name, value);
  if (reply.body === null) {
    res.writeHead(reply.status).end();
    return;
  }
  const json = JSON.stringify(reply.body);
  res.writeHead(reply.status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(json),
  });
  res.end(json);
}

/** Starts a mock on `port` (0 picks a free one). Resolves once it listens. */
export async function startMockErp({ port = 0, host = '127.0.0.1', origins, sheetUrl } = {}) {
  const mock = new MockErp({ origins, sheetUrl });
  const server = createServer((req, res) => {
    mock.handle(req, res).catch((error) => {
      if (!res.headersSent) {
        send(
          res,
          new Reply(500, {
            message: String(error?.message ?? error),
            correlationId: randomUUID(),
            errors: [],
          }),
        );
      }
    });
  });
  await new Promise((done, failed) => {
    server.once('error', failed);
    server.listen(port, host, done);
  });
  const address = server.address();
  const url = `http://${host === '0.0.0.0' ? '127.0.0.1' : host}:${address.port}`;
  return {
    url,
    port: address.port,
    mock,
    close: () =>
      new Promise((done) => {
        server.close(() => done());
        server.closeAllConnections();
      }),
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const i = args.indexOf(`--${name}`);
    return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
  };
  const { url } = await startMockErp({
    port: Number(option('port', '4180')),
    host: option('host', '127.0.0.1'),
    sheetUrl: option('sheet-url', 'http://localhost:4173'),
  });
  console.log(`Mock ERP listening on ${url}`);
}
