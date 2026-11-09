// Hand-written guards for every response in the contract. Each `read…` returns its input typed,
// or throws a `ContractError` naming the first path that breaks the contract. Properties the
// contract doesn't name are ignored, so the ERP can add some without breaking older sheets,
// except in a record's `values`, which hold exactly the schema's fields. An optional property
// that is `null` counts as absent, since System.Text.Json writes nulls unless told not to.

import {
  EDIT_REASONS,
  FIELD_TYPES,
  type ApiError,
  type CommitResponse,
  type CommitStatusResponse,
  type CommittedRecord,
  type Conflict,
  type ConflictBody,
  type CreateTicketResponse,
  type EntitySchema,
  type ErrorBody,
  type FieldSchema,
  type Language,
  type Localized,
  type LookupItem,
  type QueryFilter,
  type QueryResponse,
  type RecordSnapshot,
  type RenewResponse,
  type SheetSession,
  type WireValue,
} from './contract.ts';

export class ContractError extends Error {
  readonly path: string;
  readonly expected: string;

  constructor(path: string, expected: string, got: unknown) {
    super(`${path}: expected ${expected}, got ${describe(got)}`);
    this.name = 'ContractError';
    this.path = path;
    this.expected = expected;
  }
}

function describe(value: unknown): string {
  if (value === undefined) return 'nothing';
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'an array';
  if (typeof value === 'string')
    return JSON.stringify(value.length > 40 ? value.slice(0, 40) + '…' : value);
  if (typeof value === 'object') return 'an object';
  return String(value);
}

type Obj = Readonly<Record<string, unknown>>;

function object(value: unknown, path: string): Obj {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new ContractError(path, 'an object', value);
  }
  return value as Obj;
}

function array(value: unknown, path: string): readonly unknown[] {
  if (!Array.isArray(value)) throw new ContractError(path, 'an array', value);
  return value;
}

function string(value: unknown, path: string): string {
  if (typeof value !== 'string') throw new ContractError(path, 'a string', value);
  return value;
}

function nonEmpty(value: unknown, path: string): string {
  const text = string(value, path);
  if (text.length === 0) throw new ContractError(path, 'a non-empty string', value);
  return text;
}

function boolean(value: unknown, path: string): boolean {
  if (typeof value !== 'boolean') throw new ContractError(path, 'true or false', value);
  return value;
}

function integer(value: unknown, path: string, min = Number.MIN_SAFE_INTEGER): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < min) {
    throw new ContractError(path, min === 0 ? 'a whole number from 0' : 'a whole number', value);
  }
  return value;
}

function optional<T>(
  o: Obj,
  name: string,
  path: string,
  read: (value: unknown, path: string) => T,
): T | undefined {
  return o[name] === undefined || o[name] === null ? undefined : read(o[name], `${path}.${name}`);
}

/** An ISO 8601 instant with a time zone: `2026-11-09T10:00:05Z` or with `+02:00`. */
export function isIsoDateTime(text: string): boolean {
  return (
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,7})?(Z|[+-]\d{2}:\d{2})$/.test(text) &&
    !Number.isNaN(Date.parse(text))
  );
}

/** A calendar date, `YYYY-MM-DD`, that exists. */
export function isIsoDate(text: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!m) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const date = new Date(Date.UTC(y, mo - 1, d));
  return date.getUTCFullYear() === y && date.getUTCMonth() === mo - 1 && date.getUTCDate() === d;
}

/** A decimal string with at most `scale` digits after the point: `15000.00`, `-3`, `0.5`. */
export function isDecimal(text: string, scale: number): boolean {
  const m = /^-?(0|[1-9]\d*)(\.(\d+))?$/.exec(text);
  return m !== null && (m[3]?.length ?? 0) <= scale;
}

/** A row version: base64 of 8 bytes, as SQL Server's `rowversion` travels. */
export function isVersion(text: string): boolean {
  return /^[A-Za-z0-9+/]{11}=$/.test(text);
}

function dateTime(value: unknown, path: string): string {
  const text = string(value, path);
  if (!isIsoDateTime(text))
    throw new ContractError(path, 'an ISO 8601 date and time with a zone', value);
  return text;
}

function nullableDateTime(value: unknown, path: string): string | null {
  return value === null ? null : dateTime(value, path);
}

function version(value: unknown, path: string): string {
  const text = string(value, path);
  if (!isVersion(text)) throw new ContractError(path, 'a base64 row version of 8 bytes', value);
  return text;
}

function language(value: unknown, path: string): Language {
  if (value !== 'en' && value !== 'ar') throw new ContractError(path, '"en" or "ar"', value);
  return value;
}

function localized(value: unknown, path: string): Localized {
  const o = object(value, path);
  return { en: nonEmpty(o.en, `${path}.en`), ar: nonEmpty(o.ar, `${path}.ar`) };
}

function wireValue(value: unknown, path: string): WireValue {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  throw new ContractError(path, 'a string, number, boolean or null', value);
}

function filter(value: unknown, path: string): QueryFilter | null {
  if (value === null) return null;
  const o = object(value, path);
  for (const [name, v] of Object.entries(o)) wireValue(v, `${path}.${name}`);
  return o as QueryFilter;
}

function oneOf<T extends string>(list: readonly T[], value: unknown, path: string): T {
  if (typeof value !== 'string' || !(list as readonly string[]).includes(value)) {
    throw new ContractError(path, `one of ${list.join(', ')}`, value);
  }
  return value as T;
}

// ---------------------------------------------------------------------------------------------
// C.1

export function readCreateTicketResponse(value: unknown, path = '$'): CreateTicketResponse {
  const o = object(value, path);
  const ticket = string(o.ticket, `${path}.ticket`);
  if (!/^[A-Za-z0-9_-]{43}$/.test(ticket)) {
    throw new ContractError(`${path}.ticket`, '256 bits in base64url (43 characters)', ticket);
  }
  const url = nonEmpty(o.url, `${path}.url`);
  if (!url.endsWith(`#launch=${ticket}`)) {
    throw new ContractError(`${path}.url`, 'a URL ending in #launch=<ticket>', url);
  }
  return { ticket, expiresAt: dateTime(o.expiresAt, `${path}.expiresAt`), url };
}

export function readSheetSession(value: unknown, path = '$'): SheetSession {
  const o = object(value, path);
  const entityRoute = nonEmpty(o.entityRoute, `${path}.entityRoute`);
  if (!/^api(\/[A-Za-z][A-Za-z0-9]*)+$/.test(entityRoute)) {
    throw new ContractError(
      `${path}.entityRoute`,
      'a route such as api/payable/VendorSheet',
      entityRoute,
    );
  }
  return {
    token: nonEmpty(o.token, `${path}.token`),
    idleExpiresAt: dateTime(o.idleExpiresAt, `${path}.idleExpiresAt`),
    maxExpiresAt: dateTime(o.maxExpiresAt, `${path}.maxExpiresAt`),
    company: nonEmpty(o.company, `${path}.company`),
    userId: nonEmpty(o.userId, `${path}.userId`),
    userName: string(o.userName, `${path}.userName`),
    language: language(o.language, `${path}.language`),
    entity: nonEmpty(o.entity, `${path}.entity`),
    entityRoute,
    filter: filter(o.filter, `${path}.filter`),
  };
}

export function readRenewResponse(value: unknown, path = '$'): RenewResponse {
  const o = object(value, path);
  return { idleExpiresAt: dateTime(o.idleExpiresAt, `${path}.idleExpiresAt`) };
}

// ---------------------------------------------------------------------------------------------
// C.2

function bound(value: unknown, path: string, type: string): number | string {
  if (type === 'decimal') {
    const text = string(value, path);
    if (!isDecimal(text, 28)) throw new ContractError(path, 'a decimal string', value);
    return text;
  }
  return integer(value, path);
}

function field(value: unknown, path: string, lookups: Obj): FieldSchema {
  const o = object(value, path);
  const type = oneOf(FIELD_TYPES, o.type, `${path}.type`);
  const editable = boolean(o.editable, `${path}.editable`);
  const editReason = optional(o, 'editReason', path, (v, p) => oneOf(EDIT_REASONS, v, p));
  if (editable && editReason !== undefined) {
    throw new ContractError(`${path}.editReason`, 'nothing on an editable field', editReason);
  }
  const scale = optional(o, 'scale', path, (v, p) => integer(v, p, 0));
  if (type === 'decimal' && scale === undefined) {
    throw new ContractError(`${path}.scale`, 'a scale on a decimal field', undefined);
  }
  const lookup = optional(o, 'lookup', path, nonEmpty);
  if (type === 'lookup' && (lookup === undefined || lookups[lookup] === undefined)) {
    throw new ContractError(`${path}.lookup`, 'the name of a list in lookups', lookup);
  }
  return {
    name: nonEmpty(o.name, `${path}.name`),
    label: localized(o.label, `${path}.label`),
    type,
    editable,
    ...(editReason === undefined ? {} : { editReason }),
    ...spread('required', optional(o, 'required', path, boolean)),
    ...spread(
      'maxLength',
      optional(o, 'maxLength', path, (v, p) => integer(v, p, 1)),
    ),
    ...spread(
      'min',
      optional(o, 'min', path, (v, p) => bound(v, p, type)),
    ),
    ...spread(
      'max',
      optional(o, 'max', path, (v, p) => bound(v, p, type)),
    ),
    ...spread('scale', scale),
    ...spread('lookup', lookup),
  };
}

function spread<K extends string, V>(name: K, value: V | undefined): { [P in K]?: V } {
  return (value === undefined ? {} : { [name]: value }) as { [P in K]?: V };
}

function lookupItem(value: unknown, path: string): LookupItem {
  const o = object(value, path);
  return {
    code: nonEmpty(o.code, `${path}.code`),
    label: localized(o.label, `${path}.label`),
    ...spread('active', optional(o, 'active', path, boolean)),
  };
}

export function readEntitySchema(value: unknown, path = '$'): EntitySchema {
  const o = object(value, path);
  const lookupsObj = object(o.lookups, `${path}.lookups`);
  const lookups: Record<string, readonly LookupItem[]> = {};
  for (const [name, list] of Object.entries(lookupsObj)) {
    const items = array(list, `${path}.lookups.${name}`).map((v, i) =>
      lookupItem(v, `${path}.lookups.${name}[${i}]`),
    );
    const codes = new Set(items.map((item) => item.code));
    if (codes.size !== items.length) {
      throw new ContractError(`${path}.lookups.${name}`, 'unique codes', list);
    }
    lookups[name] = items;
  }
  const fields = array(o.fields, `${path}.fields`).map((v, i) =>
    field(v, `${path}.fields[${i}]`, lookupsObj),
  );
  const names = new Set(fields.map((f) => f.name));
  if (names.size !== fields.length)
    throw new ContractError(`${path}.fields`, 'unique names', o.fields);
  const keyField = nonEmpty(o.keyField, `${path}.keyField`);
  const key = fields.find((f) => f.name === keyField);
  if (!key || key.editable || (key.type !== 'text' && key.type !== 'integer')) {
    throw new ContractError(`${path}.keyField`, 'a read-only text or integer field', keyField);
  }
  const idField = nonEmpty(o.idField, `${path}.idField`);
  if (names.has(idField))
    throw new ContractError(`${path}.idField`, 'a field not sent in values', idField);
  return {
    entity: nonEmpty(o.entity, `${path}.entity`),
    schemaVersion: nonEmpty(o.schemaVersion, `${path}.schemaVersion`),
    label: localized(o.label, `${path}.label`),
    keyField,
    idField,
    maxPageSize: integer(o.maxPageSize, `${path}.maxPageSize`, 1),
    maxRecords: integer(o.maxRecords, `${path}.maxRecords`, 1),
    maxCommitRecords: integer(o.maxCommitRecords, `${path}.maxCommitRecords`, 1),
    fields,
    lookups,
  };
}

// ---------------------------------------------------------------------------------------------
// C.3

function values(value: unknown, path: string): Readonly<Record<string, WireValue>> {
  const o = object(value, path);
  for (const [name, v] of Object.entries(o)) wireValue(v, `${path}.${name}`);
  return o as Readonly<Record<string, WireValue>>;
}

function snapshot(value: unknown, path: string): RecordSnapshot {
  const o = object(value, path);
  return {
    id: integer(o.id, `${path}.id`, 1),
    key: nonEmpty(o.key, `${path}.key`),
    version: version(o.version, `${path}.version`),
    values: values(o.values, `${path}.values`),
  };
}

export function readQueryResponse(value: unknown, path = '$'): QueryResponse {
  const o = object(value, path);
  const records = array(o.records, `${path}.records`).map((v, i) =>
    snapshot(v, `${path}.records[${i}]`),
  );
  for (let i = 1; i < records.length; i++) {
    if (records[i]!.id <= records[i - 1]!.id) {
      throw new ContractError(`${path}.records[${i}].id`, 'ids in ascending order', records[i]!.id);
    }
  }
  const nextAfterId =
    o.nextAfterId === null ? null : integer(o.nextAfterId, `${path}.nextAfterId`, 1);
  if (
    nextAfterId !== null &&
    records.length > 0 &&
    nextAfterId !== records[records.length - 1]!.id
  ) {
    throw new ContractError(`${path}.nextAfterId`, 'the last record’s id', nextAfterId);
  }
  return {
    records,
    nextAfterId,
    total: integer(o.total, `${path}.total`, 0),
    asOf: dateTime(o.asOf, `${path}.asOf`),
  };
}

/**
 * Checks a record's values against the schema: every field present and nothing else, each in
 * its wire format, and the key field equal to the record's key. Returns the problems found.
 */
export function recordProblems(
  schema: EntitySchema,
  record: { readonly key: string; readonly values: Readonly<Record<string, WireValue>> },
): string[] {
  const problems: string[] = [];
  const names = new Set(schema.fields.map((f) => f.name));
  for (const name of Object.keys(record.values)) {
    if (!names.has(name)) problems.push(`${name}: not in the schema`);
  }
  for (const f of schema.fields) {
    if (!(f.name in record.values)) {
      problems.push(`${f.name}: missing`);
      continue;
    }
    const problem = valueProblem(f, record.values[f.name]!);
    if (problem) problems.push(`${f.name}: ${problem}`);
  }
  const key = record.values[schema.keyField];
  if (String(key) !== record.key)
    problems.push(`${schema.keyField}: differs from the record's key`);
  return problems;
}

/** Why `value` isn't a valid wire value for field `f`, or `null` if it is. Not business rules. */
export function valueProblem(f: FieldSchema, value: WireValue): string | null {
  if (value === null) return null;
  switch (f.type) {
    case 'text':
      return typeof value === 'string' ? null : 'not text';
    case 'lookup':
      return typeof value === 'string' ? null : 'not a code';
    case 'integer':
      return typeof value === 'number' && Number.isSafeInteger(value) ? null : 'not a whole number';
    case 'decimal':
      return typeof value === 'string' && isDecimal(value, f.scale ?? 0)
        ? null
        : `not a decimal string at scale ${f.scale ?? 0}`;
    case 'boolean':
      return typeof value === 'boolean' ? null : 'not true or false';
    case 'date':
      return typeof value === 'string' && isIsoDate(value) ? null : 'not YYYY-MM-DD';
    case 'dateTime':
      return typeof value === 'string' && isIsoDateTime(value) ? null : 'not ISO 8601 with a zone';
  }
}

// ---------------------------------------------------------------------------------------------
// C.5 to C.7

function apiError(value: unknown, path: string): ApiError {
  const o = object(value, path);
  return {
    ...spread(
      'id',
      optional(o, 'id', path, (v, p) => integer(v, p, 1)),
    ),
    ...spread('key', optional(o, 'key', path, string)),
    ...spread('field', optional(o, 'field', path, nonEmpty)),
    code: nonEmpty(o.code, `${path}.code`),
    message: string(o.message, `${path}.message`),
  };
}

export function readErrorBody(value: unknown, path = '$'): ErrorBody {
  const o = object(value, path);
  return {
    message: string(o.message, `${path}.message`),
    correlationId: nonEmpty(o.correlationId, `${path}.correlationId`),
    errors: array(o.errors, `${path}.errors`).map((v, i) => apiError(v, `${path}.errors[${i}]`)),
  };
}

function conflict(value: unknown, path: string): Conflict {
  const o = object(value, path);
  return {
    id: integer(o.id, `${path}.id`, 1),
    key: nonEmpty(o.key, `${path}.key`),
    version: version(o.version, `${path}.version`),
    values: values(o.values, `${path}.values`),
    fields: array(o.fields, `${path}.fields`).map((v, i) => nonEmpty(v, `${path}.fields[${i}]`)),
    modifiedBy: o.modifiedBy === null ? null : string(o.modifiedBy, `${path}.modifiedBy`),
    modifiedAt: nullableDateTime(o.modifiedAt, `${path}.modifiedAt`),
  };
}

export function readConflictBody(value: unknown, path = '$'): ConflictBody {
  const o = object(value, path);
  const conflicts = array(o.conflicts, `${path}.conflicts`).map((v, i) =>
    conflict(v, `${path}.conflicts[${i}]`),
  );
  if (conflicts.length === 0)
    throw new ContractError(`${path}.conflicts`, 'at least one conflict', []);
  return {
    message: string(o.message, `${path}.message`),
    correlationId: nonEmpty(o.correlationId, `${path}.correlationId`),
    conflicts,
  };
}

function committed(value: unknown, path: string): CommittedRecord {
  const o = object(value, path);
  return {
    id: integer(o.id, `${path}.id`, 1),
    key: nonEmpty(o.key, `${path}.key`),
    version: version(o.version, `${path}.version`),
    values: values(o.values, `${path}.values`),
  };
}

export function readCommitResponse(value: unknown, path = '$'): CommitResponse {
  const o = object(value, path);
  const dryRun = boolean(o.dryRun, `${path}.dryRun`);
  const committedAt = nullableDateTime(o.committedAt, `${path}.committedAt`);
  if (dryRun !== (committedAt === null)) {
    throw new ContractError(
      `${path}.committedAt`,
      dryRun ? 'null on a dry run' : 'a time',
      committedAt,
    );
  }
  return {
    commitId: nonEmpty(o.commitId, `${path}.commitId`),
    dryRun,
    committedAt,
    records: array(o.records, `${path}.records`).map((v, i) =>
      committed(v, `${path}.records[${i}]`),
    ),
  };
}

export function readCommitStatus(value: unknown, path = '$'): CommitStatusResponse {
  const o = object(value, path);
  const status = oneOf(['committed', 'unknown'] as const, o.status, `${path}.status`);
  if (status === 'unknown') return { status };
  const body = readCommitResponse(o, path);
  if (body.dryRun) throw new ContractError(`${path}.dryRun`, 'false on a stored commit', true);
  return { status, ...body };
}
