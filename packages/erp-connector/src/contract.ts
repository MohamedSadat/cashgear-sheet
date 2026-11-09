// The ERP sheet contract: every request and response the sheet and Cash Gear ERP exchange.
// `docs/erp-contract.md` describes the routes, status codes and C# DTOs that mirror these types,
// and `ERP_SHEET_INTEGRATION_PLAN.md` section C is where they were designed.

/** Text in both of the ERP's languages. */
export interface Localized {
  readonly en: string;
  readonly ar: string;
}

export type Language = 'en' | 'ar';

/**
 * A field's value on the wire. Decimals are strings, dates ISO strings, and empty is `null`. A
 * `dateTime` carries the ERP host's own offset, and the sheet shows its wall-clock time in that
 * offset, as the ERP shows it.
 */
export type WireValue = string | number | boolean | null;

/**
 * A query's filter. `search` matches the key or the name, ignoring case; any other property is a
 * field name whose value must be equal. The ERP decides which fields it filters on.
 */
export type QueryFilter = Readonly<Record<string, WireValue>>;

// ---------------------------------------------------------------------------------------------
// Routes

/** Routes outside the entity: the API key's ticket route, and the sheet session's. */
export const ROUTES = {
  createTicket: 'api/system/SheetLaunch/CreateTicket',
  redeem: 'api/sheet/Session/Redeem',
  renew: 'api/sheet/Session/Renew',
  end: 'api/sheet/Session/End',
} as const;

/** Routes under the session's `entityRoute`, for example `api/payable/VendorSheet`. */
export const ENTITY_ROUTES = {
  schema: 'Schema',
  query: 'Query',
  commit: 'Commit',
  commitStatus: 'CommitStatus',
} as const;

/** The header that carries the sheet session's token. Never `Authorization`. */
export const SESSION_HEADER = 'X-Sheet-Session';
/** The header that carries the ERP user's API key, on `CreateTicket` only. */
export const API_KEY_HEADER = 'X-API-Key';
/** Every response carries one, and an error body repeats it as `correlationId`. */
export const CORRELATION_HEADER = 'X-Correlation-Id';

// ---------------------------------------------------------------------------------------------
// C.1 Launch and session

/** `POST api/system/SheetLaunch/CreateTicket`, sent by the ERP client with its API key. */
export interface CreateTicketRequest {
  readonly entity: string;
  readonly filter: QueryFilter | null;
  readonly language: Language;
}

export interface CreateTicketResponse {
  /** 256 random bits, base64url without padding (43 characters). Single use. */
  readonly ticket: string;
  readonly expiresAt: string;
  /** The sheet's URL with the ticket in its fragment: `…/erp#launch=<ticket>`. */
  readonly url: string;
}

/** `POST api/sheet/Session/Redeem`, anonymous, from an allow-listed `Origin`. */
export interface RedeemRequest {
  readonly ticket: string;
}

/** Redeem's answer: the session and what it may reach. */
export interface SheetSession {
  /** Opaque; held in memory only, and sent in `X-Sheet-Session`. */
  readonly token: string;
  readonly idleExpiresAt: string;
  readonly maxExpiresAt: string;
  readonly company: string;
  readonly userId: string;
  readonly userName: string;
  readonly language: Language;
  readonly entity: string;
  /** For example `api/payable/VendorSheet`, with no leading or trailing slash. */
  readonly entityRoute: string;
  readonly filter: QueryFilter | null;
}

/** `POST api/sheet/Session/Renew` slides the idle expiry, never past `maxExpiresAt`. */
export interface RenewResponse {
  readonly idleExpiresAt: string;
}

// `POST api/sheet/Session/End` answers 204 with no body.

// ---------------------------------------------------------------------------------------------
// C.2 Entity schema

export const FIELD_TYPES = [
  'text',
  'integer',
  'decimal',
  'boolean',
  'date',
  'dateTime',
  'lookup',
] as const;
export type FieldType = (typeof FIELD_TYPES)[number];

/** Why a field is read-only for this caller. */
export const EDIT_REASONS = ['notEditable', 'needsEdit', 'needsAdvEdit'] as const;
export type EditReason = (typeof EDIT_REASONS)[number];

export interface FieldSchema {
  readonly name: string;
  readonly label: Localized;
  readonly type: FieldType;
  /** Already resolved for this caller. The server checks every field again on commit. */
  readonly editable: boolean;
  /** Present when `editable` is false. */
  readonly editReason?: EditReason;
  readonly required?: boolean;
  readonly maxLength?: number;
  /** Bounds for `integer` and `decimal`. Decimals give theirs as decimal strings. */
  readonly min?: number | string;
  readonly max?: number | string;
  /** Digits after the point, for `decimal`. */
  readonly scale?: number;
  /** The name of the list in `EntitySchema.lookups`, for `lookup`. */
  readonly lookup?: string;
}

export interface LookupItem {
  readonly code: string;
  /** The ERP keeps one description per item, so `en` and `ar` may be the same text. */
  readonly label: Localized;
  /**
   * `false` for an item records may still hold but no change may choose: the sheet doesn't
   * offer it, and the ERP refuses it with `lookup.notFound`. Absent means active.
   */
  readonly active?: boolean;
}

/** `GET {entityRoute}/Schema`. */
export interface EntitySchema {
  readonly entity: string;
  /** Names this field list. A commit naming another one is refused with `schema.changed`. */
  readonly schemaVersion: string;
  readonly label: Localized;
  /** The business key, shown in a read-only cell. Vendors: `vendorAccount`. */
  readonly keyField: string;
  /** The ERP's surrogate id, which travels only as a record's `id`. Vendors: `recId`. */
  readonly idField: string;
  readonly maxPageSize: number;
  readonly maxRecords: number;
  readonly maxCommitRecords: number;
  readonly fields: readonly FieldSchema[];
  readonly lookups: Readonly<Record<string, readonly LookupItem[]>>;
}

// ---------------------------------------------------------------------------------------------
// C.3 Record snapshot (keyset paging)

/**
 * `POST {entityRoute}/Query`. Records come in ascending `id` after `afterId`. With `ids`, only
 * those records come (those still in the ERP and in the session's company) and `filter` is
 * ignored; refresh uses this.
 */
export interface QueryRequest {
  readonly filter: QueryFilter | null;
  readonly ids: readonly number[] | null;
  readonly afterId: number;
  /** 1 to the schema's `maxPageSize`. */
  readonly pageSize: number;
}

export interface RecordSnapshot {
  readonly id: number;
  readonly key: string;
  /** The record's row version, base64. */
  readonly version: string;
  /** Every field in the schema, by name, and nothing else. */
  readonly values: Readonly<Record<string, WireValue>>;
}

export interface QueryResponse {
  readonly records: readonly RecordSnapshot[];
  /** The `afterId` for the next page, or `null` on the last page. */
  readonly nextAfterId: number | null;
  /** How many records match the whole query. */
  readonly total: number;
  readonly asOf: string;
}

// ---------------------------------------------------------------------------------------------
// C.4 Changeset

export interface FieldChange {
  /** For audit and diagnostics only: concurrency is judged by `version` alone. */
  readonly from: WireValue;
  readonly to: WireValue;
}

export interface RecordChange {
  readonly id: number;
  readonly key: string;
  readonly version: string;
  readonly changes: Readonly<Record<string, FieldChange>>;
}

/** `POST {entityRoute}/Commit`. With `dryRun`, everything is checked and nothing is written. */
export interface CommitRequest {
  /** A UUID made per commit attempt and reused on every retry of it. */
  readonly commitId: string;
  /** Must equal the session's company. */
  readonly company: string;
  readonly schemaVersion: string;
  readonly dryRun: boolean;
  readonly records: readonly RecordChange[];
}

// ---------------------------------------------------------------------------------------------
// C.5 to C.7 Results

/**
 * Every error's code, so the sheet can show its own English or Arabic text. The server's
 * `message` is the fallback. Plan C.8's list, with the four codes Phase 14 added at the end.
 */
export const ERROR_CODES = [
  'required',
  'maxLength',
  'type',
  'range',
  'scale',
  'lookup.notFound',
  'field.readOnly',
  'privilege.denied',
  'record.notFound',
  'record.locked',
  'version.missing',
  'version.conflict',
  'batch.tooLarge',
  'company.mismatch',
  'schema.changed',
  'session.expired',
  'ticket.invalid',
  'origin.denied',
  'commit.reused',
  'request.invalid',
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];

export function isErrorCode(code: string): code is ErrorCode {
  return (ERROR_CODES as readonly string[]).includes(code);
}

/**
 * One error. A field error has `id`, `key` and `field`; a record error has `id` and `key`; a
 * batch error has neither. A code the sheet doesn't know is shown by its `message`.
 */
export interface ApiError {
  readonly id?: number;
  readonly key?: string;
  readonly field?: string;
  readonly code: ErrorCode | (string & Record<never, never>);
  readonly message: string;
}

/** The ERP's error body, for 400, 401, 403, 413, 429, 500 and 503. */
export interface ErrorBody {
  readonly message: string;
  readonly correlationId: string;
  readonly errors: readonly ApiError[];
}

export interface Conflict {
  readonly id: number;
  readonly key: string;
  /** The record's version now. */
  readonly version: string;
  /** The record now. */
  readonly values: Readonly<Record<string, WireValue>>;
  /** The commit's fields whose value now differs from their `from`; empty if none clash. */
  readonly fields: readonly string[];
  readonly modifiedBy: string | null;
  readonly modifiedAt: string | null;
}

/** 409: some records changed since they were read. Lists every one; nothing is written. */
export interface ConflictBody {
  readonly message: string;
  readonly correlationId: string;
  readonly conflicts: readonly Conflict[];
}

/** A record as persisted (or, on a dry run, as it would be). */
export interface CommittedRecord {
  readonly id: number;
  readonly key: string;
  readonly version: string;
  readonly values: Readonly<Record<string, WireValue>>;
}

/** 200 from Commit. A dry run gives `committedAt: null` and versions unchanged. */
export interface CommitResponse {
  readonly commitId: string;
  readonly dryRun: boolean;
  readonly committedAt: string | null;
  readonly records: readonly CommittedRecord[];
}

/**
 * `GET {entityRoute}/CommitStatus/{commitId}`. `unknown` means nothing was written under that
 * id in the session's company, so retrying it is safe.
 */
export type CommitStatusResponse =
  ({ readonly status: 'committed' } & CommitResponse) | { readonly status: 'unknown' };

// ---------------------------------------------------------------------------------------------
// Status codes

/** What each status means on the sheet's routes. */
export const STATUS = {
  ok: 200,
  noContent: 204,
  /** Errors in the request: fields, records or the batch. Nothing is written. */
  invalid: 400,
  /** The session or ticket is missing, unknown, used or expired. */
  unauthorized: 401,
  /** A unit privilege or the Origin is missing. */
  forbidden: 403,
  /** Records changed since they were read (`ConflictBody`). Nothing is written. */
  conflict: 409,
  /** Past `maxCommitRecords`, or more `ids` than `maxRecords`. */
  tooLarge: 413,
  /** With `Retry-After`. */
  tooManyRequests: 429,
  /** With `Retry-After`. */
  unavailable: 503,
} as const;
