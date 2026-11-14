// The `sheet-session.1` contract suite (`docs/workspace-contract.md`, Track S5). It checks the
// wire, raw, and the sheet's own session code against it, so it runs against any server:
//
//   pnpm test                                              starts the mock in tests/workspace-mock
//   WORKSPACE_API_URL=http://127.0.0.1:5280 \
//   WORKSPACE_LAUNCHER_URL=http://127.0.0.1:5280/... pnpm test   Workspace's real API (its `pnpm dev:server`)
//
// Workspace's launch needs a signed-in Workspace user, which a test can't be, so against a real
// server the tickets come from WORKSPACE_LAUNCHER_URL: Workspace's test launcher, which answers
// `POST {user, workbook, access?, language?}` with `{ticket}` as the mock's `/__mock/launch` does.
// `user` is `editor`, `viewer` (view access) or `other`; `workbook` is `budget` (one version at
// least, which the suite adds to) or `empty` (a workbook with no version, new for each launch).
// WORKSPACE_TEST_ORIGIN is the sheet origin the suite sends (http://localhost:4173 unless set).
// Cases that need the mock's clock skip elsewhere. The suite saves versions and copies, so it
// refuses a server that isn't on this machine unless WORKSPACE_ALLOW_WRITES=1.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { startMockWorkspace } from '../../../tests/workspace-mock/server.mjs';
import { workbook } from '../../../tests/fixtures/workbooks.mjs';
import {
  CORRELATION_HEADER,
  ROUTES,
  SESSION_HEADER,
  XLSX_TYPE,
  type FileSession,
} from './contract.ts';
import { WorkspaceHttp } from './http.ts';
import { WorkspaceSession, sha256Hex } from './session.ts';
import {
  readCopyResponse,
  readFileSession,
  readProblem,
  readRenewResponse,
  readVersionResponse,
  readWorkbookResponse,
} from './validate.ts';

const env =
  (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env ?? {};
const external = env.WORKSPACE_API_URL?.replace(/\/+$/, '');
const onMock = !external || env.WORKSPACE_IS_MOCK === '1';
const ORIGIN = env.WORKSPACE_TEST_ORIGIN ?? 'http://localhost:4173';
const STRANGER = 'https://evil.example';

let base = external ?? '';
let launcher = env.WORKSPACE_LAUNCHER_URL ?? '';
let stopMock: (() => Promise<void>) | undefined;

beforeAll(async () => {
  if (!external) {
    const server = await startMockWorkspace();
    base = server.url;
    launcher = `${base}/__mock/launch`;
    stopMock = server.close;
    return;
  }
  if (!launcher) {
    throw new Error('WORKSPACE_LAUNCHER_URL names where the suite gets tickets from Workspace.');
  }
  if (
    !/^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/.test(base) &&
    env.WORKSPACE_ALLOW_WRITES !== '1'
  ) {
    throw new Error(
      `The suite saves versions and copies at ${base}. Set WORKSPACE_ALLOW_WRITES=1 if it is a test server.`,
    );
  }
});

afterAll(async () => stopMock?.());

beforeEach(async () => {
  if (onMock) await fetch(`${base}/__mock/reset`, { method: 'POST' });
});

type User = 'editor' | 'viewer' | 'other';

/** A ticket, as Workspace's "Open in Cash Gear Sheet" gives one. */
async function launch(user: User = 'editor', book: 'budget' | 'empty' = 'budget') {
  const response = await fetch(launcher, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ user, workbook: book }),
  });
  expect(response.status, 'the launcher').toBe(200);
  const answer = (await response.json()) as { ticket: string };
  expect(answer.ticket).toMatch(/^[A-Za-z0-9_-]{43}$/);
  return answer.ticket;
}

interface Call {
  session?: string;
  json?: unknown;
  bytes?: Uint8Array;
  headers?: Record<string, string>;
  origin?: string | null;
}

/** A raw request as the sheet sends it: no credentials, no redirects, no referrer. */
async function call(method: string, route: string, options: Call = {}) {
  const headers: Record<string, string> = { ...options.headers };
  const origin = options.origin === undefined ? ORIGIN : options.origin;
  if (origin) headers.Origin = origin;
  if (options.session) headers[SESSION_HEADER] = options.session;
  let body: BodyInit | undefined;
  if (options.json !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(options.json);
  } else if (options.bytes) {
    headers['Content-Type'] ??= XLSX_TYPE;
    body = options.bytes as BodyInit;
  }
  const response = await fetch(`${base}/${route}`, {
    method,
    headers,
    body,
    credentials: 'omit',
    redirect: 'error',
    referrerPolicy: 'no-referrer',
  });
  // Every sheet-session answer: never cached and always correlated. A redirect throws.
  expect(response.headers.get('Cache-Control') ?? '', `${method} ${route}`).toMatch(/no-store/);
  expect(response.headers.get(CORRELATION_HEADER), `${method} ${route}`).toBeTruthy();
  return response;
}

/** The answer's problem, checked as ProblemDetails with the request's correlation id. */
async function problem(response: Response, status: number, code: string) {
  expect(response.status).toBe(status);
  expect(response.headers.get('Content-Type') ?? '').toMatch(/application\/problem\+json/);
  const body = readProblem(await response.json());
  expect(body.code).toBe(code);
  expect(body.status).toBe(status);
  expect(body.correlationId).toBe(response.headers.get(CORRELATION_HEADER));
  return body;
}

async function redeem(ticket: string): Promise<FileSession> {
  const response = await call('POST', ROUTES.redeem, { json: { ticket } });
  expect(response.status).toBe(200);
  return readFileSession(await response.json());
}

/** A small workbook, different for each `text`, so each save's SHA-256 is its own. */
const book = (text: string) =>
  new Uint8Array(workbook([{ name: 'Sheet1', rows: [[1, [[1, text]]]] }]));
const sha = (bytes: Uint8Array) => sha256Hex(new Blob([bytes as BlobPart]));

describe('Redeem', () => {
  it('gives a session for one workbook, and uses the ticket up', async () => {
    const ticket = await launch();
    const session = await redeem(ticket);
    expect(session.access).toBe('edit');
    expect(['en', 'ar']).toContain(session.language);
    expect(Date.parse(session.expiresAt)).toBeLessThanOrEqual(Date.parse(session.maxExpiresAt));
    expect(Date.parse(session.expiresAt) - Date.now()).toBeGreaterThan(25 * 60_000);
    await problem(await call('POST', ROUTES.redeem, { json: { ticket } }), 401, 'ticket.invalid');
  });

  it('refuses an origin not on the list before it looks at the ticket', async () => {
    const ticket = await launch();
    await problem(
      await call('POST', ROUTES.redeem, { json: { ticket }, origin: STRANGER }),
      403,
      'origin.denied',
    );
    await problem(
      await call('POST', ROUTES.redeem, { json: { ticket }, origin: null }),
      403,
      'origin.denied',
    );
    await redeem(ticket);
  });

  it('refuses a missing ticket and an unknown one', async () => {
    await problem(await call('POST', ROUTES.redeem, { json: {} }), 400, 'request.invalid');
    await problem(
      await call('POST', ROUTES.redeem, { json: { ticket: 'A'.repeat(43) } }),
      401,
      'ticket.invalid',
    );
  });

  it.runIf(onMock)('refuses a ticket older than 120 seconds', async () => {
    const ticket = await launch();
    await fetch(`${base}/__mock/clock`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ advanceMs: 121_000 }),
    });
    await problem(await call('POST', ROUTES.redeem, { json: { ticket } }), 401, 'ticket.invalid');
  });
});

describe('Renew and End', () => {
  it('renew slides the expiry; end ends the session; an unknown token has none', async () => {
    const session = await redeem(await launch());
    const renewed = await call('POST', ROUTES.renew, { session: session.token });
    expect(renewed.status).toBe(200);
    const { expiresAt } = readRenewResponse(await renewed.json());
    expect(Date.parse(expiresAt)).toBeGreaterThanOrEqual(Date.parse(session.expiresAt));
    expect(Date.parse(expiresAt)).toBeLessThanOrEqual(Date.parse(session.maxExpiresAt));
    const ended = await call('POST', ROUTES.end, { session: session.token });
    expect(ended.status).toBe(204);
    await problem(
      await call('POST', ROUTES.renew, { session: session.token }),
      401,
      'session.expired',
    );
    await problem(
      await call('GET', ROUTES.workbook, { session: 'not-a-session' }),
      401,
      'session.expired',
    );
    await problem(await call('GET', ROUTES.workbook), 401, 'session.expired');
  });
});

describe('Reading', () => {
  it('gives a signed URL for exactly the current version, read with no credentials', async () => {
    const session = await redeem(await launch());
    const response = await call('GET', ROUTES.workbook, { session: session.token });
    expect(response.status).toBe(200);
    const found = readWorkbookResponse(await response.json());
    expect(found.readUrl).not.toBeNull();
    expect(response.headers.get('ETag')).toBe(found.etag);
    const blob = await fetch(found.readUrl!, {
      headers: { Origin: ORIGIN },
      credentials: 'omit',
      redirect: 'error',
      referrerPolicy: 'no-referrer',
    });
    expect(blob.status).toBe(200);
    expect(blob.headers.get('Access-Control-Allow-Origin')).toBe(ORIGIN);
    const bytes = new Uint8Array(await blob.arrayBuffer());
    expect([...bytes.subarray(0, 2)]).toEqual([0x50, 0x4b]);
    expect(bytes.length).toBe(found.size);
  });

  it('a workbook with no version has no URL, ETag or size', async () => {
    const session = await redeem(await launch('editor', 'empty'));
    const response = await call('GET', ROUTES.workbook, { session: session.token });
    const found = readWorkbookResponse(await response.json());
    expect([found.readUrl, found.etag, found.size]).toEqual([null, null, null]);
  });
});

describe('Saving', () => {
  async function current(token: string) {
    const response = await call('GET', ROUTES.workbook, { session: token });
    return readWorkbookResponse(await response.json());
  }

  it('saves with If-Match, and a stale ETag is 412 naming the version that won', async () => {
    const session = await redeem(await launch());
    const start = (await current(session.token)).etag!;
    const first = book('first save');
    const saved = await call('POST', ROUTES.versions, {
      session: session.token,
      bytes: first,
      headers: { 'If-Match': start },
    });
    expect(saved.status).toBe(201);
    const version = readVersionResponse(await saved.json());
    expect(saved.headers.get('ETag')).toBe(version.etag);
    expect(saved.headers.get('Location')).toBeTruthy();
    expect(version.etag).not.toBe(start);
    expect((await current(session.token)).etag).toBe(version.etag);

    const stale = await call('POST', ROUTES.versions, {
      session: session.token,
      bytes: book('second save'),
      headers: { 'If-Match': start },
    });
    const conflict = await problem(stale, 412, 'version.conflict');
    expect(conflict.current).toMatchObject({
      versionId: version.versionId,
      etag: version.etag,
      sha256: await sha(first),
      savedBy: { id: session.user.id },
    });
  });

  it('needs a precondition, an xlsx, and edit access', async () => {
    const session = await redeem(await launch());
    await problem(
      await call('POST', ROUTES.versions, { session: session.token, bytes: book('none') }),
      428,
      'version.required',
    );
    const etag = (await current(session.token)).etag!;
    await problem(
      await call('POST', ROUTES.versions, {
        session: session.token,
        bytes: new TextEncoder().encode('not a workbook'),
        headers: { 'If-Match': etag },
      }),
      400,
      'request.invalid',
    );
    const viewer = await redeem(await launch('viewer'));
    expect(viewer.access).toBe('view');
    await problem(
      await call('POST', ROUTES.versions, {
        session: viewer.token,
        bytes: book('viewer'),
        headers: { 'If-Match': (await current(viewer.token)).etag! },
      }),
      403,
      'access.denied',
    );
  });

  it('the first version of an empty workbook is saved with If-None-Match: *', async () => {
    const session = await redeem(await launch('editor', 'empty'));
    const first = await call('POST', ROUTES.versions, {
      session: session.token,
      bytes: book('first version'),
      headers: { 'If-None-Match': '*' },
    });
    expect(first.status).toBe(201);
    readVersionResponse(await first.json());
    await problem(
      await call('POST', ROUTES.versions, {
        session: session.token,
        bytes: book('again'),
        headers: { 'If-None-Match': '*' },
      }),
      412,
      'version.conflict',
    );
  });
});

describe('Copies', () => {
  it('make a new workbook beside the original, which the session moves to', async () => {
    const session = await redeem(await launch('viewer'));
    const before = readWorkbookResponse(
      await (await call('GET', ROUTES.workbook, { session: session.token })).json(),
    );
    const made = await call('POST', ROUTES.copies, {
      session: session.token,
      bytes: book('a copy'),
    });
    expect(made.status).toBe(201);
    const copy = readCopyResponse(await made.json());
    expect(copy.access).toBe('edit');
    expect(copy.workbook.id).not.toBe(session.workbook.id);
    expect(copy.workbook.name).toMatch(/ \((copy|نسخة)\)$/);
    const after = readWorkbookResponse(
      await (await call('GET', ROUTES.workbook, { session: session.token })).json(),
    );
    expect(after.name).toBe(copy.workbook.name);
    expect(after.etag).toBe(copy.etag);
    expect(after.name).not.toBe(before.name);
  });
});

describe('CORS', () => {
  it('lets the sheet’s origins call with its headers, never with credentials', async () => {
    const preflight = await fetch(`${base}/${ROUTES.versions}`, {
      method: 'OPTIONS',
      headers: {
        Origin: ORIGIN,
        'Access-Control-Request-Method': 'POST',
        'Access-Control-Request-Headers': 'content-type,x-workspace-file-session,if-match',
      },
    });
    expect(preflight.status).toBeLessThan(300);
    expect(preflight.headers.get('Access-Control-Allow-Origin')).toBe(ORIGIN);
    expect(preflight.headers.get('Access-Control-Allow-Methods') ?? '').toMatch(/POST/);
    const allowed = (preflight.headers.get('Access-Control-Allow-Headers') ?? '').toLowerCase();
    for (const header of [
      'content-type',
      'x-workspace-file-session',
      'if-match',
      'if-none-match',
    ]) {
      expect(allowed).toContain(header);
    }
    expect(preflight.headers.get('Access-Control-Allow-Credentials')).not.toBe('true');

    const stranger = await fetch(`${base}/${ROUTES.redeem}`, {
      method: 'OPTIONS',
      headers: { Origin: STRANGER, 'Access-Control-Request-Method': 'POST' },
    });
    expect(stranger.headers.get('Access-Control-Allow-Origin')).toBeNull();

    const session = await redeem(await launch());
    const answer = await call('GET', ROUTES.workbook, { session: session.token });
    expect(answer.headers.get('Access-Control-Allow-Origin')).toBe(ORIGIN);
    const exposed = (answer.headers.get('Access-Control-Expose-Headers') ?? '').toLowerCase();
    for (const header of ['x-correlation-id', 'etag', 'retry-after', 'location']) {
      expect(exposed).toContain(header);
    }
  });
});

describe('the sheet’s own session code', () => {
  const http = () =>
    new WorkspaceHttp({
      baseUrl: base,
      fetch: (input, init) =>
        fetch(input, { ...init, headers: { ...(init?.headers as object), Origin: ORIGIN } }),
    });

  it('opens, reads, saves twice and saves a copy, as the cloud platform does', async () => {
    const session = await WorkspaceSession.redeem(http(), await launch());
    const read = await session.read();
    expect(read.blob).not.toBeNull();
    const first = await session.saveVersion(
      new Blob([book('from the sheet') as BlobPart]),
      read.etag,
    );
    const second = await session.saveVersion(new Blob([book('and again') as BlobPart]), first.etag);
    expect(second.etag).not.toBe(first.etag);
    const copy = await session.saveCopy(new Blob([book('its copy') as BlobPart]));
    expect(session.workbook).toEqual(copy.workbook);
    expect((await session.read()).etag).toBe(copy.etag);
    session.end();
  });
});
