// HTTP to Cash Gear Workspace's sheet-session routes: timeouts, cancellation, retries after 429
// and 503, and every failure mapped to a `WorkspaceError` the shell can word. The file session's
// token travels only in the `X-Workspace-File-Session` header; it never goes in a URL, an error or
// a log. Signed read URLs are fetched here too, with no credentials and no redirects.

import { CODES, CORRELATION_HEADER, SESSION_HEADER, STATUS, type Problem } from './contract.ts';
import { ContractError, readProblem } from './validate.ts';

/** What went wrong, as the sheet tells the user. */
export type WorkspaceErrorKind =
  /** The launch link was unknown, used or expired (401 `ticket.invalid`). */
  | 'ticket'
  /** The file session ended or expired (401 `session.expired`). */
  | 'session'
  /** This site isn't allowed to connect (403 `origin.denied`). */
  | 'origin'
  /** The user may not do this to the workbook, or has lost access to it (403). */
  | 'denied'
  /** The workbook was trashed or deleted (410 `workbook.gone`, or 404). */
  | 'gone'
  /** Someone saved a newer version (412 `version.conflict`). */
  | 'conflict'
  /** The workbook is larger than Workspace takes (413 `limit.size`). */
  | 'tooLarge'
  /** The request was refused as it was (400, 428). */
  | 'invalid'
  /** Still 429 or 503 after waiting. */
  | 'busy'
  /** Workspace failed (500 and others). */
  | 'server'
  /** A signed read URL was refused: expired, or its blob is gone. */
  | 'readUrl'
  /** No answer: offline, refused, or timed out. */
  | 'network'
  /** The answer broke the contract. */
  | 'contract'
  /** The user cancelled. */
  | 'cancelled';

export class WorkspaceError extends Error {
  readonly kind: WorkspaceErrorKind;
  readonly status: number | null;
  readonly code: string | null;
  readonly correlationId: string | null;
  /** The problem body, when the answer had one. */
  readonly problem: Problem | null;

  constructor(
    kind: WorkspaceErrorKind,
    message: string,
    details: {
      status?: number;
      code?: string | null;
      correlationId?: string | null;
      problem?: Problem | null;
    } = {},
  ) {
    super(message);
    this.name = 'WorkspaceError';
    this.kind = kind;
    this.status = details.status ?? null;
    this.code = details.code ?? null;
    this.correlationId = details.correlationId ?? null;
    this.problem = details.problem ?? null;
  }
}

export const isWorkspaceError = (error: unknown): error is WorkspaceError =>
  error instanceof WorkspaceError;

/** The last request that failed: its status (null for no answer), when, and its correlation id. */
export interface WorkspaceFailure {
  readonly status: number | null;
  readonly at: number;
  readonly correlationId: string | null;
}

export interface HttpOptions {
  /** Workspace API's origin, without a trailing slash. */
  readonly baseUrl: string;
  readonly fetch?: typeof fetch;
  /** Per attempt. Saves and reads of large workbooks get longer (`transferTimeoutMs`). */
  readonly timeoutMs?: number;
  /** Per attempt, for requests that carry or fetch a workbook's bytes. */
  readonly transferTimeoutMs?: number;
  /** Attempts after the first, for answers that say to try again, and for no answer. */
  readonly retries?: number;
  /** The longest wait between attempts, whatever `Retry-After` asks. */
  readonly maxWaitMs?: number;
  readonly sleep?: (ms: number, signal?: AbortSignal) => Promise<void>;
}

export interface RequestOptions {
  /** Sent as JSON. */
  readonly json?: unknown;
  /** Sent as they are, with `contentType`: a workbook's bytes. */
  readonly bytes?: Blob;
  readonly contentType?: string;
  /** The file session's token, sent as `X-Workspace-File-Session`. */
  readonly session?: string;
  /** `If-Match` or `If-None-Match`, verbatim. */
  readonly headers?: Readonly<Record<string, string>>;
  readonly signal?: AbortSignal;
  /**
   * Whether a request can be sent again after no answer at all. Reads can; Redeem and copies
   * can't, since Workspace may have acted before the answer was lost.
   */
  readonly idempotent?: boolean;
  /** Sent even as the page closes, as End is (`fetch`'s `keepalive`). Bodiless requests only. */
  readonly keepalive?: boolean;
  /** Whether this request moves a workbook's bytes, which gets `transferTimeoutMs`. */
  readonly transfer?: boolean;
}

export interface Answer {
  readonly status: number;
  /** The `ETag` header, when the answer had one. */
  readonly etag: string | null;
  /** The JSON body, or null for an empty one (End's 204). */
  readonly json: unknown;
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

const cancelled = () => new WorkspaceError('cancelled', 'Cancelled.');

/** The kind of error a status and code mean. Codes decide where a status has several. */
export function kindOf(status: number, code: string | null): WorkspaceErrorKind {
  if (status === STATUS.unauthorized) return code === CODES.ticketInvalid ? 'ticket' : 'session';
  if (status === STATUS.forbidden) return code === CODES.originDenied ? 'origin' : 'denied';
  if (status === STATUS.gone || status === STATUS.notFound) return 'gone';
  if (status === STATUS.preconditionFailed) return 'conflict';
  if (status === STATUS.tooLarge) return 'tooLarge';
  if (status === STATUS.tooManyRequests || status === STATUS.unavailable) return 'busy';
  if (status === STATUS.invalid || status === STATUS.preconditionRequired) return 'invalid';
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

export class WorkspaceHttp {
  readonly baseUrl: string;
  readonly #fetch: typeof fetch;
  readonly #timeoutMs: number;
  readonly #transferTimeoutMs: number;
  readonly #retries: number;
  readonly #maxWaitMs: number;
  readonly #sleep: (ms: number, signal?: AbortSignal) => Promise<void>;
  #lastFailure: WorkspaceFailure | null = null;

  constructor(options: HttpOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, '');
    this.#fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.#timeoutMs = options.timeoutMs ?? 30_000;
    this.#transferTimeoutMs = options.transferTimeoutMs ?? 300_000;
    this.#retries = options.retries ?? 3;
    this.#maxWaitMs = options.maxWaitMs ?? 10_000;
    this.#sleep = options.sleep ?? wait;
  }

  /** The last request that failed, for the badge's details. */
  get lastFailure(): WorkspaceFailure | null {
    return this.#lastFailure;
  }

  /** Sends a request to one of Workspace's routes and reads its JSON answer. */
  async request(
    method: 'GET' | 'POST',
    route: string,
    options: RequestOptions = {},
  ): Promise<Answer> {
    const response = await this.#withRetries(options, () =>
      this.#send(`${this.baseUrl}/${route}`, method, options),
    );
    const body = await response.text();
    let json: unknown = null;
    if (body !== '') {
      try {
        json = JSON.parse(body);
      } catch {
        throw new WorkspaceError(
          'contract',
          'Cash Gear Workspace sent an answer the sheet can’t read.',
        );
      }
    }
    return { status: response.status, etag: response.headers.get('ETag'), json };
  }

  /**
   * Reads a signed URL's bytes. Never carries the session, cookies or a referrer, and refuses
   * redirects. A refusal (an expired signature, a blob gone) is `readUrl`, so the caller can ask
   * Workspace for a fresh URL.
   */
  async readSigned(
    url: string,
    options: { signal?: AbortSignal; onProgress?: (received: number) => void } = {},
  ): Promise<Blob> {
    for (let attempt = 0; ; attempt++) {
      if (options.signal?.aborted) throw cancelled();
      const timeout = new AbortController();
      const timer = setTimeout(() => timeout.abort(), this.#transferTimeoutMs);
      const stop = () => timeout.abort();
      options.signal?.addEventListener('abort', stop, { once: true });
      try {
        let response: Response;
        try {
          response = await this.#fetch(url, {
            method: 'GET',
            signal: timeout.signal,
            credentials: 'omit',
            cache: 'no-store',
            mode: 'cors',
            redirect: 'error',
            referrerPolicy: 'no-referrer',
          });
        } catch {
          if (options.signal?.aborted) throw cancelled();
          this.#lastFailure = { status: null, at: Date.now(), correlationId: null };
          if (attempt < this.#retries) {
            await this.#sleep(this.#backoff(attempt), options.signal);
            continue;
          }
          throw new WorkspaceError('network', 'The workbook can’t be downloaded.');
        }
        if (response.status === STATUS.unavailable && attempt < this.#retries) {
          await response.arrayBuffer().catch(() => undefined);
          await this.#sleep(this.#backoff(attempt), options.signal);
          continue;
        }
        if (!response.ok) {
          await response.arrayBuffer().catch(() => undefined);
          this.#lastFailure = { status: response.status, at: Date.now(), correlationId: null };
          if (response.status === 403 || response.status === 404) {
            throw new WorkspaceError('readUrl', 'The workbook’s download link was refused.', {
              status: response.status,
            });
          }
          throw new WorkspaceError(
            'server',
            `The workbook’s download answered ${response.status}.`,
            {
              status: response.status,
            },
          );
        }
        return await readBody(response, options.onProgress);
      } catch (error) {
        if (options.signal?.aborted) throw cancelled();
        if (error instanceof WorkspaceError) throw error;
        throw new WorkspaceError('network', 'The workbook’s download stopped.');
      } finally {
        clearTimeout(timer);
        options.signal?.removeEventListener('abort', stop);
      }
    }
  }

  async #withRetries(options: RequestOptions, send: () => Promise<Response>): Promise<Response> {
    const idempotent = options.idempotent ?? false;
    for (let attempt = 0; ; attempt++) {
      if (options.signal?.aborted) throw cancelled();
      let response: Response;
      try {
        response = await send();
      } catch (error) {
        if (error instanceof WorkspaceError && error.kind === 'cancelled') throw error;
        if (idempotent && attempt < this.#retries) {
          await this.#sleep(this.#backoff(attempt), options.signal);
          continue;
        }
        throw error instanceof WorkspaceError
          ? error
          : new WorkspaceError('network', 'Cash Gear Workspace can’t be reached.');
      }
      if (response.ok) return response;
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

  async #send(url: string, method: string, options: RequestOptions): Promise<Response> {
    const headers: Record<string, string> = { ...options.headers };
    let body: BodyInit | undefined;
    if (options.json !== undefined) {
      headers['Content-Type'] = 'application/json';
      body = JSON.stringify(options.json);
    } else if (options.bytes !== undefined) {
      headers['Content-Type'] = options.contentType ?? 'application/octet-stream';
      body = options.bytes;
    }
    if (options.session !== undefined) headers[SESSION_HEADER] = options.session;
    const timeout = new AbortController();
    const limit = options.transfer ? this.#transferTimeoutMs : this.#timeoutMs;
    const timer = setTimeout(() => timeout.abort(), limit);
    const stop = () => timeout.abort();
    options.signal?.addEventListener('abort', stop, { once: true });
    try {
      return await this.#fetch(url, {
        method,
        headers,
        body,
        signal: timeout.signal,
        credentials: 'omit',
        cache: 'no-store',
        mode: 'cors',
        // A redirect would carry the session header to wherever it points: Workspace's sheet
        // routes never redirect, so one is refused.
        redirect: 'error',
        referrerPolicy: 'no-referrer',
        ...(options.keepalive ? { keepalive: true } : {}),
      });
    } catch {
      if (options.signal?.aborted) throw cancelled();
      this.#lastFailure = { status: null, at: Date.now(), correlationId: null };
      if (timeout.signal.aborted) {
        throw new WorkspaceError('network', 'Cash Gear Workspace took too long to answer.');
      }
      throw new WorkspaceError('network', 'Cash Gear Workspace can’t be reached.');
    } finally {
      clearTimeout(timer);
      options.signal?.removeEventListener('abort', stop);
    }
  }

  async #failure(response: Response): Promise<WorkspaceError> {
    const correlationId = response.headers.get(CORRELATION_HEADER);
    this.#lastFailure = { status: response.status, at: Date.now(), correlationId };
    let message = `Cash Gear Workspace answered ${response.status}.`;
    let problem: Problem | null = null;
    try {
      const text = await response.text();
      if (text !== '') {
        problem = readProblem(JSON.parse(text));
        if (problem.title) message = problem.title;
      }
    } catch (error) {
      // An answer without a problem body: the status alone says what it means.
      if (!(error instanceof ContractError) && !(error instanceof SyntaxError)) throw error;
    }
    return new WorkspaceError(kindOf(response.status, problem?.code ?? null), message, {
      status: response.status,
      code: problem?.code ?? null,
      correlationId: problem?.correlationId || correlationId,
      problem,
    });
  }
}

/** A response's bytes as a Blob, telling `onProgress` as they come. */
async function readBody(
  response: Response,
  onProgress?: (received: number) => void,
): Promise<Blob> {
  const type = response.headers.get('Content-Type') ?? '';
  if (!onProgress || !response.body) return new Blob([await response.arrayBuffer()], { type });
  const reader = response.body.getReader();
  const parts: Uint8Array[] = [];
  let received = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    received += value.byteLength;
    onProgress(received);
  }
  return new Blob(parts as BlobPart[], { type });
}
