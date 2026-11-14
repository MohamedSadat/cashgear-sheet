// A mock of Cash Gear Workspace's sheet-session routes (`docs/workspace-contract.md`), for the
// connector's tests and the browser tests (Track S1 and S2). Node only, with no dependencies.
// Workspace's Phase 5 builds the real routes.
//
// It keeps workbooks with versions and per-user levels, tickets and sessions on a clock the tests
// can move, ETags that move on every save, and CORS with preflight. A second server, one port up,
// stands in for Blob Storage: signed read URLs for one version each, expiring on the same clock.
// `/__mock/*` routes reset it, move its clock, make launches, inject failures, save versions as
// someone else, change levels, trash workbooks and turn copies off.
//
//   node tests/workspace-mock/server.mjs [--port 4280] [--blob-port 4281] [--host 127.0.0.1]
//     [--sheet-url http://127.0.0.1:4173]

import { createHash, createHmac, randomBytes, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { workbook } from '../fixtures/workbooks.mjs';

/** The contract's lifetimes and limits (decisions, 2026-11-12, Track S1 and S2). */
export const LIMITS = {
  ticketMs: 120_000,
  idleMs: 30 * 60_000,
  maxMs: 10 * 60 * 60_000,
  readUrlMs: 5 * 60_000,
  maxBytes: 100 * 1024 * 1024,
};

/** Origins allowed to redeem tickets and read answers: the sheet's dev, preview and live hosts. */
export const DEFAULT_ORIGINS = [
  'http://localhost:4173',
  'http://127.0.0.1:4173',
  'http://localhost:5173',
  'http://127.0.0.1:5173',
  'https://sheet.cashgearerp.com',
];

export const XLSX_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';

/** Workspace's users here, by the names tests call them. */
export const USERS = {
  editor: { id: '0193a1c0-0000-7000-8000-00000000e001', name: 'Rana Editor', language: 'en' },
  viewer: { id: '0193a1c0-0000-7000-8000-00000000e002', name: 'Sami Viewer', language: 'en' },
  other: { id: '0193a1c0-0000-7000-8000-00000000e003', name: 'Omar Other', language: 'en' },
  arabic: { id: '0193a1c0-0000-7000-8000-00000000e004', name: 'سارة', language: 'ar' },
};

export const WORKBOOKS = {
  budget: '0193a1c0-0000-7000-8000-0000000b0001',
  empty: '0193a1c0-0000-7000-8000-0000000b0002',
};

/** The seeded budget: two items and their total. */
export function budgetBook() {
  return workbook([
    {
      name: 'Budget',
      rows: [
        [
          1,
          [
            [1, 'Item'],
            [2, 'Amount'],
          ],
        ],
        [
          2,
          [
            [1, 'Rent'],
            [2, 1200],
          ],
        ],
        [
          3,
          [
            [1, 'Travel'],
            [2, 300],
          ],
        ],
        [
          4,
          [
            [1, 'Total'],
            [2, { f: 'SUM(B2:B3)', v: 1500 }],
          ],
        ],
      ],
    },
  ]);
}

/** A control body: someone else's version can be a large test workbook, in base64. */
const MAX_JSON = 32 * 1024 * 1024;
const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const iso = (ms) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const etagOf = (version) => `"${version.id}"`;
const TITLES = {
  'request.invalid': 'The request is invalid',
  'ticket.invalid': 'This link has expired or has already been used',
  'session.expired': 'The file session has ended',
  'origin.denied': 'This site may not open Workspace workbooks',
  'access.denied': 'Access denied',
  'not.found': 'Not found',
  'workbook.gone': 'The workbook was deleted',
  'version.conflict': 'A newer version was saved',
  'version.required': 'Send If-Match or If-None-Match',
  'limit.size': 'The workbook is too large',
  'limit.rate': 'Too many requests',
  'server.error': 'Something went wrong',
};
const CODE_FOR = {
  400: 'request.invalid',
  401: 'session.expired',
  403: 'access.denied',
  404: 'not.found',
  410: 'workbook.gone',
  412: 'version.conflict',
  413: 'limit.size',
  428: 'version.required',
  429: 'limit.rate',
};

class Reply {
  constructor(status, body, headers = {}) {
    this.status = status;
    this.body = body;
    this.headers = headers;
  }
}

export class MockWorkspace {
  constructor({
    origins = DEFAULT_ORIGINS,
    sheetUrl = 'http://127.0.0.1:4173',
    blobUrl = '',
  } = {}) {
    this.origins = new Set(origins);
    this.sheetUrl = sheetUrl.replace(/\/+$/, '');
    this.blobUrl = blobUrl.replace(/\/+$/, '');
    this.signingKey = randomBytes(32);
    this.reset();
  }

  /** Back to the seed: workbooks, levels, tickets, sessions, clock, faults and counts. */
  reset() {
    this.offsetMs = 0;
    this.faults = [];
    this.tickets = new Map();
    this.sessions = new Map();
    this.copiesAllowed = true;
    this.counts = { redeems: 0, renews: 0, ends: 0, reads: 0, saves: 0, copies: 0 };
    this.saves = [];
    this.workbooks = new Map();
    const all = Object.fromEntries(Object.keys(USERS).map((name) => [USERS[name].id, 'edit']));
    this.addWorkbook({
      id: WORKBOOKS.budget,
      name: 'Budget 2027',
      levels: { ...all, [USERS.viewer.id]: 'view' },
      bytes: budgetBook(),
      by: USERS.other,
    });
    this.addWorkbook({
      id: WORKBOOKS.empty,
      name: 'New workbook',
      levels: { ...all, [USERS.viewer.id]: 'view' },
    });
  }

  now() {
    return Date.now() + this.offsetMs;
  }

  addWorkbook({ id = randomUUID(), name, levels, bytes, by }) {
    const book = {
      id,
      name,
      levels: new Map(Object.entries(levels)),
      versions: [],
      trashed: false,
    };
    this.workbooks.set(id, book);
    if (bytes) this.addVersion(book, bytes, by);
    return book;
  }

  addVersion(book, bytes, by) {
    const version = {
      id: randomUUID(),
      bytes: Buffer.from(bytes),
      sha256: sha256(bytes),
      savedBy: { id: by.id, name: by.name },
      savedAt: iso(this.now()),
    };
    book.versions.push(version);
    return version;
  }

  current(book) {
    return book.versions.at(-1) ?? null;
  }

  /** A signed URL for one version's blob, as a SAS covers exactly one blob for a few minutes. */
  readUrl(version) {
    const expires = this.now() + LIMITS.readUrlMs;
    const signature = this.sign(version.id, expires);
    return `${this.blobUrl}/blobs/${version.id}?se=${expires}&sig=${signature}`;
  }

  sign(versionId, expires) {
    return createHmac('sha256', this.signingKey).update(`${versionId}\n${expires}`).digest('hex');
  }

  // -------------------------------------------------------------------------------------------
  // HTTP

  cors(req, res, { methods, headers, expose }) {
    const origin = req.headers.origin;
    const allowed = typeof origin === 'string' && this.origins.has(origin);
    res.setHeader('Vary', 'Origin');
    if (allowed) {
      res.setHeader('Access-Control-Allow-Origin', origin);
      res.setHeader('Access-Control-Expose-Headers', expose);
    }
    if (req.method === 'OPTIONS') {
      if (allowed) {
        res.setHeader('Access-Control-Allow-Methods', methods);
        res.setHeader('Access-Control-Allow-Headers', headers);
        res.setHeader('Access-Control-Max-Age', '600');
      }
      res.writeHead(204).end();
      return null;
    }
    return { origin, allowed };
  }

  async handle(req, res) {
    const correlationId = randomUUID().replaceAll('-', '');
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('X-Correlation-Id', correlationId);
    const seen = this.cors(req, res, {
      methods: 'GET, POST, OPTIONS',
      headers: 'Content-Type, X-Workspace-File-Session, If-Match, If-None-Match',
      expose: 'X-Correlation-Id, ETag, Retry-After, Location',
    });
    if (!seen) return;
    const url = new URL(req.url ?? '/', 'http://mock');
    const path = decodeURIComponent(url.pathname).replace(/^\/+|\/+$/g, '');
    const ctx = { req, res, url, path, correlationId, ...seen };
    const mock = path.startsWith('__mock/');
    try {
      ctx.body = await readBody(req, mock ? MAX_JSON : LIMITS.maxBytes);
    } catch (error) {
      const status = error.status ?? 400;
      const reply = this.problem(ctx, status, CODE_FOR[status], { limit: LIMITS.maxBytes });
      return send(res, reply);
    }
    if (mock) return send(res, this.control(ctx));

    const fault = this.takeFault(path);
    if (fault?.latencyMs) await sleep(fault.latencyMs);
    if (fault?.drop === 'before') return void req.socket.destroy();
    if (fault?.status) {
      const reply = this.problem(
        ctx,
        fault.status,
        fault.code ?? CODE_FOR[fault.status] ?? 'server.error',
      );
      if (fault.retryAfter !== undefined) reply.headers['Retry-After'] = String(fault.retryAfter);
      return send(res, reply);
    }
    const reply = this.route(ctx);
    if (fault?.drop === 'after') return void req.socket.destroy();
    send(res, reply);
  }

  /** Blob Storage: one version's bytes for a valid, unexpired signature. */
  async handleBlob(req, res) {
    const seen = this.cors(req, res, {
      methods: 'GET, OPTIONS',
      headers: '*',
      expose: 'Content-Length, Content-Type, ETag',
    });
    if (!seen) return;
    const url = new URL(req.url ?? '/', 'http://blob');
    const path = url.pathname.replace(/^\/+/, '');
    const fault = this.takeFault(path);
    if (fault?.latencyMs) await sleep(fault.latencyMs);
    if (fault?.drop === 'before') return void req.socket.destroy();
    const match = /^blobs\/([0-9a-f-]{36})$/.exec(path);
    const versionId = match?.[1];
    const expires = Number(url.searchParams.get('se'));
    const given = url.searchParams.get('sig') ?? '';
    const version = versionId && this.findVersion(versionId);
    const refused = (status, code) => {
      const xml = `<?xml version="1.0" encoding="utf-8"?><Error><Code>${code}</Code><Message>${code}</Message></Error>`;
      res.writeHead(status, { 'Content-Type': 'application/xml' }).end(xml);
    };
    if (fault?.status) return refused(fault.status, 'InjectedFault');
    if (req.method !== 'GET') return refused(405, 'UnsupportedHttpVerb');
    if (!versionId || !Number.isSafeInteger(expires) || given !== this.sign(versionId, expires)) {
      return refused(403, 'AuthenticationFailed');
    }
    if (this.now() >= expires) return refused(403, 'AuthenticationFailed');
    if (!version) return refused(404, 'BlobNotFound');
    this.counts.reads++;
    res.writeHead(200, {
      'Content-Type': XLSX_TYPE,
      'Content-Length': version.bytes.length,
      ETag: `"0x${version.sha256.slice(0, 16).toUpperCase()}"`,
    });
    res.end(version.bytes);
  }

  findVersion(id) {
    for (const book of this.workbooks.values()) {
      const version = book.versions.find((v) => v.id === id);
      if (version) return version;
    }
    return null;
  }

  takeFault(path) {
    // `match` is a regular expression over the path without its leading slash, ignoring case:
    // `workbook$` is the workbook's GET alone, where `workbook` is its versions and copies too.
    const index = this.faults.findIndex((f) => f.pattern.test(path));
    if (index < 0) return null;
    const fault = this.faults[index];
    if (fault.times > 0 && --fault.times === 0) this.faults.splice(index, 1);
    return fault;
  }

  problem(ctx, status, code, extra = {}) {
    return new Reply(
      status,
      {
        type: `https://tools.ietf.org/html/rfc9110#status.${status}`,
        title: TITLES[code] ?? 'Something went wrong',
        status,
        code,
        correlationId: ctx.correlationId,
        ...extra,
      },
      { 'Content-Type': 'application/problem+json' },
    );
  }

  route(ctx) {
    const { req, path } = ctx;
    const lower = path.toLowerCase();
    const post = req.method === 'POST';
    const get = req.method === 'GET';
    const routes = {
      'api/v1/sheet-sessions/redeem': [post, () => this.redeem(ctx)],
      'api/v1/sheet-sessions/renew': [post, () => this.withSession(ctx, (s) => this.renew(ctx, s))],
      'api/v1/sheet-sessions/end': [post, () => this.withSession(ctx, (s) => this.end(ctx, s))],
      'api/v1/sheet-sessions/workbook': [
        get,
        () => this.withSession(ctx, (s) => this.workbook(ctx, s)),
      ],
      'api/v1/sheet-sessions/workbook/versions': [
        post,
        () => this.withSession(ctx, (s) => this.save(ctx, s)),
      ],
      'api/v1/sheet-sessions/workbook/copies': [
        post,
        () => this.withSession(ctx, (s) => this.copy(ctx, s)),
      ],
    };
    const found = routes[lower];
    if (!found) return this.problem(ctx, 404, 'not.found');
    if (!found[0]) return this.problem(ctx, 405, 'request.invalid');
    return found[1]();
  }

  // -------------------------------------------------------------------------------------------
  // The routes

  redeem(ctx) {
    // The Origin is checked before the ticket is looked at, so a refused site can't use it up.
    if (!ctx.allowed) return this.problem(ctx, 403, 'origin.denied');
    const body = parseJson(ctx);
    const ticket = isObject(body) ? body.ticket : undefined;
    if (typeof ticket !== 'string' || ticket.length === 0) {
      return this.problem(ctx, 400, 'request.invalid', { detail: 'ticket is required.' });
    }
    const row = this.tickets.get(sha256(ticket));
    const now = this.now();
    if (!row || row.used || now >= row.expiresAt) return this.problem(ctx, 401, 'ticket.invalid');
    row.used = true;
    const book = this.workbooks.get(row.workbookId);
    if (!book || book.trashed) return this.problem(ctx, 410, 'workbook.gone');
    const level = this.level(book, row.userId);
    if (level === 'none') return this.problem(ctx, 403, 'access.denied');
    const token = randomBytes(32).toString('base64url');
    const session = {
      userId: row.userId,
      workbookId: book.id,
      access: level === 'edit' && row.access === 'edit' ? 'edit' : 'view',
      language: row.language,
      idleExpiresAt: now + LIMITS.idleMs,
      maxExpiresAt: now + LIMITS.maxMs,
      ended: false,
      renews: 0,
    };
    this.sessions.set(sha256(token), session);
    this.counts.redeems++;
    return new Reply(200, {
      token,
      expiresAt: iso(Math.min(session.idleExpiresAt, session.maxExpiresAt)),
      maxExpiresAt: iso(session.maxExpiresAt),
      workbook: { id: book.id, name: book.name },
      access: session.access,
      user: this.userRef(row.userId),
      language: session.language,
    });
  }

  withSession(ctx, action) {
    const token = ctx.req.headers['x-workspace-file-session'];
    const session = typeof token === 'string' ? this.sessions.get(sha256(token)) : undefined;
    const now = this.now();
    if (!session || session.ended || now >= Math.min(session.idleExpiresAt, session.maxExpiresAt)) {
      return this.problem(ctx, 401, 'session.expired');
    }
    return action(session);
  }

  renew(_ctx, session) {
    session.idleExpiresAt = Math.min(this.now() + LIMITS.idleMs, session.maxExpiresAt);
    session.renews++;
    this.counts.renews++;
    return new Reply(200, { expiresAt: iso(session.idleExpiresAt) });
  }

  end(_ctx, session) {
    session.ended = true;
    this.counts.ends++;
    return new Reply(204, null);
  }

  /** The session's workbook, checked again on every request: Workspace decides on the server. */
  sessionBook(ctx, session) {
    const book = this.workbooks.get(session.workbookId);
    if (!book || book.trashed) return this.problem(ctx, 410, 'workbook.gone');
    if (this.level(book, session.userId) === 'none') return this.problem(ctx, 403, 'access.denied');
    return book;
  }

  workbook(ctx, session) {
    const book = this.sessionBook(ctx, session);
    if (book instanceof Reply) return book;
    const version = this.current(book);
    const headers = version ? { ETag: etagOf(version) } : {};
    return new Reply(
      200,
      {
        readUrl: version ? this.readUrl(version) : null,
        etag: version ? etagOf(version) : null,
        name: book.name,
        size: version ? version.bytes.length : null,
      },
      headers,
    );
  }

  /** The bytes of a save or a copy: an xlsx, which is a zip. */
  workbookBytes(ctx) {
    const type = (ctx.req.headers['content-type'] ?? '').split(';')[0].trim().toLowerCase();
    if (type !== XLSX_TYPE) {
      return this.problem(ctx, 400, 'request.invalid', {
        detail: `Content-Type must be ${XLSX_TYPE}.`,
      });
    }
    const bytes = ctx.body;
    if (!Buffer.isBuffer(bytes) || bytes.length < 4 || bytes.readUInt32LE(0) !== 0x04034b50) {
      return this.problem(ctx, 400, 'request.invalid', { detail: 'The body isn’t an xlsx.' });
    }
    return bytes;
  }

  save(ctx, session) {
    const book = this.sessionBook(ctx, session);
    if (book instanceof Reply) return book;
    if (session.access !== 'edit' || this.level(book, session.userId) !== 'edit') {
      return this.problem(ctx, 403, 'access.denied');
    }
    const bytes = this.workbookBytes(ctx);
    if (bytes instanceof Reply) return bytes;
    const ifMatch = ctx.req.headers['if-match'];
    const ifNoneMatch = ctx.req.headers['if-none-match'];
    const current = this.current(book);
    if (ifMatch === undefined && ifNoneMatch === undefined) {
      return this.problem(ctx, 428, 'version.required');
    }
    const stale =
      ifMatch !== undefined
        ? !current || !ifMatch.split(',').some((tag) => tag.trim() === etagOf(current))
        : ifNoneMatch.trim() !== '*' || current !== null;
    if (stale) {
      return this.problem(ctx, 412, 'version.conflict', {
        current: current && {
          versionId: current.id,
          etag: etagOf(current),
          sha256: current.sha256,
          savedBy: current.savedBy,
          savedAt: current.savedAt,
        },
      });
    }
    const user = this.userRef(session.userId);
    const version = this.addVersion(book, bytes, user);
    this.counts.saves++;
    this.saves.push({
      workbookId: book.id,
      ifMatch: ifMatch ?? null,
      ifNoneMatch: ifNoneMatch ?? null,
      size: bytes.length,
    });
    return new Reply(
      201,
      { versionId: version.id, etag: etagOf(version) },
      {
        ETag: etagOf(version),
        Location: `/api/v1/sheet-sessions/workbook/versions/${version.id}`,
      },
    );
  }

  copy(ctx, session) {
    const book = this.sessionBook(ctx, session);
    if (book instanceof Reply) return book;
    if (!this.copiesAllowed) return this.problem(ctx, 403, 'access.denied');
    const bytes = this.workbookBytes(ctx);
    if (bytes instanceof Reply) return bytes;
    const user = this.userRef(session.userId);
    const suffix = session.language === 'ar' ? ' (نسخة)' : ' (copy)';
    const copy = this.addWorkbook({
      name: book.name + suffix,
      levels: { [session.userId]: 'edit' },
      bytes,
      by: user,
    });
    session.workbookId = copy.id;
    session.access = 'edit';
    this.counts.copies++;
    const version = this.current(copy);
    return new Reply(
      201,
      {
        workbook: { id: copy.id, name: copy.name },
        access: 'edit',
        versionId: version.id,
        etag: etagOf(version),
      },
      { ETag: etagOf(version) },
    );
  }

  level(book, userId) {
    return book.levels.get(userId) ?? 'none';
  }

  userRef(userId) {
    const user = Object.values(USERS).find((u) => u.id === userId);
    return { id: userId, name: user?.name ?? 'Someone' };
  }

  // -------------------------------------------------------------------------------------------
  // /__mock/*: test control

  control(ctx) {
    const route = ctx.path.slice('__mock/'.length).toLowerCase();
    let body = {};
    try {
      const parsed = ctx.body && ctx.body.length ? JSON.parse(ctx.body.toString('utf8')) : {};
      if (isObject(parsed)) body = parsed;
    } catch {
      return new Reply(400, { message: 'The body isn’t JSON.' });
    }
    const ok = (value) => new Reply(value === undefined ? 204 : 200, value ?? null);
    const bad = (why) => new Reply(400, { message: why });
    const userOf = (name) => USERS[name] ?? Object.values(USERS).find((u) => u.id === name);
    const bookOf = (id) => this.workbooks.get(WORKBOOKS[id] ?? id);
    switch (route) {
      case 'reset':
        this.reset();
        return ok();
      case 'clock':
        if (Number.isFinite(body.advanceMs)) this.offsetMs += body.advanceMs;
        return ok({ now: iso(this.now()) });
      case 'fault': {
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
      case 'launch': {
        // What Workspace's `POST /api/v1/workbooks/{id}/launch` does for a signed-in user.
        const user = userOf(body.user ?? 'editor');
        const book = bookOf(body.workbook ?? 'budget');
        if (!user || !book) return bad('Unknown user or workbook.');
        const level = this.level(book, user.id);
        if (level === 'none') return new Reply(403, { message: 'No access.' });
        const ticket = randomBytes(32).toString('base64url');
        const expiresAt = this.now() + LIMITS.ticketMs;
        this.tickets.set(sha256(ticket), {
          userId: user.id,
          workbookId: book.id,
          access: body.access === 'view' || level === 'view' ? 'view' : 'edit',
          language: body.language ?? user.language,
          expiresAt,
          used: false,
        });
        return ok({
          ticket,
          expiresAt: iso(expiresAt),
          url: `${this.sheetUrl}/cloud#launch=${ticket}`,
        });
      }
      case 'version': {
        // Someone else saves: a concurrent edit in Workspace.
        const book = bookOf(body.workbook ?? 'budget');
        const user = userOf(body.user ?? 'other');
        if (!book || !user) return bad('Unknown user or workbook.');
        const bytes =
          typeof body.bytes === 'string' ? Buffer.from(body.bytes, 'base64') : budgetBook();
        const version = this.addVersion(book, bytes, user);
        return ok({ versionId: version.id, etag: etagOf(version) });
      }
      case 'level': {
        const book = bookOf(body.workbook ?? 'budget');
        const user = userOf(body.user);
        if (!book || !user || !['edit', 'view', 'none'].includes(body.level)) {
          return bad('Unknown workbook or user, or level isn’t edit, view or none.');
        }
        book.levels.set(user.id, body.level);
        return ok();
      }
      case 'trash': {
        const book = bookOf(body.workbook ?? 'budget');
        if (!book) return bad('Unknown workbook.');
        book.trashed = body.trashed !== false;
        return ok();
      }
      case 'copies':
        this.copiesAllowed = body.allowed !== false;
        return ok();
      case 'content': {
        // What Workspace's `GET /api/v1/workbooks/{id}/content` gives its page for the embed
        // (Track S3): a signed URL for the current version, on the clock like every other.
        const book = bookOf(body.workbook ?? ctx.url.searchParams.get('workbook') ?? 'budget');
        if (!book) return bad('Unknown workbook.');
        const version = this.current(book);
        if (!version) return ok({ readUrl: null, etag: null, size: null, versionId: null });
        return ok({
          readUrl: this.readUrl(version),
          etag: etagOf(version),
          size: version.bytes.length,
          versionId: version.id,
        });
      }
      case 'bytes': {
        const book = bookOf(ctx.url.searchParams.get('workbook') ?? 'budget');
        const version = book && this.current(book);
        if (!version) return new Reply(404, { message: 'No version.' });
        return new Reply(200, version.bytes, { 'Content-Type': XLSX_TYPE });
      }
      case 'state':
        return ok({
          now: iso(this.now()),
          counts: { ...this.counts },
          saves: this.saves,
          tickets: [...this.tickets.values()].map((t) => ({
            used: t.used,
            workbookId: t.workbookId,
          })),
          sessions: [...this.sessions.values()].map((s) => ({
            userId: s.userId,
            workbookId: s.workbookId,
            access: s.access,
            ended: s.ended,
            renews: s.renews,
            expiresAt: iso(Math.min(s.idleExpiresAt, s.maxExpiresAt)),
          })),
          workbooks: [...this.workbooks.values()].map((book) => {
            const current = this.current(book);
            return {
              id: book.id,
              name: book.name,
              trashed: book.trashed,
              versions: book.versions.length,
              etag: current ? etagOf(current) : null,
              savedBy: current?.savedBy.name ?? null,
            };
          }),
        });
      default:
        return new Reply(404, { message: 'Not found.' });
    }
  }
}

function parseJson(ctx) {
  const type = (ctx.req.headers['content-type'] ?? '').split(';')[0].trim().toLowerCase();
  if (type !== 'application/json' || !ctx.body?.length) return undefined;
  try {
    return JSON.parse(ctx.body.toString('utf8'));
  } catch {
    return undefined;
  }
}

function readBody(req, limit) {
  return new Promise((done, failed) => {
    const chunks = [];
    let size = 0;
    let refused = false;
    req.on('data', (chunk) => {
      if (refused) return;
      size += chunk.length;
      if (size > limit) {
        refused = true;
        failed(Object.assign(new Error('The request is too large.'), { status: 413 }));
      } else chunks.push(chunk);
    });
    req.on('end', () => {
      if (!refused) done(Buffer.concat(chunks));
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
  if (Buffer.isBuffer(reply.body)) {
    res.writeHead(reply.status, { 'Content-Length': reply.body.length });
    res.end(reply.body);
    return;
  }
  const json = JSON.stringify(reply.body);
  const type = reply.headers['Content-Type'] ?? 'application/json; charset=utf-8';
  res.writeHead(reply.status, { 'Content-Type': type, 'Content-Length': Buffer.byteLength(json) });
  res.end(json);
}

function listen(server, port, host) {
  return new Promise((done, failed) => {
    server.once('error', failed);
    server.listen(port, host, () => done(server.address().port));
  });
}

/**
 * Starts the mock's API on `port` and its Blob Storage on `blobPort` (0 picks free ones).
 * Resolves once both listen.
 */
export async function startMockWorkspace({
  port = 0,
  blobPort = 0,
  host = '127.0.0.1',
  origins,
  sheetUrl,
} = {}) {
  const shown = host === '0.0.0.0' ? '127.0.0.1' : host;
  const mock = new MockWorkspace({ origins, sheetUrl });
  const fail = (res, error) => {
    if (!res.headersSent) {
      res.writeHead(500, { 'Content-Type': 'application/problem+json' });
      res.end(
        JSON.stringify({
          title: String(error?.message ?? error),
          status: 500,
          code: 'server.error',
        }),
      );
    }
  };
  const api = createServer((req, res) => mock.handle(req, res).catch((error) => fail(res, error)));
  const blob = createServer((req, res) =>
    mock.handleBlob(req, res).catch((error) => fail(res, error)),
  );
  const apiPort = await listen(api, port, host);
  const blobAt = await listen(blob, blobPort, host);
  mock.blobUrl = `http://${shown}:${blobAt}`;
  const close = (server) =>
    new Promise((done) => {
      server.close(() => done());
      server.closeAllConnections();
    });
  return {
    url: `http://${shown}:${apiPort}`,
    blobUrl: mock.blobUrl,
    port: apiPort,
    mock,
    close: async () => {
      await Promise.all([close(api), close(blob)]);
    },
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const i = args.indexOf(`--${name}`);
    return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
  };
  const port = Number(option('port', '4280'));
  const { url, blobUrl } = await startMockWorkspace({
    port,
    blobPort: Number(option('blob-port', String(port + 1))),
    host: option('host', '127.0.0.1'),
    sheetUrl: option('sheet-url', 'http://127.0.0.1:4173'),
  });
  console.log(`Mock Workspace listening on ${url}, its Blob Storage on ${blobUrl}`);
}
