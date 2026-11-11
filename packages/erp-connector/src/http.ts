// HTTP to Cash Gear ERP's sheet routes: timeouts, cancellation, retries after 429 and 503, and
// every failure mapped to an `ErpError` the UI can word. The session token travels only in the
// `X-Sheet-Session` header; it never goes in a URL, an error or a log.

import { CORRELATION_HEADER, SESSION_HEADER, STATUS } from './contract.ts';
import { ContractError, readConflictBody, readErrorBody } from './validate.ts';

/** What went wrong, as the sheet tells the user. */
export type ErpErrorKind =
  /** The launch link was unknown, used or expired (`ticket.invalid`). */
  | 'ticket'
  /** The session ended or expired (401 `session.expired`). */
  | 'session'
  /** A privilege is missing (403 `privilege.denied`). */
  | 'privilege'
  /** This site isn't allowed to connect (403 `origin.denied`). */
  | 'origin'
  /** Too many records or ids (413). */
  | 'tooMany'
  /** The request was refused as it was (400). */
  | 'invalid'
  /** Records changed in the ERP since they were read (409). */
  | 'conflict'
  /** Still 429 or 503 after waiting. */
  | 'busy'
  /** The ERP failed (500 and others). */
  | 'server'
  /** No answer: offline, refused, or timed out. */
  | 'network'
  /** The answer broke the contract, or the records couldn't be read. */
  | 'contract'
  /** The user cancelled. */
  | 'cancelled';

export class ErpError extends Error {
  readonly kind: ErpErrorKind;
  readonly status: number | null;
  readonly code: string | null;
  readonly correlationId: string | null;
  /** The answer's JSON body, unchecked: a 400's errors or a 409's conflicts, for the caller. */
  readonly body: unknown;

  constructor(
    kind: ErpErrorKind,
    message: string,
    details: {
      status?: number;
      code?: string;
      correlationId?: string | null;
      body?: unknown;
    } = {},
  ) {
    super(message);
    this.name = 'ErpError';
    this.kind = kind;
    this.status = details.status ?? null;
    this.code = details.code ?? null;
    this.correlationId = details.correlationId ?? null;
    this.body = details.body;
  }
}

export interface HttpOptions {
  /** The ERP API's origin, without a trailing slash. */
  readonly baseUrl: string;
  readonly fetch?: typeof fetch;
  /** Per attempt. */
  readonly timeoutMs?: number;
  /** Attempts after the first, for answers that say to try again. */
  readonly retries?: number;
  /** The longest wait between attempts, whatever `Retry-After` asks. */
  readonly maxWaitMs?: number;
  readonly sleep?: (ms: number, signal?: AbortSignal) => Promise<void>;
}

export interface RequestOptions {
  readonly body?: unknown;
  /** The session token, sent as `X-Sheet-Session`. */
  readonly session?: string;
  readonly signal?: AbortSignal;
  /**
   * Whether a request can be sent again after no answer at all. Reads can; Redeem can't, since
   * the ERP may have used the ticket up before the answer was lost.
   */
  readonly idempotent?: boolean;
  /** Sent even as the page closes, as End is (`fetch`'s `keepalive`). */
  readonly keepalive?: boolean;
}

function wait(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((done, failed) => {
    if (signal?.aborted) {
      failed(cancelled());
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', stop);
      done();
    }, ms);
    const stop = () => {
      clearTimeout(timer);
      failed(cancelled());
    };
    signal?.addEventListener('abort', stop, { once: true });
  });
}

const cancelled = () => new ErpError('cancelled', 'Cancelled.');

/** The kind of error a status and code mean. */
export function kindOf(status: number, code: string | null): ErpErrorKind {
  if (status === STATUS.unauthorized) return code === 'ticket.invalid' ? 'ticket' : 'session';
  if (status === STATUS.forbidden) return code === 'origin.denied' ? 'origin' : 'privilege';
  if (status === STATUS.tooLarge) return 'tooMany';
  if (status === STATUS.conflict) return 'conflict';
  if (status === STATUS.tooManyRequests || status === STATUS.unavailable) return 'busy';
  if (status === STATUS.invalid) return 'invalid';
  return 'server';
}

/** Seconds or an HTTP date, as `Retry-After` gives them. */
export function retryAfterMs(header: string | null, now = Date.now()): number | null {
  if (header === null || header.trim() === '') return null;
  const seconds = Number(header);
  if (Number.isFinite(seconds) && seconds >= 0) return seconds * 1000;
  const at = Date.parse(header);
  return Number.isNaN(at) ? null : Math.max(0, at - now);
}

export class ErpHttp {
  readonly baseUrl: string;
  readonly #fetch: typeof fetch;
  readonly #timeoutMs: number;
  readonly #retries: number;
  readonly #maxWaitMs: number;
  readonly #sleep: (ms: number, signal?: AbortSignal) => Promise<void>;

  constructor(options: HttpOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, '');
    this.#fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.#timeoutMs = options.timeoutMs ?? 30_000;
    this.#retries = options.retries ?? 3;
    this.#maxWaitMs = options.maxWaitMs ?? 10_000;
    this.#sleep = options.sleep ?? wait;
  }

  /** Sends a request and reads its JSON answer, or `null` for an empty one (End's 204). */
  async json(
    method: 'GET' | 'POST',
    route: string,
    options: RequestOptions = {},
  ): Promise<unknown> {
    const bytes = await this.bytes(method, route, options);
    if (bytes.byteLength === 0) return null;
    try {
      return JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      throw new ErpError('contract', 'Cash Gear ERP sent an answer the sheet can’t read.');
    }
  }

  /** Sends a request and gives its answer's bytes, unread, for the engine to parse. */
  async bytes(
    method: 'GET' | 'POST',
    route: string,
    options: RequestOptions = {},
  ): Promise<Uint8Array> {
    const idempotent = options.idempotent ?? method === 'GET';
    for (let attempt = 0; ; attempt++) {
      if (options.signal?.aborted) throw cancelled();
      let response: Response;
      try {
        response = await this.#send(method, route, options);
      } catch (error) {
        if (error instanceof ErpError && error.kind === 'cancelled') throw error;
        if (idempotent && attempt < this.#retries) {
          await this.#sleep(this.#backoff(attempt), options.signal);
          continue;
        }
        throw error instanceof ErpError
          ? error
          : new ErpError('network', 'Cash Gear ERP can’t be reached.');
      }
      if (response.ok) return new Uint8Array(await response.arrayBuffer());
      const retryable =
        response.status === STATUS.tooManyRequests || response.status === STATUS.unavailable;
      if (retryable && attempt < this.#retries) {
        const asked = retryAfterMs(response.headers.get('Retry-After'));
        await response.arrayBuffer().catch(() => undefined);
        await this.#sleep(
          Math.min(asked ?? this.#backoff(attempt), this.#maxWaitMs),
          options.signal,
        );
        continue;
      }
      throw await this.#failure(response);
    }
  }

  #backoff(attempt: number): number {
    return Math.min(500 * 2 ** attempt, this.#maxWaitMs);
  }

  async #send(method: string, route: string, options: RequestOptions): Promise<Response> {
    const headers: Record<string, string> = {};
    if (options.body !== undefined) headers['Content-Type'] = 'application/json';
    if (options.session !== undefined) headers[SESSION_HEADER] = options.session;
    const timeout = new AbortController();
    const timer = setTimeout(() => timeout.abort(), this.#timeoutMs);
    const stop = () => timeout.abort();
    options.signal?.addEventListener('abort', stop, { once: true });
    try {
      return await this.#fetch(`${this.baseUrl}/${route}`, {
        method,
        headers,
        body: options.body === undefined ? undefined : JSON.stringify(options.body),
        signal: timeout.signal,
        credentials: 'omit',
        cache: 'no-store',
        mode: 'cors',
        ...(options.keepalive ? { keepalive: true } : {}),
      });
    } catch {
      if (options.signal?.aborted) throw cancelled();
      if (timeout.signal.aborted) {
        throw new ErpError('network', 'Cash Gear ERP took too long to answer.');
      }
      throw new ErpError('network', 'Cash Gear ERP can’t be reached.');
    } finally {
      clearTimeout(timer);
      options.signal?.removeEventListener('abort', stop);
    }
  }

  async #failure(response: Response): Promise<ErpError> {
    const correlationId = response.headers.get(CORRELATION_HEADER);
    let message = `Cash Gear ERP answered ${response.status}.`;
    let code: string | null = null;
    let body: unknown;
    try {
      const text = await response.text();
      if (text !== '') {
        body = JSON.parse(text);
        if (response.status === STATUS.conflict) {
          message = readConflictBody(body).message || message;
        } else {
          const errors = readErrorBody(body);
          message = errors.message || message;
          code = errors.errors[0]?.code ?? null;
        }
      }
    } catch (error) {
      // An answer without the ERP's error body: the status alone says what it means.
      if (!(error instanceof ContractError) && !(error instanceof SyntaxError)) throw error;
    }
    return new ErpError(kindOf(response.status, code), message, {
      status: response.status,
      code: code ?? undefined,
      correlationId,
      body,
    });
  }
}
