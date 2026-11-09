// Excel 2016's key tips, for the ribbon's tabs and the Home tab's commands. Anything not listed
// gets a tip the ribbon makes from its text.
import type { CgRibbonTab } from '@cashgear/ui';

/** By tab key. Contextual tabs start with J, as in Excel. */
export const TAB_TIPS: Readonly<Record<string, string>> = {
  home: 'H',
  insert: 'N',
  'page-layout': 'P',
  formulas: 'M',
  data: 'A',
  review: 'R',
  view: 'W',
  'table-design': 'JT',
  'erp-data': 'JE',
  'pivot-analyze': 'JT',
  'pivot-design': 'JY',
  'chart-design': 'JC',
  'chart-format': 'JA',
  'picture-format': 'JP',
};

/** The Home tab's commands, by name. */
const HOME_TIPS: Readonly<Record<string, string>> = {
  paste: 'V',
  cut: 'X',
  copy: 'C',
  'format-painter': 'FP',
  'font-name': 'FF',
  'font-size': 'FS',
  grow: 'FG',
  shrink: 'FK',
  bold: '1',
  italic: '2',
  underline: '3',
  borders: 'B',
  'fill-color': 'H',
  'font-color': 'FC',
  'align-top': 'AT',
  'align-middle': 'AM',
  'align-bottom': 'AB',
  orientation: 'FQ',
  wrap: 'W',
  'align-left': 'AL',
  'align-center': 'AC',
  'align-right': 'AR',
  'indent-less': 'AO',
  'indent-more': 'AI',
  merge: 'M',
  'number-format': 'N',
  accounting: 'AN',
  percent: 'P',
  comma: 'K',
  'more-decimals': '0',
  'fewer-decimals': '9',
  'conditional-formatting': 'L',
  'format-as-table': 'T',
  'cell-styles': 'J',
  insert: 'I',
  delete: 'D',
  format: 'O',
  autosum: 'U',
  fill: 'FI',
  clear: 'E',
  find: 'FD',
};

/** The Home tab's dialog launchers, by group key. */
const HOME_LAUNCHERS: Readonly<Record<string, string>> = {
  clipboard: 'FO',
  font: 'FN',
  alignment: 'FA',
  number: 'FM',
};

/** Alt and these letters stay the browser's own: its File, Edit and address bar keys. */
export const BROWSER_ALT_KEYS: ReadonlyArray<string> = ['F', 'E', 'D'];

/** The tabs with Excel's tips on them. */
export function withKeyTips<T>(tabs: ReadonlyArray<CgRibbonTab<T>>): CgRibbonTab<T>[] {
  return tabs.map((tab) => {
    const tip = TAB_TIPS[tab.key];
    const tipped = tip ? { ...tab, keyTip: tip } : { ...tab };
    if (tab.key !== 'home') return tipped;
    return {
      ...tipped,
      groups: tab.groups.map((group) => ({
        ...group,
        ...(HOME_LAUNCHERS[group.key] ? { launcherKeyTip: HOME_LAUNCHERS[group.key] } : {}),
        items: group.items.map((item) =>
          HOME_TIPS[item.name] ? { ...item, keyTip: HOME_TIPS[item.name] } : item,
        ),
      })),
    };
  });
}
