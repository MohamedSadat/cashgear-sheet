// Conditional formatting's dialogs: the small ones from Highlight Cells Rules and Top/Bottom Rules,
// and the Rules Manager. Both only build commands; the engine makes and keeps the rules.
import { useEffect, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  CfFormat,
  CfRuleInfo,
  CfRuleSpec,
  Command,
  EngineClient,
  ManagedCfRule,
} from '@cgs/engine-client';

export type CfQuickKind =
  | 'greaterThan'
  | 'lessThan'
  | 'between'
  | 'equal'
  | 'textContains'
  | 'dateOccurring'
  | 'duplicates'
  | 'top10'
  | 'top10Percent'
  | 'bottom10'
  | 'bottom10Percent'
  | 'aboveAverage'
  | 'belowAverage';

/** The "with" list's formats, as Excel 2016 has them. */
export const CF_PRESETS: readonly { key: string; text: string; format: CfFormat }[] = [
  {
    key: 'lightRed',
    text: 'Light Red Fill with Dark Red Text',
    format: { fill: 'FFC7CE', color: '9C0006' },
  },
  {
    key: 'yellow',
    text: 'Yellow Fill with Dark Yellow Text',
    format: { fill: 'FFEB9C', color: '9C5700' },
  },
  {
    key: 'green',
    text: 'Green Fill with Dark Green Text',
    format: { fill: 'C6EFCE', color: '006100' },
  },
  { key: 'lightRedFill', text: 'Light Red Fill', format: { fill: 'FFC7CE' } },
  { key: 'redText', text: 'Red Text', format: { color: '9C0006' } },
  { key: 'redBorder', text: 'Red Border', format: { border: 'FF0000' } },
];

/** A Date Occurring's periods, in Excel's order. */
export const CF_PERIODS: readonly [string, string][] = [
  ['yesterday', 'Yesterday'],
  ['today', 'Today'],
  ['tomorrow', 'Tomorrow'],
  ['last7Days', 'In the last 7 days'],
  ['lastWeek', 'Last week'],
  ['thisWeek', 'This week'],
  ['nextWeek', 'Next week'],
  ['lastMonth', 'Last month'],
  ['thisMonth', 'This month'],
  ['nextMonth', 'Next month'],
];

/** Each dialog's title and the line above its boxes. */
export const CF_QUICK_TEXT: Record<CfQuickKind, { title: string; prompt: string }> = {
  greaterThan: { title: 'Greater Than', prompt: 'Format cells that are GREATER THAN:' },
  lessThan: { title: 'Less Than', prompt: 'Format cells that are LESS THAN:' },
  between: { title: 'Between', prompt: 'Format cells that are BETWEEN:' },
  equal: { title: 'Equal To', prompt: 'Format cells that are EQUAL TO:' },
  textContains: { title: 'Text That Contains', prompt: 'Format cells that contain the text:' },
  dateOccurring: {
    title: 'A Date Occurring',
    prompt: 'Format cells that contain a date occurring:',
  },
  duplicates: { title: 'Duplicate Values', prompt: 'Format cells that contain:' },
  top10: { title: 'Top 10 Items', prompt: 'Format cells that rank in the TOP:' },
  top10Percent: { title: 'Top 10%', prompt: 'Format cells that rank in the TOP:' },
  bottom10: { title: 'Bottom 10 Items', prompt: 'Format cells that rank in the BOTTOM:' },
  bottom10Percent: { title: 'Bottom 10%', prompt: 'Format cells that rank in the BOTTOM:' },
  aboveAverage: { title: 'Above Average', prompt: 'Format cells that are ABOVE AVERAGE:' },
  belowAverage: { title: 'Below Average', prompt: 'Format cells that are BELOW AVERAGE:' },
};

/** What the dialog's boxes hold. */
export interface CfQuickInputs {
  first: string;
  second: string;
  period: string;
  unique: boolean;
  rank: string;
  preset: string;
}

export const CF_QUICK_DEFAULTS: CfQuickInputs = {
  first: '',
  second: '',
  period: 'yesterday',
  unique: false,
  rank: '10',
  preset: 'lightRed',
};

const NUMBER = /^[+-]?(\d+(\.\d*)?|\.\d+)(e[+-]?\d+)?%?$/i;

/**
 * A value box's text as a rule formula: a number or a formula stays as it is, and anything else is
 * text, quoted as Excel writes it (`abc` becomes `"abc"`). Null when the box is empty.
 */
export function cfValueFormula(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('=') || NUMBER.test(trimmed) || /^(TRUE|FALSE)$/i.test(trimmed)) {
    return trimmed;
  }
  return '"' + text.replace(/"/g, '""') + '"';
}

const OPS: Partial<Record<CfQuickKind, string>> = {
  greaterThan: 'greaterThan',
  lessThan: 'lessThan',
  between: 'between',
  equal: 'equal',
};

/** The rule a quick dialog asks for, or the message that says what's wrong. */
export function cfQuickRule(
  kind: CfQuickKind,
  inputs: CfQuickInputs,
): { rule: CfRuleSpec } | { error: string } {
  const format = (CF_PRESETS.find((preset) => preset.key === inputs.preset) ?? CF_PRESETS[0])
    .format;
  const op = OPS[kind];
  if (op) {
    const boxes = kind === 'between' ? [inputs.first, inputs.second] : [inputs.first];
    const formulas = boxes.map(cfValueFormula);
    if (formulas.some((formula) => formula === null)) {
      return { error: 'Enter a value for each box.' };
    }
    return { rule: { type: 'cellIs', op, formulas: formulas as string[], format: { ...format } } };
  }
  switch (kind) {
    case 'textContains':
      if (!inputs.first) return { error: 'Enter the text to look for.' };
      return { rule: { type: 'text', op: 'contains', text: inputs.first, format: { ...format } } };
    case 'dateOccurring':
      return { rule: { type: 'timePeriod', period: inputs.period, format: { ...format } } };
    case 'duplicates':
      return {
        rule: {
          type: 'duplicates',
          ...(inputs.unique ? { unique: true } : {}),
          format: { ...format },
        },
      };
    case 'aboveAverage':
    case 'belowAverage':
      return { rule: { type: 'average', above: kind === 'aboveAverage', format: { ...format } } };
    default: {
      const percent = kind === 'top10Percent' || kind === 'bottom10Percent';
      const most = percent ? 100 : 1000;
      const text = inputs.rank.trim();
      const rank = /^\d+$/.test(text) ? Number(text) : NaN;
      if (!(rank >= 1 && rank <= most)) {
        return { error: `Enter a whole number from 1 to ${most}.` };
      }
      return {
        rule: {
          type: 'top10',
          ...(kind === 'bottom10' || kind === 'bottom10Percent' ? { bottom: true } : {}),
          ...(percent ? { percent: true } : {}),
          rank,
          format: { ...format },
        },
      };
    }
  }
}

export interface CfQuickRuleProps {
  open: boolean;
  kind: CfQuickKind;
  sheet: number;
  areas: CellArea[];
  run: (command: Command) => Promise<Applied>;
  onClose: () => void;
}

/** Highlight Cells Rules and Top/Bottom Rules: one prompt, its boxes and a format. */
export function CfQuickRule(props: CfQuickRuleProps) {
  const { open, kind } = props;
  const [inputs, setInputs] = useState<CfQuickInputs>(CF_QUICK_DEFAULTS);
  const [message, setMessage] = useState('');
  const text = CF_QUICK_TEXT[kind];

  useEffect(() => {
    if (!open) return;
    setInputs(CF_QUICK_DEFAULTS);
    setMessage('');
  }, [open, kind]);

  const set = (change: Partial<CfQuickInputs>) => setInputs((old) => ({ ...old, ...change }));

  const apply = async () => {
    const built = cfQuickRule(kind, inputs);
    if ('error' in built) {
      setMessage(built.error);
      return;
    }
    try {
      await props.run({ kind: 'addCf', sheet: props.sheet, areas: props.areas, rule: built.rule });
      props.onClose();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const onEnter = (event: KeyboardEvent) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      void apply();
    }
  };

  const valueBox = (
    testId: string,
    value: string,
    change: (value: string) => void,
    focus = false,
  ) => (
    <input
      data-testid={testId}
      className="cgs-dialog-grow"
      autoFocus={focus}
      value={value}
      onChange={(event) => change(event.target.value)}
      onKeyDown={onEnter}
    />
  );

  let boxes: ReactNode = null;
  let joiner = 'with';
  switch (kind) {
    case 'greaterThan':
    case 'lessThan':
    case 'equal':
    case 'textContains':
      boxes = valueBox('cf-quick-value', inputs.first, (first) => set({ first }), true);
      break;
    case 'between':
      boxes = (
        <>
          {valueBox('cf-quick-value', inputs.first, (first) => set({ first }), true)}
          <span>and</span>
          {valueBox('cf-quick-value2', inputs.second, (second) => set({ second }))}
        </>
      );
      break;
    case 'dateOccurring':
      boxes = (
        <select
          data-testid="cf-quick-period"
          autoFocus
          value={inputs.period}
          onChange={(event) => set({ period: event.target.value })}
        >
          {CF_PERIODS.map(([key, label]) => (
            <option key={key} value={key}>
              {label}
            </option>
          ))}
        </select>
      );
      break;
    case 'duplicates':
      boxes = (
        <select
          data-testid="cf-quick-unique"
          autoFocus
          value={inputs.unique ? 'unique' : 'duplicate'}
          onChange={(event) => set({ unique: event.target.value === 'unique' })}
        >
          <option value="duplicate">Duplicate</option>
          <option value="unique">Unique</option>
        </select>
      );
      joiner = 'values with';
      break;
    case 'aboveAverage':
    case 'belowAverage':
      joiner = 'for the selected range with';
      break;
    default:
      boxes = (
        <>
          <input
            data-testid="cf-quick-rank"
            type="number"
            min={1}
            max={kind.endsWith('Percent') ? 100 : 1000}
            autoFocus
            style={{ inlineSize: '5rem' }}
            value={inputs.rank}
            onChange={(event) => set({ rank: event.target.value })}
            onKeyDown={onEnter}
          />
          {kind.endsWith('Percent') && <span>%</span>}
        </>
      );
  }

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={text.title}
      width={480}
      allowDrag
      data-testid="cf-quick"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>{text.prompt}</span>
          <div className="cgs-dialog-row" style={{ alignItems: 'center' }}>
            {boxes}
            <span>{joiner}</span>
            <select
              data-testid="cf-quick-format"
              className="cgs-dialog-grow"
              value={inputs.preset}
              onChange={(event) => set({ preset: event.target.value })}
            >
              {CF_PRESETS.map((preset) => (
                <option key={preset.key} value={preset.key}>
                  {preset.text}
                </option>
              ))}
            </select>
          </div>
          {message && (
            <p role="alert" data-testid="cf-quick-message" className="cgs-dialog-message">
              {message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="cf-quick-ok" onClick={() => void apply()}>
            OK
          </CgButton>
          <CgButton data-testid="cf-quick-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

/** An Applies to box's parts: split at spaces and commas outside quoted sheet names. */
export function splitAppliesTo(text: string): string[] {
  const parts: string[] = [];
  let part = '';
  let quoted = false;
  for (const char of text.trim().replace(/^=/, '')) {
    if (char === "'") quoted = !quoted;
    if (!quoted && (char === ' ' || char === ',')) {
      if (part) parts.push(part);
      part = '';
    } else {
      part += char;
    }
  }
  if (part) parts.push(part);
  return parts;
}

/** Colour scales, data bars and icon sets go on regardless of Stop If True. */
export function cfStopApplies(kind: string): boolean {
  return kind !== 'colorScale' && kind !== 'dataBar' && kind !== 'iconSet';
}

/** A manager row: the rule as the engine listed it, and what's been changed here. */
export interface CfManagerRow {
  info: CfRuleInfo;
  appliesTo: string;
  stop: boolean;
}

/** One row moved up (-1) or down (+1), or the same list when it can't go. */
export function moveCfRow<T>(rows: T[], index: number, by: -1 | 1): T[] {
  const to = index + by;
  if (index < 0 || index >= rows.length || to < 0 || to >= rows.length) return rows;
  const next = rows.slice();
  [next[index], next[to]] = [next[to], next[index]];
  return next;
}

/**
 * The `setCf` list: the classic rules in their new order, each with its cells. Extended rules are
 * left out, since the engine keeps those as the file has them.
 */
export function managedCfRules(rows: CfManagerRow[], areas: CellArea[][]): ManagedCfRule[] {
  const list: ManagedCfRule[] = [];
  rows.forEach((row, at) => {
    if (row.info.extended) return;
    list.push({
      block: row.info.block,
      rule: row.info.rule,
      areas: areas[at],
      stop: cfStopApplies(row.info.kind) && row.stop,
    });
  });
  return list;
}

export interface CfManagerProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  run: (command: Command) => Promise<Applied>;
  onClose: () => void;
  onNewRule?: () => void;
}

const cell: CSSProperties = {
  padding: '2px 6px',
  borderBlockEnd: 'var(--cg-border-style)',
  textAlign: 'start',
  verticalAlign: 'middle',
};

/** Conditional Formatting Rules Manager: the sheet's rules in priority order. */
export function CfManager(props: CfManagerProps) {
  const { open, sheet } = props;
  const [rows, setRows] = useState<CfManagerRow[]>([]);
  const [selected, setSelected] = useState(0);
  const [dirty, setDirty] = useState(false);
  const [message, setMessage] = useState('');

  const load = async () => {
    const engine = props.engine();
    if (!engine) return;
    const list = await engine.query({ kind: 'cfRules', sheet });
    setRows(list.map((info) => ({ info, appliesTo: info.areas, stop: info.stop })));
    setSelected((at) => Math.min(at, Math.max(list.length - 1, 0)));
    setDirty(false);
  };

  useEffect(() => {
    if (!open) return;
    setSelected(0);
    setMessage('');
    // The list is read once each time the dialog opens.
    void load();
  }, [open, sheet]);

  const change = (next: CfManagerRow[]) => {
    setRows(next);
    setDirty(true);
    setMessage('');
  };

  const current = rows[selected] as CfManagerRow | undefined;
  const editable = current !== undefined && !current.info.extended;

  // Returns false when the list couldn't go to the engine, and says why.
  const apply = async (): Promise<boolean> => {
    if (!dirty) return true;
    const engine = props.engine();
    if (!engine) return false;
    const areas: CellArea[][] = [];
    for (const row of rows) {
      const list: CellArea[] = [];
      if (!row.info.extended) {
        const parts = splitAppliesTo(row.appliesTo);
        if (parts.length === 0) {
          setMessage('Each rule needs cells to apply to.');
          return false;
        }
        for (const part of parts) {
          const found = await engine.resolveName(part, sheet);
          if (!found || found.sheet !== sheet) {
            setMessage(`${part} isn't a reference on this sheet.`);
            return false;
          }
          list.push({ top: found.top, left: found.left, bottom: found.bottom, right: found.right });
        }
      }
      areas.push(list);
    }
    try {
      await props.run({ kind: 'setCf', sheet, rules: managedCfRules(rows, areas) });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
      return false;
    }
    await load();
    return true;
  };

  const preview = (info: CfRuleInfo) =>
    cfStopApplies(info.kind) ? (
      <span
        style={{
          display: 'inline-block',
          padding: '0 6px',
          background: info.fill ? '#' + info.fill : undefined,
          color: info.color ? '#' + info.color : undefined,
          fontWeight: info.bold ? 'bold' : undefined,
          border: 'var(--cg-border-style)',
        }}
      >
        AaBbCcYyZz
      </span>
    ) : null;

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Conditional Formatting Rules Manager"
      width={720}
      allowDrag
      data-testid="cf-manager"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-row" style={{ alignItems: 'center' }}>
            <span>Show formatting rules for:</span>
            <select data-testid="cf-manager-scope" value="sheet" onChange={() => undefined}>
              <option value="sheet">This Worksheet</option>
            </select>
          </label>
          <div className="cgs-dialog-row">
            {props.onNewRule && (
              <CgButton data-testid="cf-new" onClick={props.onNewRule}>
                New Rule...
              </CgButton>
            )}
            <CgButton
              data-testid="cf-delete"
              disabled={!editable}
              onClick={() => {
                change(rows.filter((_, at) => at !== selected));
                setSelected((at) => Math.max(0, Math.min(at, rows.length - 2)));
              }}
            >
              Delete Rule
            </CgButton>
            <CgButton
              data-testid="cf-up"
              aria-label="Move Up"
              disabled={!editable || selected === 0}
              onClick={() => {
                change(moveCfRow(rows, selected, -1));
                setSelected(selected - 1);
              }}
            >
              ▲
            </CgButton>
            <CgButton
              data-testid="cf-down"
              aria-label="Move Down"
              disabled={!editable || selected >= rows.length - 1}
              onClick={() => {
                change(moveCfRow(rows, selected, 1));
                setSelected(selected + 1);
              }}
            >
              ▼
            </CgButton>
          </div>
          <div style={{ blockSize: '14rem', overflow: 'auto', border: 'var(--cg-border-style)' }}>
            <table
              data-testid="cf-rules"
              style={{ inlineSize: '100%', borderCollapse: 'collapse' }}
            >
              <thead>
                <tr>
                  <th style={cell}>Rule (applied in order shown)</th>
                  <th style={cell}>Format</th>
                  <th style={cell}>Applies to</th>
                  <th style={cell}>Stop If True</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row, at) => {
                  const fixed = row.info.extended;
                  return (
                    <tr
                      key={row.info.block + ':' + row.info.rule}
                      data-testid={'cf-rule-' + at}
                      aria-selected={at === selected}
                      onClick={() => setSelected(at)}
                      style={{
                        background: at === selected ? 'var(--cg-hover)' : undefined,
                        cursor: 'default',
                      }}
                    >
                      <td style={cell}>{row.info.description}</td>
                      <td style={cell}>{preview(row.info)}</td>
                      <td style={cell}>
                        <input
                          data-testid={'cf-applies-' + at}
                          style={{ inlineSize: '100%', boxSizing: 'border-box', font: 'inherit' }}
                          value={row.appliesTo}
                          readOnly={fixed}
                          onFocus={() => setSelected(at)}
                          onChange={(event) =>
                            change(
                              rows.map((old, i) =>
                                i === at ? { ...old, appliesTo: event.target.value } : old,
                              ),
                            )
                          }
                        />
                      </td>
                      <td style={{ ...cell, textAlign: 'center' }}>
                        <input
                          type="checkbox"
                          data-testid={'cf-stop-' + at}
                          aria-label="Stop If True"
                          checked={cfStopApplies(row.info.kind) && row.stop}
                          disabled={fixed || !cfStopApplies(row.info.kind)}
                          onChange={(event) =>
                            change(
                              rows.map((old, i) =>
                                i === at ? { ...old, stop: event.target.checked } : old,
                              ),
                            )
                          }
                        />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          {message && (
            <p role="alert" data-testid="cf-manager-message" className="cgs-dialog-message">
              {message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="cf-ok"
            onClick={() =>
              void apply().then((done) => {
                if (done) props.onClose();
              })
            }
          >
            OK
          </CgButton>
          <CgButton data-testid="cf-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
          <CgButton data-testid="cf-apply" disabled={!dirty} onClick={() => void apply()}>
            Apply
          </CgButton>
        </div>
      )}
    />
  );
}
