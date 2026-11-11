// Validating and committing ERP changes (Phase 17, plan E.4 and E.5): one request shape for both,
// since Validate is Commit with `dryRun: true` (contract C.4). The engine builds the records from
// its changeset; this sends them and says what came back, as values the engine can take in. A
// commit is idempotent by its `commitId`, so a request with no answer is sent again with the same
// id, and the ERP saves it once whatever happened to the answers before.

import {
  ENTITY_ROUTES,
  type ApiError,
  type CommitRequest,
  type CommitResponse,
  type CommitStatusResponse,
  type Conflict,
  type RecordChange,
} from './contract.ts';
import { ErpError } from './http.ts';
import type { ErpSession } from './session.ts';
import {
  readCommitResponse,
  readCommitStatus,
  readConflictBody,
  readErrorBody,
} from './validate.ts';

/** What Cash Gear ERP made of a commit or a dry run. Other failures throw an `ErpError`. */
export type CommitOutcome =
  /** Saved, or on a dry run every check passed. */
  | { readonly kind: 'ok'; readonly response: CommitResponse }
  /** 400: refused with errors, by record and field or for the whole batch. Nothing written. */
  | { readonly kind: 'invalid'; readonly message: string; readonly errors: readonly ApiError[] }
  /** 409: records changed since they were read. Nothing written. */
  | {
      readonly kind: 'conflict';
      readonly message: string;
      readonly conflicts: readonly Conflict[];
    };

/** A new commit id, a lower-case UUID v4 as the contract asks. */
export function newCommitId(): string {
  return globalThis.crypto.randomUUID().toLowerCase();
}

/**
 * A commit's records, given what they hold rather than how the engine lists them: each record's
 * `row` and `hidden` stay in the sheet.
 */
export function commitRecords(
  records: readonly (RecordChange & { readonly row?: number; readonly hidden?: boolean })[],
): RecordChange[] {
  return records.map(({ id, key, version, changes }) => ({ id, key, version, changes }));
}

/** The records of a commit, as text that compares equal exactly when they would be sent alike. */
export function payloadKey(request: Omit<CommitRequest, 'commitId'>): string {
  return JSON.stringify([
    request.company,
    request.schemaVersion,
    request.dryRun,
    [...request.records].sort((a, b) => a.id - b.id),
  ]);
}

/**
 * Sends a commit, or a dry run of one. The same request goes again after no answer, up to the
 * HTTP layer's retries: the `commitId` makes that safe. Answers 200, 400 and 409 as outcomes.
 */
export async function sendCommit(
  session: ErpSession,
  request: CommitRequest,
  options: { readonly signal?: AbortSignal } = {},
): Promise<CommitOutcome> {
  let answer: unknown;
  try {
    answer = await session.json('POST', session.route(ENTITY_ROUTES.commit), {
      body: request,
      signal: options.signal,
      idempotent: true,
    });
  } catch (error) {
    if (!(error instanceof ErpError)) throw error;
    if (error.kind === 'invalid' && error.body !== undefined) {
      try {
        const body = readErrorBody(error.body);
        return { kind: 'invalid', message: body.message, errors: body.errors };
      } catch {
        // Not the ERP's error body: the error as it is.
      }
    }
    if (error.kind === 'conflict' && error.body !== undefined) {
      try {
        const body = readConflictBody(error.body);
        return { kind: 'conflict', message: body.message, conflicts: body.conflicts };
      } catch {
        // As above.
      }
    }
    throw error;
  }
  let response: CommitResponse;
  try {
    response = readCommitResponse(answer);
  } catch {
    throw new ErpError('contract', 'Cash Gear ERP sent an answer the sheet can’t read.', {
      status: 200,
    });
  }
  if (response.commitId.toLowerCase() !== request.commitId || response.dryRun !== request.dryRun) {
    throw new ErpError('contract', 'Cash Gear ERP answered another commit.', { status: 200 });
  }
  return { kind: 'ok', response };
}

/**
 * Asks Cash Gear ERP whether a commit whose outcome isn't known was saved (Phase 18, plan B.4),
 * before anything sends it again: `committed` carries the stored answer, `unknown` means nothing
 * was written under that id, so sending it again is safe.
 */
export async function commitStatus(
  session: ErpSession,
  commitId: string,
  options: { readonly signal?: AbortSignal } = {},
): Promise<CommitStatusResponse> {
  const answer = await session.json(
    'GET',
    `${session.route(ENTITY_ROUTES.commitStatus)}/${encodeURIComponent(commitId)}`,
    { signal: options.signal, idempotent: true },
  );
  let status: CommitStatusResponse;
  try {
    status = readCommitStatus(answer);
  } catch {
    throw new ErpError('contract', 'Cash Gear ERP sent an answer the sheet can’t read.', {
      status: 200,
    });
  }
  if (status.status === 'committed' && status.commitId.toLowerCase() !== commitId) {
    throw new ErpError('contract', 'Cash Gear ERP answered another commit.', { status: 200 });
  }
  return status;
}
