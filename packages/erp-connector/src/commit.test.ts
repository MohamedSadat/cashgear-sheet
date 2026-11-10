// Validating and committing against the mock ERP (Phase 17): dry runs write nothing, a commit
// saves once, 400 and 409 come back as outcomes with every error and conflict, and a response lost
// after the write is answered from the ERP's store when the same request goes again.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { startMockErp } from '../../../tests/erp-mock/server.mjs';
import { commitRecords, newCommitId, payloadKey, sendCommit } from './commit.ts';
import type { CommitRequest, QueryResponse, RecordChange } from './contract.ts';
import { ErpError, ErpHttp } from './http.ts';
import { ErpSession } from './session.ts';
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

async function session(key = 'mock-kit-editor'): Promise<ErpSession> {
  const response = await fetch(`${base}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': key },
    body: JSON.stringify({ entity: 'vendors', filter: null, language: 'en' }),
  });
  const { ticket } = (await response.json()) as { ticket: string };
  return ErpSession.redeem(http(), ticket);
}

async function records(s: ErpSession, keys: string[]): Promise<QueryResponse['records']> {
  const page = readQueryResponse(
    await s.json('POST', s.route('Query'), {
      body: { filter: { search: null }, ids: null, afterId: 0, pageSize: 1000 },
      idempotent: true,
    }),
  );
  return keys.map((key) => {
    const found = page.records.find((r) => r.key === key);
    if (!found) throw new Error(`no ${key}`);
    return found;
  });
}

async function state() {
  const response = await fetch(`${base}/__mock/state?company=Kit`);
  return (await response.json()) as { writes: number; commits: number; audit: number };
}

async function vendor(key: string) {
  const response = await fetch(`${base}/__mock/vendor?company=Kit&key=${key}`);
  return (await response.json()) as Record<string, unknown>;
}

async function fault(body: Record<string, unknown>) {
  await fetch(`${base}/__mock/fault`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
}

function request(s: ErpSession, list: RecordChange[], dryRun = false): CommitRequest {
  return {
    commitId: newCommitId(),
    company: s.company,
    schemaVersion: 'vendors.1',
    dryRun,
    records: list,
  };
}

describe('commit', () => {
  it('makes lower-case v4 ids and leaves the sheet’s rows out', () => {
    const id = newCommitId();
    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(newCommitId()).not.toBe(id);
    expect(
      commitRecords([
        { id: 1, key: 'V-1', version: 'AAAAAAAAB9E=', row: 4, hidden: true, changes: {} },
      ]),
    ).toEqual([{ id: 1, key: 'V-1', version: 'AAAAAAAAB9E=', changes: {} }]);
    const a = { company: 'Kit', schemaVersion: 'vendors.1', dryRun: false };
    const one = { id: 1, key: 'V-1', version: 'x', changes: {} };
    const two = { id: 2, key: 'V-2', version: 'x', changes: {} };
    expect(payloadKey({ ...a, records: [one, two] })).toBe(
      payloadKey({ ...a, records: [two, one] }),
    );
    expect(payloadKey({ ...a, records: [one] })).not.toBe(payloadKey({ ...a, records: [two] }));
  });

  it('dry runs check and write nothing, then the commit saves once', async () => {
    const s = await session();
    const [v1, v2] = await records(s, ['V-1001', 'V-1002']);
    const list: RecordChange[] = [
      {
        id: v1!.id,
        key: v1!.key,
        version: v1!.version,
        changes: { mobile: { from: v1!.values.mobile ?? null, to: '01009876543' } },
      },
      {
        id: v2!.id,
        key: v2!.key,
        version: v2!.version,
        changes: { payMethod: { from: v2!.values.payMethod ?? null, to: 'NET60' } },
      },
    ];
    const before = await state();
    const dry = await sendCommit(s, request(s, list, true));
    expect(dry.kind).toBe('ok');
    if (dry.kind !== 'ok') return;
    expect(dry.response.dryRun).toBe(true);
    expect(dry.response.records.map((r) => r.version)).toEqual([v1!.version, v2!.version]);
    expect(await state()).toEqual(before);
    const done = await sendCommit(s, request(s, list));
    expect(done.kind).toBe('ok');
    if (done.kind !== 'ok') return;
    expect(done.response.committedAt).not.toBe(null);
    expect(done.response.records[0]!.values.mobile).toBe('01009876543');
    expect(done.response.records[0]!.version).not.toBe(v1!.version);
    expect((await state()).writes).toBe(before.writes + 2);
    expect((await vendor('V-1002')).payMethod).toBe('NET60');
  });

  it('answers 400 with every error and 409 with every conflict, and writes nothing', async () => {
    const s = await session();
    const [v1, v2, v3] = await records(s, ['V-1001', 'V-1002', 'V-1003']);
    const change = (r: typeof v1, field: string, to: string | null): RecordChange => ({
      id: r!.id,
      key: r!.key,
      version: r!.version,
      changes: { [field]: { from: r!.values[field] ?? null, to } },
    });
    const before = await state();
    const invalid = await sendCommit(
      s,
      request(s, [change(v1, 'payMethod', 'NOPE'), change(v2, 'vendorName', null)], true),
    );
    expect(invalid.kind).toBe('invalid');
    if (invalid.kind !== 'invalid') return;
    expect(invalid.errors.map((e) => [e.key, e.field, e.code])).toEqual([
      ['V-1001', 'payMethod', 'lookup.notFound'],
      ['V-1002', 'vendorName', 'required'],
    ]);
    // Another user changes V-1003 in the ERP.
    await fetch(`${base}/__mock/edit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        company: 'Kit',
        key: 'V-1003',
        values: { mobile: '0111' },
        by: 'omar',
      }),
    });
    const conflict = await sendCommit(
      s,
      request(s, [change(v1, 'mobile', '0100'), change(v3, 'mobile', '0122')]),
    );
    expect(conflict.kind).toBe('conflict');
    if (conflict.kind !== 'conflict') return;
    expect(conflict.conflicts.map((c) => [c.key, c.fields, c.modifiedBy])).toEqual([
      ['V-1003', ['mobile'], 'omar'],
    ]);
    expect((await state()).writes).toBe(before.writes + 1);
  });

  it('sends a commit again after a lost answer, and the ERP saves it once', async () => {
    const s = await session();
    const [v1] = await records(s, ['V-1004']);
    const list: RecordChange[] = [
      {
        id: v1!.id,
        key: v1!.key,
        version: v1!.version,
        changes: { address: { from: v1!.values.address ?? null, to: '12 Tahrir St' } },
      },
    ];
    const before = await state();
    await fault({ match: 'VendorSheet/Commit$', drop: 'after', times: 1 });
    const sent = request(s, list);
    const done = await sendCommit(s, sent);
    expect(done.kind).toBe('ok');
    if (done.kind !== 'ok') return;
    expect(done.response.commitId).toBe(sent.commitId);
    const after = await state();
    expect(after.writes).toBe(before.writes + 1);
    expect(after.commits).toBe(before.commits + 1);
    // Every answer lost, the first and the HTTP layer's three retries: the outcome is unknown,
    // and the same request later still saves once.
    await fault({ match: 'VendorSheet/Commit$', drop: 'after', times: 4 });
    const again = request(s, [
      {
        ...list[0]!,
        version: done.response.records[0]!.version,
        changes: { address: { from: '12 Tahrir St', to: '13 Tahrir St' } },
      },
    ]);
    const lost = (await sendCommit(s, again).catch((e: unknown) => e)) as ErpError;
    expect(lost).toBeInstanceOf(ErpError);
    expect(lost.kind).toBe('network');
    const retried = await sendCommit(s, again);
    expect(retried.kind).toBe('ok');
    expect((await state()).writes).toBe(after.writes + 1);
    expect((await vendor('V-1004')).address).toBe('13 Tahrir St');
  });

  it('refuses a viewer, and past the batch limit', async () => {
    const viewer = await session('mock-kit-viewer');
    const [v1] = await records(viewer, ['V-1001']);
    const one: RecordChange = {
      id: v1!.id,
      key: v1!.key,
      version: v1!.version,
      changes: { mobile: { from: null, to: '0100' } },
    };
    const refused = (await sendCommit(viewer, request(viewer, [one], true)).catch(
      (e: unknown) => e,
    )) as ErpError;
    expect(refused.kind).toBe('privilege');
    const editor = await session();
    const many = Array.from({ length: 501 }, (_, i) => ({ ...one, id: one.id + i + 1 }));
    const big = (await sendCommit(editor, request(editor, many, true)).catch(
      (e: unknown) => e,
    )) as ErpError;
    expect(big.kind).toBe('tooMany');
  });
});
