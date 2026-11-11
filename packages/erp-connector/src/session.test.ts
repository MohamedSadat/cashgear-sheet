// Sessions over their life (Phase 18) against the mock ERP: Renew slides the idle expiry after
// activity and at most every 5 minutes, End ends the session in the ERP, a session handed over to
// another tab works there, CommitStatus finds a saved commit and nothing else, and Reconnect's
// channel hands a session only to a tab waiting for the same company, user and entity.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { startMockErp } from '../../../tests/erp-mock/server.mjs';
import { commitStatus, newCommitId, sendCommit } from './commit.ts';
import type { SheetSession } from './contract.ts';
import { ErpError, ErpHttp } from './http.ts';
import {
  offerHandover,
  sameScope,
  waitForHandover,
  type Channel,
  type ChannelFactory,
} from './reconnect.ts';
import { ErpSession, RENEW_EVERY_MS } from './session.ts';
import { readQueryResponse } from './validate.ts';

let base = '';
let stop: (() => Promise<void>) | undefined;
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

async function ticket(key = 'mock-kit-editor'): Promise<string> {
  const response = await fetch(`${base}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': key },
    body: JSON.stringify({ entity: 'vendors', filter: null, language: 'en' }),
  });
  return ((await response.json()) as { ticket: string }).ticket;
}

async function clock(advanceMs: number) {
  const response = await fetch(`${base}/__mock/clock`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ advanceMs }),
  });
  return ((await response.json()) as { now: string }).now;
}

async function querying(s: ErpSession) {
  return s.json('POST', s.route('Query'), {
    body: { filter: null, ids: null, afterId: 0, pageSize: 1 },
    idempotent: true,
  });
}

describe('a session', () => {
  it('renews after activity, at most every 5 minutes, and slides its expiry', async () => {
    let now = Date.now();
    const s = await ErpSession.redeem(http(), await ticket(), { now: () => now });
    const first = s.expiresAt;
    // Activity soon after Redeem sends nothing.
    s.touch();
    expect(s.expiresAt).toBe(first);
    now += RENEW_EVERY_MS;
    await clock(RENEW_EVERY_MS);
    await s.renew();
    expect(s.expiresAt).toBeGreaterThan(first);
    expect(s.state).toBe('open');
  });

  it('ends in the ERP on End, and every later request is refused', async () => {
    const s = await ErpSession.redeem(http(), await ticket());
    const handed = s.handover();
    s.end();
    expect(s.state).toBe('expired');
    // End is sent as the page might close; give it a moment to land.
    let refused: ErpError | null = null;
    for (let i = 0; i < 20 && !refused; i++) {
      await new Promise((resolve) => setTimeout(resolve, 25));
      const other = ErpSession.adopt(http(), handed);
      refused = await querying(other).then(
        () => null,
        (error: unknown) => (error instanceof ErpError ? error : null),
      );
    }
    expect(refused?.kind).toBe('session');
  });

  it('ends here when the ERP answers 401, and never sends again', async () => {
    const s = await ErpSession.redeem(http(), await ticket());
    const heard: string[] = [];
    s.onState((state) => heard.push(state));
    await clock(31 * 60_000);
    await expect(querying(s)).rejects.toMatchObject({ kind: 'session' });
    expect(s.state).toBe('expired');
    expect(heard).toEqual(['expired']);
    await expect(s.renew()).rejects.toMatchObject({ kind: 'session' });
  });

  it('handed over to another tab works there, and is the same session', async () => {
    const s = await ErpSession.redeem(http(), await ticket());
    const handed = s.handover();
    expect(handed.token).toMatch(/\S{20,}/);
    expect(JSON.stringify(s)).not.toContain(handed.token);
    const there = ErpSession.adopt(http(), handed);
    expect(there.info.company).toBe('Kit');
    const page = readQueryResponse(await querying(there));
    expect(page.records).toHaveLength(1);
    expect(() => ErpSession.adopt(http(), { ...handed, token: 1 })).toThrow(ErpError);
  });
});

describe('CommitStatus', () => {
  it('finds a commit that was saved, and nothing for one that never was', async () => {
    const s = await ErpSession.redeem(http(), await ticket());
    const page = readQueryResponse(
      await s.json('POST', s.route('Query'), {
        body: { filter: null, ids: null, afterId: 0, pageSize: 3 },
        idempotent: true,
      }),
    );
    const record = page.records[1]!;
    const commitId = newCommitId();
    const outcome = await sendCommit(s, {
      commitId,
      company: 'Kit',
      schemaVersion: 'vendors.1',
      dryRun: false,
      records: [
        {
          id: record.id,
          key: record.key,
          version: record.version,
          changes: { mobile: { from: record.values.mobile ?? null, to: '01234' } },
        },
      ],
    });
    expect(outcome.kind).toBe('ok');
    const found = await commitStatus(s, commitId);
    expect(found.status).toBe('committed');
    if (found.status === 'committed' && outcome.kind === 'ok') {
      expect(found.records).toEqual(outcome.response.records);
    }
    expect(await commitStatus(s, newCommitId())).toEqual({ status: 'unknown' });
    await expect(commitStatus(s, 'not-a-uuid')).rejects.toMatchObject({ kind: 'invalid' });
    // Another company's session never sees it.
    const dot = await ErpSession.redeem(http(), await ticket('mock-dot-editor'));
    expect(await commitStatus(dot, commitId)).toEqual({ status: 'unknown' });
  });
});

/** Channels that reach each other as `BroadcastChannel`s of one origin do, but never themselves. */
function bus(): ChannelFactory {
  const open = new Set<Channel & { deliver(data: unknown): void }>();
  return () => {
    const listeners = new Set<(event: { data: unknown }) => void>();
    const channel = {
      postMessage(message: unknown) {
        const copy = structuredClone(message);
        for (const other of open) if (other !== channel) queueMicrotask(() => other.deliver(copy));
      },
      addEventListener(_: 'message', listener: (event: { data: unknown }) => void) {
        listeners.add(listener);
      },
      removeEventListener(_: 'message', listener: (event: { data: unknown }) => void) {
        listeners.delete(listener);
      },
      close() {
        open.delete(channel);
      },
      deliver(data: unknown) {
        for (const listener of listeners) listener({ data });
      },
    };
    open.add(channel);
    return channel;
  };
}

const sheetSession = (company: string, userId = 'u17'): SheetSession => ({
  token: 't'.repeat(43),
  idleExpiresAt: '2026-11-11T10:30:00Z',
  maxExpiresAt: '2026-11-11T20:00:00Z',
  company,
  userId,
  userName: 'Sara',
  language: 'en',
  entity: 'vendors',
  entityRoute: 'api/payable/VendorSheet',
  filter: null,
});

describe('Reconnect', () => {
  const scope = { company: 'Kit', userId: 'u17', entity: 'vendors' };

  it('hands a session to the tab waiting for its company, user and entity', async () => {
    const channel = bus();
    const taken: SheetSession[] = [];
    const stopWaiting = waitForHandover({
      scope,
      channel,
      take: async (session) => {
        taken.push(session);
        return true;
      },
    });
    expect(await offerHandover(sheetSession('kit'), { channel })).toBe('taken');
    expect(taken).toHaveLength(1);
    expect(taken[0]?.token).toBe('t'.repeat(43));
    stopWaiting();
    expect(await offerHandover(sheetSession('Kit'), { channel, askMs: 50 })).toBe('none');
  });

  it('never offers a launch for another company or user to a waiting tab', async () => {
    const channel = bus();
    let asked = 0;
    const stopWaiting = waitForHandover({
      scope,
      channel,
      take: async () => {
        asked++;
        return true;
      },
    });
    expect(await offerHandover(sheetSession('Dot'), { channel, askMs: 50 })).toBe('none');
    expect(await offerHandover(sheetSession('Kit', 'u18'), { channel, askMs: 50 })).toBe('none');
    expect(asked).toBe(0);
    stopWaiting();
  });

  it('refuses a session handed straight to it for another company', async () => {
    const channel = bus();
    const refusals: string[] = [];
    let asked = 0;
    const stopWaiting = waitForHandover({
      scope,
      channel,
      take: async () => {
        asked++;
        return true;
      },
      onRefused: (why) => refusals.push(why),
    });
    // A tab that answers "waiting?" for Kit, then is handed Dot's session by a forged message.
    const forger = channel();
    const replies: unknown[] = [];
    forger.addEventListener('message', (event) => replies.push(event.data));
    forger.postMessage({ type: 'waiting?', ask: 'a1', scope });
    await new Promise((resolve) => setTimeout(resolve, 5));
    const waiting = replies.find((r) => (r as { type: string }).type === 'waiting') as {
      tab: string;
    };
    forger.postMessage({
      type: 'handover',
      ask: 'a1',
      tab: waiting.tab,
      session: sheetSession('Dot'),
    });
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(asked).toBe(0);
    expect(refusals).toEqual(['scope']);
    expect(replies.some((r) => (r as { type: string }).type === 'refused')).toBe(true);
    stopWaiting();
    forger.close();
  });

  it('compares companies and entities without case, and users exactly', () => {
    expect(sameScope(scope, { company: 'KIT', userId: 'u17', entity: 'Vendors' })).toBe(true);
    expect(sameScope(scope, { company: 'Kit', userId: 'U17', entity: 'vendors' })).toBe(false);
  });
});
