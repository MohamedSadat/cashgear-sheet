// What the ERP features say, in English and Arabic (decisions, 2026-11-09, Phase 15; 2026-11-10,
// Phases 16 and 17; 2026-11-11, Phase 18). `textsFor` picks the catalogue for the ERP user's
// language: the ERP Data tab, its dialogs, the badge, hover details and messages. The rest of the
// app stays English. Headings and lookups come from the ERP in the user's language, and the engine
// and the ERP name broken rules and errors by the contract's codes, which the catalogue words.

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
export const isolate = (text: string) => `⁨${text}⁩`;

/** `1240` as `1,240`, in either language: the ERP's own lists show Latin digits. */
export const count = (n: number) => n.toLocaleString('en-US');

/** Whether a binding has changes that would be lost: changed values or formulas in its fields. */
export const hasChanges = (counts: ErpCounts | undefined) =>
  Boolean(counts && counts.changed + counts.formulas > 0);

/** `1 formula`, `2 formulas`. */
export const counted = (n: number, one: string, many: string) =>
  `${count(n)} ${n === 1 ? one : many}`;

/** One of an entity's records, from its plural English noun: `vendors` gives `vendor`. */
export const singular = (noun: string) => noun.replace(/ies$/, 'y').replace(/s$/, '');

/** `1 vendor`, `12 vendors`. */
export const records = (n: number, noun: string) => counted(n, singular(noun), noun);

/** `1 change`, `37 changes`. */
export const changes = (n: number) => counted(n, 'change', 'changes');

/** The cap on a bound sheet's recovery draft, as the badge and Connection Info name it. */
export const DRAFT_CAP_MB = 16;

/** What the engine's own English refusals and limits say in English: themselves. */
function localizedEn(message: string) {
  return message;
}

/** The opening panel's failure, and whether Try again can help. */
function failureText(
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
      return { text: 'Nothing was opened.', retry: false };
    default:
      // The engine's and the ERP's own words, such as the 10,000-record cap's.
      return { text: message, retry: false };
  }
}

/**
 * The status badge: "Vendors · 1,240 · 12 changed · 2 invalid · 1 formula · Sara @ Kit", or
 * "Read-only" for an ERP user who may edit no column, "Disconnected" for a recovered draft and
 * "Session ended" once the session has.
 */
function badgeText(
  label: string,
  total: number,
  state: { expired: boolean; disconnected?: boolean; noDraft?: boolean },
  userName: string,
  company: string,
  counts: ErpCounts = {
    changed: 0,
    changedRecords: 0,
    invalid: 0,
    formulas: 0,
    conflicts: 0,
    gone: 0,
  },
  editable = false,
) {
  const parts = [isolate(label), count(total)];
  if (counts.changed > 0) parts.push(`${count(counts.changed)} changed`);
  if (counts.invalid > 0) parts.push(`${count(counts.invalid)} invalid`);
  if (counts.formulas > 0) parts.push(counted(counts.formulas, 'formula', 'formulas'));
  if (counts.conflicts > 0) parts.push(counted(counts.conflicts, 'conflict', 'conflicts'));
  if (counts.gone > 0) parts.push(`${count(counts.gone)} not in ERP`);
  if (state.disconnected) parts.push('Disconnected');
  else if (state.expired) parts.push('Session ended');
  else if (!editable) parts.push('Read-only');
  if (state.noDraft) parts.push('No recovery copy');
  parts.push(`${isolate(userName)} @ ${isolate(company)}`);
  return parts.join(' · ');
}

/** Why a column is read-only, for hover details. */
function reasonText(reason: string | undefined, noun: string) {
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
function ruleText(label: string, rule: ErpRuleInfo) {
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
function serverText(label: string, error: ErpServerError, noun: string, value?: string): string {
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
const when = (iso: string) =>
  new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** Who changed a conflicting record in Cash Gear ERP, when, and the field's value there now. */
function conflictText(conflict: ErpConflictInfo): string {
  const parts = ['Changed in Cash Gear ERP'];
  if (conflict.by) parts[0] += ` by ${isolate(conflict.by)}`;
  if (conflict.at) parts[0] += ` on ${when(conflict.at)}`;
  if (conflict.known) parts.push(`now ${conflict.now ? isolate(conflict.now) : '(empty)'}`);
  return parts.join(' · ');
}

/** Hover details for a bound cell: its field, what changed and why it is refused or read-only. */
function cellTip(info: ErpCellInfo, noun: string): { title: string; text: string } | null {
  const shown = (value: string | undefined) => (value === undefined ? '(empty)' : isolate(value));
  if (info.state === 'gone') {
    return {
      title: isolate(info.label),
      text: `This ${singular(noun)} isn't in Cash Gear ERP any more, so its changes can't be committed.`,
    };
  }
  if (!info.editable) return { title: isolate(info.label), text: reasonText(info.reason, noun) };
  if (info.heading || !info.state) return null;
  const lines: string[] = [];
  if (info.state === 'formula') {
    lines.push('A formula: convert it to a value to send it to Cash Gear ERP.');
  } else if (info.state === 'committed') {
    lines.push('Committed to Cash Gear ERP.');
  } else if (info.original !== undefined || info.now !== undefined) {
    lines.push(`Original ${shown(info.original)} → New ${shown(info.now)}`);
  }
  if (info.rule) lines.push(ruleText(info.label, info.rule));
  if (info.server) lines.push(serverText(info.label, info.server, noun, info.now));
  if (info.conflict) lines.push(conflictText(info.conflict));
  return { title: isolate(info.label), text: lines.join('\n') };
}

/** What Validate found, for the status bar. */
function validatedText(
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

const localErrorsText = (n: number) =>
  `${counted(n, 'cell breaks', 'cells break')} a rule, so nothing was sent. Fix ${n === 1 ? 'it' : 'them'} or discard ${n === 1 ? 'that change' : 'those changes'}, then commit.`;

const conflictsWaitText = (n: number, noun: string) =>
  n === 1
    ? `1 ${singular(noun)} has fields you and Cash Gear ERP both changed. Choose Resolve Conflicts, then commit.`
    : `${records(n, noun)} have fields you and Cash Gear ERP both changed. Choose Resolve Conflicts, then commit.`;

/** Past the commit limit (decisions, 2026-11-10, Phase 17). */
const tooManyText = (n: number, max: number, noun: string, visible: boolean) =>
  `${records(n, noun)}${visible ? ' in the rows shown' : ''} have changes, and Cash Gear ERP takes at most ${count(max)} in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.`;

/** The confirmation's lines (decisions, 2026-11-10, Phase 17). */
function confirmLines(
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

function committedText(n: number, m: number, noun: string, kept: number) {
  const done = `Committed ${changes(n)} to ${records(m, noun)}.`;
  return kept > 0
    ? `${done} ${changes(kept)} made since ${kept === 1 ? 'stays' : 'stay'} in the sheet.`
    : done;
}

/** A 400 to a commit: what was refused, and the batch's own errors. */
function refusedText(
  noun: string,
  found: { errors: number; errorRecords: number },
  batch: string[],
) {
  const marked = found.errors
    ? `Cash Gear ERP refused ${changes(found.errors)} to ${records(found.errorRecords, noun)}, so nothing was saved. The cells are marked; Next Error goes through them.`
    : 'Cash Gear ERP refused the commit, so nothing was saved.';
  return [marked, ...batch].join(' ');
}

/** A 409 to a commit, merged three ways (Phase 18): what merged, and what's left in conflict. */
function conflictText409(found: { conflicts: number; merged: number }, noun: string) {
  const n = found.conflicts + found.merged;
  const head =
    n === 1
      ? `1 ${singular(noun)} was changed in Cash Gear ERP since you opened it, so nothing was saved.`
      : `${records(n, noun)} were changed in Cash Gear ERP since you opened them, so nothing was saved.`;
  const parts = [head];
  if (found.merged > 0)
    parts.push(
      `${records(found.merged, noun)} merged with your changes: commit again to send ${found.merged === 1 ? 'it' : 'them'}.`,
    );
  if (found.conflicts > 0)
    parts.push(
      `${records(found.conflicts, noun)} ${found.conflicts === 1 ? 'has' : 'have'} fields you both changed: choose Resolve Conflicts, then commit again.`,
    );
  return parts.join(' ');
}

/** A commit's failure other than 400 and 409. */
function commitFailureText(kind: ErpErrorKind, message: string, noun: string, max: number) {
  switch (kind) {
    case 'session':
      return 'Your Cash Gear ERP session has ended, so it isn’t known whether the changes were saved. Choose Reconnect: the sheet then asks Cash Gear ERP before it sends them again.';
    case 'privilege':
      return `Your Cash Gear ERP account can't edit ${noun}, so nothing was saved.`;
    case 'tooMany':
      return `Cash Gear ERP takes at most ${count(max)} ${noun} in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.`;
    case 'contract':
      return EN.UNREADABLE_ANSWER;
    default:
      return message;
  }
}

/** A batch error from a 400, which no cell can show, by its code. */
function batchErrorText(code: string, message: string, noun: string): string {
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
function statusText(entry: ErpReviewEntry): string {
  switch (entry.state) {
    case 'formula':
      return 'Formula, not committed';
    case 'conflict':
      return 'Conflict';
    case 'invalid':
      return 'Invalid';
    case 'gone':
      return 'Not in ERP';
    default:
      return 'Changed';
  }
}

/** The Review dialog's message for a field: what's wrong with it, if anything. */
function reviewMessage(entry: ErpReviewEntry, noun: string): string {
  const lines: string[] = [];
  if (entry.state === 'gone')
    lines.push(`This ${singular(noun)} isn't in Cash Gear ERP any more, so it isn't committed.`);
  if (entry.state === 'formula') lines.push('Convert it to a value to commit it.');
  if (entry.rule) lines.push(ruleText(entry.label, entry.rule));
  if (entry.server) lines.push(serverText(entry.label, entry.server, noun, entry.now));
  if (entry.conflictWith) lines.push(conflictText(entry.conflictWith));
  if (entry.hidden) lines.push('In a row the filter hides.');
  return lines.join(' ');
}

/** What a refresh's three-way merge did, for the status bar (decisions, 2026-11-11, Phase 18). */
function refreshedText(
  noun: string,
  summary: { updated: number; changed: number; merged: number; conflicts: number; missing: number },
) {
  const parts = [`Refreshed ${records(summary.updated, noun)}.`];
  if (summary.changed === 0) parts.push('Nothing changed in Cash Gear ERP.');
  else {
    const detail: string[] = [];
    if (summary.merged > 0) detail.push(`${count(summary.merged)} merged with your changes`);
    if (summary.conflicts > 0) detail.push(`${count(summary.conflicts)} in conflict`);
    parts.push(
      `${count(summary.changed)} changed in Cash Gear ERP${detail.length ? `: ${detail.join(', ')}` : ''}.`,
    );
  }
  if (summary.missing > 0)
    parts.push(
      summary.missing === 1
        ? `1 isn't in Cash Gear ERP any more.`
        : `${count(summary.missing)} aren't in Cash Gear ERP any more.`,
    );
  return parts.join(' ');
}

/** What Resolve did, for the status bar. */
function resolvedText(found: { fields: number; records: number; left: number }, noun: string) {
  const parts = [`Resolved ${counted(found.fields, 'field', 'fields')}.`];
  if (found.records > 0)
    parts.push(
      `${records(found.records, noun)} ${found.records === 1 ? 'is' : 'are'} ready to commit.`,
    );
  if (found.left > 0)
    parts.push(
      `${records(found.left, noun)} still ${found.left === 1 ? 'has' : 'have'} conflicts.`,
    );
  return parts.join(' ');
}

export const EN = {
  language: 'en' as 'en' | 'ar',
  dir: 'ltr' as 'ltr' | 'rtl',
  /** The engine's English refusal or limit, in this language. */
  localized: localizedEn,
  count,
  records,
  changes,
  // Opening
  openingTitle: (noun: string | null) =>
    noun ? `Opening ${noun} from Cash Gear ERP` : 'Opening from Cash Gear ERP',
  CONNECTING: 'Connecting to Cash Gear ERP…',
  loadingText: (noun: string, loaded: number, total: number | null) =>
    total === null ? `Loading ${noun}…` : `Loading ${noun} · ${count(loaded)} of ${count(total)}`,
  CANCELLED: 'Nothing was opened.',
  failureText,
  // The badge and hover details
  badgeText,
  reasonText,
  ruleText,
  serverText,
  conflictText,
  cellTip,
  // The ERP Data tab
  TAB: 'ERP Data',
  GROUP_DATA: 'Data',
  GROUP_CHANGES: 'Changes',
  GROUP_CONFLICTS: 'Conflicts',
  GROUP_CONNECTION: 'Connection',
  REFRESH: 'Refresh',
  VALIDATE: 'Validate',
  REVIEW: 'Review Changes',
  COMMIT: 'Commit',
  COMMIT_ALL: 'Commit All Changes…',
  COMMIT_VISIBLE: 'Commit Changes in Visible Rows…',
  DISCARD: 'Discard Changes',
  DISCARD_ALL: 'Discard All Changes',
  DISCARD_SELECTED: 'Discard Changes in Selected Cells',
  NEXT_ERROR: 'Next Error',
  RESOLVE: 'Resolve Conflicts',
  NEXT_CONFLICT: 'Next Conflict',
  CONNECTION_INFO: 'Connection Info',
  RECONNECT: 'Reconnect',
  // Buttons
  TRY_AGAIN: 'Try again',
  CANCEL: 'Cancel',
  CLOSE: 'Close',
  OK: 'OK',
  GO_TO_CELL: 'Go to Cell',
  // Notices
  NO_ERRORS: 'No cells break a rule.',
  NO_CONFLICTS: 'No cells are in conflict.',
  NOTHING_TO_DISCARD: 'There are no changes to discard.',
  refreshingText: (noun: string) => `Refreshing ${noun}…`,
  refreshedText,
  HIDDEN_ROW: 'This row is hidden by a filter.',
  // Validate, Review and Commit (Phase 17)
  NOTHING_TO_VALIDATE: 'There are no changes to validate.',
  NOTHING_TO_COMMIT: 'There are no changes to commit.',
  NOTHING_VISIBLE: 'There are no changes in the rows shown.',
  validatingText: (n: number) => `Checking ${changes(n)} with Cash Gear ERP…`,
  validatedText,
  localErrorsText,
  conflictsWaitText,
  tooManyText,
  COMMIT_TITLE: 'Commit to Cash Gear ERP',
  confirmLines,
  commitButton: (n: number) => `Commit ${changes(n)}`,
  reviewCommitButton: (n: number, m: number, noun: string) =>
    `Commit ${changes(n)} to ${records(m, noun)}`,
  committingText: (n: number, m: number, noun: string) =>
    `Committing ${changes(n)} to ${records(m, noun)}…`,
  committedText,
  refusedText,
  conflictText409,
  UNKNOWN_OUTCOME:
    "Cash Gear ERP didn't answer, so it isn't known whether the changes were saved. Try again asks Cash Gear ERP first, and sends the same commit only if it wasn't saved.",
  /** A 500: the ERP may have failed as it finished, so its outcome isn't known either. */
  SERVER_FAILED:
    "Cash Gear ERP couldn't finish the commit, so it isn't known whether the changes were saved. Try again asks Cash Gear ERP first, and sends the same commit only if it wasn't saved.",
  /** Still 429 or 503 after waiting as asked: refused before anything was done. */
  BUSY_FAILED: 'Cash Gear ERP is busy, so nothing was saved. Try again in a moment.',
  UNREADABLE_ANSWER:
    "Cash Gear ERP answered in a way the sheet can't read, so it isn't known what was saved. Refresh to see what Cash Gear ERP holds.",
  ASKING_STATUS: 'Asking Cash Gear ERP whether the commit was saved…',
  STATUS_UNKNOWN:
    'Cash Gear ERP has no record of the commit, so nothing was saved. Your changes are still here: commit them again when ready.',
  commitFailureText,
  batchErrorText,
  // The Review dialog
  REVIEW_TITLE: 'Review Changes',
  FILTER_ALL: 'All',
  FILTER_INVALID: 'Invalid',
  FILTER_CONFLICTS: 'Conflicts',
  FILTER_HIDDEN: 'In hidden rows',
  FILTER_FORMULAS: 'Formulas',
  FILTERS_LABEL: 'Show',
  COLUMN_RECORD: 'Record',
  COLUMN_FIELD: 'Field',
  COLUMN_ORIGINAL: 'Original',
  COLUMN_NEW: 'New',
  COLUMN_STATUS: 'Status',
  COLUMN_MESSAGE: 'Message',
  NOTHING_HERE: 'Nothing to show here.',
  DISCARD_CHOSEN: 'Discard Selected',
  statusText,
  reviewMessage,
  // Resolve Conflicts (Phase 18)
  RESOLVE_TITLE: 'Resolve Conflicts',
  resolveIntro: (n: number, noun: string) =>
    `${records(n, noun)} changed in Cash Gear ERP since you loaded ${n === 1 ? 'it' : 'them'}, in fields you changed too.`,
  COLUMN_MINE: 'Mine',
  COLUMN_ERP: 'ERP now',
  COLUMN_BY: 'Changed by',
  KEEP_MINE: 'Keep Mine',
  TAKE_ERP: "Take ERP's",
  KEEP_ALL_MINE: 'Keep All Mine',
  TAKE_ALL_ERP: "Take All ERP's",
  UNKNOWN_VALUE: 'Refresh to see what Cash Gear ERP holds.',
  EMPTY: '(empty)',
  NO_CONFLICTS_LEFT: 'No conflicts are left.',
  resolvedText,
  changedAt: (by: string | undefined, at: string | undefined) =>
    [by ? isolate(by) : '', at ? when(at) : ''].filter(Boolean).join(' · '),
  // Sessions and Reconnect (Phase 18)
  SESSION_ENDED:
    'Your Cash Gear ERP session has ended. Your changes stay in this sheet: choose Reconnect to carry on.',
  RECONNECT_TITLE: 'Reconnect to Cash Gear ERP',
  reconnectText: (noun: string) =>
    `Open Cash Gear ERP's ${noun} list and choose Edit in Cash Gear Sheet. The new tab hands its connection to this sheet and closes, and this sheet then refreshes. Your changes stay here meanwhile.`,
  WAITING: 'Waiting for Cash Gear ERP…',
  RECONNECTED: 'Connected to Cash Gear ERP again.',
  refusedHandoverText: (company: string, user: string, noun: string) =>
    `A connection for another company, user or list was refused: this sheet holds ${noun} of ${isolate(company)} for ${isolate(user)}.`,
  HANDED_OVER: 'This connection went to the sheet that was waiting for it. You can close this tab.',
  HANDOVER_REFUSED:
    'The sheet waiting in another tab refused this connection. Open the sheet again from Cash Gear ERP.',
  // Recovery (Phase 18)
  RECOVER_TITLE: 'Recover your changes to ERP data?',
  recoverText: (name: string, noun: string) =>
    `${isolate(name)} has changes to Cash Gear ERP's ${noun} that weren't committed when its session ended. They open disconnected: reconnect from Cash Gear ERP, then commit them. Nothing is sent by itself.`,
  RECOVER: 'Recover',
  RECOVER_DISCARD: 'Discard these changes',
  RECOVER_LAUNCH_TITLE: 'Recover your earlier changes?',
  recoverLaunchText: (label: string, company: string) =>
    `You have changes to ${isolate(label)} – ${isolate(company)} from a session that ended before they were committed. Recover them into this connection, which then refreshes, or open the data afresh.`,
  RECOVER_CHANGES: 'Recover Changes',
  OPEN_AFRESH: 'Open Afresh',
  recoverFailedText: (noun: string) =>
    `This ERP connection can't be recovered. Open the ${noun} again from Cash Gear ERP.`,
  RECOVERED:
    'Recovered disconnected from Cash Gear ERP. Choose Reconnect, then commit your changes.',
  noDraftText: () =>
    `This sheet passed ${DRAFT_CAP_MB} MB, so it keeps no recovery copy: commit or save before you close it.`,
  // Connection Info
  INFO_TITLE: 'Connection Info',
  INFO_DATA: 'Data',
  INFO_COMPANY: 'Company',
  INFO_USER: 'User',
  INFO_RECORDS: 'Records',
  INFO_READ: 'Read from the ERP',
  INFO_SESSION: 'Session',
  INFO_SERVER: 'Server',
  INFO_EDITING: 'Editing',
  INFO_RECOVERY: 'Recovery copy',
  infoRecords: (total: number, gone: number) =>
    gone ? `${count(total)} · ${count(gone)} not in ERP` : count(total),
  SESSION_ENDED_SHORT: 'Ended',
  DISCONNECTED: 'Disconnected',
  openUntil: (at: number) =>
    `Open until ${new Date(at).toLocaleTimeString('en-US', { timeStyle: 'short' })}`,
  readAt: (iso: string) => new Date(iso).toLocaleString('en-US'),
  INFO_PROBLEM: 'Last failed request',
  NO_PROBLEM: 'None',
  problemText: (status: number | null, at: number, reference: string | null) =>
    `${status === null ? 'No answer' : `Answered ${status}`} at ${new Date(at).toLocaleTimeString('en-US', { timeStyle: 'short' })}${reference ? ` · Reference ${isolate(reference)}` : ''}`,
  editingText: (editable: number, all: number) =>
    editable === 0
      ? 'Every column is read-only for you.'
      : `${count(editable)} of ${count(all)} columns can be edited. Changes stay in this sheet until you commit them.`,
  draftText: (kept: boolean): string =>
    kept
      ? 'Kept on this device until you commit, discard or close.'
      : `None: this sheet passed ${DRAFT_CAP_MB} MB.`,
  /** The new workbook's name: "Vendors – Kit". */
  workbookTitle: (label: string, company: string) => `${label} – ${company}`,
};

export type ErpTexts = typeof EN;

// Kept as named exports for the English wording's tests.
export {
  badgeText,
  batchErrorText,
  cellTip,
  committedText,
  confirmLines,
  conflictText409,
  conflictsWaitText,
  failureText,
  localErrorsText,
  refreshedText,
  refusedText,
  reviewMessage,
  serverText,
  statusText,
  tooManyText,
  validatedText,
};

export const workbookTitle = EN.workbookTitle;
