// The sheet session: Redeem's answer, with its token held here and nowhere else. The token is a
// private field, so it can't be read off the object, serialised with it or logged by accident; it
// leaves only as the `X-Sheet-Session` header (plan F), and once, in memory, when Reconnect hands
// the session to the tab that waits for it (`handover`). Renew slides the idle expiry while the
// user works (decisions, 2026-11-11, Phase 18); End ends it.

import { ENTITY_ROUTES, ROUTES, type QueryFilter, type SheetSession } from './contract.ts';
import { ErpError, type ErpHttp, type RequestOptions } from './http.ts';
import { readRenewResponse, readSheetSession } from './validate.ts';

/** What a session may show of itself: everything Redeem answered but the token. */
export type SessionInfo = Omit<SheetSession, 'token'>;

export type SessionState = 'open' | 'expired';

/** Renew at most this often, however busy the user is (decisions, 2026-11-11). */
export const RENEW_EVERY_MS = 5 * 60_000;

export class ErpSession {
  readonly info: SessionInfo;
  readonly #http: ErpHttp;
  readonly #token: string;
  #idleExpiresAt: number;
  readonly #maxExpiresAt: number;
  #ended = false;
  readonly #now: () => number;
  readonly #listeners = new Set<(state: SessionState) => void>();
  #renewedAt: number;
  #renewing: Promise<void> | null = null;

  private constructor(http: ErpHttp, session: SheetSession, now: () => number) {
    const { token, ...info } = session;
    this.info = Object.freeze(info);
    this.#http = http;
    this.#token = token;
    this.#idleExpiresAt = Date.parse(session.idleExpiresAt);
    this.#maxExpiresAt = Date.parse(session.maxExpiresAt);
    this.#now = now;
    this.#renewedAt = now();
  }

  /** Redeems a launch ticket, once. Redeem is never sent twice, since a lost answer may have
   * used the ticket up. */
  static async redeem(
    http: ErpHttp,
    ticket: string,
    options: { signal?: AbortSignal; now?: () => number } = {},
  ): Promise<ErpSession> {
    const answer = await http.json('POST', ROUTES.redeem, {
      body: { ticket },
      signal: options.signal,
      idempotent: false,
    });
    let session: SheetSession;
    try {
      session = readSheetSession(answer);
    } catch {
      throw new ErpError('contract', 'Cash Gear ERP sent a session the sheet can’t read.');
    }
    return new ErpSession(http, session, options.now ?? Date.now);
  }

  /**
   * Takes over a session another tab redeemed and handed over (Reconnect, plan B.4). The answer
   * is checked as Redeem's is.
   */
  static adopt(http: ErpHttp, given: unknown, now: () => number = Date.now): ErpSession {
    let session: SheetSession;
    try {
      session = readSheetSession(given);
    } catch {
      throw new ErpError('contract', 'The session handed over can’t be read.');
    }
    return new ErpSession(http, session, now);
  }

  /**
   * Redeem's answer with its token, for Reconnect's handover to another tab of this origin over
   * `BroadcastChannel`, in memory only. Nothing else may call it.
   */
  handover(): SheetSession {
    return {
      token: this.#token,
      ...this.info,
      idleExpiresAt: new Date(this.#idleExpiresAt).toISOString(),
    };
  }

  get company(): string {
    return this.info.company;
  }

  get filter(): QueryFilter | null {
    return this.info.filter;
  }

  /** When the session ends if nothing renews it, in milliseconds since 1970. */
  get expiresAt(): number {
    return Math.min(this.#idleExpiresAt, this.#maxExpiresAt);
  }

  get state(): SessionState {
    return this.#ended || this.#now() >= this.expiresAt ? 'expired' : 'open';
  }

  /** Hears the session end, by expiring or by the ERP answering 401. */
  onState(listener: (state: SessionState) => void): () => void {
    this.#listeners.add(listener);
    this.#watch();
    return () => {
      this.#listeners.delete(listener);
      if (this.#listeners.size === 0) this.#unwatch();
    };
  }

  #timer: ReturnType<typeof setTimeout> | undefined;

  /** Ends the session when its time runs out, so the badge says so without a request. */
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
      const answer = await this.json('POST', ROUTES.renew, { idempotent: true });
      let idle: number;
      try {
        idle = Date.parse(readRenewResponse(answer).idleExpiresAt);
      } catch {
        throw new ErpError('contract', 'Cash Gear ERP sent a renewal the sheet can’t read.');
      }
      this.#idleExpiresAt = Math.min(idle, this.#maxExpiresAt);
      // The timer goes again from the new expiry.
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
   * Ends the session in the ERP and here. Sent with `keepalive`, so it goes even as the page
   * closes; its answer doesn't matter, since an unended session expires anyway.
   */
  end(): void {
    if (this.#ended) return;
    void this.#http
      .json('POST', ROUTES.end, { session: this.#token, keepalive: true, idempotent: false })
      .catch(() => undefined);
    this.#end();
  }

  /** An entity route, such as `api/payable/VendorSheet/Query`. */
  route(name: (typeof ENTITY_ROUTES)[keyof typeof ENTITY_ROUTES]): string {
    return `${this.info.entityRoute}/${name}`;
  }

  /** Sends a request in this session. A 401 ends it here too. */
  async json(method: 'GET' | 'POST', route: string, options: RequestOptions = {}) {
    return this.#guard(() => this.#http.json(method, route, { ...options, session: this.#token }));
  }

  /** As `json`, giving the answer's bytes unread. */
  async bytes(method: 'GET' | 'POST', route: string, options: RequestOptions = {}) {
    return this.#guard(() => this.#http.bytes(method, route, { ...options, session: this.#token }));
  }

  async #guard<T>(send: () => Promise<T>): Promise<T> {
    if (this.state === 'expired') {
      throw new ErpError('session', 'Your Cash Gear ERP session has ended.');
    }
    try {
      return await send();
    } catch (error) {
      if (error instanceof ErpError && error.kind === 'session') this.#end();
      throw error;
    }
  }

  #end() {
    if (this.#ended) return;
    this.#ended = true;
    this.#unwatch();
    for (const listener of this.#listeners) listener('expired');
  }

  /** The session as JSON for the engine's binding: no token. */
  toJSON(): SessionInfo {
    return this.info;
  }
}
