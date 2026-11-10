// What the ERP features say, in English (decisions, 2026-11-09, Phase 15, and 2026-11-10, Phase
// 16). Phase 18 adds the Arabic catalogue; headings and lookups already come in the ERP user's
// language, and the engine names broken rules by the contract's codes so that it can.

import type { ErpCellInfo, ErpCounts, ErpRuleInfo } from '@cgs/engine-client';
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
const counted = (n: number, one: string, many: string) => `${count(n)} ${n === 1 ? one : many}`;

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

/** Hover details for a bound cell: its field, what changed and why it is refused or read-only. */
export function cellTip(info: ErpCellInfo, noun: string): { title: string; text: string } | null {
  const shown = (value: string | undefined) => (value === undefined ? '(empty)' : isolate(value));
  if (!info.editable) return { title: isolate(info.label), text: reasonText(info.reason, noun) };
  if (info.heading || !info.state) return null;
  const lines: string[] = [];
  if (info.state === 'formula') {
    lines.push('A formula: convert it to a value to send it to Cash Gear ERP.');
  } else {
    lines.push(`Original ${shown(info.original)} → New ${shown(info.now)}`);
  }
  if (info.rule) lines.push(ruleText(info.label, info.rule));
  return { title: isolate(info.label), text: lines.join('\n') };
}

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
