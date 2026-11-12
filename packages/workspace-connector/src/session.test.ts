// The file session against the mock Workspace (Track S1): Redeem from an allowed origin only and
// once, Renew and End, reading by a fresh signed URL, saving with If-Match and If-None-Match, a
// stale ETag's 412 with who saved, a lost answer's save counted once, copies that move the
// session, view access, and Reconnect's handover for the same workbook, user and API only.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import {
  budgetBook,
  startMockWorkspace,
  USERS,
  WORKBOOKS,
  XLSX_TYPE,
} from '../../../tests/workspace-mock/server.mjs';
import type { FileSession } from './contract.ts';
import { offerHandover, waitForHandover, type Channel, type ChannelFactory } from './handover.ts';
import { WorkspaceError, WorkspaceHttp, kindOf, retryAfterMs } from './http.ts';
import { RENEW_EVERY_MS, WorkspaceSession, sha256Hex } from './session.ts';

let base = '';
let stop: (() => Promise<void>) | undefined;
const ORIGIN = 'http://localhost:4173';
const withOrigin =
  (origin: string | null = ORIGIN): typeof fetch =>
  (input, init) =>
    fetch(input, {
      ...init,
      headers: { ...(init?.headers as object), ...(origin ? { Origin: origin } : {}) },
    });

beforeAll(async () => {
  const server = await startMockWorkspace();
  base = server.url;
  stop = server.close;
});
afterAll(async () => stop?.());
beforeEach(async () => {
  await mock('reset');
});

const http = (origin?: string | null) =>
  new WorkspaceHttp({ baseUrl: base, fetch: withOrigin(origin), sleep: async () => {} });

async function mock(route: string, body: unknown = {}) {
  const response = await fetch(`${base}/__mock/${route}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  return response.status === 204 ? null : ((await response.json()) as Record<string, unknown>);
}

interface State {
  counts: Record<string, number>;
  saves: { ifMatch: string | null; ifNoneMatch: string | null; size: number }[];
  tickets: { used: boolean }[];
  sessions: { ended: boolean; renews: number; workbookId: string; access: string }[];
  workbooks: { id: string; name: string; versions: number; etag: string | null }[];
}

async function state(): Promise<State> {
  return (await (await fetch(`${base}/__mock/state`)).json()) as State;
}

async function ticket(user = 'editor', workbook = 'budget'): Promise<string> {
  return (await mock('launch', { user, workbook }))!.ticket as string;
}

async function open(user = 'editor', workbook = 'budget', now?: () => number) {
  return WorkspaceSession.redeem(http(), await ticket(user, workbook), { now });
}

const xlsx = (bytes: Uint8Array = budgetBook()) =>
  new Blob([bytes as BlobPart], { type: XLSX_TYPE });
/** A valid workbook's bytes with something after them, so their SHA-256 differs. */
const changed = (tail: string) => {
  const book = budgetBook();
  const extra = new TextEncoder().encode(tail);
  const bytes = new Uint8Array(book.length + extra.length);
  bytes.set(book);
  bytes.set(extra, book.length);
  return bytes;
};
const same = (a: Uint8Array, b: Uint8Array) =>
  a.length === b.length && a.every((byte, i) => byte === b[i]);

async function caught(work: Promise<unknown>): Promise<WorkspaceError> {
  try {
    await work;
  } catch (error) {
    if (error instanceof WorkspaceError) return error;
    throw error;
  }
  throw new Error('It didn’t fail.');
}

describe('Redeem', () => {
  it('gives a session for one workbook, its access, user and language, and uses the ticket up', async () => {
    const t = await ticket();
    const session = await WorkspaceSession.redeem(http(), t);
    expect(session.workbook).toEqual({ id: WORKBOOKS.budget, name: 'Budget 2027' });
    expect(session.access).toBe('edit');
    expect(session.user).toEqual({ id: USERS.editor.id, name: USERS.editor.name });
    expect(session.language).toBe('en');
    expect(session.state).toBe('open');
    expect(session.expiresAt - Date.now()).toBeGreaterThan(29 * 60_000);
    const again = await caught(WorkspaceSession.redeem(http(), t));
    expect([again.kind, again.status, again.code]).toEqual(['ticket', 401, 'ticket.invalid']);
    expect((await state()).tickets[0]!.used).toBe(true);
  });

  it('refuses another origin before it looks at the ticket, which stays usable', async () => {
    const t = await ticket();
    const refused = await caught(WorkspaceSession.redeem(http('https://evil.example'), t));
    expect([refused.kind, refused.status, refused.code]).toEqual(['origin', 403, 'origin.denied']);
    expect(refused.correlationId).toMatch(/^[0-9a-f]{32}$/);
    const none = await caught(WorkspaceSession.redeem(http(null), t));
    expect(none.kind).toBe('origin');
    await expect(WorkspaceSession.redeem(http(), t)).resolves.toBeInstanceOf(WorkspaceSession);
  });

  it('refuses an expired ticket', async () => {
    const t = await ticket();
    await mock('clock', { advanceMs: 121_000 });
    expect((await caught(WorkspaceSession.redeem(http(), t))).kind).toBe('ticket');
  });

  it('is never sent twice, since a lost answer may have used the ticket up', async () => {
    await mock('fault', { match: 'redeem$', drop: 'after' });
    const lost = await caught(WorkspaceSession.redeem(http(), await ticket()));
    expect(lost.kind).toBe('network');
    expect((await state()).counts.redeems).toBe(1);
  });

  it('waits out a 429 as Retry-After asks, and gives up when it lasts', async () => {
    const waits: number[] = [];
    const patient = new WorkspaceHttp({
      baseUrl: base,
      fetch: withOrigin(),
      sleep: async (ms) => void waits.push(ms),
    });
    await mock('fault', { match: 'redeem$', status: 429, retryAfter: 2, times: 2 });
    await expect(WorkspaceSession.redeem(patient, await ticket())).resolves.toBeDefined();
    expect(waits).toEqual([2000, 2000]);
    await mock('fault', { match: 'redeem$', status: 429, times: 0 });
    expect((await caught(WorkspaceSession.redeem(patient, await ticket()))).kind).toBe('busy');
  });

  it('keeps its token out of every way it shows itself', async () => {
    let token = '';
    const spying: typeof fetch = async (input, init) => {
      const response = await withOrigin()(input, init);
      if (String(input).endsWith('/redeem')) {
        token = ((await response.clone().json()) as { token: string }).token;
      }
      return response;
    };
    const session = await WorkspaceSession.redeem(
      new WorkspaceHttp({ baseUrl: base, fetch: spying }),
      await ticket(),
    );
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(JSON.stringify(session)).not.toContain(token);
    expect(Object.values(session).join(' ')).not.toContain(token);
    expect(String(session)).not.toContain(token);
    expect(session.handover().token).toBe(token);
  });
});

describe('Renew and End', () => {
  it('renew slides the idle expiry, at most every 5 minutes after activity', async () => {
    let now = Date.now();
    const session = await open('editor', 'budget', () => now);
    const before = session.expiresAt;
    session.touch();
    expect((await state()).counts.renews).toBe(0);
    await mock('clock', { advanceMs: RENEW_EVERY_MS });
    now += RENEW_EVERY_MS;
    session.touch();
    await expect.poll(async () => (await state()).counts.renews).toBe(1);
    await expect.poll(() => session.expiresAt).toBeGreaterThan(before);
  });

  it('a session idle for 30 minutes has ended, here and in Workspace', async () => {
    let now = Date.now();
    const session = await open('editor', 'budget', () => now);
    now += 31 * 60_000;
    await mock('clock', { advanceMs: 31 * 60_000 });
    expect(session.state).toBe('expired');
    const ended = await caught(session.read());
    expect(ended.kind).toBe('session');
  });

  it('a 401 ends the session here too', async () => {
    const session = await open();
    const heard: string[] = [];
    session.onState((s) => heard.push(s));
    await mock('fault', { match: 'workbook$', status: 401 });
    expect((await caught(session.read())).kind).toBe('session');
    expect(session.state).toBe('expired');
    expect(heard).toEqual(['expired']);
  });

  it('end ends the session in Workspace, with keepalive, and once', async () => {
    const session = await open();
    session.end();
    session.end();
    expect(session.state).toBe('expired');
    await expect.poll(async () => (await state()).sessions[0]!.ended).toBe(true);
    expect((await state()).counts.ends).toBe(1);
  });

  it('giving it away forgets it here and leaves it open in Workspace', async () => {
    const session = await open();
    session.giveAway();
    expect(session.state).toBe('expired');
    expect((await state()).sessions[0]!.ended).toBe(false);
  });
});

describe('Reading the workbook', () => {
  it('reads the current version’s bytes by a fresh signed URL', async () => {
    const session = await open();
    const progress: number[] = [];
    const read = await session.read({ onProgress: (received) => progress.push(received) });
    expect(read.name).toBe('Budget 2027');
    expect(read.etag).toMatch(/^"[0-9a-f-]{36}"$/);
    const bytes = new Uint8Array(await read.blob!.arrayBuffer());
    expect(same(bytes, budgetBook())).toBe(true);
    expect(progress.at(-1)).toBe(bytes.length);
    expect((await state()).counts.reads).toBe(1);
  });

  it('asks for a fresh URL once when the signed URL is refused', async () => {
    const session = await open();
    await mock('fault', { match: '^blobs/', status: 403 });
    const read = await session.read();
    expect(read.blob!.size).toBe(budgetBook().length);
    await mock('fault', { match: '^blobs/', status: 403, times: 2 });
    expect((await caught(session.read())).kind).toBe('readUrl');
  });

  it('a workbook with no version yet has no bytes and no ETag', async () => {
    const session = await open('editor', 'empty');
    expect(await session.read()).toEqual({ name: 'New workbook', etag: null, blob: null });
  });

  it('a workbook trashed since the launch is gone, and one whose access went is denied', async () => {
    const session = await open();
    await mock('trash', { workbook: 'budget' });
    expect((await caught(session.read())).kind).toBe('gone');
    await mock('trash', { workbook: 'budget', trashed: false });
    await mock('level', { workbook: 'budget', user: 'editor', level: 'none' });
    expect((await caught(session.read())).kind).toBe('denied');
  });
});

describe('Saving', () => {
  it('saves a new version with If-Match, and the next save sends the new ETag', async () => {
    const session = await open();
    const { etag } = await session.read();
    const first = await session.saveVersion(xlsx(), etag);
    expect(first.etag).not.toBe(etag);
    const second = await session.saveVersion(xlsx(), first.etag);
    const { saves, workbooks } = await state();
    expect(saves.map((s) => s.ifMatch)).toEqual([etag, first.etag]);
    expect(workbooks.find((w) => w.id === WORKBOOKS.budget)).toMatchObject({
      versions: 3,
      etag: second.etag,
    });
  });

  it('a stale ETag is a conflict naming who saved and when, and nothing is written', async () => {
    const session = await open();
    const { etag } = await session.read();
    await mock('version', { workbook: 'budget', user: 'other' });
    const conflict = await caught(session.saveVersion(xlsx(changed('mine')), etag));
    expect([conflict.kind, conflict.status, conflict.code]).toEqual([
      'conflict',
      412,
      'version.conflict',
    ]);
    expect(conflict.problem?.current?.savedBy.name).toBe(USERS.other.name);
    expect(Date.parse(conflict.problem!.current!.savedAt)).not.toBeNaN();
    expect((await state()).counts.saves).toBe(0);
  });

  it('a save whose answer was lost is sent again, and its own landing counts as saved', async () => {
    const session = await open();
    const { etag } = await session.read();
    await mock('fault', { match: 'versions$', drop: 'after' });
    const saved = await session.saveVersion(xlsx(changed('changed')), etag);
    const { workbooks, counts } = await state();
    expect(counts.saves).toBe(1);
    expect(workbooks.find((w) => w.id === WORKBOOKS.budget)!.etag).toBe(saved.etag);
    expect(saved.versionId).toBe(saved.etag.slice(1, -1));
  });

  it('the first save of a workbook with no version sends If-None-Match: *', async () => {
    const session = await open('editor', 'empty');
    const saved = await session.saveVersion(xlsx(), null);
    expect((await state()).saves[0]).toMatchObject({ ifMatch: null, ifNoneMatch: '*' });
    expect((await caught(session.saveVersion(xlsx(changed('again')), null))).kind).toBe('conflict');
    await expect(session.saveVersion(xlsx(), saved.etag)).resolves.toBeDefined();
  });

  it('a view session can’t save, and a session whose edit access went turns to view', async () => {
    const viewer = await open('viewer');
    expect(viewer.access).toBe('view');
    const { etag } = await viewer.read();
    expect((await caught(viewer.saveVersion(xlsx(), etag))).kind).toBe('denied');
    const editor = await open();
    await mock('level', { workbook: 'budget', user: 'editor', level: 'view' });
    expect((await caught(editor.saveVersion(xlsx(), etag))).kind).toBe('denied');
    expect(editor.access).toBe('view');
  });

  it('refuses what isn’t an xlsx, and what is too large', async () => {
    const session = await open();
    const { etag } = await session.read();
    const notZip = await caught(session.saveVersion(new Blob(['hello']), etag));
    expect([notZip.kind, notZip.code]).toEqual(['invalid', 'request.invalid']);
    await mock('fault', { match: 'versions$', status: 413 });
    expect((await caught(session.saveVersion(xlsx(), etag))).kind).toBe('tooLarge');
  });

  it('works out SHA-256 as Workspace records it', async () => {
    expect(await sha256Hex(new Blob(['abc']))).toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    );
  });
});

describe('Copies', () => {
  it('a copy is a new workbook beside the original, and the session moves to it', async () => {
    const session = await open();
    const copy = await session.saveCopy(xlsx());
    expect(copy.workbook.name).toBe('Budget 2027 (copy)');
    expect(copy.access).toBe('edit');
    expect(session.workbook).toEqual(copy.workbook);
    const read = await session.read();
    expect(read.etag).toBe(copy.etag);
    const saved = await session.saveVersion(xlsx(), copy.etag);
    const { workbooks } = await state();
    expect(workbooks.find((w) => w.id === copy.workbook.id)).toMatchObject({
      versions: 2,
      etag: saved.etag,
    });
    expect(workbooks.find((w) => w.id === WORKBOOKS.budget)!.versions).toBe(1);
  });

  it('a viewer’s copy is theirs to edit, named in their language', async () => {
    const viewer = await open('viewer');
    expect((await viewer.saveCopy(xlsx())).access).toBe('edit');
    const arabic = await open('arabic');
    expect(arabic.language).toBe('ar');
    expect((await arabic.saveCopy(xlsx())).workbook.name).toBe('Budget 2027 (نسخة)');
  });

  it('a refused copy is denied, and a copy is never sent twice', async () => {
    const session = await open();
    await mock('copies', { allowed: false });
    expect((await caught(session.saveCopy(xlsx()))).kind).toBe('denied');
    await mock('copies', { allowed: true });
    await mock('fault', { match: 'copies$', drop: 'after' });
    expect((await caught(session.saveCopy(xlsx()))).kind).toBe('network');
    expect((await state()).counts.copies).toBe(1);
  });
});

describe('HTTP', () => {
  it('maps statuses and codes to what the sheet says', () => {
    expect(kindOf(401, 'ticket.invalid')).toBe('ticket');
    expect(kindOf(401, 'session.expired')).toBe('session');
    expect(kindOf(401, null)).toBe('session');
    expect(kindOf(403, 'origin.denied')).toBe('origin');
    expect(kindOf(403, 'access.denied')).toBe('denied');
    expect(kindOf(410, 'workbook.gone')).toBe('gone');
    expect(kindOf(412, 'version.conflict')).toBe('conflict');
    expect(kindOf(413, 'limit.size')).toBe('tooLarge');
    expect(kindOf(428, 'version.required')).toBe('invalid');
    expect(kindOf(503, null)).toBe('busy');
    expect(kindOf(500, 'server.error')).toBe('server');
    expect(retryAfterMs('3')).toBe(3000);
    expect(retryAfterMs(null)).toBeNull();
  });

  it('refuses redirects and sends no cookies or referrer, to the API and to signed URLs', async () => {
    const asked: RequestInit[] = [];
    const recording = new WorkspaceHttp({
      baseUrl: 'http://127.0.0.1:1',
      fetch: async (_input, init) => {
        asked.push(init!);
        throw new TypeError('refused');
      },
      retries: 0,
      sleep: async () => {},
    });
    const token = 'secret-token-value';
    const error = await caught(recording.request('GET', 'x', { session: token, idempotent: true }));
    expect(error.kind).toBe('network');
    expect(error.message).not.toContain(token);
    expect((await caught(recording.readSigned('http://127.0.0.1:1/blob'))).kind).toBe('network');
    for (const init of asked) {
      expect(init).toMatchObject({
        redirect: 'error',
        credentials: 'omit',
        referrerPolicy: 'no-referrer',
      });
    }
    expect(
      (asked[1]!.headers as Record<string, string> | undefined)?.['X-Workspace-File-Session'],
    ).toBeUndefined();
  });
});

/** Tabs of one origin, as `BroadcastChannel` joins them, here in one process. */
function channels(): ChannelFactory {
  const open = new Set<{ listeners: Set<(event: { data: unknown }) => void> }>();
  return () => {
    const me = { listeners: new Set<(event: { data: unknown }) => void>() };
    open.add(me);
    const channel: Channel = {
      postMessage(message) {
        const data = structuredClone(message);
        for (const other of open) {
          if (other === me) continue;
          for (const listener of other.listeners) queueMicrotask(() => listener({ data }));
        }
      },
      addEventListener: (_type, listener) => void me.listeners.add(listener),
      removeEventListener: (_type, listener) => void me.listeners.delete(listener),
      close: () => void open.delete(me),
    };
    return channel;
  };
}

describe('Reconnect’s handover', () => {
  it('hands a new session to a tab waiting for the same workbook, user and API, which uses it', async () => {
    const channel = channels();
    const first = await open();
    first.giveAway();
    const taken: FileSession[] = [];
    const stopWaiting = waitForHandover({
      scope: { apiBase: base, workbookId: first.workbook.id, userId: first.user.id },
      channel,
      take: async (session) => {
        taken.push(session);
        return true;
      },
    });
    const second = await open();
    expect(await offerHandover(second.handover(), base, { channel })).toBe('taken');
    stopWaiting();
    const adopted = WorkspaceSession.adopt(http(), taken[0]);
    const { etag } = await adopted.read();
    await expect(adopted.saveVersion(xlsx(), etag)).resolves.toBeDefined();
  });

  it('never hands over to a tab waiting for another workbook, user or API', async () => {
    const channel = channels();
    const editor = await open();
    const scopes = [
      { apiBase: base, workbookId: WORKBOOKS.empty, userId: USERS.editor.id },
      { apiBase: base, workbookId: WORKBOOKS.budget, userId: USERS.other.id },
      {
        apiBase: 'https://elsewhere.example',
        workbookId: WORKBOOKS.budget,
        userId: USERS.editor.id,
      },
    ];
    const stops = scopes.map((scope) =>
      waitForHandover({ scope, channel, take: async () => true }),
    );
    expect(await offerHandover(editor.handover(), base, { channel, askMs: 50 })).toBe('none');
    for (const stopIt of stops) stopIt();
  });

  it('a waiting tab that can’t use the session refuses it, and the new tab opens its own', async () => {
    const channel = channels();
    const session = await open();
    const stopWaiting = waitForHandover({
      scope: { apiBase: base, workbookId: WORKBOOKS.budget, userId: USERS.editor.id },
      channel,
      take: async () => false,
    });
    expect(await offerHandover(session.handover(), base, { channel })).toBe('refused');
    stopWaiting();
  });
});
