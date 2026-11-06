// The menu under an AutoFilter button, as Excel 2016's: sorting, the filters for the column's kind
// of values, and the list of values to tick. Everything it changes goes through `run`.
import { useEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  FilterCriteria,
  FilterList,
} from '@cgs/engine-client';
import { columnName } from '../grid/geometry';

export interface FilterMenuProps {
  engine: () => EngineClient | null;
  sheet: number;
  /** A table's name, or null for the sheet's own filter. */
  table: string | null;
  /** The filter's whole area (headings row first), 1-based. */
  area: CellArea;
  /** The column whose button was clicked, 1-based sheet column. */
  col: number;
  /** Where to show it: the button's box in page pixels. */
  anchor: { left: number; top: number; width: number; height: number };
  run: (command: Command) => Promise<Applied>;
  onClose: () => void;
}

// ---- Pure helpers, tested in FilterMenu.test.ts ----

export type FilterKind = 'text' | 'number' | 'date';

export type Day = readonly [number, number, number];

export const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
];

const NUMBER_TEXT = /^[\s(+-]*[$€£¥]?\s*-?(\d[\d,]*\.?\d*|\.\d+)(e[+-]?\d+)?\s*%?\s*\)?$/i;

/** Which filters the column offers: Date Filters for dates, Number Filters for mostly numbers. */
export function filterKind(list: FilterList): FilterKind {
  if (list.dates.length > 0 && list.dates.length >= list.values.length) return 'date';
  const numbers = list.values.filter((value) => NUMBER_TEXT.test(value.text)).length;
  return numbers > 0 && numbers * 2 > list.values.length ? 'number' : 'text';
}

/** The ticks in the list: values by their text, days as `y-m-d`. */
export interface Ticks {
  values: ReadonlySet<string>;
  dates: ReadonlySet<string>;
  blanks: boolean;
}

export const dayKey = (day: readonly number[]) => day.join('-');

export function initialTicks(list: FilterList): Ticks {
  return {
    values: new Set(list.values.filter((value) => value.checked).map((value) => value.text)),
    dates: new Set(list.dates.filter((date) => date.checked).map((date) => dayKey(date.date))),
    blanks: list.blanks && list.blanksChecked,
  };
}

/** Ticks or unticks some of the list, leaving the rest as it was. */
export function setTicks(
  ticks: Ticks,
  change: { values?: readonly string[]; dates?: readonly string[]; blanks?: boolean },
  on: boolean,
): Ticks {
  const values = new Set(ticks.values);
  const dates = new Set(ticks.dates);
  for (const value of change.values ?? []) {
    if (on) values.add(value);
    else values.delete(value);
  }
  for (const date of change.dates ?? []) {
    if (on) dates.add(date);
    else dates.delete(date);
  }
  return { values, dates, blanks: change.blanks ? on : ticks.blanks };
}

/** Every item of `list` is ticked. */
export function allTicked(list: FilterList, ticks: Ticks): boolean {
  return (
    list.values.every((value) => ticks.values.has(value.text)) &&
    list.dates.every((date) => ticks.dates.has(dayKey(date.date))) &&
    (!list.blanks || ticks.blanks)
  );
}

/** No item of `list` is ticked. */
export function noneTicked(list: FilterList, ticks: Ticks): boolean {
  return (
    !list.values.some((value) => ticks.values.has(value.text)) &&
    !list.dates.some((date) => ticks.dates.has(dayKey(date.date))) &&
    !(list.blanks && ticks.blanks)
  );
}

export interface YearNode {
  year: number;
  months: { month: number; days: Day[] }[];
}

/** Days grouped by year and month, in the order given. */
export function dateTree(days: readonly Day[]): YearNode[] {
  const years: YearNode[] = [];
  for (const day of days) {
    let year = years.find((node) => node.year === day[0]);
    if (!year) {
      year = { year: day[0], months: [] };
      years.push(year);
    }
    let month = year.months.find((node) => node.month === day[1]);
    if (!month) {
      month = { month: day[1], days: [] };
      year.months.push(month);
    }
    month.days.push(day);
  }
  return years;
}

/** The ticked days as Excel saves them: a whole year as `[y]`, a whole month `[y, m]`, else days. */
export function dateGroups(days: readonly Day[], ticked: ReadonlySet<string>): number[][] {
  const groups: number[][] = [];
  const isTicked = (day: Day) => ticked.has(dayKey(day));
  for (const year of dateTree(days)) {
    if (year.months.every((month) => month.days.every(isTicked))) {
      groups.push([year.year]);
      continue;
    }
    for (const month of year.months) {
      if (month.days.every(isTicked)) groups.push([year.year, month.month]);
      else for (const day of month.days) if (isTicked(day)) groups.push([...day]);
    }
  }
  return groups;
}

/** The criteria for the ticks, or none when everything is ticked. */
export function valuesCriteria(list: FilterList, ticks: Ticks): FilterCriteria | undefined {
  if (allTicked(list, ticks)) return undefined;
  return {
    kind: 'values',
    values: list.values.filter((value) => ticks.values.has(value.text)).map((value) => value.text),
    blank: list.blanks && ticks.blanks,
    dates: dateGroups(
      list.dates.map((date) => date.date),
      ticks.dates,
    ),
  };
}

/** A search as Excel's: a part of the text, with `*` and `?` as wildcards, any case. */
export function searchMatches(text: string, query: string): boolean {
  const wanted = query.trim();
  if (!wanted) return true;
  if (!/[*?]/.test(wanted)) return text.toLowerCase().includes(wanted.toLowerCase());
  const pattern = Array.from(wanted, (ch) =>
    ch === '*' ? '.*' : ch === '?' ? '.' : ch.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
  ).join('');
  return new RegExp(pattern, 'i').test(text);
}

const pad = (n: number) => String(n).padStart(2, '0');

/** A day as the custom filter's value box shows it. */
export const dayText = (day: Day) => `${day[1]}/${day[2]}/${day[0]}`;

function daySearchText(day: Day) {
  return `${day[0]} ${MONTHS[day[1] - 1]} ${pad(day[2])} ${dayText(day)}`;
}

/** The part of the list a search shows. Blanks are left out while searching, as in Excel. */
export function visibleList(list: FilterList, query: string): FilterList {
  if (!query.trim()) return list;
  return {
    ...list,
    values: list.values.filter((value) => searchMatches(value.text, query)),
    dates: list.dates.filter((date) => searchMatches(daySearchText(date.date), query)),
    blanks: false,
  };
}

/** A search ticks everything it finds. */
export function searchTicks(list: FilterList, query: string): Ticks {
  const found = visibleList(list, query);
  return {
    values: new Set(found.values.map((value) => value.text)),
    dates: new Set(found.dates.map((date) => dayKey(date.date))),
    blanks: false,
  };
}

/** What OK applies: the search's ticks, added to the list's own when asked. */
export function finalTicks(base: Ticks, found: Ticks | null, add: boolean): Ticks {
  if (!found) return base;
  if (!add) return found;
  return {
    values: new Set([...base.values, ...found.values]),
    dates: new Set([...base.dates, ...found.dates]),
    blanks: base.blanks || found.blanks,
  };
}

export type CustomOp =
  | 'equal'
  | 'notEqual'
  | 'greaterThan'
  | 'greaterThanOrEqual'
  | 'lessThan'
  | 'lessThanOrEqual'
  | 'beginsWith'
  | 'notBeginsWith'
  | 'endsWith'
  | 'notEndsWith'
  | 'contains'
  | 'notContains';

export interface CustomCondition {
  /** Empty for the second row left unused. */
  op: CustomOp | '';
  value: string;
}

type SavedCondition = Extract<FilterCriteria, { kind: 'custom' }>['conditions'][number];

/** Excel's serial for a day, in the 1900 date system. */
export function daySerial(year: number, month: number, day: number): number {
  return Math.round((Date.UTC(year, month - 1, day) - Date.UTC(1899, 11, 30)) / 86_400_000);
}

/** A typed date (`m/d/yyyy` or `yyyy-mm-dd`) as a serial, or null. */
export function parseDay(text: string): number | null {
  const trimmed = text.trim();
  let year: number, month: number, day: number;
  const us = /^(\d{1,2})\/(\d{1,2})\/(\d{2}|\d{4})$/.exec(trimmed);
  const iso = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(trimmed);
  if (us) {
    month = Number(us[1]);
    day = Number(us[2]);
    year = Number(us[3]);
    // Two-digit years as Excel reads them: 00 to 29 are this century.
    if (us[3].length === 2) year += year < 30 ? 2000 : 1900;
  } else if (iso) {
    year = Number(iso[1]);
    month = Number(iso[2]);
    day = Number(iso[3]);
  } else {
    return null;
  }
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCFullYear() !== year || date.getUTCMonth() !== month - 1) return null;
  return daySerial(year, month, day);
}

/** A comparison's value as the engine wants it: dates as serials, numbers without separators. */
function compareValue(kind: FilterKind, text: string): string {
  if (kind === 'date') {
    const serial = parseDay(text);
    if (serial !== null) return String(serial);
  }
  if (kind === 'number') {
    const plain = text.trim().replace(/,/g, '');
    if (/^[-+]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?%?$/i.test(plain)) {
      const percent = plain.endsWith('%');
      const number = Number(percent ? plain.slice(0, -1) : plain);
      if (Number.isFinite(number)) return String(percent ? number / 100 : number);
    }
  }
  return text;
}

/** One row of the Custom AutoFilter dialog as saved: the text operators become wildcards. */
export function savedCondition(kind: FilterKind, op: CustomOp, value: string): SavedCondition {
  switch (op) {
    case 'beginsWith':
      return { op: 'equal', value: `${value}*` };
    case 'notBeginsWith':
      return { op: 'notEqual', value: `${value}*` };
    case 'endsWith':
      return { op: 'equal', value: `*${value}` };
    case 'notEndsWith':
      return { op: 'notEqual', value: `*${value}` };
    case 'contains':
      return { op: 'equal', value: `*${value}*` };
    case 'notContains':
      return { op: 'notEqual', value: `*${value}*` };
    default:
      return { op, value: compareValue(kind, value) };
  }
}

/** The Custom AutoFilter dialog's criteria, or null when neither row has an operator. */
export function customCriteria(
  kind: FilterKind,
  conditions: readonly CustomCondition[],
  and: boolean,
): FilterCriteria | null {
  const saved = conditions.flatMap((condition) =>
    condition.op ? [savedCondition(kind, condition.op, condition.value)] : [],
  );
  if (saved.length === 0) return null;
  return { kind: 'custom', and, conditions: saved };
}

/** The Top 10 AutoFilter dialog's criteria, or what's wrong with its number. */
export function top10Criteria(
  top: boolean,
  text: string,
  percent: boolean,
): { criteria: FilterCriteria } | { error: string } {
  const most = percent ? 100 : 500;
  const value = Number(text.trim());
  if (!text.trim() || !Number.isInteger(value) || value < 1 || value > most) {
    return { error: `Enter a whole number between 1 and ${most}.` };
  }
  return { criteria: { kind: 'top10', top, percent, value } };
}

// ---- The filters each kind offers ----

type Pick =
  | { label: string; dynamic: string }
  | { label: string; custom: [CustomOp, CustomOp | '', boolean] }
  | { label: string; top10: true };

const TEXT_PICKS: (Pick | null)[] = [
  { label: 'Equals…', custom: ['equal', '', true] },
  { label: 'Does Not Equal…', custom: ['notEqual', '', true] },
  null,
  { label: 'Begins With…', custom: ['beginsWith', '', true] },
  { label: 'Ends With…', custom: ['endsWith', '', true] },
  null,
  { label: 'Contains…', custom: ['contains', '', true] },
  { label: 'Does Not Contain…', custom: ['notContains', '', true] },
  null,
  { label: 'Custom Filter…', custom: ['equal', '', true] },
];

const NUMBER_PICKS: (Pick | null)[] = [
  { label: 'Equals…', custom: ['equal', '', true] },
  { label: 'Does Not Equal…', custom: ['notEqual', '', true] },
  null,
  { label: 'Greater Than…', custom: ['greaterThan', '', true] },
  { label: 'Greater Than Or Equal To…', custom: ['greaterThanOrEqual', '', true] },
  { label: 'Less Than…', custom: ['lessThan', '', true] },
  { label: 'Less Than Or Equal To…', custom: ['lessThanOrEqual', '', true] },
  { label: 'Between…', custom: ['greaterThanOrEqual', 'lessThanOrEqual', true] },
  null,
  { label: 'Top 10…', top10: true },
  { label: 'Above Average', dynamic: 'aboveAverage' },
  { label: 'Below Average', dynamic: 'belowAverage' },
  null,
  { label: 'Custom Filter…', custom: ['equal', '', true] },
];

const DATE_PICKS: (Pick | null)[] = [
  { label: 'Equals…', custom: ['equal', '', true] },
  { label: 'Before…', custom: ['lessThan', '', true] },
  { label: 'After…', custom: ['greaterThan', '', true] },
  { label: 'Between…', custom: ['greaterThanOrEqual', 'lessThanOrEqual', true] },
  null,
  { label: 'Tomorrow', dynamic: 'tomorrow' },
  { label: 'Today', dynamic: 'today' },
  { label: 'Yesterday', dynamic: 'yesterday' },
  null,
  { label: 'Next Week', dynamic: 'nextWeek' },
  { label: 'This Week', dynamic: 'thisWeek' },
  { label: 'Last Week', dynamic: 'lastWeek' },
  null,
  { label: 'Next Month', dynamic: 'nextMonth' },
  { label: 'This Month', dynamic: 'thisMonth' },
  { label: 'Last Month', dynamic: 'lastMonth' },
  null,
  { label: 'Next Quarter', dynamic: 'nextQuarter' },
  { label: 'This Quarter', dynamic: 'thisQuarter' },
  { label: 'Last Quarter', dynamic: 'lastQuarter' },
  null,
  { label: 'Next Year', dynamic: 'nextYear' },
  { label: 'This Year', dynamic: 'thisYear' },
  { label: 'Last Year', dynamic: 'lastYear' },
  null,
  { label: 'Year to Date', dynamic: 'yearToDate' },
  null,
  { label: 'Custom Filter…', custom: ['equal', '', true] },
];

const PICKS: Record<FilterKind, (Pick | null)[]> = {
  text: TEXT_PICKS,
  number: NUMBER_PICKS,
  date: DATE_PICKS,
};

const KIND_LABEL: Record<FilterKind, string> = {
  text: 'Text Filters',
  number: 'Number Filters',
  date: 'Date Filters',
};

const SORT_LABELS: Record<FilterKind, [string, string]> = {
  text: ['Sort A to Z', 'Sort Z to A'],
  number: ['Sort Smallest to Largest', 'Sort Largest to Smallest'],
  date: ['Sort Oldest to Newest', 'Sort Newest to Oldest'],
};

const OP_LABELS: Record<CustomOp, string> = {
  equal: 'equals',
  notEqual: 'does not equal',
  greaterThan: 'is greater than',
  greaterThanOrEqual: 'is greater than or equal to',
  lessThan: 'is less than',
  lessThanOrEqual: 'is less than or equal to',
  beginsWith: 'begins with',
  notBeginsWith: 'does not begin with',
  endsWith: 'ends with',
  notEndsWith: 'does not end with',
  contains: 'contains',
  notContains: 'does not contain',
};

const DATE_OP_LABELS: Partial<Record<CustomOp, string>> = {
  greaterThan: 'is after',
  greaterThanOrEqual: 'is after or equal to',
  lessThan: 'is before',
  lessThanOrEqual: 'is before or equal to',
};

const opLabel = (kind: FilterKind, op: CustomOp) =>
  (kind === 'date' && DATE_OP_LABELS[op]) || OP_LABELS[op];

const testName = (label: string) =>
  label
    .replace(/…/g, '')
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-');

// ---- The component ----

const MENU_WIDTH = 270;
const ITEM_HEIGHT = 24;

type Dialog =
  | { kind: 'custom'; first: CustomCondition; second: CustomCondition; and: boolean }
  | { kind: 'top10'; top: boolean; value: string; percent: boolean };

const panelStyle: CSSProperties = {
  color: 'var(--cg-text)',
  fontSize: 'var(--cg-font-size-sm)',
  background: 'var(--cg-surface)',
  border: 'var(--cg-border-style)',
  borderRadius: 'var(--cg-radius-sm)',
  boxShadow: '0 2px 8px rgb(0 0 0 / 25%)',
};

export function FilterMenu(props: FilterMenuProps) {
  const { sheet, table, area, col } = props;
  const [list, setList] = useState<FilterList | null>(null);
  const [heading, setHeading] = useState('');
  const [ticks, setTicksState] = useState<Ticks>({
    values: new Set(),
    dates: new Set(),
    blanks: false,
  });
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<Ticks | null>(null);
  const [addToFilter, setAddToFilter] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [sub, setSub] = useState<{ left: number; top: number } | null>(null);
  const [hot, setHot] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [message, setMessage] = useState('');
  const panel = useRef<HTMLDivElement>(null);
  const kindRow = useRef<HTMLDivElement>(null);
  const close = useRef(props.onClose);
  close.current = props.onClose;

  useEffect(() => {
    const engine = props.engine();
    if (!engine) return;
    let live = true;
    engine
      .query({ kind: 'filterList', sheet, table, col })
      .then((answer) => {
        if (!live) return;
        setList(answer);
        setTicksState(initialTicks(answer));
      })
      .catch((error: unknown) => {
        if (live) setMessage(error instanceof Error ? error.message : String(error));
      });
    engine
      .cellText(sheet, area.top, col)
      .then((text) => {
        if (live) setHeading(text.display);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
    // The engine getter is read once, when the menu opens.
  }, [sheet, table, col, area.top]);

  // Escape and a click outside close the menu. The dialogs handle their own.
  useEffect(() => {
    if (dialog) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      if (sub) setSub(null);
      else close.current();
    };
    const onDown = (event: PointerEvent) => {
      if (panel.current && !panel.current.contains(event.target as Node)) close.current();
    };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('pointerdown', onDown, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('pointerdown', onDown, true);
    };
  }, [dialog, sub]);

  const kind: FilterKind = list ? filterKind(list) : 'text';
  const searching = query.trim() !== '';
  const visible = list ? visibleList(list, query) : null;
  const shown = found ?? ticks;
  const setShown = (next: Ticks) => (found ? setFound(next) : setTicksState(next));
  const name = heading || `Column ${columnName(col - 1)}`;
  const tableField = table ? { table } : {};

  const apply = async (command: Command) => {
    try {
      await props.run(command);
      close.current();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const sort = (descending: boolean) =>
    apply({
      kind: 'sort',
      sheet,
      area: { ...area, top: area.top + 1 },
      keys: [{ index: col, descending }],
      header: false,
    });

  const setFilter = (criteria?: FilterCriteria) =>
    apply({ kind: 'setFilter', sheet, ...tableField, col, ...(criteria ? { criteria } : {}) });

  const ok = () => {
    if (!list || !visible || noneTicked(visible, shown)) return;
    void setFilter(valuesCriteria(list, finalTicks(ticks, found, addToFilter)));
  };

  const search = (text: string) => {
    setQuery(text);
    setFound(list && text.trim() ? searchTicks(list, text) : null);
  };

  const pick = (choice: Pick) => {
    setSub(null);
    setMessage('');
    if ('dynamic' in choice) void setFilter({ kind: 'dynamic', name: choice.dynamic });
    else if ('top10' in choice)
      setDialog({ kind: 'top10', top: true, value: '10', percent: false });
    else {
      const [first, second, and] = choice.custom;
      setDialog({
        kind: 'custom',
        first: { op: first, value: '' },
        second: { op: second, value: '' },
        and,
      });
    }
  };

  const openSub = () => {
    const row = kindRow.current?.getBoundingClientRect();
    if (!row) return;
    const count = PICKS[kind].length;
    const height = count * ITEM_HEIGHT + 8;
    const right = row.right + MENU_WIDTH > window.innerWidth;
    setSub({
      left: right ? row.left - MENU_WIDTH : row.right,
      top: Math.max(4, Math.min(row.top, window.innerHeight - height - 4)),
    });
  };

  if (dialog?.kind === 'custom') {
    return (
      <CustomDialog
        kind={kind}
        heading={name}
        dialog={dialog}
        list={list}
        message={message}
        onChange={setDialog}
        onOk={() => {
          const criteria = customCriteria(kind, [dialog.first, dialog.second], dialog.and);
          if (criteria) void setFilter(criteria);
          else close.current();
        }}
        onCancel={() => close.current()}
      />
    );
  }

  if (dialog?.kind === 'top10') {
    return (
      <Top10Dialog
        dialog={dialog}
        message={message}
        onChange={setDialog}
        onOk={() => {
          const result = top10Criteria(dialog.top, dialog.value, dialog.percent);
          if ('error' in result) setMessage(result.error);
          else void setFilter(result.criteria);
        }}
        onCancel={() => close.current()}
      />
    );
  }

  const left = Math.max(4, Math.min(props.anchor.left, window.innerWidth - MENU_WIDTH - 4));
  const top = props.anchor.top + props.anchor.height + 1;
  const advanced =
    list?.filtered === 'custom' || list?.filtered === 'top10' || list?.filtered === 'dynamic';

  const item = (
    key: string,
    label: ReactNode,
    onClick: (() => void) | null,
    extra: { testid?: string; checked?: boolean; more?: boolean } = {},
  ) => (
    <button
      key={key}
      type="button"
      role="menuitem"
      data-testid={extra.testid}
      disabled={!onClick}
      onClick={onClick ?? undefined}
      onMouseEnter={() => {
        setHot(key);
        if (key !== 'kind' && !key.startsWith('pick-')) setSub(null);
      }}
      onMouseLeave={() => setHot((current) => (current === key ? null : current))}
      style={{
        display: 'flex',
        alignItems: 'center',
        flex: '0 0 auto',
        inlineSize: '100%',
        minBlockSize: ITEM_HEIGHT,
        padding: '0 8px 0 0',
        color: onClick ? 'inherit' : 'var(--cg-disabled-text)',
        font: 'inherit',
        textAlign: 'start',
        background: hot === key && onClick ? 'var(--cg-hover)' : 'none',
        border: 0,
        cursor: 'default',
      }}
    >
      <span style={{ inlineSize: 24, flex: '0 0 auto', textAlign: 'center' }}>
        {extra.checked ? '✓' : ''}
      </span>
      <span style={{ flex: '1 1 auto' }}>{label}</span>
      {extra.more && <span aria-hidden="true">▸</span>}
    </button>
  );

  const separator = (key: string) => (
    <div
      key={key}
      role="separator"
      style={{
        flex: '0 0 auto',
        margin: '3px 0 3px 24px',
        borderBlockStart: 'var(--cg-border-style)',
      }}
    />
  );

  const [ascLabel, descLabel] = SORT_LABELS[kind];

  return (
    <div
      ref={panel}
      role="menu"
      data-testid="filter-menu"
      style={{
        ...panelStyle,
        position: 'fixed',
        left,
        top,
        zIndex: 1000,
        display: 'flex',
        flexDirection: 'column',
        inlineSize: MENU_WIDTH,
        maxBlockSize: Math.max(240, window.innerHeight - top - 4),
        padding: '3px 0',
        boxSizing: 'border-box',
      }}
    >
      {item('asc', ascLabel, () => void sort(false), { testid: 'filter-sort-asc' })}
      {item('desc', descLabel, () => void sort(true), { testid: 'filter-sort-desc' })}
      {item('sortColor', 'Sort by Color', null, { more: true })}
      {separator('s1')}
      {item(
        'clear',
        `Clear Filter From "${name}"`,
        list && list.filtered !== 'none' ? () => void setFilter() : null,
        { testid: 'filter-clear' },
      )}
      {item('filterColor', 'Filter by Color', null, { more: true })}
      <div ref={kindRow} onMouseEnter={openSub} style={{ flex: '0 0 auto' }}>
        {item('kind', KIND_LABEL[kind], list ? () => (sub ? setSub(null) : openSub()) : null, {
          testid: 'filter-kind',
          checked: advanced,
          more: true,
        })}
        {sub && (
          <div
            role="menu"
            data-testid="filter-submenu"
            style={{
              ...panelStyle,
              position: 'fixed',
              left: sub.left,
              top: sub.top,
              zIndex: 1001,
              inlineSize: MENU_WIDTH,
              maxBlockSize: window.innerHeight - 8,
              overflowY: 'auto',
              padding: '3px 0',
              boxSizing: 'border-box',
            }}
          >
            {PICKS[kind].map((choice, index) =>
              choice
                ? item(`pick-${index}`, choice.label, () => pick(choice), {
                    testid: `filter-item-${testName(choice.label)}`,
                  })
                : separator(`pick-${index}`),
            )}
          </div>
        )}
      </div>
      <div style={{ flex: '0 0 auto', padding: '4px 8px 4px 24px' }}>
        <input
          data-testid="filter-search"
          type="search"
          placeholder={kind === 'date' ? 'Search (All)' : 'Search'}
          aria-label="Search"
          autoFocus
          value={query}
          onFocus={() => setSub(null)}
          onChange={(event) => search(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              ok();
            }
          }}
          style={{
            boxSizing: 'border-box',
            inlineSize: '100%',
            padding: '2px 4px',
            color: 'var(--cg-text)',
            font: 'inherit',
            background: 'var(--cg-surface)',
            border: 'var(--cg-border-style)',
          }}
        />
      </div>
      <div
        data-testid="filter-list"
        style={{
          flex: '1 1 auto',
          minBlockSize: 80,
          blockSize: 200,
          margin: '0 8px 0 24px',
          padding: '2px 4px',
          overflow: 'auto',
          border: 'var(--cg-border-style)',
        }}
      >
        {list && visible && (
          <CheckList
            list={list}
            visible={visible}
            ticks={shown}
            searching={searching}
            expanded={expanded}
            addToFilter={addToFilter}
            onTicks={setShown}
            onExpanded={setExpanded}
            onAddToFilter={setAddToFilter}
          />
        )}
      </div>
      {message && (
        <p
          role="alert"
          data-testid="filter-message"
          className="cgs-dialog-message"
          style={{ flex: '0 0 auto', padding: '4px 8px 0 24px', color: 'var(--cg-color-danger)' }}
        >
          {message}
        </p>
      )}
      <div className="cgs-dialog-actions" style={{ flex: '0 0 auto', padding: '6px 8px 3px' }}>
        <CgButton
          intent="primary"
          data-testid="filter-ok"
          disabled={!list || !visible || noneTicked(visible, shown)}
          onClick={ok}
        >
          OK
        </CgButton>
        <CgButton data-testid="filter-cancel" onClick={() => close.current()}>
          Cancel
        </CgButton>
      </div>
    </div>
  );
}

type CheckState = boolean | 'mixed';

function stateOf(ticked: number, total: number): CheckState {
  return ticked === 0 ? false : ticked === total ? true : 'mixed';
}

function CheckRow(props: {
  testid: string;
  label: ReactNode;
  state: CheckState;
  depth: number;
  onChange: (on: boolean) => void;
  /** The expand button for a year or a month. */
  open?: { expanded: boolean; toggle: () => void; testid: string };
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', paddingInlineStart: props.depth * 16 }}>
      <span style={{ inlineSize: 16, flex: '0 0 auto' }}>
        {props.open && (
          <button
            type="button"
            data-testid={props.open.testid}
            aria-label={props.open.expanded ? 'Collapse' : 'Expand'}
            aria-expanded={props.open.expanded}
            onClick={props.open.toggle}
            style={{
              padding: 0,
              inlineSize: 14,
              color: 'inherit',
              font: 'inherit',
              background: 'none',
              border: 0,
              cursor: 'pointer',
            }}
          >
            {props.open.expanded ? '▾' : '▸'}
          </button>
        )}
      </span>
      <label className="cgs-dialog-check" style={{ gap: 4, whiteSpace: 'nowrap' }}>
        <input
          type="checkbox"
          data-testid={props.testid}
          checked={props.state === true}
          aria-checked={props.state === 'mixed' ? 'mixed' : props.state}
          ref={(element) => {
            if (element) element.indeterminate = props.state === 'mixed';
          }}
          onChange={(event) => props.onChange(event.target.checked)}
        />
        {props.label}
      </label>
    </div>
  );
}

function CheckList(props: {
  list: FilterList;
  visible: FilterList;
  ticks: Ticks;
  searching: boolean;
  expanded: ReadonlySet<string>;
  addToFilter: boolean;
  onTicks: (ticks: Ticks) => void;
  onExpanded: (keys: ReadonlySet<string>) => void;
  onAddToFilter: (on: boolean) => void;
}) {
  const { visible, ticks } = props;
  const everything = {
    values: visible.values.map((value) => value.text),
    dates: visible.dates.map((date) => dayKey(date.date)),
    blanks: visible.blanks,
  };
  const total = everything.values.length + everything.dates.length + (visible.blanks ? 1 : 0);
  const ticked =
    everything.values.filter((value) => ticks.values.has(value)).length +
    everything.dates.filter((date) => ticks.dates.has(date)).length +
    (visible.blanks && ticks.blanks ? 1 : 0);
  const isOpen = (key: string) => props.searching || props.expanded.has(key);
  const toggleOpen = (key: string) => {
    const next = new Set(props.expanded);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    props.onExpanded(next);
  };
  const daysState = (days: readonly Day[]) =>
    stateOf(days.filter((day) => ticks.dates.has(dayKey(day))).length, days.length);
  const tickDays = (days: readonly Day[], on: boolean) =>
    props.onTicks(setTicks(ticks, { dates: days.map(dayKey) }, on));

  const rows: ReactNode[] = [];
  rows.push(
    <CheckRow
      key="all"
      testid="filter-select-all"
      label={props.searching ? '(Select All Search Results)' : '(Select All)'}
      state={stateOf(ticked, total)}
      depth={0}
      onChange={(on) => props.onTicks(setTicks(ticks, everything, on))}
    />,
  );
  if (props.searching) {
    rows.push(
      <CheckRow
        key="add"
        testid="filter-add-current"
        label="Add current selection to filter"
        state={props.addToFilter}
        depth={0}
        onChange={props.onAddToFilter}
      />,
    );
  }
  for (const year of dateTree(visible.dates.map((date) => date.date))) {
    const yearKey = String(year.year);
    const yearDays = year.months.flatMap((month) => month.days);
    rows.push(
      <CheckRow
        key={yearKey}
        testid={`filter-value-${yearKey}`}
        label={yearKey}
        state={daysState(yearDays)}
        depth={0}
        onChange={(on) => tickDays(yearDays, on)}
        open={{
          expanded: isOpen(yearKey),
          toggle: () => toggleOpen(yearKey),
          testid: `filter-expand-${yearKey}`,
        }}
      />,
    );
    if (!isOpen(yearKey)) continue;
    for (const month of year.months) {
      const monthKey = `${year.year}-${month.month}`;
      rows.push(
        <CheckRow
          key={monthKey}
          testid={`filter-value-${monthKey}`}
          label={MONTHS[month.month - 1]}
          state={daysState(month.days)}
          depth={1}
          onChange={(on) => tickDays(month.days, on)}
          open={{
            expanded: isOpen(monthKey),
            toggle: () => toggleOpen(monthKey),
            testid: `filter-expand-${monthKey}`,
          }}
        />,
      );
      if (!isOpen(monthKey)) continue;
      for (const day of month.days) {
        const key = dayKey(day);
        rows.push(
          <CheckRow
            key={key}
            testid={`filter-value-${key}`}
            label={pad(day[2])}
            state={ticks.dates.has(key)}
            depth={2}
            onChange={(on) => tickDays([day], on)}
          />,
        );
      }
    }
  }
  for (const value of visible.values) {
    rows.push(
      <CheckRow
        key={'v:' + value.text}
        testid={`filter-value-${value.text}`}
        label={value.text}
        state={ticks.values.has(value.text)}
        depth={0}
        onChange={(on) => props.onTicks(setTicks(ticks, { values: [value.text] }, on))}
      />,
    );
  }
  if (visible.blanks) {
    rows.push(
      <CheckRow
        key="blanks"
        testid="filter-value-(Blanks)"
        label="(Blanks)"
        state={ticks.blanks}
        depth={0}
        onChange={(on) => props.onTicks(setTicks(ticks, { blanks: true }, on))}
      />,
    );
  }
  if (props.searching && total === 0) {
    rows.push(
      <p key="none" className="cgs-dialog-message" style={{ padding: 4 }}>
        No matches
      </p>,
    );
  }
  return <>{rows}</>;
}

const fieldStyle: CSSProperties = {
  boxSizing: 'border-box',
  padding: '2px 4px',
  color: 'var(--cg-text)',
  font: 'inherit',
  background: 'var(--cg-surface)',
  border: 'var(--cg-border-style)',
};

function CustomDialog(props: {
  kind: FilterKind;
  heading: string;
  dialog: Extract<Dialog, { kind: 'custom' }>;
  list: FilterList | null;
  message: string;
  onChange: (dialog: Dialog) => void;
  onOk: () => void;
  onCancel: () => void;
}) {
  const { dialog, kind } = props;
  const options = props.list
    ? [
        ...props.list.dates.map((date) => dayText(date.date)),
        ...props.list.values.map((value) => value.text),
      ]
    : [];
  const ops = Object.keys(OP_LABELS) as CustomOp[];
  const row = (which: 'first' | 'second', index: number) => {
    const condition = dialog[which];
    const change = (next: Partial<CustomCondition>) =>
      props.onChange({ ...dialog, [which]: { ...condition, ...next } });
    return (
      <div className="cgs-dialog-row" style={{ flexWrap: 'nowrap' }}>
        <select
          data-testid={`custom-op-${index}`}
          aria-label={`Operator ${index}`}
          value={condition.op}
          onChange={(event) => change({ op: event.target.value as CustomOp | '' })}
          style={{ ...fieldStyle, inlineSize: 200 }}
        >
          {index === 2 && <option value="" />}
          {ops.map((op) => (
            <option key={op} value={op}>
              {opLabel(kind, op)}
            </option>
          ))}
        </select>
        <input
          data-testid={`custom-value-${index}`}
          aria-label={`Value ${index}`}
          list="cgs-filter-values"
          autoFocus={index === 1}
          value={condition.value}
          onChange={(event) => change({ value: event.target.value })}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              props.onOk();
            }
          }}
          style={{ ...fieldStyle, flex: '1 1 auto', minInlineSize: 0 }}
        />
      </div>
    );
  };
  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) props.onCancel();
      }}
      headerText="Custom AutoFilter"
      width={480}
      allowDrag
      data-testid="custom-filter-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Show rows where:</span>
          <span style={{ paddingInlineStart: 8 }}>{props.heading}</span>
          {row('first', 1)}
          <div className="cgs-dialog-row" style={{ paddingInlineStart: 24 }}>
            <label className="cgs-dialog-check">
              <input
                type="radio"
                name="cgs-filter-join"
                data-testid="custom-and"
                checked={dialog.and}
                onChange={() => props.onChange({ ...dialog, and: true })}
              />
              And
            </label>
            <label className="cgs-dialog-check">
              <input
                type="radio"
                name="cgs-filter-join"
                data-testid="custom-or"
                checked={!dialog.and}
                onChange={() => props.onChange({ ...dialog, and: false })}
              />
              Or
            </label>
          </div>
          {row('second', 2)}
          <datalist id="cgs-filter-values">
            {options.map((option) => (
              <option key={option} value={option} />
            ))}
          </datalist>
          <span>Use ? to represent any single character</span>
          <span>Use * to represent any series of characters</span>
          {props.message && (
            <p role="alert" data-testid="custom-message" className="cgs-dialog-message">
              {props.message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="custom-ok" onClick={props.onOk}>
            OK
          </CgButton>
          <CgButton data-testid="custom-cancel" onClick={props.onCancel}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

function Top10Dialog(props: {
  dialog: Extract<Dialog, { kind: 'top10' }>;
  message: string;
  onChange: (dialog: Dialog) => void;
  onOk: () => void;
  onCancel: () => void;
}) {
  const { dialog } = props;
  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) props.onCancel();
      }}
      headerText="Top 10 AutoFilter"
      width={360}
      allowDrag
      data-testid="top10-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Show</span>
          <div className="cgs-dialog-row" style={{ flexWrap: 'nowrap' }}>
            <select
              data-testid="top10-top"
              aria-label="Top or bottom"
              value={dialog.top ? 'top' : 'bottom'}
              onChange={(event) => props.onChange({ ...dialog, top: event.target.value === 'top' })}
              style={fieldStyle}
            >
              <option value="top">Top</option>
              <option value="bottom">Bottom</option>
            </select>
            <input
              data-testid="top10-value"
              aria-label="How many"
              type="number"
              min={1}
              max={dialog.percent ? 100 : 500}
              autoFocus
              value={dialog.value}
              onChange={(event) => props.onChange({ ...dialog, value: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  props.onOk();
                }
              }}
              style={{ ...fieldStyle, inlineSize: 72 }}
            />
            <select
              data-testid="top10-percent"
              aria-label="Items or percent"
              value={dialog.percent ? 'percent' : 'items'}
              onChange={(event) =>
                props.onChange({ ...dialog, percent: event.target.value === 'percent' })
              }
              style={fieldStyle}
            >
              <option value="items">Items</option>
              <option value="percent">Percent</option>
            </select>
          </div>
          {props.message && (
            <p role="alert" data-testid="top10-message" className="cgs-dialog-message">
              {props.message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="top10-ok" onClick={props.onOk}>
            OK
          </CgButton>
          <CgButton data-testid="top10-cancel" onClick={props.onCancel}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
