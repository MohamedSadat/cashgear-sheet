// Pivot table dialogs as Excel 2016 has them: Create PivotTable, Value Field Settings, PivotTable
// Options, and the filter menu of a field. Each sends one command; the engine does the work.
import { useEffect, useMemo, useState } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  PivotInfo,
  PivotShowAs,
  PivotSummary,
  PivotValueSpec,
} from '@cgs/engine-client';
import { quoteSheet } from '../formula-text';
import { absoluteRange } from '../data/SmallDialogs';

type Run = (command: Command) => Promise<Applied>;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

export const SUMMARIES: { key: PivotSummary; text: string }[] = [
  { key: 'sum', text: 'Sum' },
  { key: 'count', text: 'Count' },
  { key: 'average', text: 'Average' },
  { key: 'max', text: 'Max' },
  { key: 'min', text: 'Min' },
  { key: 'product', text: 'Product' },
  { key: 'countNums', text: 'Count Numbers' },
  { key: 'stdDev', text: 'StdDev' },
  { key: 'stdDevp', text: 'StdDevp' },
  { key: 'var', text: 'Var' },
  { key: 'varp', text: 'Varp' },
];

/** Excel 2016's Show Values As list, in its order, and whether each needs a base field or item. */
export const SHOW_AS: { key: PivotShowAs; text: string; field?: boolean; item?: boolean }[] = [
  { key: 'normal', text: 'No Calculation' },
  { key: 'percentOfTotal', text: '% of Grand Total' },
  { key: 'percentOfCol', text: '% of Column Total' },
  { key: 'percentOfRow', text: '% of Row Total' },
  { key: 'percent', text: '% Of', field: true, item: true },
  { key: 'percentOfParentRow', text: '% of Parent Row Total' },
  { key: 'percentOfParentCol', text: '% of Parent Column Total' },
  { key: 'percentOfParent', text: '% of Parent Total', field: true },
  { key: 'difference', text: 'Difference From', field: true, item: true },
  { key: 'percentDiff', text: '% Difference From', field: true, item: true },
  { key: 'runTotal', text: 'Running Total In', field: true },
  { key: 'percentOfRunningTotal', text: '% Running Total In', field: true },
  { key: 'rankAscending', text: 'Rank Smallest to Largest', field: true },
  { key: 'rankDescending', text: 'Rank Largest to Smallest', field: true },
  { key: 'index', text: 'Index' },
];

const SUMMARY_WORD: Record<PivotSummary, string> = {
  sum: 'Sum',
  count: 'Count',
  average: 'Average',
  max: 'Max',
  min: 'Min',
  product: 'Product',
  countNums: 'Count',
  stdDev: 'StdDev',
  stdDevp: 'StdDevp',
  var: 'Var',
  varp: 'Varp',
};

/** Excel's own name for a value field: "Sum of Sales". */
export function valueName(summary: PivotSummary, field: string) {
  return `${SUMMARY_WORD[summary]} of ${field}`;
}

// Create PivotTable

export interface CreatePivotDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  sheetName: string;
  area: CellArea;
  run: Run;
  onClose: () => void;
  /** Called with the new pivot table's sheet once it's made. */
  onMade: (sheet: number, row: number, col: number) => void;
}

export function CreatePivotDialog(props: CreatePivotDialogProps) {
  const { open, area, sheetName } = props;
  const start = () => `${quoteSheet(sheetName)}!${absoluteRange(area)}`;
  const [range, setRange] = useState(start);
  const [where, setWhere] = useState<'new' | 'existing'>('new');
  const [place, setPlace] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) return;
    setRange(start());
    setWhere('new');
    setPlace('');
    setError('');
    // Start again each time the dialog opens.
  }, [open]);

  const ok = async () => {
    const engine = props.engine();
    if (!engine) return;
    const found = await engine.resolveName(range.trim().replace(/^=/, ''), props.sheet);
    if (!found) return setError('The PivotTable source reference isn’t valid.');
    const { sheet, ...source } = found;
    let to: Extract<Command, { kind: 'createPivot' }>['to'] = { kind: 'newSheet' };
    if (where === 'existing') {
      const at = place.trim()
        ? await engine.resolveName(place.trim().replace(/^=/, ''), props.sheet)
        : null;
      if (!at) return setError('The destination reference isn’t valid.');
      to = { kind: 'existing', sheet: at.sheet, row: at.top, col: at.left };
    }
    try {
      await props.run({ kind: 'createPivot', sheet, area: source, to });
      props.onClose();
      if (to.kind === 'existing') props.onMade(to.sheet, to.row, to.col);
      else props.onMade(-1, 3, 1);
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Create PivotTable"
      width={380}
      allowDrag
      data-testid="pivot-create-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Choose the data that you want to analyze</span>
          <label className="cgs-dialog-field">
            <span>Table/Range:</span>
            <input
              data-testid="pivot-range"
              autoFocus
              value={range}
              onChange={(event) => setRange(event.target.value)}
            />
          </label>
          <span>Choose where you want the PivotTable report to be placed</span>
          <label className="cgs-dialog-check">
            <input
              type="radio"
              name="pivot-where"
              data-testid="pivot-new-sheet"
              checked={where === 'new'}
              onChange={() => setWhere('new')}
            />
            New Worksheet
          </label>
          <label className="cgs-dialog-check">
            <input
              type="radio"
              name="pivot-where"
              data-testid="pivot-existing-sheet"
              checked={where === 'existing'}
              onChange={() => setWhere('existing')}
            />
            Existing Worksheet
          </label>
          <label className="cgs-dialog-field">
            <span>Location:</span>
            <input
              data-testid="pivot-location"
              disabled={where !== 'existing'}
              value={place}
              onChange={(event) => setPlace(event.target.value)}
            />
          </label>
          {error && (
            <p role="alert" data-testid="pivot-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="pivot-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton data-testid="pivot-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

// Value Field Settings

export interface ValueFieldDialogProps {
  open: boolean;
  info: PivotInfo;
  /** Which value field, by its place in the Values area. */
  index: number;
  onApply: (spec: PivotValueSpec) => void;
  onClose: () => void;
}

export function ValueFieldDialog(props: ValueFieldDialogProps) {
  const { open, info, index } = props;
  const value = info.values[index];
  const field = value ? info.fields[value.field] : undefined;
  const [tab, setTab] = useState<'summarize' | 'show'>('summarize');
  const [summary, setSummary] = useState<PivotSummary>(value?.summary ?? 'sum');
  const [name, setName] = useState(value?.name ?? '');
  const [showAs, setShowAs] = useState<PivotShowAs>(value?.showAs ?? 'normal');
  const [baseField, setBaseField] = useState(value?.baseField ?? 0);
  const [baseItem, setBaseItem] = useState<number | 'previous' | 'next'>(value?.baseItem ?? 0);
  const [format, setFormat] = useState(value?.numberFormat ?? '');
  // The name follows the summary until it is typed over, as in Excel.
  const [named, setNamed] = useState(false);

  useEffect(() => {
    if (!open || !value) return;
    setTab('summarize');
    setSummary(value.summary);
    setName(value.name);
    setShowAs(value.showAs);
    setBaseField(value.baseField);
    setBaseItem(value.baseItem);
    setFormat(value.numberFormat ?? '');
    setNamed(false);
    // Start again each time the dialog opens.
  }, [open]);

  const kind = SHOW_AS.find((entry) => entry.key === showAs) ?? SHOW_AS[0]!;
  const baseFields = useMemo(
    () => info.fields.filter((f) => f.axis === 'row' || f.axis === 'col'),
    [info.fields],
  );
  const items = info.fields[baseField]?.items ?? [];
  if (!value || !field) return null;

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Value Field Settings"
      width={420}
      allowDrag
      data-testid="pivot-value-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Source Name: {field.name}</span>
          <label className="cgs-dialog-field">
            <span>Custom Name:</span>
            <input
              data-testid="pivot-value-name"
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                setNamed(true);
              }}
            />
          </label>
          <div className="cgs-dialog-tabs" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected={tab === 'summarize'}
              data-testid="pivot-tab-summarize"
              onClick={() => setTab('summarize')}
            >
              Summarize Values By
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={tab === 'show'}
              data-testid="pivot-tab-show"
              onClick={() => setTab('show')}
            >
              Show Values As
            </button>
          </div>
          {tab === 'summarize' ? (
            <label className="cgs-dialog-field">
              <span>Summarize value field by</span>
              <select
                size={8}
                data-testid="pivot-summary"
                value={summary}
                onChange={(event) => {
                  const next = event.target.value as PivotSummary;
                  setSummary(next);
                  if (!named) setName(valueName(next, field.caption));
                }}
              >
                {SUMMARIES.map((entry) => (
                  <option key={entry.key} value={entry.key}>
                    {entry.text}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <div className="cgs-dialog-column">
              <label className="cgs-dialog-field">
                <span>Show values as</span>
                <select
                  data-testid="pivot-show-as"
                  value={showAs}
                  onChange={(event) => setShowAs(event.target.value as PivotShowAs)}
                >
                  {SHOW_AS.map((entry) => (
                    <option key={entry.key} value={entry.key}>
                      {entry.text}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cgs-dialog-field">
                <span>Base field:</span>
                <select
                  data-testid="pivot-base-field"
                  disabled={!kind.field}
                  value={baseField}
                  onChange={(event) => setBaseField(Number(event.target.value))}
                >
                  {baseFields.map((entry) => (
                    <option key={entry.index} value={entry.index}>
                      {entry.caption}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cgs-dialog-field">
                <span>Base item:</span>
                <select
                  data-testid="pivot-base-item"
                  disabled={!kind.item}
                  value={String(baseItem)}
                  onChange={(event) => {
                    const text = event.target.value;
                    setBaseItem(text === 'previous' || text === 'next' ? text : Number(text));
                  }}
                >
                  <option value="previous">(previous)</option>
                  <option value="next">(next)</option>
                  {items.map((item) => (
                    <option key={item.index} value={item.index}>
                      {item.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}
          <label className="cgs-dialog-field">
            <span>Number Format:</span>
            <input
              data-testid="pivot-value-format"
              placeholder="General"
              value={format}
              onChange={(event) => setFormat(event.target.value)}
            />
          </label>
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="pivot-value-ok"
            onClick={() => {
              props.onApply({
                field: value.field,
                summary,
                showAs,
                baseField,
                baseItem,
                name: name.trim() || valueName(summary, field.caption),
                numberFormat: format.trim() || 'General',
              });
              props.onClose();
            }}
          >
            OK
          </CgButton>
          <CgButton data-testid="pivot-value-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

// PivotTable Options

export interface PivotOptionsDialogProps {
  open: boolean;
  info: PivotInfo;
  run: Run;
  onClose: () => void;
}

export function PivotOptionsDialog(props: PivotOptionsDialogProps) {
  const { open, info } = props;
  const [name, setName] = useState(info.name);
  const [empty, setEmpty] = useState(info.options.emptyText ?? '');
  const [showErrors, setShowErrors] = useState(info.options.errorText !== null);
  const [errors, setErrors] = useState(info.options.errorText ?? '');
  const [rowGrand, setRowGrand] = useState(info.options.rowGrand);
  const [colGrand, setColGrand] = useState(info.options.colGrand);
  const [headers, setHeaders] = useState(info.options.headers);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) return;
    setName(info.name);
    setEmpty(info.options.emptyText ?? '');
    setShowErrors(info.options.errorText !== null);
    setErrors(info.options.errorText ?? '');
    setRowGrand(info.options.rowGrand);
    setColGrand(info.options.colGrand);
    setHeaders(info.options.headers);
    setError('');
    // Start again each time the dialog opens.
  }, [open]);

  const ok = async () => {
    const grand = rowGrand ? (colGrand ? 'both' : 'rows') : colGrand ? 'cols' : 'off';
    try {
      await props.run({
        kind: 'pivotOptions',
        pivot: { sheet: info.sheet, name: info.name },
        options: {
          ...(name.trim() !== info.name ? { name: name.trim() } : {}),
          emptyText: empty,
          errorText: showErrors ? errors : null,
          grandTotals: grand,
          headers,
        },
      });
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="PivotTable Options"
      width={400}
      allowDrag
      data-testid="pivot-options-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>PivotTable Name:</span>
            <input
              data-testid="pivot-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="pivot-show-errors"
              checked={showErrors}
              onChange={(event) => setShowErrors(event.target.checked)}
            />
            For error values show:
            <input
              data-testid="pivot-error-text"
              disabled={!showErrors}
              value={errors}
              onChange={(event) => setErrors(event.target.value)}
            />
          </label>
          <label className="cgs-dialog-field">
            <span>For empty cells show:</span>
            <input
              data-testid="pivot-empty-text"
              value={empty}
              onChange={(event) => setEmpty(event.target.value)}
            />
          </label>
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="pivot-row-grand"
              checked={rowGrand}
              onChange={(event) => setRowGrand(event.target.checked)}
            />
            Show grand totals for rows
          </label>
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="pivot-col-grand"
              checked={colGrand}
              onChange={(event) => setColGrand(event.target.checked)}
            />
            Show grand totals for columns
          </label>
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="pivot-headers"
              checked={headers}
              onChange={(event) => setHeaders(event.target.checked)}
            />
            Display field captions and filter drop downs
          </label>
          {error && (
            <p role="alert" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="pivot-options-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton onClick={props.onClose}>Cancel</CgButton>
        </div>
      )}
    />
  );
}

// A field's filter: sort and which items show.

export interface PivotFilterMenuProps {
  info: PivotInfo;
  /** The fields this button filters; the first is chosen to start with. */
  fields: number[];
  anchor: { left: number; top: number; width: number; height: number };
  run: Run;
  onClose: () => void;
}

export function PivotFilterMenu(props: PivotFilterMenuProps) {
  const { info } = props;
  const [field, setField] = useState(props.fields[0] ?? 0);
  const entry = info.fields[field];
  const [search, setSearch] = useState('');
  const [hidden, setHidden] = useState<Set<number>>(
    () => new Set(entry?.items.filter((i) => i.hidden).map((i) => i.index) ?? []),
  );
  useEffect(() => {
    setHidden(new Set(info.fields[field]?.items.filter((i) => i.hidden).map((i) => i.index) ?? []));
    setSearch('');
  }, [field, info]);
  if (!entry) return null;
  const pivot = { sheet: info.sheet, name: info.name };
  const isPage = entry.axis === 'page';
  const shown = entry.items.filter((item) =>
    item.label.toLowerCase().includes(search.toLowerCase()),
  );
  const all = shown.every((item) => !hidden.has(item.index));
  const send = async (command: Command) => {
    try {
      await props.run(command);
    } finally {
      props.onClose();
    }
  };
  return (
    <div
      className="cgs-pivot-filter"
      role="dialog"
      aria-label={`Filter ${entry.caption}`}
      data-testid="pivot-filter-menu"
      style={{ left: props.anchor.left, top: props.anchor.top + props.anchor.height }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') props.onClose();
      }}
    >
      {props.fields.length > 1 && (
        <label className="cgs-dialog-field">
          <span>Select field:</span>
          <select
            data-testid="pivot-filter-field"
            value={field}
            onChange={(event) => setField(Number(event.target.value))}
          >
            {props.fields.map((f) => (
              <option key={f} value={f}>
                {info.fields[f]?.caption}
              </option>
            ))}
          </select>
        </label>
      )}
      {!isPage && (
        <>
          <button
            type="button"
            className="cgs-menu-button"
            data-testid="pivot-sort-ascending"
            onClick={() =>
              void send({
                kind: 'pivotField',
                pivot,
                field,
                change: { sort: 'ascending', sortBy: null },
              })
            }
          >
            Sort A to Z
          </button>
          <button
            type="button"
            className="cgs-menu-button"
            data-testid="pivot-sort-descending"
            onClick={() =>
              void send({
                kind: 'pivotField',
                pivot,
                field,
                change: { sort: 'descending', sortBy: null },
              })
            }
          >
            Sort Z to A
          </button>
        </>
      )}
      <input
        className="cgs-pivot-filter-search"
        placeholder="Search"
        aria-label="Search"
        data-testid="pivot-filter-search"
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
      <div className="cgs-pivot-filter-list" role="group" aria-label="Items">
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="pivot-filter-all"
            checked={all}
            onChange={(event) => {
              const next = new Set(hidden);
              for (const item of shown) {
                if (event.target.checked) next.delete(item.index);
                else next.add(item.index);
              }
              setHidden(next);
            }}
          />
          (Select All)
        </label>
        {shown.map((item) => (
          <label key={item.index} className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid={`pivot-filter-item-${item.index}`}
              checked={!hidden.has(item.index)}
              onChange={(event) => {
                const next = new Set(hidden);
                if (event.target.checked) next.delete(item.index);
                else next.add(item.index);
                setHidden(next);
              }}
            />
            {item.label}
          </label>
        ))}
      </div>
      <div className="cgs-dialog-actions">
        <CgButton
          intent="primary"
          data-testid="pivot-filter-ok"
          disabled={hidden.size >= entry.items.length}
          onClick={() =>
            void send({
              kind: 'pivotField',
              pivot,
              field,
              change: isPage
                ? hidden.size === 0
                  ? { page: null, hidden: [] }
                  : { multiple: true, page: null, hidden: [...hidden] }
                : { hidden: [...hidden] },
            })
          }
        >
          OK
        </CgButton>
        <CgButton data-testid="pivot-filter-cancel" onClick={props.onClose}>
          Cancel
        </CgButton>
      </div>
    </div>
  );
}
