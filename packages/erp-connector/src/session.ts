// The sheet session: Redeem's answer, with its token held here and nowhere else. The token is a
// private field, so it can't be read off the object, serialised with it or logged by accident; it
// leaves only as the `X-Sheet-Session` header (plan F). Idle expiry is tracked from the answer,
// since only Renew slides it and Renew comes with Phase 18.

import { ENTITY_ROUTES, ROUTES, type QueryFilter, type SheetSession } from './contract.ts';
import { ErpError, type ErpHttp, type RequestOptions } from './http.ts';
import { readSheetSession } from './validate.ts';

/** What a session may show of itself: everything Redeem answered but the token. */
export type SessionInfo = Omit<SheetSession, 'token'>;

export type SessionState = 'open' | 'expired';

export class ErpSession {
  readonly info: SessionInfo;
  readonly #http: ErpHttp;
  readonly #token: string;
  #idleExpiresAt: number;
  readonly #maxExpiresAt: number;
  #ended = false;
  readonly #now: () => number;
  readonly #listeners = new Set<(state: SessionState) => void>();

  private constructor(http: ErpHttp, session: SheetSession, now: () => number) {
    const { token, ...info } = session;
    this.info = Object.freeze(info);
    this.#http = http;
    this.#token = token;
    this.#idleExpiresAt = Date.parse(session.idleExpiresAt);
    this.#maxExpiresAt = Date.parse(session.maxExpiresAt);
    this.#now = now;
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
