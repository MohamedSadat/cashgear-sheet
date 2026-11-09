// What the ERP features say, in English (decisions, 2026-11-09, Phase 15). Phase 18 adds the
// Arabic catalogue; headings and lookups already come in the ERP user's language.

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

/** The status badge: "Vendors · 1,240 · Read-only · Sara @ Kit". */
export function badgeText(
  label: string,
  records: number,
  missing: number,
  expired: boolean,
  userName: string,
  company: string,
) {
  const parts = [isolate(label), count(records)];
  if (missing > 0) parts.push(`${count(missing)} not in ERP`);
  parts.push(expired ? 'Session ended' : 'Read-only');
  parts.push(`${isolate(userName)} @ ${isolate(company)}`);
  return parts.join(' · ');
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
