// What the ERP features say, in English (decisions, 2026-11-09, Phase 15, and 2026-11-10, Phases
// 16 and 17). Phase 18 adds the Arabic catalogue; headings and lookups already come in the ERP
// user's language, and the engine and the ERP name broken rules by the contract's codes so that
// it can.

import type {
  ErpCellInfo,
  ErpConflictInfo,
  ErpCounts,
  ErpReviewEntry,
  ErpRuleInfo,
  ErpServerError,
} from '@cgs/engine-client';
import type { ErpErrorKind } from '@cgs/erp-connector';

/** The engine's refusals on ERP data, which show in a message box as protection's do. */
export const ERP_REFUSALS: ReadonlySet<string> = new Set([
  'This column is read-only in Cash Gear ERP.',
  "Rows can't be added or deleted in ERP data.",
  "Columns can't be added or deleted in ERP data.",
  "The table that holds ERP data can't be resized, renamed or reshaped.",
  'ERP data can only be sorted top to bottom.',
  'ERP data is sorted as a whole table. Select a cell in it and sort again.',
  "A sheet that holds ERP data can't be deleted.",
  'Wait until the commit to Cash Gear ERP finishes.',
]);

export const READ_ONLY_MESSAGE = 'This column is read-only in Cash Gear ERP.';

export const isErpRefusal = (message: string) => ERP_REFUSALS.has(message);

/** Text set apart from what surrounds it, so Arabic in an English line keeps its place. */
export const isolate = (text: string) => `\u2068${text}\u2069`;

/** `1240` as `1,240`. */
export const count = (n: number) => n.toLocaleString('en-US');

/** The load panel's title, once the entity is known. */
export const openingTitle = (noun: string | null) =>
  noun ? `Opening ${noun} from Cash Gear ERP` : 'Opening from Cash Gear ERP';

export const CONNECTING = 'Connecting to Cash Gear ERP…';

export const loadingText = (noun: string, loaded: number, total: number | null) =>
  total === null ? `Loading ${noun}…` : `Loading ${noun} · ${count(loaded)} of ${count(total)}`;

export const CANCELLED = 'Nothing was opened.';

/** What the panel says when opening fails, and whether Try again can help. */
export function failureText(
  kind: ErpErrorKind,
  message: string,
  noun: string,
): { text: string; retry: boolean } {
  switch (kind) {
    case 'ticket':
      return {
        text: 'This link has expired or was already used. Go back to Cash Gear ERP and choose Edit in Cash Gear Sheet again.',
        retry: false,
      };
    case 'privilege':
      return { text: `Your Cash Gear ERP account can't view ${noun}.`, retry: false };
    case 'origin':
      return { text: "This site can't connect to Cash Gear ERP.", retry: false };
    case 'network':
      return {
        text: "Cash Gear ERP can't be reached. Check your connection, then try again.",
        retry: true,
      };
    case 'busy':
    case 'server':
      return { text: "Cash Gear ERP couldn't answer. Try again in a moment.", retry: true };
    case 'session':
      return {
        text: 'Your Cash Gear ERP session has ended. Open the sheet again from Cash Gear ERP.',
        retry: false,
      };
    case 'cancelled':
      return { text: CANCELLED, retry: false };
    default:
      // The engine's and the ERP's own words, such as the 10,000-record cap's.
      return { text: message, retry: false };
  }
}

/** `1 formula`, `2 formulas`. */
export const counted = (n: number, one: string, many: string) =>
  `${count(n)} ${n === 1 ? one : many}`;

/** One of an entity's records, from its plural English noun: `vendors` gives `vendor`. */
export const singular = (noun: string) => noun.replace(/ies$/, 'y').replace(/s$/, '');

/** `1 vendor`, `12 vendors`. */
export const records = (n: number, noun: string) => counted(n, singular(noun), noun);

/** `1 change`, `37 changes`. */
export const changes = (n: number) => counted(n, 'change', 'changes');

/**
 * The status badge: "Vendors · 1,240 · 12 changed · 2 invalid · 1 formula · Sara @ Kit", or
 * "Read-only" for an ERP user who may edit no column.
 */
export function badgeText(
  label: string,
  records: number,
  missing: number,
  expired: boolean,
  userName: string,
  company: string,
  counts: ErpCounts = { changed: 0, changedRecords: 0, invalid: 0, formulas: 0, conflicts: 0 },
  editable = false,
) {
  const parts = [isolate(label), count(records)];
  if (missing > 0) parts.push(`${count(missing)} not in ERP`);
  if (counts.changed > 0) parts.push(`${count(counts.changed)} changed`);
  if (counts.invalid > 0) parts.push(`${count(counts.invalid)} invalid`);
  if (counts.formulas > 0) parts.push(counted(counts.formulas, 'formula', 'formulas'));
  if (counts.conflicts > 0) parts.push(counted(counts.conflicts, 'conflict', 'conflicts'));
  if (expired) parts.push('Session ended');
  else if (!editable) parts.push('Read-only');
  parts.push(`${isolate(userName)} @ ${isolate(company)}`);
  return parts.join(' · ');
}

/** Whether a binding has changes that would be lost: changed values or formulas in its fields. */
export const hasChanges = (counts: ErpCounts | undefined) =>
  Boolean(counts && counts.changed + counts.formulas > 0);

/** Refresh with changes in the sheet, until Phase 18 merges them. */
export const refreshReplaces = (changes: number) =>
  `Refreshing replaces your ${counted(changes, 'change', 'changes')} with the values in Cash Gear ERP.`;

export const REFRESH_ANYWAY = 'Refresh anyway';

export const NO_ERRORS = 'No cells break a rule.';
export const NOTHING_TO_DISCARD = 'There are no changes to discard.';

/** Why a column is read-only, for hover details. */
export function reasonText(reason: string | undefined, noun: string) {
  switch (reason) {
    case 'key':
      return `The key that keeps each row with its record in Cash Gear ERP.`;
    case 'needsAdvEdit':
      return `Editing this needs the Advanced Edit privilege for ${noun} in Cash Gear ERP.`;
    case 'needsEdit':
      return `Editing this needs the Edit privilege for ${noun} in Cash Gear ERP.`;
    default:
      return `Cash Gear ERP doesn't let this be edited from the sheet.`;
  }
}

const KIND_NEEDS: Record<ErpRuleInfo['type'], string> = {
  text: 'text',
  lookup: 'a code from its list',
  integer: 'a whole number',
  decimal: 'a number',
  boolean: 'TRUE or FALSE',
  date: 'a date',
  dateTime: 'a date and time',
};

/** What a broken local rule says, by its contract code. */
export function ruleText(label: string, rule: ErpRuleInfo) {
  const field = isolate(label);
  switch (rule.code) {
    case 'required':
      return `${field} can't be empty.`;
    case 'maxLength':
      return `${field} can be at most ${count(rule.maxLength ?? 0)} characters.`;
    case 'type':
      return `${field} needs ${KIND_NEEDS[rule.type]}.`;
    case 'range':
      if (rule.min !== null && rule.max !== null)
        return `${field} must be between ${rule.min} and ${rule.max}.`;
      if (rule.min !== null) return `${field} must be at least ${rule.min}.`;
      return `${field} must be at most ${rule.max ?? 0}.`;
    case 'scale':
      return `${field} can have at most ${rule.scale ?? 0} decimal places, and 15 digits in all.`;
    case 'lookup.notFound':
      return rule.inactive
        ? `${isolate(rule.value ?? '')} is no longer offered for ${field}.`
        : `${isolate(rule.value ?? '')} isn't in the list for ${field}.`;
  }
}

/** What an error Cash Gear ERP set on a field says, by its code; its own message otherwise. */
export function serverText(
  label: string,
  error: ErpServerError,
  noun: string,
  value?: string,
): string {
  const field = isolate(label);
  const one = singular(noun);
  switch (error.code) {
    case 'required':
      return `${field} can't be empty in Cash Gear ERP.`;
    case 'maxLength':
      return `${field} is longer than Cash Gear ERP takes.`;
    case 'type':
      return `${field} isn't the kind of value Cash Gear ERP takes.`;
    case 'range':
      return `${field} is outside what Cash Gear ERP takes.`;
    case 'scale':
      return `${field} has more decimal places than Cash Gear ERP takes.`;
    case 'lookup.notFound':
      return value
        ? `${isolate(value)} isn't in Cash Gear ERP's list for ${field}.`
        : `This isn't in Cash Gear ERP's list for ${field}.`;
    case 'field.readOnly':
      return `Cash Gear ERP doesn't let ${field} be edited from the sheet.`;
    case 'privilege.denied':
      return `Editing ${field} needs a privilege your Cash Gear ERP account doesn't have.`;
    case 'record.notFound':
      return `This ${one} isn't in Cash Gear ERP any more.`;
    case 'record.locked':
      return `This ${one} is locked in Cash Gear ERP.`;
    case 'version.missing':
      return `The sheet sent this ${one} without its version. Refresh, then try again.`;
    default:
      return error.message || `Cash Gear ERP refused this (${error.code}).`;
  }
}

/** When something happened, as a short local date and time. */
export const when = (iso: string) =>
  new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** Who changed a conflicting record in Cash Gear ERP, when, and the field's value there now. */
export function conflictText(conflict: ErpConflictInfo): string {
  const parts = ['Changed in Cash Gear ERP'];
  if (conflict.by) parts[0] += ` by ${isolate(conflict.by)}`;
  if (conflict.at) parts[0] += ` on ${when(conflict.at)}`;
  if (conflict.known) parts.push(`now ${conflict.now ? isolate(conflict.now) : '(empty)'}`);
  return parts.join(' · ');
}

/** Hover details for a bound cell: its field, what changed and why it is refused or read-only. */
export function cellTip(info: ErpCellInfo, noun: string): { title: string; text: string } | null {
  const shown = (value: string | undefined) => (value === undefined ? '(empty)' : isolate(value));
  if (!info.editable) return { title: isolate(info.label), text: reasonText(info.reason, noun) };
  if (info.heading || !info.state) return null;
  const lines: string[] = [];
  if (info.state === 'formula') {
    lines.push('A formula: convert it to a value to send it to Cash Gear ERP.');
  } else if (info.state === 'committed') {
    lines.push('Committed to Cash Gear ERP.');
  } else {
    lines.push(`Original ${shown(info.original)} → New ${shown(info.now)}`);
  }
  if (info.rule) lines.push(ruleText(info.label, info.rule));
  if (info.server) lines.push(serverText(info.label, info.server, noun, info.now));
  if (info.conflict) lines.push(conflictText(info.conflict));
  return { title: isolate(info.label), text: lines.join('\n') };
}

// Validate, Review and Commit (Phase 17, decisions 2026-11-10).

export const NOTHING_TO_VALIDATE = 'There are no changes to validate.';
export const NOTHING_TO_COMMIT = 'There are no changes to commit.';
export const NOTHING_VISIBLE = 'There are no changes in the rows shown.';

export const validatingText = (n: number) => `Checking ${changes(n)} with Cash Gear ERP…`;

/** What Validate found, for the status bar. */
export function validatedText(
  noun: string,
  sent: { changes: number; records: number },
  found: { errors: number; errorRecords: number; conflicts: number; batch: string[] },
) {
  if (!found.errors && !found.conflicts && !found.batch.length)
    return `Cash Gear ERP checked ${changes(sent.changes)} to ${records(sent.records, noun)} and found no problems.`;
  const parts: string[] = [];
  if (found.errors)
    parts.push(
      `Cash Gear ERP refuses ${changes(found.errors)} to ${records(found.errorRecords, noun)}`,
    );
  if (found.conflicts)
    parts.push(
      `${records(found.conflicts, noun)} changed in Cash Gear ERP since you opened ${found.conflicts === 1 ? 'it' : 'them'}`,
    );
  const marked = parts.length ? `${parts.join(', and ')}. The cells are marked.` : '';
  return [marked, ...found.batch].filter(Boolean).join(' ');
}

export const localErrorsText = (n: number) =>
  `${counted(n, 'cell breaks', 'cells break')} a rule, so nothing was sent. Fix ${n === 1 ? 'it' : 'them'} or discard ${n === 1 ? 'that change' : 'those changes'}, then commit.`;

export const conflictsWaitText = (n: number, noun: string) =>
  n === 1
    ? `1 ${singular(noun)} was changed in Cash Gear ERP since you opened it. Discard your changes to it, or Refresh to take Cash Gear ERP's values, then commit.`
    : `${records(n, noun)} were changed in Cash Gear ERP since you opened them. Discard your changes to them, or Refresh to take Cash Gear ERP's values, then commit.`;

/** Past the commit limit (decisions, 2026-11-10, Phase 17). */
export const tooManyText = (n: number, max: number, noun: string, visible: boolean) =>
  `${records(n, noun)}${visible ? ' in the rows shown' : ''} have changes, and Cash Gear ERP takes at most ${count(max)} in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.`;

export const COMMIT_TITLE = 'Commit to Cash Gear ERP';

/** The confirmation's lines (decisions, 2026-11-10, Phase 17). */
export function confirmLines(
  noun: string,
  company: string,
  set: { changes: number; records: number; hidden: number; formulas: number },
): string[] {
  const lines = [
    `Commit ${changes(set.changes)} to ${records(set.records, noun)} in Cash Gear ERP (${isolate(company)})?`,
    "Cash Gear ERP checks them again and saves all of them or none. Undo is cleared once they're saved.",
  ];
  if (set.hidden > 0)
    lines.push(
      set.hidden === 1
        ? '1 of these changes is in a row the filter hides.'
        : `${count(set.hidden)} of these changes are in rows the filter hides.`,
    );
  if (set.formulas > 0)
    lines.push(
      set.formulas === 1
        ? '1 formula is left out: convert it to a value to commit it.'
        : `${count(set.formulas)} formulas are left out: convert them to values to commit them.`,
    );
  return lines;
}

export const commitButton = (n: number) => `Commit ${changes(n)}`;

export const reviewCommitButton = (n: number, m: number, noun: string) =>
  `Commit ${changes(n)} to ${records(m, noun)}`;

export const committingText = (n: number, m: number, noun: string) =>
  `Committing ${changes(n)} to ${records(m, noun)}…`;

export function committedText(n: number, m: number, noun: string, kept: number) {
  const done = `Committed ${changes(n)} to ${records(m, noun)}.`;
  return kept > 0
    ? `${done} ${changes(kept)} made since ${kept === 1 ? 'stays' : 'stay'} in the sheet.`
    : done;
}

/** A 400 to a commit: what was refused, and the batch's own errors. */
export function refusedText(
  noun: string,
  found: { errors: number; errorRecords: number },
  batch: string[],
) {
  const marked = found.errors
    ? `Cash Gear ERP refused ${changes(found.errors)} to ${records(found.errorRecords, noun)}, so nothing was saved. The cells are marked; Next Error goes through them.`
    : 'Cash Gear ERP refused the commit, so nothing was saved.';
  return [marked, ...batch].join(' ');
}

/** A 409 to a commit. */
export const conflictText409 = (n: number, noun: string) =>
  n === 1
    ? `1 ${singular(noun)} was changed in Cash Gear ERP since you opened it, so nothing was saved. Its cells are marked. Discard your changes to it, or Refresh to take Cash Gear ERP's values, then commit again.`
    : `${records(n, noun)} were changed in Cash Gear ERP since you opened them, so nothing was saved. Their cells are marked. Discard your changes to them, or Refresh to take Cash Gear ERP's values, then commit again.`;

export const UNKNOWN_OUTCOME =
  "Cash Gear ERP didn't answer, so it isn't known whether the changes were saved. Try again sends the same commit, which Cash Gear ERP saves only once.";

/** A 500: the ERP may have failed as it finished, so its outcome isn't known either. */
export const SERVER_FAILED =
  "Cash Gear ERP couldn't finish the commit, so it isn't known whether the changes were saved. Try again sends the same commit, which Cash Gear ERP saves only once.";

/** Still 429 or 503 after waiting as asked: refused before anything was done. */
export const BUSY_FAILED = 'Cash Gear ERP is busy, so nothing was saved. Try again in a moment.';

export const UNREADABLE_ANSWER =
  "Cash Gear ERP answered in a way the sheet can't read, so it isn't known what was saved. Refresh to see what Cash Gear ERP holds.";

/** A commit's failure other than 400 and 409, and whether Try again can help. */
export function commitFailureText(
  kind: ErpErrorKind,
  message: string,
  noun: string,
  max: number,
): string {
  switch (kind) {
    case 'session':
      return 'Your Cash Gear ERP session has ended, so nothing was sent. Open the sheet again from Cash Gear ERP.';
    case 'privilege':
      return `Your Cash Gear ERP account can't edit ${noun}, so nothing was saved.`;
    case 'tooMany':
      return `Cash Gear ERP takes at most ${count(max)} ${noun} in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.`;
    case 'contract':
      return UNREADABLE_ANSWER;
    default:
      return message;
  }
}

/** A batch error from a 400, which no cell can show, by its code. */
export function batchErrorText(code: string, message: string, noun: string): string {
  switch (code) {
    case 'schema.changed':
      return `Cash Gear ERP's ${noun} have changed since the sheet opened them. Open the sheet again from Cash Gear ERP.`;
    case 'company.mismatch':
      return "This sheet's company isn't the session's.";
    case 'commit.reused':
      return 'Cash Gear ERP already holds another commit under this id. Commit again.';
    case 'batch.tooLarge':
      return `Cash Gear ERP takes fewer ${noun} in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.`;
    default:
      return message;
  }
}

/** The Review dialog's status for a field. */
export function statusText(entry: ErpReviewEntry): string {
  switch (entry.state) {
    case 'formula':
      return 'Formula, not committed';
    case 'conflict':
      return 'Conflict';
    case 'invalid':
      return 'Invalid';
    default:
      return 'Changed';
  }
}

/** The Review dialog's message for a field: what's wrong with it, if anything. */
export function reviewMessage(entry: ErpReviewEntry, noun: string): string {
  const lines: string[] = [];
  if (entry.state === 'formula') lines.push('Convert it to a value to commit it.');
  if (entry.rule) lines.push(ruleText(entry.label, entry.rule));
  if (entry.server) lines.push(serverText(entry.label, entry.server, noun, entry.now));
  if (entry.conflictWith) lines.push(conflictText(entry.conflictWith));
  if (entry.hidden) lines.push('In a row the filter hides.');
  return lines.join(' ');
}

export const HIDDEN_ROW = 'This row is hidden by a filter.';

/** What a refresh did, for the status bar. */
export function refreshedText(
  noun: string,
  summary: { updated: number; changed: number; missing: number },
) {
  const parts = [`Refreshed ${count(summary.updated)} ${noun}`];
  parts.push(summary.changed === 0 ? 'nothing changed' : `${count(summary.changed)} changed`);
  if (summary.missing > 0) parts.push(`${count(summary.missing)} not in ERP`);
  return parts.join(' · ');
}

/** The new workbook's name: "Vendors – Kit". */
export const workbookTitle = (label: string, company: string) => `${label} – ${company}`;
