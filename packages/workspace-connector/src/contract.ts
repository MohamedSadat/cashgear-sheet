// The Workspace sheet-session contract (Track S1 and S2): every request and response the sheet and
// Cash Gear Workspace exchange to open and save one of Workspace's workbooks. `docs/workspace-
// contract.md` describes the routes, headers, status codes and lifetimes; Workspace's own
// `docs/INTEGRATIONS.md` ("Opening the full editor") is where they were first drawn.

export type Language = 'en' | 'ar';

/**
 * What the session may do with its workbook. Workspace maps its levels: `full` and `edit` are
 * `edit`; `comment` and `view` are `view`. A launch is never made for `none`.
 */
export type Access = 'edit' | 'view';

// ---------------------------------------------------------------------------------------------
// Routes, relative to Workspace's API origin

export const ROUTES = {
  redeem: 'api/v1/sheet-sessions/redeem',
  renew: 'api/v1/sheet-sessions/renew',
  end: 'api/v1/sheet-sessions/end',
  workbook: 'api/v1/sheet-sessions/workbook',
  versions: 'api/v1/sheet-sessions/workbook/versions',
  copies: 'api/v1/sheet-sessions/workbook/copies',
} as const;

/** The header that carries the file session's token. Never `Authorization`, never a cookie. */
export const SESSION_HEADER = 'X-Workspace-File-Session';
/** Every answer carries one, and a problem body repeats it as `correlationId`. */
export const CORRELATION_HEADER = 'X-Correlation-Id';

/** What a workbook's bytes travel as, both ways. */
export const XLSX_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';

/** The launch link's path on the sheet's origin: `/cloud#launch=<ticket>`. */
export const LAUNCH_PATH = '/cloud';

/** Lifetimes Workspace keeps (its INTEGRATIONS.md). The sheet reads expiries from the answers. */
export const LIFETIMES = {
  ticketMs: 120_000,
  idleMs: 30 * 60_000,
  maxMs: 10 * 60 * 60_000,
  /** A signed read URL: about five minutes (Workspace's D17). */
  readUrlMs: 5 * 60_000,
} as const;

// ---------------------------------------------------------------------------------------------
// Bodies

/** `POST api/v1/sheet-sessions/redeem`, anonymous, from an allow-listed `Origin`. */
export interface RedeemRequest {
  /** 256 random bits, base64url without padding (43 characters). Single use, 120 seconds. */
  readonly ticket: string;
}

export interface WorkbookRef {
  /** Workspace's id for the workbook, a UUID. */
  readonly id: string;
  /** Its name as Workspace shows it, with or without an extension. */
  readonly name: string;
}

export interface UserRef {
  readonly id: string;
  readonly name: string;
}

/** Redeem's answer: a file session scoped to one workbook. */
export interface FileSession {
  /** Opaque. Sent back only as `X-Workspace-File-Session`. */
  readonly token: string;
  /** When the session ends unless renewed: 30 minutes idle, never past `maxExpiresAt`. ISO 8601. */
  readonly expiresAt: string;
  /** When it ends whatever happens: 10 hours after Redeem. ISO 8601. */
  readonly maxExpiresAt: string;
  readonly workbook: WorkbookRef;
  readonly access: Access;
  readonly user: UserRef;
  /** The Workspace user's language, which the sheet's cloud texts follow. */
  readonly language: Language;
}

/** `POST api/v1/sheet-sessions/renew`: 200, the idle expiry slid by 30 minutes, capped. */
export interface RenewResponse {
  readonly expiresAt: string;
}

/**
 * `GET api/v1/sheet-sessions/workbook`: where its current version's bytes are. A workbook with no
 * version yet (just created in Workspace) has `readUrl`, `etag` and `size` null; the sheet then
 * makes it blank and saves it with `If-None-Match: *`.
 */
export interface WorkbookResponse {
  /** A signed URL for exactly the current version's blob, for about five minutes. */
  readonly readUrl: string | null;
  /** The current version's entity tag, quoted (`"…"`), as the `ETag` header has it. */
  readonly etag: string | null;
  readonly name: string;
  readonly size: number | null;
}

/** `POST api/v1/sheet-sessions/workbook/versions` with the bytes and `If-Match`: 201. */
export interface VersionResponse {
  readonly versionId: string;
  readonly etag: string;
}

/** `POST api/v1/sheet-sessions/workbook/copies` with the bytes: 201, the session now on the copy. */
export interface CopyResponse {
  readonly workbook: WorkbookRef;
  readonly access: Access;
  readonly versionId: string;
  readonly etag: string;
}

/** The current version, as a 412 `version.conflict` names it. */
export interface CurrentVersion {
  readonly versionId: string;
  readonly etag: string;
  /** Lower-case hex SHA-256 of its bytes: equal to the sheet's own means its save landed. */
  readonly sha256: string;
  readonly savedBy: UserRef;
  readonly savedAt: string;
}

/**
 * RFC 9457 ProblemDetails, as every Workspace error is (`application/problem+json`), with
 * Workspace's stable `code` and the request's `correlationId`. Clients switch on `code` only.
 */
export interface Problem {
  readonly type?: string;
  readonly title: string;
  readonly status: number;
  readonly detail?: string;
  readonly code: string;
  readonly correlationId: string;
  /** On a 412 `version.conflict`. */
  readonly current?: CurrentVersion;
  /** On a 413 `limit.size`: the largest body accepted, in bytes. */
  readonly limit?: number;
}

/** The codes the sheet's routes answer. Workspace's own where it has one. */
export const CODES = {
  requestInvalid: 'request.invalid',
  ticketInvalid: 'ticket.invalid',
  sessionExpired: 'session.expired',
  originDenied: 'origin.denied',
  accessDenied: 'access.denied',
  notFound: 'not.found',
  workbookGone: 'workbook.gone',
  versionConflict: 'version.conflict',
  versionRequired: 'version.required',
  limitSize: 'limit.size',
  limitRate: 'limit.rate',
  serverError: 'server.error',
} as const;

export const STATUS = {
  invalid: 400,
  unauthorized: 401,
  forbidden: 403,
  notFound: 404,
  gone: 410,
  preconditionFailed: 412,
  tooLarge: 413,
  preconditionRequired: 428,
  tooManyRequests: 429,
  unavailable: 503,
} as const;
