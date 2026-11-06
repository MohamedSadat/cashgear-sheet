// The chart dialogs: Insert Chart and Change Chart Type, Select Data, Move Chart, and the format
// pane that sits beside the grid while a chart element is chosen. Each change is a command.
import { useEffect, useState } from 'react';
import {
  CgButton,
  CgColorPicker,
  CgIcon,
  CgWindow,
  type CgColorPickerThemeColor,
  type CgColorPickerValue,
  type CgIconName,
} from '@cashgear/ui';
import type { ChartColor, ChartEdit, ChartType, ChartView } from '@cgs/engine-client';
import { elementLabel, type EngineElement } from './chartProps';

export interface ChartTypeChoice {
  type: ChartType;
  text: string;
}

export interface ChartFamily {
  key: string;
  text: string;
  icon: CgIconName;
  types: ChartTypeChoice[];
}

/** Excel's chart families and their kinds, as Insert and Change Chart Type list them. */
export const CHART_FAMILIES: ChartFamily[] = [
  {
    key: 'column',
    text: 'Column',
    icon: 'chart-column',
    types: [
      { type: 'columnClustered', text: 'Clustered Column' },
      { type: 'columnStacked', text: 'Stacked Column' },
      { type: 'columnPercent', text: '100% Stacked Column' },
    ],
  },
  {
    key: 'line',
    text: 'Line',
    icon: 'chart-line',
    types: [
      { type: 'line', text: 'Line' },
      { type: 'lineStacked', text: 'Stacked Line' },
      { type: 'linePercent', text: '100% Stacked Line' },
      { type: 'lineMarkers', text: 'Line with Markers' },
      { type: 'lineMarkersStacked', text: 'Stacked Line with Markers' },
      { type: 'lineMarkersPercent', text: '100% Stacked Line with Markers' },
    ],
  },
  {
    key: 'pie',
    text: 'Pie',
    icon: 'chart-pie',
    types: [
      { type: 'pie', text: 'Pie' },
      { type: 'doughnut', text: 'Doughnut' },
    ],
  },
  {
    key: 'bar',
    text: 'Bar',
    icon: 'chart-bar',
    types: [
      { type: 'barClustered', text: 'Clustered Bar' },
      { type: 'barStacked', text: 'Stacked Bar' },
      { type: 'barPercent', text: '100% Stacked Bar' },
    ],
  },
  {
    key: 'area',
    text: 'Area',
    icon: 'chart-area',
    types: [
      { type: 'area', text: 'Area' },
      { type: 'areaStacked', text: 'Stacked Area' },
      { type: 'areaPercent', text: '100% Stacked Area' },
    ],
  },
  {
    key: 'scatter',
    text: 'X Y (Scatter)',
    icon: 'chart-scatter',
    types: [
      { type: 'scatter', text: 'Scatter' },
      { type: 'scatterSmoothMarkers', text: 'Scatter with Smooth Lines and Markers' },
      { type: 'scatterSmooth', text: 'Scatter with Smooth Lines' },
      { type: 'scatterLinesMarkers', text: 'Scatter with Straight Lines and Markers' },
      { type: 'scatterLines', text: 'Scatter with Straight Lines' },
    ],
  },
  {
    key: 'combo',
    text: 'Combo',
    icon: 'chart-combo',
    types: [
      { type: 'comboColumnLine', text: 'Clustered Column - Line' },
      { type: 'comboColumnLineSecondary', text: 'Clustered Column - Line on Secondary Axis' },
      { type: 'comboAreaColumn', text: 'Stacked Area - Clustered Column' },
    ],
  },
];

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** The theme colour names the engine knows, in the colour picker's order. */
const SCHEME = [
  'bg1',
  'tx1',
  'bg2',
  'tx2',
  'accent1',
  'accent2',
  'accent3',
  'accent4',
  'accent5',
  'accent6',
];

/** A picked colour as the engine takes it: a theme colour keeps its name, as Excel writes it. */
export function chartColor(value: CgColorPickerValue): ChartColor | undefined {
  if (value.kind === 'rgb') return { rgb: value.color };
  if (value.kind !== 'theme') return undefined;
  const theme = SCHEME[Number(value.key)] ?? 'accent1';
  const tint = value.tint;
  if (!tint) return { theme };
  if (tint > 0) {
    return { theme, lumMod: Math.round((1 - tint) * 100000), lumOff: Math.round(tint * 100000) };
  }
  return { theme, lumMod: Math.round((1 + tint) * 100000) };
}

interface TypeDialogProps {
  open: boolean;
  /** Insert makes a new chart; Change keeps the chart's data. */
  mode: 'insert' | 'change';
  view: ChartView | null;
  onPick: (type: ChartType) => Promise<void>;
  onSeriesTypes?: (list: { index: number; type: ChartType; secondary: boolean }[]) => Promise<void>;
  onClose: () => void;
}

/** Insert Chart and Change Chart Type: a family on the left, its kinds on the right. */
export function ChartTypeDialog(props: TypeDialogProps) {
  const [family, setFamily] = useState('column');
  const [type, setType] = useState<ChartType>('columnClustered');
  const [error, setError] = useState('');
  const [combo, setCombo] = useState<
    { index: number; name: string; type: ChartType; secondary: boolean }[]
  >([]);
  useEffect(() => {
    if (!props.open) return;
    setError('');
    const view = props.view;
    if (view) {
      setCombo(
        view.series.map((series) => {
          const group = view.groups[series.group];
          const kind: ChartType =
            group?.kind === 'line'
              ? 'lineMarkers'
              : group?.kind === 'area'
                ? 'areaStacked'
                : 'columnClustered';
          return {
            index: series.index,
            name: series.name,
            type: kind,
            secondary: group?.axis === 1,
          };
        }),
      );
    }
  }, [props.open, props.view]);
  const shown = CHART_FAMILIES.find((entry) => entry.key === family) ?? CHART_FAMILIES[0]!;
  const ok = async () => {
    try {
      if (
        family === 'combo' &&
        props.mode === 'change' &&
        props.onSeriesTypes &&
        combo.length > 1
      ) {
        await props.onSeriesTypes(combo);
      } else {
        await props.onPick(type);
      }
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };
  return (
    <CgWindow
      open={props.open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={props.mode === 'insert' ? 'Insert Chart' : 'Change Chart Type'}
      width={620}
      allowDrag
      data-testid="chart-type-dialog"
      renderBody={() => (
        <div className="cgs-chart-type">
          <div className="cgs-chart-families" role="listbox" aria-label="Chart types">
            {CHART_FAMILIES.map((entry) => (
              <button
                key={entry.key}
                type="button"
                role="option"
                aria-selected={entry.key === family}
                data-testid={'chart-family-' + entry.key}
                className="cgs-chart-family"
                onClick={() => {
                  setFamily(entry.key);
                  setType(entry.types[0]!.type);
                }}
              >
                <CgIcon name={entry.icon} />
                <span>{entry.text}</span>
              </button>
            ))}
          </div>
          <div className="cgs-chart-kinds">
            <div className="cgs-chart-kind-list" role="radiogroup" aria-label={shown.text}>
              {shown.types.map((choice) => (
                <button
                  key={choice.type}
                  type="button"
                  role="radio"
                  aria-checked={choice.type === type}
                  title={choice.text}
                  data-testid={'chart-kind-' + choice.type}
                  className="cgs-chart-kind"
                  onClick={() => setType(choice.type)}
                  onDoubleClick={() => {
                    setType(choice.type);
                    void ok();
                  }}
                >
                  <CgIcon name={shown.icon} size="large" />
                  <span>{choice.text}</span>
                </button>
              ))}
            </div>
            {family === 'combo' && props.mode === 'change' && combo.length > 1 && (
              <table className="cgs-chart-combo" data-testid="chart-combo">
                <thead>
                  <tr>
                    <th>Series Name</th>
                    <th>Chart Type</th>
                    <th>Secondary Axis</th>
                  </tr>
                </thead>
                <tbody>
                  {combo.map((row, at) => (
                    <tr key={row.index}>
                      <td>{row.name}</td>
                      <td>
                        <select
                          value={row.type}
                          aria-label={'Chart type for ' + row.name}
                          onChange={(event) => {
                            const next = [...combo];
                            next[at] = { ...row, type: event.target.value as ChartType };
                            setCombo(next);
                          }}
                        >
                          <option value="columnClustered">Clustered Column</option>
                          <option value="columnStacked">Stacked Column</option>
                          <option value="line">Line</option>
                          <option value="lineMarkers">Line with Markers</option>
                          <option value="area">Area</option>
                          <option value="areaStacked">Stacked Area</option>
                        </select>
                      </td>
                      <td>
                        <input
                          type="checkbox"
                          aria-label={'Secondary axis for ' + row.name}
                          checked={row.secondary}
                          onChange={(event) => {
                            const next = [...combo];
                            next[at] = { ...row, secondary: event.target.checked };
                            setCombo(next);
                          }}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {error && (
              <p role="alert" className="cgs-dialog-message">
                {error}
              </p>
            )}
          </div>
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="chart-type-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton onClick={props.onClose}>Cancel</CgButton>
        </div>
      )}
    />
  );
}

interface SeriesRow {
  name: string;
  values: string;
  categories: string;
}

interface DataDialogProps {
  open: boolean;
  view: ChartView | null;
  /** Resolves a typed reference to its sheet and cells. */
  onRange: (text: string) => Promise<void>;
  onSeries: (list: { name?: string; categories?: string; values: string }[]) => Promise<void>;
  onSwitch: () => Promise<void>;
  onClose: () => void;
}

/** Select Data: the chart's range, or its series one by one. */
export function SelectDataDialog(props: DataDialogProps) {
  const [range, setRange] = useState('');
  const [rows, setRows] = useState<SeriesRow[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!props.open || !props.view) return;
    setError('');
    setRange('');
    setRows(
      props.view.series.map((series) => ({
        name: series.nameRef ?? '',
        values: series.valuesRef ?? '',
        categories: series.categoriesRef ?? '',
      })),
    );
  }, [props.open, props.view]);
  const update = (at: number, key: keyof SeriesRow, value: string) => {
    const next = [...rows];
    next[at] = { ...next[at]!, [key]: value };
    setRows(next);
  };
  const ok = async () => {
    try {
      if (range.trim()) {
        await props.onRange(range.trim());
      } else {
        await props.onSeries(
          rows
            .filter((row) => row.values.trim())
            .map((row) => ({
              name: row.name.trim() || undefined,
              categories: row.categories.trim().replace(/^=/, '') || undefined,
              values: row.values.trim().replace(/^=/, ''),
            })),
        );
      }
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };
  return (
    <CgWindow
      open={props.open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Select Data Source"
      width={640}
      allowDrag
      data-testid="select-data-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>Chart data range (leave empty to edit the series below)</span>
            <input
              data-testid="chart-range"
              value={range}
              placeholder="=Sheet1!$A$1:$C$5"
              onChange={(event) => setRange(event.target.value)}
            />
          </label>
          <div className="cgs-dialog-row">
            <CgButton
              data-testid="chart-switch"
              onClick={() => {
                void props
                  .onSwitch()
                  .then(props.onClose)
                  .catch((failure) => setError(errorText(failure)));
              }}
            >
              Switch Row/Column
            </CgButton>
          </div>
          <table className="cgs-chart-series" data-testid="chart-series-list">
            <thead>
              <tr>
                <th>Series name</th>
                <th>Series values</th>
                <th>Category (X) labels</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((row, at) => (
                <tr key={at}>
                  <td>
                    <input
                      aria-label={'Name of series ' + (at + 1)}
                      value={row.name}
                      onChange={(event) => update(at, 'name', event.target.value)}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={'Values of series ' + (at + 1)}
                      value={row.values}
                      onChange={(event) => update(at, 'values', event.target.value)}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={'Categories of series ' + (at + 1)}
                      value={row.categories}
                      onChange={(event) => update(at, 'categories', event.target.value)}
                    />
                  </td>
                  <td>
                    <CgButton
                      size="small"
                      aria-label={'Remove series ' + (at + 1)}
                      disabled={rows.length < 2}
                      onClick={() => setRows(rows.filter((_, other) => other !== at))}
                    >
                      Remove
                    </CgButton>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="cgs-dialog-row">
            <CgButton
              data-testid="chart-add-series"
              onClick={() =>
                setRows([...rows, { name: '', values: '', categories: rows[0]?.categories ?? '' }])
              }
            >
              Add
            </CgButton>
          </div>
          {error && (
            <p role="alert" className="cgs-dialog-message" data-testid="select-data-error">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="select-data-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton onClick={props.onClose}>Cancel</CgButton>
        </div>
      )}
    />
  );
}

interface MoveDialogProps {
  open: boolean;
  sheets: { index: number; name: string }[];
  current: number;
  onMove: (to: number) => Promise<void>;
  onClose: () => void;
}

export function MoveChartDialog(props: MoveDialogProps) {
  const [to, setTo] = useState(props.current);
  const [error, setError] = useState('');
  useEffect(() => {
    if (props.open) {
      setTo(props.current);
      setError('');
    }
  }, [props.open, props.current]);
  return (
    <CgWindow
      open={props.open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Move Chart"
      width={360}
      allowDrag
      data-testid="move-chart-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>Object in</span>
            <select
              data-testid="move-chart-sheet"
              value={to}
              onChange={(event) => setTo(Number(event.target.value))}
            >
              {props.sheets.map((sheet) => (
                <option key={sheet.index} value={sheet.index}>
                  {sheet.name}
                </option>
              ))}
            </select>
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
          <CgButton
            intent="primary"
            data-testid="move-chart-ok"
            onClick={() => {
              props
                .onMove(to)
                .then(props.onClose)
                .catch((failure) => setError(errorText(failure)));
            }}
          >
            OK
          </CgButton>
          <CgButton onClick={props.onClose}>Cancel</CgButton>
        </div>
      )}
    />
  );
}

interface FormatPaneProps {
  view: ChartView;
  element: EngineElement;
  themeColors: CgColorPickerThemeColor[];
  onEdit: (edit: ChartEdit) => Promise<void>;
  onClose: () => void;
}

function NumberBox(props: {
  label: string;
  value: number | null;
  testid: string;
  onCommit: (value: number | null) => void;
}) {
  const [text, setText] = useState(props.value === null ? '' : String(props.value));
  useEffect(() => setText(props.value === null ? '' : String(props.value)), [props.value]);
  const commit = () => {
    const trimmed = text.trim();
    const value = trimmed === '' ? null : Number(trimmed);
    if (value !== null && !Number.isFinite(value)) return;
    if (value !== props.value) props.onCommit(value);
  };
  return (
    <label className="cgs-dialog-field">
      <span>{props.label}</span>
      <input
        data-testid={props.testid}
        value={text}
        placeholder="Auto"
        inputMode="decimal"
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') commit();
        }}
      />
    </label>
  );
}

/** Format pane: fill, outline and font of the chosen element, and axis and bar options. */
export function FormatPane({ view, element, themeColors, onEdit, onClose }: FormatPaneProps) {
  const [error, setError] = useState('');
  useEffect(() => setError(''), [element]);
  const run = (edit: ChartEdit) => {
    onEdit(edit).catch((failure) => setError(errorText(failure)));
  };
  const engineRef = element as never;
  const hasFill =
    element.kind !== 'axis' && element.kind !== 'gridlines' && element.kind !== 'dataLabels';
  const hasLine = element.kind !== 'dataLabels';
  const hasText = ['chartArea', 'title', 'legend', 'axis', 'axisTitle', 'dataLabels'].includes(
    element.kind,
  );
  const axis =
    element.kind === 'axis'
      ? (view.valueAxes.find((a) => a.id === element.id) ??
        (view.categoryAxis?.id === element.id ? view.categoryAxis : null))
      : null;
  const numeric = axis && axis.min !== null;
  const series =
    element.kind === 'series' ? view.series.find((s) => s.index === element.index) : null;
  const group = series ? view.groups[series.group] : null;
  const [scale, setScale] = useState({
    min: null as number | null,
    max: null as number | null,
    unit: null as number | null,
  });
  useEffect(() => setScale({ min: null, max: null, unit: null }), [element]);
  const sendScale = (next: typeof scale) => {
    setScale(next);
    if (!axis) return;
    run({
      kind: 'axisScale',
      id: axis.id,
      min: next.min ?? undefined,
      max: next.max ?? undefined,
      major: next.unit ?? undefined,
      reversed: axis.reversed,
    });
  };
  return (
    <aside className="cgs-format-pane" data-testid="format-pane" aria-label="Format">
      <header className="cgs-format-pane-header">
        <h2>Format {elementLabel(view, element)}</h2>
        <CgButton size="small" aria-label="Close the format pane" onClick={onClose}>
          ✕
        </CgButton>
      </header>
      {hasFill && (
        <section>
          <h3>Fill</h3>
          <CgColorPicker
            themeColors={themeColors}
            allowNone
            allowCustomColor
            labels={{ none: 'No Fill' }}
            onValueChange={(value) =>
              run({
                kind: 'fill',
                element: engineRef,
                color: chartColor(value),
                none: value.kind === 'none',
              })
            }
          />
        </section>
      )}
      {hasLine && (
        <section>
          <h3>{element.kind === 'series' && group?.kind === 'line' ? 'Line' : 'Border'}</h3>
          <CgColorPicker
            themeColors={themeColors}
            allowNone
            allowCustomColor
            labels={{ none: 'No Line' }}
            onValueChange={(value) =>
              run({
                kind: 'outline',
                element: engineRef,
                color: chartColor(value),
                none: value.kind === 'none',
              })
            }
          />
          <label className="cgs-dialog-field">
            <span>Width</span>
            <select
              data-testid="format-line-width"
              defaultValue=""
              onChange={(event) =>
                run({ kind: 'outline', element: engineRef, width: Number(event.target.value) })
              }
            >
              <option value="" disabled>
                Choose
              </option>
              {[0.25, 0.5, 0.75, 1, 1.5, 2.25, 3, 4.5, 6].map((width) => (
                <option key={width} value={width}>
                  {width} pt
                </option>
              ))}
            </select>
          </label>
          <label className="cgs-dialog-field">
            <span>Dash type</span>
            <select
              defaultValue=""
              onChange={(event) =>
                run({ kind: 'outline', element: engineRef, dash: event.target.value })
              }
            >
              <option value="" disabled>
                Choose
              </option>
              <option value="solid">Solid</option>
              <option value="sysDot">Round Dot</option>
              <option value="sysDash">Square Dot</option>
              <option value="dash">Dash</option>
              <option value="dashDot">Dash Dot</option>
              <option value="lgDash">Long Dash</option>
            </select>
          </label>
        </section>
      )}
      {hasText && (
        <section>
          <h3>Text</h3>
          <label className="cgs-dialog-field">
            <span>Size</span>
            <select
              data-testid="format-font-size"
              defaultValue=""
              onChange={(event) =>
                run({ kind: 'font', element: engineRef, size: Number(event.target.value) })
              }
            >
              <option value="" disabled>
                Choose
              </option>
              {[8, 9, 10, 11, 12, 14, 16, 18, 20, 24].map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </select>
          </label>
          <div className="cgs-dialog-row">
            <CgButton
              size="small"
              onClick={() => run({ kind: 'font', element: engineRef, bold: true })}
            >
              Bold
            </CgButton>
            <CgButton
              size="small"
              onClick={() => run({ kind: 'font', element: engineRef, bold: false, italic: false })}
            >
              Regular
            </CgButton>
            <CgButton
              size="small"
              onClick={() => run({ kind: 'font', element: engineRef, italic: true })}
            >
              Italic
            </CgButton>
          </div>
          <CgColorPicker
            themeColors={themeColors}
            allowCustomColor
            onValueChange={(value) => {
              const color = chartColor(value);
              if (color) run({ kind: 'font', element: engineRef, color });
            }}
          />
        </section>
      )}
      {axis && (
        <section>
          <h3>Axis Options</h3>
          {numeric && (
            <>
              <NumberBox
                label="Minimum"
                testid="axis-min"
                value={scale.min}
                onCommit={(min) => sendScale({ ...scale, min })}
              />
              <NumberBox
                label="Maximum"
                testid="axis-max"
                value={scale.max}
                onCommit={(max) => sendScale({ ...scale, max })}
              />
              <NumberBox
                label="Major unit"
                testid="axis-major"
                value={scale.unit}
                onCommit={(unit) => sendScale({ ...scale, unit })}
              />
            </>
          )}
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              checked={axis.reversed}
              onChange={(event) =>
                run({
                  kind: 'axisScale',
                  id: axis.id,
                  min: scale.min ?? undefined,
                  max: scale.max ?? undefined,
                  major: scale.unit ?? undefined,
                  reversed: event.target.checked,
                })
              }
            />
            <span>{numeric ? 'Values in reverse order' : 'Categories in reverse order'}</span>
          </label>
          {numeric && (
            <label className="cgs-dialog-field">
              <span>Number format</span>
              <input
                data-testid="axis-format"
                placeholder="Linked to source"
                onKeyDown={(event) => {
                  if (event.key !== 'Enter') return;
                  const code = (event.target as HTMLInputElement).value.trim();
                  run({
                    kind: 'axisScale',
                    id: axis.id,
                    min: scale.min ?? undefined,
                    max: scale.max ?? undefined,
                    major: scale.unit ?? undefined,
                    reversed: axis.reversed,
                    format: code || undefined,
                  });
                }}
              />
            </label>
          )}
        </section>
      )}
      {group?.kind === 'bar' && series && (
        <section>
          <h3>Series Options</h3>
          <NumberBox
            label="Gap Width (%)"
            testid="series-gap"
            value={group.gapWidth}
            onCommit={(gap) =>
              gap !== null &&
              run({
                kind: 'gapWidth',
                group: series.group,
                gap: Math.round(gap),
                overlap: group.overlap,
              })
            }
          />
          <NumberBox
            label="Series Overlap (%)"
            testid="series-overlap"
            value={group.overlap}
            onCommit={(overlap) =>
              overlap !== null &&
              run({
                kind: 'gapWidth',
                group: series.group,
                gap: group.gapWidth,
                overlap: Math.round(overlap),
              })
            }
          />
        </section>
      )}
      {error && (
        <p role="alert" className="cgs-dialog-message" data-testid="format-error">
          {error}
        </p>
      )}
    </aside>
  );
}
