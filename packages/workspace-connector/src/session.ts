// The file session: Redeem's answer, with its token held here and nowhere else. The token is a
// private field, so it can't be read off the object, serialised with it or logged by accident; it
// leaves only as the `X-Workspace-File-Session` header, and once, in memory, when Reconnect hands
// the session to the tab that waits for it (`handover`). Renew slides the idle expiry while the
// user works; End ends it. The session reads its one workbook by signed URL and saves versions of
// it with `If-Match`, or a copy beside it, which the session then moves to.

import {
  ROUTES,
  STATUS,
  XLSX_TYPE,
  type Access,
  type FileSession,
  type Language,
  type UserRef,
  type WorkbookRef,
} from './contract.ts';
import { WorkspaceError, type WorkspaceFailure, type WorkspaceHttp } from './http.ts';
import {
  readCopyResponse,
  readFileSession,
  readRenewResponse,
  readVersionResponse,
  readWorkbookResponse,
} from './validate.ts';

export type SessionState = 'open' | 'expired';

/** Renew at most this often, however busy the user is, as ERP data's sessions are. */
export const RENEW_EVERY_MS = 5 * 60_000;

/** The workbook's current version, read. `blob` and `etag` are null for one with no version. */
export interface ReadWorkbook {
  readonly name: string;
  readonly etag: string | null;
  readonly blob: Blob | null;
}

export interface SavedVersion {
  readonly versionId: string;
  readonly etag: string;
}

export interface SavedCopy extends SavedVersion {
  readonly workbook: WorkbookRef;
  readonly access: Access;
}

/** Lower-case hex SHA-256 of a blob's bytes, as Workspace's versions record theirs. */
export async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await globalThis.crypto.subtle.digest('SHA-256', await blob.arrayBuffer());
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

export class WorkspaceSession {
  readonly #http: WorkspaceHttp;
  readonly #token: string;
  readonly #user: UserRef;
  readonly #language: Language;
  readonly #maxExpiresAt: number;
  readonly #now: () => number;
  readonly #listeners = new Set<(state: SessionState) => void>();
  #workbook: WorkbookRef;
  #access: Access;
  #idleExpiresAt: number;
  #ended = false;
  #renewedAt: number;
  #renewing: Promise<void> | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;

  private constructor(http: WorkspaceHttp, session: FileSession, now: () => number) {
    this.#http = http;
    this.#token = session.token;
    this.#user = Object.freeze({ ...session.user });
    this.#language = session.language;
    this.#workbook = Object.freeze({ ...session.workbook });
    this.#access = session.access;
    this.#idleExpiresAt = Date.parse(session.expiresAt);
    this.#maxExpiresAt = Date.parse(session.maxExpiresAt);
    this.#now = now;
    this.#renewedAt = now();
  }

  /**
   * Redeems a launch ticket, once. Redeem is never sent twice, since a lost answer may have used
   * the ticket up.
   */
  static async redeem(
    http: WorkspaceHttp,
    ticket: string,
    options: { signal?: AbortSignal; now?: () => number } = {},
  ): Promise<WorkspaceSession> {
    const answer = await http.request('POST', ROUTES.redeem, {
      json: { ticket },
      signal: options.signal,
      idempotent: false,
    });
    let session: FileSession;
    try {
      session = readFileSession(answer.json);
    } catch {
      throw new WorkspaceError(
        'contract',
        'Cash Gear Workspace sent a session the sheet can’t read.',
      );
    }
    return new WorkspaceSession(http, session, options.now ?? Date.now);
  }

  /** Takes over a session another tab redeemed and handed over (Reconnect), checked as Redeem's. */
  static adopt(
    http: WorkspaceHttp,
    given: unknown,
    now: () => number = Date.now,
  ): WorkspaceSession {
    let session: FileSession;
    try {
      session = readFileSession(given);
    } catch {
      throw new WorkspaceError('contract', 'The session handed over can’t be read.');
    }
    return new WorkspaceSession(http, session, now);
  }

  /**
   * Redeem's answer with its token, for Reconnect's handover to another tab of this origin over
   * `BroadcastChannel`, in memory only. Nothing else may call it.
   */
  handover(): FileSession {
    return {
      token: this.#token,
      expiresAt: new Date(this.#idleExpiresAt).toISOString(),
      maxExpiresAt: new Date(this.#maxExpiresAt).toISOString(),
      workbook: this.#workbook,
      access: this.#access,
      user: this.#user,
      language: this.#language,
    };
  }

  /** Workspace API's origin this session belongs to. */
  get apiBase(): string {
    return this.#http.baseUrl;
  }

  get workbook(): WorkbookRef {
    return this.#workbook;
  }

  get access(): Access {
    return this.#access;
  }

  get user(): UserRef {
    return this.#user;
  }

  get language(): Language {
    return this.#language;
  }

  /** The last request of this session's connection that failed, with its correlation id. */
  get lastFailure(): WorkspaceFailure | null {
    return this.#http.lastFailure;
  }

  /** When the session ends if nothing renews it, in milliseconds since 1970. */
  get expiresAt(): number {
    return Math.min(this.#idleExpiresAt, this.#maxExpiresAt);
  }

  get state(): SessionState {
    return this.#ended || this.#now() >= this.expiresAt ? 'expired' : 'open';
  }

  /** Hears the session end, by expiring, by Workspace answering 401, or by End. */
  onState(listener: (state: SessionState) => void): () => void {
    this.#listeners.add(listener);
    this.#watch();
    return () => {
      this.#listeners.delete(listener);
      if (this.#listeners.size === 0) this.#unwatch();
    };
  }

  /** Ends the session here when its time runs out, so the badge says so without a request. */
  #watch() {
    if (this.#timer !== undefined || this.#ended) return;
    const left = this.expiresAt - this.#now();
    if (left <= 0) {
      queueMicrotask(() => this.#end());
      return;
    }
    // Timers past 2^31 ms fire at once; a 10-hour session is far below that.
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      if (this.state === 'expired') this.#end();
      else this.#watch();
    }, left + 50);
    (this.#timer as { unref?: () => void }).unref?.();
  }

  #unwatch() {
    if (this.#timer !== undefined) clearTimeout(this.#timer);
    this.#timer = undefined;
  }

  /**
   * The user did something in the tab: renews the session if it last renewed `RENEW_EVERY_MS`
   * ago or more, so it stays open while they work, whether or not anything is sent.
   */
  touch(): void {
    if (this.state === 'expired' || this.#renewing) return;
    if (this.#now() - this.#renewedAt < RENEW_EVERY_MS) return;
    void this.renew().catch(() => undefined);
  }

  /** Slides the idle expiry, never past the maximum. A 401 ends the session here too. */
  async renew(): Promise<void> {
    if (this.#renewing) return this.#renewing;
    this.#renewedAt = this.#now();
    const renewing = (async () => {
      const answer = await this.#request('POST', ROUTES.renew, { idempotent: true });
      let idle: number;
      try {
        idle = Date.parse(readRenewResponse(answer.json).expiresAt);
      } catch {
        throw new WorkspaceError(
          'contract',
          'Cash Gear Workspace sent a renewal the sheet can’t read.',
        );
      }
      this.#idleExpiresAt = Math.min(idle, this.#maxExpiresAt);
      if (this.#listeners.size > 0) {
        this.#unwatch();
        this.#watch();
      }
    })();
    this.#renewing = renewing;
    try {
      await renewing;
    } finally {
      this.#renewing = null;
    }
  }

  /**
   * Ends the session in Workspace and here. Sent with `keepalive`, so it goes even as the page
   * closes; its answer doesn't matter, since an unended session expires anyway.
   */
  end(): void {
    if (this.#ended) return;
    void this.#http
      .request('POST', ROUTES.end, { session: this.#token, keepalive: true, idempotent: false })
      .catch(() => undefined);
    this.#end();
  }

  /** Forgets the session here without ending it in Workspace: it was handed to another tab. */
  giveAway(): void {
    this.#end();
  }

  /**
   * Reads the workbook's current version: a fresh signed URL from Workspace, then its bytes. A
   * URL refused (it expired before the read began, or its version was replaced) is asked for
   * again once.
   */
  async read(
    options: {
      signal?: AbortSignal;
      onProgress?: (received: number, total: number | null) => void;
    } = {},
  ): Promise<ReadWorkbook> {
    for (let attempt = 0; ; attempt++) {
      const answer = await this.#request('GET', ROUTES.workbook, {
        signal: options.signal,
        idempotent: true,
      });
      let found;
      try {
        found = readWorkbookResponse(answer.json);
      } catch {
        throw new WorkspaceError(
          'contract',
          'Cash Gear Workspace sent a workbook the sheet can’t read.',
        );
      }
      this.#workbook = Object.freeze({ id: this.#workbook.id, name: found.name });
      if (found.readUrl === null) return { name: found.name, etag: null, blob: null };
      const total = found.size;
      try {
        const blob = await this.#http.readSigned(found.readUrl, {
          signal: options.signal,
          onProgress: options.onProgress && ((received) => options.onProgress!(received, total)),
        });
        return { name: found.name, etag: found.etag, blob };
      } catch (error) {
        if (error instanceof WorkspaceError && error.kind === 'readUrl' && attempt === 0) continue;
        throw error;
      }
    }
  }

  /**
   * Saves `bytes` as the workbook's new version, if `etag` is still its current one (`null`: if
   * it has no version yet). A stale `etag` is a `conflict` carrying the current version, unless
   * that version holds these very bytes, which means an earlier attempt landed and its answer was
   * lost: that counts as saved. A 403 means the session may only view the workbook now.
   */
  async saveVersion(
    bytes: Blob,
    etag: string | null,
    options: { signal?: AbortSignal } = {},
  ): Promise<SavedVersion> {
    const precondition: Record<string, string> =
      etag === null ? { 'If-None-Match': '*' } : { 'If-Match': etag };
    try {
      const answer = await this.#request('POST', ROUTES.versions, {
        bytes,
        contentType: XLSX_TYPE,
        headers: precondition,
        signal: options.signal,
        idempotent: true,
        transfer: true,
      });
      try {
        return readVersionResponse(answer.json);
      } catch {
        throw new WorkspaceError(
          'contract',
          'Cash Gear Workspace sent a version the sheet can’t read.',
        );
      }
    } catch (error) {
      if (!(error instanceof WorkspaceError)) throw error;
      const current = error.problem?.current;
      if (error.kind === 'conflict' && current && current.sha256 === (await sha256Hex(bytes))) {
        return { versionId: current.versionId, etag: current.etag };
      }
      if (error.kind === 'denied') this.#access = 'view';
      throw error;
    }
  }

  /**
   * Saves `bytes` as a new workbook beside this one, named by Workspace ("<name> (copy)"), and
   * moves the session to it. Not sent again after no answer, since the copy may have been made.
   */
  async saveCopy(bytes: Blob, options: { signal?: AbortSignal } = {}): Promise<SavedCopy> {
    const answer = await this.#request('POST', ROUTES.copies, {
      bytes,
      contentType: XLSX_TYPE,
      signal: options.signal,
      idempotent: false,
      transfer: true,
    });
    let copy;
    try {
      copy = readCopyResponse(answer.json);
    } catch {
      throw new WorkspaceError('contract', 'Cash Gear Workspace sent a copy the sheet can’t read.');
    }
    this.#workbook = Object.freeze({ ...copy.workbook });
    this.#access = copy.access;
    return copy;
  }

  /** Sends a request in this session. A 401 ends it here too. */
  async #request(
    method: 'GET' | 'POST',
    route: string,
    options: Parameters<WorkspaceHttp['request']>[2] = {},
  ) {
    if (this.state === 'expired') {
      throw new WorkspaceError('session', 'Your Cash Gear Workspace session has ended.', {
        status: STATUS.unauthorized,
      });
    }
    try {
      return await this.#http.request(method, route, { ...options, session: this.#token });
    } catch (error) {
      if (error instanceof WorkspaceError && error.kind === 'session') this.#end();
      throw error;
    }
  }

  #end() {
    if (this.#ended) return;
    this.#ended = true;
    this.#unwatch();
    for (const listener of this.#listeners) listener('expired');
  }

  /** What the session may show of itself: no token. */
  toJSON() {
    return {
      workbook: this.#workbook,
      access: this.#access,
      user: this.#user,
      language: this.#language,
      expiresAt: new Date(this.expiresAt).toISOString(),
    };
  }
}
