// Hand-written guards for every answer in the Workspace contract. Each `read…` returns its input
// typed, or throws a `ContractError` naming the first path that breaks the contract. Properties
// the contract doesn't name are ignored, so Workspace can add some without breaking the sheet.

import type {
  Access,
  CopyResponse,
  CurrentVersion,
  FileSession,
  Language,
  Problem,
  RenewResponse,
  UserRef,
  VersionResponse,
  WorkbookRef,
  WorkbookResponse,
} from './contract.ts';

export class ContractError extends Error {
  readonly path: string;

  constructor(path: string, expected: string, got: unknown) {
    super(`${path}: expected ${expected}, got ${describe(got)}`);
    this.name = 'ContractError';
    this.path = path;
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

function text(value: unknown, path: string): string {
  if (typeof value !== 'string' || value === '') throw new ContractError(path, 'text', value);
  return value;
}

function textOrNull(value: unknown, path: string): string | null {
  return value === null || value === undefined ? null : text(value, path);
}

function time(value: unknown, path: string): string {
  const at = text(value, path);
  if (Number.isNaN(Date.parse(at))) throw new ContractError(path, 'an ISO 8601 time', value);
  return at;
}

/** A quoted entity tag, strong or weak, as `ETag` carries it. */
const ENTITY_TAG = /^(W\/)?"[\x21\x23-\x7e\x80-\xff]*"$/;

function etag(value: unknown, path: string): string {
  const tag = text(value, path);
  if (!ENTITY_TAG.test(tag)) throw new ContractError(path, 'a quoted entity tag', value);
  return tag;
}

function language(value: unknown, path: string): Language {
  if (value !== 'en' && value !== 'ar') throw new ContractError(path, '"en" or "ar"', value);
  return value;
}

function access(value: unknown, path: string): Access {
  if (value !== 'edit' && value !== 'view')
    throw new ContractError(path, '"edit" or "view"', value);
  return value;
}

function workbookRef(value: unknown, path: string): WorkbookRef {
  const o = object(value, path);
  return { id: text(o.id, `${path}.id`), name: text(o.name, `${path}.name`) };
}

function userRef(value: unknown, path: string): UserRef {
  const o = object(value, path);
  return { id: text(o.id, `${path}.id`), name: text(o.name, `${path}.name`) };
}

export function readFileSession(value: unknown): FileSession {
  const o = object(value, 'session');
  return {
    token: text(o.token, 'session.token'),
    expiresAt: time(o.expiresAt, 'session.expiresAt'),
    maxExpiresAt: time(o.maxExpiresAt, 'session.maxExpiresAt'),
    workbook: workbookRef(o.workbook, 'session.workbook'),
    access: access(o.access, 'session.access'),
    user: userRef(o.user, 'session.user'),
    language: language(o.language, 'session.language'),
  };
}

export function readRenewResponse(value: unknown): RenewResponse {
  const o = object(value, 'renew');
  return { expiresAt: time(o.expiresAt, 'renew.expiresAt') };
}

export function readWorkbookResponse(value: unknown): WorkbookResponse {
  const o = object(value, 'workbook');
  const readUrl = textOrNull(o.readUrl, 'workbook.readUrl');
  const tag = o.etag === null || o.etag === undefined ? null : etag(o.etag, 'workbook.etag');
  if ((readUrl === null) !== (tag === null)) {
    throw new ContractError('workbook.etag', 'null exactly when readUrl is', o.etag);
  }
  if (readUrl !== null) {
    let url: URL;
    try {
      url = new URL(readUrl);
    } catch {
      throw new ContractError('workbook.readUrl', 'an absolute URL', readUrl);
    }
    if (url.protocol !== 'https:' && url.protocol !== 'http:') {
      throw new ContractError('workbook.readUrl', 'an http or https URL', readUrl);
    }
  }
  let size: number | null = null;
  if (o.size !== null && o.size !== undefined) {
    if (!Number.isSafeInteger(o.size) || (o.size as number) < 0) {
      throw new ContractError('workbook.size', 'a byte count', o.size);
    }
    size = o.size as number;
  }
  return { readUrl, etag: tag, name: text(o.name, 'workbook.name'), size };
}

export function readVersionResponse(value: unknown): VersionResponse {
  const o = object(value, 'version');
  return { versionId: text(o.versionId, 'version.versionId'), etag: etag(o.etag, 'version.etag') };
}

export function readCopyResponse(value: unknown): CopyResponse {
  const o = object(value, 'copy');
  return {
    workbook: workbookRef(o.workbook, 'copy.workbook'),
    access: access(o.access, 'copy.access'),
    versionId: text(o.versionId, 'copy.versionId'),
    etag: etag(o.etag, 'copy.etag'),
  };
}

function currentVersion(value: unknown, path: string): CurrentVersion {
  const o = object(value, path);
  const sha256 = text(o.sha256, `${path}.sha256`);
  if (!/^[0-9a-f]{64}$/.test(sha256)) {
    throw new ContractError(`${path}.sha256`, 'lower-case hex SHA-256', sha256);
  }
  return {
    versionId: text(o.versionId, `${path}.versionId`),
    etag: etag(o.etag, `${path}.etag`),
    sha256,
    savedBy: userRef(o.savedBy, `${path}.savedBy`),
    savedAt: time(o.savedAt, `${path}.savedAt`),
  };
}

/** A problem body. `current` and `limit` are read only when present. */
export function readProblem(value: unknown): Problem {
  const o = object(value, 'problem');
  if (!Number.isSafeInteger(o.status))
    throw new ContractError('problem.status', 'a status', o.status);
  const problem: {
    -readonly [K in keyof Problem]: Problem[K];
  } = {
    title: typeof o.title === 'string' ? o.title : '',
    status: o.status as number,
    code: text(o.code, 'problem.code'),
    correlationId: typeof o.correlationId === 'string' ? o.correlationId : '',
  };
  if (typeof o.type === 'string') problem.type = o.type;
  if (typeof o.detail === 'string') problem.detail = o.detail;
  if (o.current !== undefined && o.current !== null) {
    problem.current = currentVersion(o.current, 'problem.current');
  }
  if (Number.isSafeInteger(o.limit)) problem.limit = o.limit as number;
  return problem;
}
