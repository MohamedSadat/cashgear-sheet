// The embed's own few words, in Workspace's language (`locale`): what screen readers hear for
// the view, and the page shown when `/embed` is opened on its own. Workspace shows its own text
// for every error, by code.
import type { Locale } from './contract';

export interface EmbedTexts {
  /** The range's label: its sheet and cells. */
  range: (sheet: string, range: string) => string;
  /** The cells' table, for screen readers. */
  cells: (sheet: string, range: string) => string;
  chart: (title: string, sheet: string) => string;
  /** A chart this app can't draw. */
  unreadable: (name: string) => string;
  /** `/embed` opened in a tab of its own. */
  alone: string;
  openSheet: string;
}

export const EMBED_TEXTS: Record<Locale, EmbedTexts> = {
  en: {
    range: (sheet, range) => `${sheet}, ${range}`,
    cells: (sheet, range) => `Cells of ${sheet}, ${range}`,
    chart: (title, sheet) => `${title}, a chart on ${sheet}`,
    unreadable: (name) => `${name}: this chart can’t be shown here`,
    alone: 'This address shows a workbook inside a page of Cash Gear Workspace.',
    openSheet: 'Open Cash Gear Sheet',
  },
  ar: {
    range: (sheet, range) => `${sheet}، ${range}`,
    cells: (sheet, range) => `خلايا ${sheet}، ${range}`,
    chart: (title, sheet) => `${title}، مخطط في ${sheet}`,
    unreadable: (name) => `${name}: لا يمكن عرض هذا المخطط هنا`,
    alone: 'يعرض هذا العنوان مصنفًا داخل صفحة من Cash Gear Workspace.',
    openSheet: 'فتح Cash Gear Sheet',
  },
};

/** Arabic when the browser's language is, for the page opened on its own. */
export const browserLocale = (language = globalThis.navigator?.language ?? 'en'): Locale =>
  /^ar\b/i.test(language) ? 'ar' : 'en';
