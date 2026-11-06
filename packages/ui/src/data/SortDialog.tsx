// Data > Sort: levels of keys, each a column (or a row, left to right), what to sort on and the
// order. OK sends one `sort` command; the engine does the sorting.
import { useEffect, useRef, useState } from 'react';
import {
  CgButton,
  CgColorPicker,
  CgDropDownButton,
  CgWindow,
  type CgColorPickerValue,
} from '@cashgear/ui';
import type { Applied, CellArea, Command, EngineClient, SortKey } from '@cgs/engine-client';
import { columnName } from '../grid/geometry';

/** As many levels as Excel allows. */
export const MAX_LEVELS = 64;

/** Excel's built-in custom lists, as its Custom Lists dialog shows them. */
export const CUSTOM_LISTS: readonly (readonly string[])[] = [
  ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'],
  ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'],
  ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'],
  [
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
  ],
];

/** A list as the Order box shows it, reversed for a descending sort. */
export function listLabel(list: readonly string[], descending = false) {
  return (descending ? [...list].reverse() : list).join(', ');
}

/** A new list's entries as typed: one a line or split by commas, blanks dropped. */
export function parseListEntries(text: string): string[] {
  return text
    .split(/[\r\n,]+/)
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '');
}

export type SortOnKind = 'value' | 'cellColor' | 'fontColor';

/** One row of the dialog. */
export interface SortLevel {
  id: number;
  /** The sheet's column, or row for left to right, 1-based; null until one is picked. */
  index: number | null;
  on: SortOnKind;
  descending: boolean;
  /** `RRGGBB`, or null for no fill or the automatic font colour. */
  color: string | null;
  list: string[] | null;
}

let nextId = 1;

export function newLevel(from?: SortLevel): SortLevel {
  const id = nextId++;
  if (from) return { ...from, id, list: from.list && [...from.list] };
  return { id, index: null, on: 'value', descending: false, color: null, list: null };
}

/** What the Column box calls a line: its heading, or `Column B` or `Row 3`. */
export function lineLabel(index: number, byColumns: boolean, heading?: string) {
  if (byColumns) return `Row ${index}`;
  return heading && heading.trim() !== '' ? heading : `Column ${columnName(index - 1)}`;
}

export function levelKey(level: SortLevel & { index: number }): SortKey {
  const key: SortKey = { index: level.index, descending: level.descending };
  if (level.on === 'value') {
    key.by = { kind: 'value' };
    if (level.list) key.list = [...level.list];
  } else {
    key.by = { kind: level.on, color: level.color };
  }
  return key;
}

export interface SortSettings {
  sheet: number;
  area: CellArea;
  headers: boolean;
  caseSensitive: boolean;
  byColumns: boolean;
  levels: SortLevel[];
  /** What the Column box shows for a line, for the messages. */
  label: (index: number) => string;
}

/** The command OK runs, or what Excel would say is wrong with the levels. */
export function sortCommand(settings: SortSettings): { command: Command } | { error: string } {
  const keys: SortKey[] = [];
  const seen = new Set<string>();
  for (const level of settings.levels) {
    if (level.index === null) {
      return {
        error:
          'All sort criteria must have a column specified. Check the selected sort criteria and try again.',
      };
    }
    const what = level.on === 'value' ? 'value' : `${level.on}:${level.color ?? ''}`;
    const twice = `${level.index}|${what}`;
    if (seen.has(twice)) {
      const by =
        level.on === 'value'
          ? 'values'
          : level.on === 'cellColor'
            ? 'the same cell color'
            : 'the same font color';
      return {
        error: `${settings.label(level.index)} is being sorted by ${by} more than once. Delete the duplicate sort criteria and try again.`,
      };
    }
    seen.add(twice);
    keys.push(levelKey({ ...level, index: level.index }));
  }
  return {
    command: {
      kind: 'sort',
      sheet: settings.sheet,
      area: settings.area,
      // Headings are a top row only; left to right has none.
      header: settings.headers && !settings.byColumns,
      caseSensitive: settings.caseSensitive,
      byColumns: settings.byColumns,
      keys,
    },
  };
}

/** The picker's colour as a key takes it. */
export function keyColor(value: CgColorPickerValue): string | null {
  return 'color' in value ? value.color.replace('#', '').toUpperCase() : null;
}

/** Headings are read for at most this many columns; the rest are named by letter. */
const HEADINGS_READ = 1024;

const ORDER_ASC = 'asc';
const ORDER_DESC = 'desc';
const ORDER_LIST_ASC = 'list-asc';
const ORDER_LIST_DESC = 'list-desc';
const ORDER_CUSTOM = 'custom';

export interface SortDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  /** The area to sort, 1-based, already grown to the current region by the caller. */
  area: CellArea;
  /** Whether the first row reads as headings (the caller asks `guessTable`). */
  headers: boolean;
  run: (command: Command) => Promise<Applied>;
  onClose: () => void;
}

export function SortDialog(props: SortDialogProps) {
  const { area } = props;
  const [levels, setLevels] = useState<SortLevel[]>(() => [newLevel()]);
  const [current, setCurrent] = useState(0);
  const [headers, setHeaders] = useState(props.headers);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [byColumns, setByColumns] = useState(false);
  const [headings, setHeadings] = useState<Map<number, string>>(new Map());
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const [options, setOptions] = useState<{ caseSensitive: boolean; byColumns: boolean } | null>(
    null,
  );
  // The Custom Lists dialog: which level it's for, the list picked (-1 for NEW LIST) and its text.
  const [lists, setLists] = useState<{ level: number; pick: number; text: string } | null>(null);
  const userLists = useRef<string[][]>([]);

  // Each opening starts afresh.
  useEffect(() => {
    if (!props.open) return;
    setLevels([newLevel()]);
    setCurrent(0);
    setHeaders(props.headers);
    setCaseSensitive(false);
    setByColumns(false);
    setMessage('');
    setOptions(null);
    setLists(null);
  }, [props.open, props.headers]);

  // The top row's text, for the Column boxes.
  useEffect(() => {
    if (!props.open) return;
    const engine = props.engine();
    if (!engine) return;
    let live = true;
    const last = Math.min(area.right, area.left + HEADINGS_READ - 1);
    const cols: number[] = [];
    for (let col = area.left; col <= last; col++) cols.push(col);
    void Promise.all(
      cols.map((col) =>
        engine.cellText(props.sheet, area.top, col).then(
          (text) => [col, text.display] as const,
          () => [col, ''] as const,
        ),
      ),
    ).then((read) => {
      if (live) setHeadings(new Map(read));
    });
    return () => {
      live = false;
    };
  }, [props.open, props.sheet, area.top, area.left, area.right]);

  const label = (index: number) =>
    lineLabel(index, byColumns, headers && !byColumns ? headings.get(index) : undefined);

  const lines: number[] = [];
  const [first, end] = byColumns ? [area.top, area.bottom] : [area.left, area.right];
  for (let line = first; line <= end; line++) lines.push(line);

  const change = (at: number, patch: Partial<SortLevel>) => {
    setLevels((all) => all.map((level, i) => (i === at ? { ...level, ...patch } : level)));
    setMessage('');
  };

  const addLevel = () => {
    if (levels.length >= MAX_LEVELS) return;
    const at = Math.min(current + 1, levels.length);
    setLevels((all) => [...all.slice(0, at), newLevel(), ...all.slice(at)]);
    setCurrent(at);
  };

  const deleteLevel = () => {
    if (levels.length === 0) return;
    setLevels((all) => all.filter((_, i) => i !== current));
    setCurrent((at) => Math.max(0, Math.min(at, levels.length - 2)));
  };

  const copyLevel = () => {
    const level = levels[current];
    if (!level || levels.length >= MAX_LEVELS) return;
    setLevels((all) => [...all.slice(0, current + 1), newLevel(level), ...all.slice(current + 1)]);
    setCurrent(current + 1);
  };

  const move = (by: -1 | 1) => {
    const to = current + by;
    if (to < 0 || to >= levels.length) return;
    setLevels((all) => {
      const next = [...all];
      [next[current], next[to]] = [next[to], next[current]];
      return next;
    });
    setCurrent(to);
  };

  const ok = async () => {
    const built = sortCommand({
      sheet: props.sheet,
      area,
      headers,
      caseSensitive,
      byColumns,
      levels,
      label,
    });
    if ('error' in built) {
      setMessage(built.error);
      return;
    }
    // No levels sorts nothing, as in Excel.
    if (levels.length === 0) {
      props.onClose();
      return;
    }
    setBusy(true);
    try {
      await props.run(built.command);
      props.onClose();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const orderOf = (level: SortLevel) =>
    level.list
      ? level.descending
        ? ORDER_LIST_DESC
        : ORDER_LIST_ASC
      : level.descending
        ? ORDER_DESC
        : ORDER_ASC;

  const pickOrder = (at: number, level: SortLevel, value: string) => {
    if (value === ORDER_CUSTOM) {
      const known = [...CUSTOM_LISTS, ...userLists.current];
      const pick = level.list
        ? known.findIndex((list) => listLabel(list) === listLabel(level.list ?? []))
        : 0;
      setLists({ level: at, pick, text: (known[pick] ?? level.list ?? []).join('\n') });
      return;
    }
    if (value === ORDER_ASC || value === ORDER_DESC) {
      change(at, { list: null, descending: value === ORDER_DESC });
    } else {
      change(at, { descending: value === ORDER_LIST_DESC });
    }
  };

  const grid = { display: 'grid', gridTemplateColumns: '5rem 1fr 1fr 1.4fr', gap: 4 } as const;

  const levelRow = (level: SortLevel, at: number) => {
    const picked = at === current;
    const colour = level.on !== 'value';
    return (
      <div
        key={level.id}
        data-testid={`sort-level-${at}`}
        aria-selected={picked}
        role="row"
        onMouseDown={() => setCurrent(at)}
        style={{
          ...grid,
          alignItems: 'center',
          padding: 2,
          background: picked ? 'var(--cg-hover)' : undefined,
        }}
      >
        <span>{at === 0 ? 'Sort by' : 'Then by'}</span>
        <select
          data-testid={`sort-column-${at}`}
          aria-label="Column"
          value={level.index ?? ''}
          onFocus={() => setCurrent(at)}
          onChange={(event) =>
            change(at, { index: event.target.value === '' ? null : Number(event.target.value) })
          }
        >
          {level.index === null && <option value="" />}
          {lines.map((line) => (
            <option key={line} value={line}>
              {label(line)}
            </option>
          ))}
        </select>
        <select
          data-testid={`sort-on-${at}`}
          aria-label="Sort On"
          value={level.on}
          onFocus={() => setCurrent(at)}
          onChange={(event) =>
            change(at, {
              on: event.target.value as SortOnKind,
              descending: false,
              color: null,
              list: null,
            })
          }
        >
          <option value="value">Values</option>
          <option value="cellColor">Cell Color</option>
          <option value="fontColor">Font Color</option>
        </select>
        {colour ? (
          <div className="cgs-dialog-row" style={{ flexWrap: 'nowrap', alignItems: 'center' }}>
            <CgDropDownButton
              data-testid={`sort-color-${at}`}
              icon={
                <span
                  className="cgs-color-chip"
                  style={{ background: level.color ? '#' + level.color : 'transparent' }}
                />
              }
              renderFlyout={({ close }) => (
                <CgColorPicker
                  value={
                    level.color
                      ? { kind: 'rgb', color: '#' + level.color.toLowerCase() }
                      : { kind: 'none' }
                  }
                  allowNone
                  allowCustomColor
                  labels={{
                    none: level.on === 'cellColor' ? 'No Cell Color' : 'Automatic',
                  }}
                  onValueChange={(next) => {
                    change(at, { color: keyColor(next) });
                    close();
                  }}
                />
              )}
            >
              {level.color
                ? '#' + level.color
                : level.on === 'cellColor'
                  ? 'No Cell Color'
                  : 'Automatic'}
            </CgDropDownButton>
            <select
              data-testid={`sort-order-${at}`}
              aria-label="Order"
              value={level.descending ? 'bottom' : 'top'}
              onFocus={() => setCurrent(at)}
              onChange={(event) => change(at, { descending: event.target.value === 'bottom' })}
            >
              <option value="top">On Top</option>
              <option value="bottom">On Bottom</option>
            </select>
          </div>
        ) : (
          <select
            data-testid={`sort-order-${at}`}
            aria-label="Order"
            value={orderOf(level)}
            onFocus={() => setCurrent(at)}
            onChange={(event) => pickOrder(at, level, event.target.value)}
          >
            <option value={ORDER_ASC}>A to Z</option>
            <option value={ORDER_DESC}>Z to A</option>
            {level.list && (
              <>
                <option value={ORDER_LIST_ASC}>{listLabel(level.list)}</option>
                <option value={ORDER_LIST_DESC}>{listLabel(level.list, true)}</option>
              </>
            )}
            <option value={ORDER_CUSTOM}>Custom List...</option>
          </select>
        )}
      </div>
    );
  };

  const known = [...CUSTOM_LISTS, ...userLists.current];

  const finishLists = () => {
    if (!lists) return;
    let list: string[];
    if (lists.pick < 0) {
      list = parseListEntries(lists.text);
      if (list.length === 0) {
        setLists(null);
        return;
      }
      userLists.current = [...userLists.current, list];
    } else {
      list = [...(known[lists.pick] ?? [])];
    }
    change(lists.level, { list, descending: false });
    setLists(null);
  };

  return (
    <>
      <CgWindow
        open={props.open}
        onOpenChange={(next) => {
          if (!next) props.onClose();
        }}
        headerText="Sort"
        width={640}
        allowDrag
        data-testid="sort-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <div className="cgs-dialog-row" style={{ alignItems: 'center' }}>
              <CgButton
                data-testid="sort-add"
                disabled={levels.length >= MAX_LEVELS}
                onClick={addLevel}
              >
                Add Level
              </CgButton>
              <CgButton
                data-testid="sort-delete"
                disabled={levels.length === 0}
                onClick={deleteLevel}
              >
                Delete Level
              </CgButton>
              <CgButton
                data-testid="sort-copy"
                disabled={levels.length === 0 || levels.length >= MAX_LEVELS}
                onClick={copyLevel}
              >
                Copy Level
              </CgButton>
              <CgButton
                data-testid="sort-up"
                aria-label="Move Up"
                title="Move Up"
                disabled={current <= 0}
                onClick={() => move(-1)}
              >
                ▲
              </CgButton>
              <CgButton
                data-testid="sort-down"
                aria-label="Move Down"
                title="Move Down"
                disabled={current >= levels.length - 1}
                onClick={() => move(1)}
              >
                ▼
              </CgButton>
              <CgButton
                data-testid="sort-options"
                onClick={() => setOptions({ caseSensitive, byColumns })}
              >
                Options...
              </CgButton>
              <label className="cgs-dialog-check" style={{ marginInlineStart: 'auto' }}>
                <input
                  type="checkbox"
                  data-testid="sort-headers"
                  checked={headers && !byColumns}
                  disabled={byColumns}
                  onChange={(event) => setHeaders(event.target.checked)}
                />
                My data has headers
              </label>
            </div>
            <div role="grid" aria-label="Sort levels" style={{ border: 'var(--cg-border-style)' }}>
              <div role="row" style={{ ...grid, padding: 2, fontWeight: 600 }}>
                <span />
                <span>{byColumns ? 'Row' : 'Column'}</span>
                <span>Sort On</span>
                <span>Order</span>
              </div>
              <div style={{ minBlockSize: '10rem', maxBlockSize: '16rem', overflowY: 'auto' }}>
                {levels.map(levelRow)}
              </div>
            </div>
            {message && (
              <p role="alert" data-testid="sort-message" className="cgs-dialog-message">
                {message}
              </p>
            )}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton
              intent="primary"
              data-testid="sort-ok"
              disabled={busy}
              onClick={() => void ok()}
            >
              OK
            </CgButton>
            <CgButton data-testid="sort-cancel" onClick={props.onClose}>
              Cancel
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={props.open && options !== null}
        onOpenChange={(next) => {
          if (!next) setOptions(null);
        }}
        headerText="Sort Options"
        width={280}
        data-testid="sort-options-dialog"
        renderBody={() =>
          options && (
            <div className="cgs-dialog-column">
              <label className="cgs-dialog-check">
                <input
                  type="checkbox"
                  data-testid="sort-case"
                  checked={options.caseSensitive}
                  onChange={(event) =>
                    setOptions({ ...options, caseSensitive: event.target.checked })
                  }
                />
                Case sensitive
              </label>
              <fieldset className="cgs-dialog-column" style={{ margin: 0 }}>
                <legend>Orientation</legend>
                <label className="cgs-dialog-check">
                  <input
                    type="radio"
                    name="sort-orientation"
                    data-testid="sort-top-to-bottom"
                    checked={!options.byColumns}
                    onChange={() => setOptions({ ...options, byColumns: false })}
                  />
                  Sort top to bottom
                </label>
                <label className="cgs-dialog-check">
                  <input
                    type="radio"
                    name="sort-orientation"
                    data-testid="sort-left-to-right"
                    checked={options.byColumns}
                    onChange={() => setOptions({ ...options, byColumns: true })}
                  />
                  Sort left to right
                </label>
              </fieldset>
            </div>
          )
        }
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton
              intent="primary"
              data-testid="sort-options-ok"
              onClick={() => {
                if (!options) return;
                setCaseSensitive(options.caseSensitive);
                // A new orientation means new lines, so the levels start again, as in Excel.
                if (options.byColumns !== byColumns) {
                  setByColumns(options.byColumns);
                  setLevels([newLevel()]);
                  setCurrent(0);
                  setMessage('');
                }
                setOptions(null);
              }}
            >
              OK
            </CgButton>
            <CgButton data-testid="sort-options-cancel" onClick={() => setOptions(null)}>
              Cancel
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={props.open && lists !== null}
        onOpenChange={(next) => {
          if (!next) setLists(null);
        }}
        headerText="Custom Lists"
        width={480}
        data-testid="sort-lists-dialog"
        renderBody={() =>
          lists && (
            <div className="cgs-dialog-row" style={{ alignItems: 'stretch', flexWrap: 'nowrap' }}>
              <label className="cgs-dialog-field cgs-dialog-grow">
                <span>Custom lists:</span>
                <select
                  data-testid="sort-lists"
                  size={8}
                  value={lists.pick}
                  onChange={(event) => {
                    const pick = Number(event.target.value);
                    setLists({ ...lists, pick, text: (known[pick] ?? []).join('\n') });
                  }}
                >
                  <option value={-1}>NEW LIST</option>
                  {known.map((list, i) => (
                    <option key={i} value={i}>
                      {listLabel(list)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cgs-dialog-field cgs-dialog-grow">
                <span>List entries:</span>
                <textarea
                  data-testid="sort-list-entries"
                  rows={8}
                  value={lists.text}
                  readOnly={lists.pick >= 0}
                  onChange={(event) => setLists({ ...lists, text: event.target.value })}
                />
              </label>
            </div>
          )
        }
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton intent="primary" data-testid="sort-lists-ok" onClick={finishLists}>
              OK
            </CgButton>
            <CgButton data-testid="sort-lists-cancel" onClick={() => setLists(null)}>
              Cancel
            </CgButton>
          </div>
        )}
      />
    </>
  );
}
