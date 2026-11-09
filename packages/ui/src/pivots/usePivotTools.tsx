// Pivot tables as the shell runs them: what the ribbon shows about the pivot table at the active
// cell, the field list pane, the filter buttons on its captions, its context menu and dialogs.
// Everything is asked of the engine, and every change is a command through the shell's queue.
import { useCallback, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  CG_PIVOT_FIELD_LIST_VALUES_KEY,
  CgPivotFieldList,
  type CgContextMenuItem,
  type CgPivotFieldListField,
  type CgPivotFieldListItem,
  type CgPivotFieldListLayout,
} from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  PivotInfo,
  PivotLayoutSpec,
  PivotShowAs,
  PivotSummary,
  PivotValueSpec,
} from '@cgs/engine-client';
import type { CellButton, ClientBox, GridController, GridSelection } from '../grid/controller';
import { toArea } from '../sheet-editing';
import { FilterPagesDialog, GroupingDialog } from './GroupDialogs';
import {
  CreatePivotDialog,
  PivotFilterMenu,
  PivotOptionsDialog,
  SHOW_AS,
  SUMMARIES,
  ValueFieldDialog,
  valueName,
} from './PivotDialogs';

export interface PivotToolsHost {
  engine: () => EngineClient | null;
  grid: () => GridController | null;
  sheet: () => number;
  selection: () => GridSelection | null;
  hasBook: boolean;
  sheetNames: string[];
  run: (command: Command) => Promise<Applied>;
  notice: (message: string | null) => void;
  /** Shows a sheet and selects a cell on it, 1-based; -1 for the sheet a command just added. */
  goTo: (sheet: number, row: number, col: number) => Promise<void>;
  /** Puts the grid's buttons back, with the pivot tables' among them. */
  placeButtons: () => void;
  /** Fits columns to what they hold, 1-based, as Excel does to a pivot table it updates. */
  fitColumns: (sheet: number, first: number, last: number) => void;
  /** Shows the sheet a command added, with this area selected. 1-based. */
  showAdded?: (sheet: number, area: CellArea | null) => Promise<void>;
}

export interface PivotState {
  hasBook: boolean;
  info: PivotInfo | null;
  fieldList: boolean;
  /** Generate GetPivotData: a click on a value while typing a formula writes GETPIVOTDATA. */
  generate: boolean;
}

export interface PivotActions {
  insertPivot: () => void;
  refresh: () => void;
  refreshAll: () => void;
  toggleFieldList: () => void;
  options: () => void;
  clear: () => void;
  valueSettings: () => void;
  expand: (expanded: boolean) => void;
  set: (options: Extract<Command, { kind: 'pivotOptions' }>['options']) => void;
  /** Group Selection: items picked by hand, or the Grouping dialog for dates and numbers. */
  groupSelection: () => void;
  /** Group Field: the Grouping dialog for the active cell's date or number field. */
  groupField: () => void;
  ungroup: () => void;
  /** Show Details on the value at the active cell, or at this cell, 0-based. */
  showDetails: (row?: number, col?: number) => void;
  filterPages: () => void;
  toggleGenerate: () => void;
}

type PivotDialog =
  | { kind: 'create'; sheet: number; area: CellArea }
  | { kind: 'value'; index: number }
  | { kind: 'options' }
  | { kind: 'group'; field: number }
  | { kind: 'pages' };

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** The field list's layout from the engine's account of the pivot table. */
export function fieldListLayout(info: PivotInfo): CgPivotFieldListLayout {
  const item = (f: number, prefix: string): CgPivotFieldListItem =>
    f === -2
      ? { id: 'values', fieldKey: CG_PIVOT_FIELD_LIST_VALUES_KEY }
      : { id: `${prefix}:${f}`, fieldKey: String(f) };
  return {
    filters: info.pages.map((p) => item(p.field, 'page')),
    rows: info.rows.map((f) => item(f, 'row')),
    columns: info.cols.map((f) => item(f, 'col')),
    values: info.values.map((v, index) => ({
      id: `value:${index}`,
      fieldKey: String(v.field),
      caption: v.name,
    })),
  };
}

/** The command's layout from the field list's: value fields keep their settings by id. */
export function layoutSpec(info: PivotInfo, next: CgPivotFieldListLayout): PivotLayoutSpec {
  const field = (item: CgPivotFieldListItem) =>
    item.fieldKey === CG_PIVOT_FIELD_LIST_VALUES_KEY ? -2 : Number(item.fieldKey);
  return {
    rows: next.rows.map(field),
    cols: next.columns.map(field),
    pages: next.filters.map(field).filter((f) => f >= 0),
    values: next.values.map((item) => {
      const was = /^value:(\d+)$/.exec(item.id);
      const old = was ? info.values[Number(was[1])] : undefined;
      if (old && String(old.field) === item.fieldKey) {
        return {
          field: old.field,
          summary: old.summary,
          showAs: old.showAs,
          baseField: old.baseField,
          baseItem: old.baseItem,
          name: item.caption ?? old.name,
          ...(old.numberFormat ? { numberFormat: old.numberFormat } : {}),
        };
      }
      const f = Number(item.fieldKey);
      return { field: f, summary: info.fields[f]?.numeric ? 'sum' : 'count' };
    }),
  };
}

export function usePivotTools(host: PivotToolsHost) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [info, setInfo] = useState<PivotInfo | null>(null);
  const [fieldList, setFieldList] = useState(true);
  const [generate, setGenerate] = useState(true);
  const [dialog, setDialog] = useState<PivotDialog | null>(null);
  const [filter, setFilter] = useState<{ fields: number[]; anchor: ClientBox } | null>(null);
  const buttons = useRef<CellButton[]>([]);
  const buttonFields = useRef<Map<string, number[]>>(new Map());
  const token = useRef(0);

  const send = useCallback((command: Command) => {
    hostRef.current.notice(null);
    hostRef.current
      .run(command)
      .then(async () => {
        // Excel fits a pivot table's columns each time it lays it out.
        if (
          command.kind === 'deletePivot' ||
          (command.kind === 'pivotOptions' && command.options.style !== undefined)
        )
          return;
        const engine = hostRef.current.engine();
        const pivot = 'pivot' in command ? command.pivot : undefined;
        if (!engine || !pivot) return;
        const boxes = await engine.pivots(pivot.sheet);
        const box = boxes.find((entry) => entry.name.toLowerCase() === pivot.name.toLowerCase());
        if (box) hostRef.current.fitColumns(pivot.sheet, box.left, box.right);
      })
      .catch((error) => hostRef.current.notice(errorText(error)));
  }, []);

  /** Filter buttons on a pivot table's captions, where the engine says they go. */
  const placeFor = useCallback((next: PivotInfo | null) => {
    const list: CellButton[] = [];
    const fields = new Map<string, number[]>();
    if (next && !next.readOnly) {
      for (const button of next.buttons) {
        const filtered = button.fields.some((f) =>
          next.fields[f]?.items.some((item) => item.hidden),
        );
        list.push({
          kind: 'pivot',
          row: button.row - 1,
          col: button.col - 1,
          look: filtered ? 'filtered' : 'arrow',
        });
        fields.set(`${button.row - 1}:${button.col - 1}`, button.fields);
      }
    }
    buttons.current = list;
    buttonFields.current = fields;
    hostRef.current.placeButtons();
  }, []);

  /** What the ribbon and the field list show, for the pivot table at the active cell. */
  const refresh = useCallback(() => {
    const engine = hostRef.current.engine();
    const selection = hostRef.current.selection();
    if (!engine || !selection) return;
    const mine = ++token.current;
    void engine
      .pivotInfo(hostRef.current.sheet(), selection.row + 1, selection.col + 1)
      .then((next) => {
        if (mine !== token.current) return;
        setInfo((old) => (JSON.stringify(old) === JSON.stringify(next) ? old : next));
        placeFor(next);
      })
      .catch(() => undefined);
  }, [placeFor]);

  const pivot = info ? { sheet: info.sheet, name: info.name } : null;

  const actions = useMemo<PivotActions>(
    () => ({
      insertPivot: () => {
        const selection = hostRef.current.selection();
        const engine = hostRef.current.engine();
        if (!selection || !engine) return;
        const sheet = hostRef.current.sheet();
        const area = toArea(selection.range);
        // One cell stands for the block of data around it, as Excel reads it.
        const single = area.top === area.bottom && area.left === area.right;
        void (
          single ? engine.query({ kind: 'guessTable', sheet, area }) : Promise.resolve({ area })
        )
          .then((found) => setDialog({ kind: 'create', sheet, area: found.area }))
          .catch(() => setDialog({ kind: 'create', sheet, area }));
      },
      refresh: () => {
        if (pivot) send({ kind: 'refreshPivot', pivot });
      },
      refreshAll: () => send({ kind: 'refreshPivot' }),
      toggleFieldList: () => setFieldList((open) => !open),
      options: () => setDialog({ kind: 'options' }),
      clear: () => {
        if (pivot) send({ kind: 'deletePivot', pivot });
      },
      valueSettings: () => {
        if (!info) return;
        const cell = info.cell;
        const index = cell.role === 'data' || cell.role === 'value' ? cell.value : 0;
        if (info.values.length) setDialog({ kind: 'value', index });
      },
      expand: (expanded) => {
        const cell = info?.cell;
        if (!pivot || cell?.role !== 'item') return;
        send({ kind: 'pivotField', pivot, field: cell.field, change: { expandAll: expanded } });
      },
      set: (options) => {
        if (pivot) send({ kind: 'pivotOptions', pivot, options });
      },
      groupSelection: () => {
        const cell = info?.cell;
        const selection = hostRef.current.selection();
        const engine = hostRef.current.engine();
        if (!pivot || !info || cell?.role !== 'item' || !selection || !engine) return;
        const field = info.fields[cell.field];
        if (field?.dates || (field?.numeric && !field.grouped)) {
          setDialog({ kind: 'group', field: cell.field });
          return;
        }
        // Every label of the field in the selection, as Excel groups them.
        const area = toArea(selection.range);
        const cells: Promise<PivotInfo | null>[] = [];
        for (let row = area.top; row <= area.bottom && cells.length < 500; row++) {
          for (let col = area.left; col <= area.right && cells.length < 500; col++) {
            cells.push(engine.pivotInfo(hostRef.current.sheet(), row, col));
          }
        }
        void Promise.all(cells).then((list) => {
          const items = new Set<number>();
          for (const entry of list) {
            const at = entry?.cell;
            if (at?.role === 'item' && at.field === cell.field && !at.subtotal) items.add(at.item);
          }
          send({
            kind: 'groupPivot',
            pivot,
            field: cell.field,
            group: { kind: 'items', items: [...items] },
          });
        });
      },
      groupField: () => {
        const cell = info?.cell;
        if (cell?.role === 'item') setDialog({ kind: 'group', field: cell.field });
      },
      ungroup: () => {
        const cell = info?.cell;
        if (pivot && cell?.role === 'item')
          send({ kind: 'ungroupPivot', pivot, field: cell.field });
      },
      showDetails: (atRow, atCol) => {
        const selection = hostRef.current.selection();
        const engine = hostRef.current.engine();
        if (!selection || !engine) return;
        const row = (atRow ?? selection.row) + 1;
        const col = (atCol ?? selection.col) + 1;
        const sheet = hostRef.current.sheet();
        hostRef.current.notice(null);
        void engine
          .pivotInfo(sheet, row, col)
          .then((here) => {
            if (here?.cell.role !== 'data') return null;
            return hostRef.current.run({ kind: 'pivotDetails', sheet, row, col });
          })
          .then((applied) => {
            if (!applied) return;
            const at = applied.selection;
            if (at) void hostRef.current.showAdded?.(at.sheet, at);
          })
          .catch((error) => hostRef.current.notice(errorText(error)));
      },
      filterPages: () => {
        if (info?.pages.length) setDialog({ kind: 'pages' });
      },
      toggleGenerate: () => setGenerate((on) => !on),
    }),
    [info, pivot, send],
  );

  const onCellButton = useCallback((button: CellButton, box: ClientBox) => {
    const fields = buttonFields.current.get(`${button.row}:${button.col}`);
    if (fields?.length) setFilter({ fields, anchor: box });
  }, []);

  const setValue = useCallback(
    (index: number, spec: PivotValueSpec) => {
      if (!info || !pivot) return;
      const values = info.values.map((v, i) =>
        i === index
          ? spec
          : {
              field: v.field,
              summary: v.summary,
              showAs: v.showAs,
              baseField: v.baseField,
              baseItem: v.baseItem,
              name: v.name,
              ...(v.numberFormat ? { numberFormat: v.numberFormat } : {}),
            },
      );
      send({
        kind: 'pivotLayout',
        pivot,
        layout: { rows: info.rows, cols: info.cols, pages: info.pages.map((p) => p.field), values },
      });
    },
    [info, pivot, send],
  );

  /** The pivot table's own context menu, as Excel shows it on its cells. */
  const menuItems = useMemo<CgContextMenuItem<unknown>[]>(() => {
    if (!info || !pivot) return [];
    const cell = info.cell;
    const items: CgContextMenuItem<unknown>[] = [
      {
        key: 'pivot-refresh',
        text: 'Refresh',
        icon: 'rotate',
        disabled: Boolean(info.noRefresh),
        command: actions.refresh,
      },
    ];
    if (cell.role === 'item') {
      const entry = info.fields[cell.field];
      items.push(
        {
          key: 'pivot-group',
          text: 'Group…',
          beginGroup: true,
          command: actions.groupSelection,
        },
        {
          key: 'pivot-ungroup',
          text: 'Ungroup…',
          disabled: !entry?.grouped,
          command: actions.ungroup,
        },
        {
          key: 'pivot-sort',
          text: 'Sort',
          beginGroup: true,
          children: [
            {
              key: 'pivot-sort-asc',
              text: 'Sort A to Z',
              icon: 'sort-ascending',
              command: () =>
                send({
                  kind: 'pivotField',
                  pivot,
                  field: cell.field,
                  change: { sort: 'ascending', sortBy: null },
                }),
            },
            {
              key: 'pivot-sort-desc',
              text: 'Sort Z to A',
              icon: 'sort-descending',
              command: () =>
                send({
                  kind: 'pivotField',
                  pivot,
                  field: cell.field,
                  change: { sort: 'descending', sortBy: null },
                }),
            },
          ],
        },
        {
          key: 'pivot-expand',
          text: 'Expand/Collapse',
          disabled: !cell.hasChildren,
          children: [
            {
              key: 'pivot-expand-one',
              text: 'Expand',
              command: () =>
                send({
                  kind: 'pivotField',
                  pivot,
                  field: cell.field,
                  change: { expand: [cell.item, true] },
                }),
            },
            {
              key: 'pivot-collapse-one',
              text: 'Collapse',
              command: () =>
                send({
                  kind: 'pivotField',
                  pivot,
                  field: cell.field,
                  change: { expand: [cell.item, false] },
                }),
            },
            {
              key: 'pivot-expand-all',
              text: 'Expand Entire Field',
              command: () => actions.expand(true),
            },
            {
              key: 'pivot-collapse-all',
              text: 'Collapse Entire Field',
              command: () => actions.expand(false),
            },
          ],
        },
        {
          key: 'pivot-remove-field',
          text: `Remove "${entry?.caption ?? ''}"`,
          icon: 'delete',
          command: () =>
            send({
              kind: 'pivotLayout',
              pivot,
              layout: {
                rows: info.rows.filter((f) => f !== cell.field),
                cols: info.cols.filter((f) => f !== cell.field),
                pages: info.pages.map((p) => p.field),
                values: info.values.map((v) => ({
                  ...v,
                  numberFormat: v.numberFormat ?? undefined,
                })),
              },
            }),
        },
      );
    }
    if (cell.role === 'data' || cell.role === 'value') {
      const index = cell.value;
      const value = info.values[index];
      if (value) {
        const field = info.fields[value.field];
        items.push(
          {
            key: 'pivot-summarize',
            text: 'Summarize Values By',
            beginGroup: true,
            children: SUMMARIES.map((s) => ({
              key: 'pivot-summary-' + s.key,
              text: s.text,
              checked: value.summary === s.key,
              command: () =>
                setValue(index, {
                  ...value,
                  numberFormat: value.numberFormat ?? undefined,
                  summary: s.key as PivotSummary,
                  name: valueName(s.key as PivotSummary, field?.caption ?? ''),
                }),
            })),
          },
          {
            key: 'pivot-show-as',
            text: 'Show Values As',
            children: SHOW_AS.filter((s) => !s.item).map((s) => ({
              key: 'pivot-show-' + s.key,
              text: s.text,
              checked: value.showAs === s.key,
              command: () =>
                setValue(index, {
                  ...value,
                  numberFormat: undefined,
                  showAs: s.key as PivotShowAs,
                }),
            })),
          },
          {
            key: 'pivot-value-settings',
            text: 'Value Field Settings…',
            command: () => setDialog({ kind: 'value', index }),
          },
        );
        if (cell.role === 'data') {
          items.push({
            key: 'pivot-details',
            text: 'Show Details',
            command: () => actions.showDetails(),
          });
        }
      }
    }
    items.push(
      {
        key: 'pivot-options',
        text: 'PivotTable Options…',
        beginGroup: true,
        command: actions.options,
      },
      {
        key: 'pivot-field-list',
        text: fieldList ? 'Hide Field List' : 'Show Field List',
        icon: 'sidebar',
        command: actions.toggleFieldList,
      },
    );
    return items;
  }, [actions, fieldList, info, pivot, send, setValue]);

  const fields = useMemo<CgPivotFieldListField[]>(
    () =>
      info?.fields.map((f) => ({
        key: String(f.index),
        caption: f.caption,
        defaultArea: f.numeric ? 'values' : 'rows',
      })) ?? [],
    [info],
  );

  const pane: ReactNode =
    info && fieldList ? (
      <aside className="cgs-pivot-pane" data-testid="pivot-field-list">
        <button
          type="button"
          className="cgs-pivot-pane-close"
          aria-label="Close the field list"
          data-testid="pivot-field-list-close"
          onClick={() => setFieldList(false)}
        >
          ×
        </button>
        <CgPivotFieldList
          fields={fields}
          layout={fieldListLayout(info)}
          readOnly={Boolean(info.readOnly)}
          readOnlyMessage={
            info.readOnly ? `This PivotTable can't be changed here: ${info.readOnly}.` : undefined
          }
          getValueCaption={(field) => {
            const f = info.fields[Number(field.key)];
            return f ? valueName(f.numeric ? 'sum' : 'count', f.caption) : undefined;
          }}
          onLayoutChange={(next) => {
            if (pivot) send({ kind: 'pivotLayout', pivot, layout: layoutSpec(info, next) });
          }}
          onValueFieldSettings={(item) => {
            const at = /^value:(\d+)$/.exec(item.id);
            if (at) setDialog({ kind: 'value', index: Number(at[1]) });
          }}
        />
      </aside>
    ) : null;

  const overlays: ReactNode = (
    <>
      {dialog?.kind === 'create' && (
        <CreatePivotDialog
          open
          engine={hostRef.current.engine}
          sheet={dialog.sheet}
          sheetName={hostRef.current.sheetNames[dialog.sheet] ?? ''}
          area={dialog.area}
          run={hostRef.current.run}
          onClose={() => setDialog(null)}
          onMade={(sheet, row, col) => {
            setFieldList(true);
            void hostRef.current.goTo(sheet, row, col).then(refresh);
          }}
        />
      )}
      {dialog?.kind === 'value' && info && (
        <ValueFieldDialog
          open
          info={info}
          index={dialog.index}
          onApply={(spec) => setValue(dialog.index, spec)}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === 'options' && info && (
        <PivotOptionsDialog
          open
          info={info}
          run={hostRef.current.run}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === 'group' && info && pivot && info.fields[dialog.field] && (
        <GroupingDialog
          field={info.fields[dialog.field]!}
          onGroup={(group) => send({ kind: 'groupPivot', pivot, field: dialog.field, group })}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === 'pages' && info && pivot && (
        <FilterPagesDialog
          fields={info.pages.map((p) => ({
            field: p.field,
            caption: info.fields[p.field]?.caption ?? '',
          }))}
          onShow={(field) => {
            hostRef.current.notice(null);
            void hostRef.current
              .run({ kind: 'pivotFilterPages', pivot, field })
              .then((applied) => {
                const at = applied.selection;
                if (at) void hostRef.current.showAdded?.(at.sheet, at);
              })
              .catch((error) => hostRef.current.notice(errorText(error)));
          }}
          onClose={() => setDialog(null)}
        />
      )}
      {filter && info && (
        <PivotFilterMenu
          info={info}
          fields={filter.fields}
          anchor={filter.anchor}
          run={hostRef.current.run}
          onClose={() => setFilter(null)}
        />
      )}
    </>
  );

  const state = useMemo<PivotState>(
    () => ({ hasBook: host.hasBook, info, fieldList, generate }),
    [host.hasBook, info, fieldList, generate],
  );

  return {
    state,
    actions,
    refresh,
    menuItems,
    pane,
    overlays,
    onCellButton,
    buttons: () => buttons.current,
  };
}
