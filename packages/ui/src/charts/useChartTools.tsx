// Charts and pictures as the shell runs them: the layer over the grid, which drawing and which of
// a chart's elements is chosen, the ribbon's Insert, Chart Design and Format commands, and the
// dialogs. Everything goes to the engine as a command through the shell's queue.
import { useCallback, useMemo, useRef, useState } from 'react';
import type { CgColorPickerThemeColor, CgContextMenuItem } from '@cashgear/ui';
import type {
  Applied,
  ChartEdit,
  ChartScreen,
  ChartType,
  ChartView,
  Command,
  DrawingBox,
  EngineClient,
} from '@cgs/engine-client';
import type { GridController, GridSelection } from '../grid/controller';
import { toArea } from '../sheet-editing';
import { ChartTypeDialog, FormatPane, MoveChartDialog, SelectDataDialog } from './ChartDialogs';
import { DrawingLayer, type DrawingPlace } from './DrawingLayer';
import { elementLabel, type EngineElement } from './chartProps';

export interface ChartToolsHost {
  engine: () => EngineClient | null;
  grid: () => GridController | null;
  sheet: () => number;
  selection: () => GridSelection | null;
  hasBook: boolean;
  /** Worksheets, by index, for Move Chart. */
  worksheets: { index: number; name: string }[];
  run: (command: Command) => Promise<Applied>;
  notice: (message: string | null) => void;
  screen: ChartScreen;
  themeColors: CgColorPickerThemeColor[];
  /** Opens the shell's context menu for a drawing. */
  menu: (x: number, y: number) => void;
}

type ChartDialog = { kind: 'insert' } | { kind: 'type' } | { kind: 'data' } | { kind: 'move' };

/** What the ribbon shows about the chosen drawing. */
export interface ChartState {
  hasBook: boolean;
  /** The chosen drawing, if any. */
  drawing: DrawingBox | null;
  /** Its chart, if it is one the engine read. */
  view: ChartView | null;
  editable: boolean;
  element: EngineElement | null;
  elementText: string;
  /** The elements Current Selection lists. */
  elements: { element: EngineElement; text: string }[];
  themeColors: CgColorPickerThemeColor[];
}

export interface ChartActions {
  insertChart(type: ChartType): void;
  moreCharts(): void;
  insertPicture(): void;
  changeType(): void;
  selectData(): void;
  switchRowCol(): void;
  moveChart(): void;
  edit(edit: ChartEdit): void;
  selectElement(element: EngineElement): void;
  formatSelection(): void;
  /** Width and height in inches. */
  resize(width: number | null, height: number | null): void;
  deleteDrawing(): void;
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Reads a picked file, turning an SVG into a PNG, which Excel 2016 needs. */
async function pictureBytes(
  file: File,
): Promise<{ bytes: Uint8Array; width: number; height: number }> {
  const url = URL.createObjectURL(file);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    const width = image.naturalWidth || 200;
    const height = image.naturalHeight || 200;
    if (file.type === 'image/svg+xml' || file.name.toLowerCase().endsWith('.svg')) {
      const canvas = document.createElement('canvas');
      canvas.width = width;
      canvas.height = height;
      canvas.getContext('2d')!.drawImage(image, 0, 0, width, height);
      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
      if (!blob) throw new Error('That picture couldn’t be read.');
      return { bytes: new Uint8Array(await blob.arrayBuffer()), width, height };
    }
    return { bytes: new Uint8Array(await file.arrayBuffer()), width, height };
  } finally {
    URL.revokeObjectURL(url);
  }
}

export function useChartTools(host: ChartToolsHost) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [drawings, setDrawings] = useState<DrawingBox[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [element, setElement] = useState<EngineElement | null>(null);
  const [views, setViews] = useState<Map<number, ChartView | null>>(new Map());
  const [version, setVersion] = useState(0);
  const [dialog, setDialog] = useState<ChartDialog | null>(null);
  const [formatOpen, setFormatOpen] = useState(false);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;

  const drawing = selected === null ? null : (drawings.find((d) => d.index === selected) ?? null);
  const view = selected === null ? null : (views.get(selected) ?? null);

  const run = useCallback(async (command: Command) => {
    try {
      return await hostRef.current.run(command);
    } catch (error) {
      hostRef.current.notice(errorText(error));
      throw error;
    }
  }, []);

  const edit = useCallback(
    (change: ChartEdit) => {
      const index = selectedRef.current;
      if (index === null) return Promise.resolve();
      return run({ kind: 'editChart', sheet: hostRef.current.sheet(), index, edit: change }).then(
        () => undefined,
      );
    },
    [run],
  );

  const select = useCallback((index: number | null, chosen: EngineElement | null) => {
    setSelected(index);
    setElement(chosen);
    if (index === null) setFormatOpen(false);
  }, []);

  const onView = useCallback((index: number, next: ChartView | null) => {
    setViews((before) => {
      if (before.get(index) === next) return before;
      const copy = new Map(before);
      copy.set(index, next);
      return copy;
    });
  }, []);

  const place = useCallback(
    (index: number, to: DrawingPlace) => {
      void run({ kind: 'setDrawingBox', sheet: hostRef.current.sheet(), index, ...to }).catch(
        () => undefined,
      );
    },
    [run],
  );

  const remove = useCallback(
    (index: number) => {
      select(null, null);
      void run({ kind: 'deleteDrawings', sheet: hostRef.current.sheet(), indexes: [index] })
        .then(() => hostRef.current.grid()?.focus())
        .catch(() => undefined);
    },
    [run, select],
  );

  const insertChart = useCallback(
    (type: ChartType) => {
      const selection = hostRef.current.selection();
      const grid = hostRef.current.grid();
      if (!selection || !grid) return Promise.resolve();
      const center = grid.viewCenter();
      const sheet = hostRef.current.sheet();
      return run({
        kind: 'insertChart',
        sheet,
        area: toArea(selection.range),
        chart: type,
        x: Math.max(0, Math.round(center.x - 240)),
        y: Math.max(0, Math.round(center.y - 144)),
      }).then(async () => {
        // The new chart is the sheet's last drawing; it is chosen, as Excel chooses it.
        const engine = hostRef.current.engine();
        const list = engine ? await engine.drawings(sheet) : [];
        const last = list[list.length - 1];
        if (last) select(last.index, { kind: 'chartArea' });
      });
    },
    [run, select],
  );

  const insertPicture = useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png,image/jpeg,image/gif,image/bmp,image/svg+xml';
    input.onchange = async () => {
      const file = input.files?.[0];
      const engine = hostRef.current.engine();
      const grid = hostRef.current.grid();
      const selection = hostRef.current.selection();
      if (!file || !engine || !grid) return;
      try {
        const { bytes, width, height } = await pictureBytes(file);
        const part = await engine.addMedia(bytes);
        if (!part) throw new Error('That file isn’t a picture this app can show.');
        const at = selection ? grid.cellOrigin(selection.row, selection.col) : { x: 0, y: 0 };
        // Very large pictures come in at a size that fits, as Excel shrinks them.
        const fit = Math.min(1, 1200 / width, 900 / height);
        await run({
          kind: 'insertPicture',
          sheet: hostRef.current.sheet(),
          part,
          x: Math.round(at.x),
          y: Math.round(at.y),
          width: Math.round(width * fit),
          height: Math.round(height * fit),
          name: file.name,
        });
      } catch (error) {
        hostRef.current.notice(errorText(error));
      }
    };
    input.click();
  }, [run]);

  const elements = useMemo(() => {
    if (!view || view.kind !== 'classic') return [];
    const list: EngineElement[] = [{ kind: 'chartArea' }, { kind: 'plotArea' }];
    if (view.title) list.push({ kind: 'title' });
    if (view.legend) list.push({ kind: 'legend' });
    if (view.categoryAxis?.visible) list.push({ kind: 'axis', id: view.categoryAxis.id });
    for (const axis of view.valueAxes) {
      if (!axis.visible) continue;
      list.push({ kind: 'axis', id: axis.id });
      if (axis.gridlines) list.push({ kind: 'gridlines', id: axis.id });
      if (axis.title) list.push({ kind: 'axisTitle', id: axis.id });
    }
    for (const series of view.series) {
      list.push({ kind: 'series', index: series.index });
      if (series.labels) list.push({ kind: 'dataLabels', index: series.index });
    }
    return list.map((entry) => ({ element: entry, text: elementLabel(view, entry) }));
  }, [view]);

  const state: ChartState = {
    hasBook: host.hasBook,
    drawing,
    view,
    editable: Boolean(view && view.kind === 'classic' && view.editable),
    element,
    elementText: view && element ? elementLabel(view, element) : '',
    elements,
    themeColors: host.themeColors,
  };

  const actions: ChartActions = useMemo(
    () => ({
      insertChart: (type) => void insertChart(type).catch(() => undefined),
      moreCharts: () => setDialog({ kind: 'insert' }),
      insertPicture,
      changeType: () => setDialog({ kind: 'type' }),
      selectData: () => setDialog({ kind: 'data' }),
      switchRowCol: () => void edit({ kind: 'switchRowCol' }).catch(() => undefined),
      moveChart: () => setDialog({ kind: 'move' }),
      edit: (change) => void edit(change).catch(() => undefined),
      selectElement: (chosen) => setElement(chosen),
      formatSelection: () => {
        if (!element) setElement({ kind: 'chartArea' });
        setFormatOpen(true);
      },
      resize: (width, height) => {
        const index = selectedRef.current;
        const box = drawings.find((d) => d.index === index);
        if (index === null || !box) return;
        place(index, {
          x: box.x,
          y: box.y,
          width: width === null ? box.width : Math.max(8, Math.round(width * 96)),
          height: height === null ? box.height : Math.max(8, Math.round(height * 96)),
        });
      },
      deleteDrawing: () => {
        if (selectedRef.current !== null) remove(selectedRef.current);
      },
    }),
    [drawings, edit, element, insertChart, insertPicture, place, remove],
  );

  const menuItems: ReadonlyArray<CgContextMenuItem<'drawing'>> = useMemo(() => {
    const chart = drawing?.kind === 'chart';
    const items: CgContextMenuItem<'drawing'>[] = [
      {
        key: 'drawing-delete',
        text: 'Delete',
        icon: 'delete',
        command: () => actions.deleteDrawing(),
      },
    ];
    if (chart && state.editable) {
      items.push(
        {
          key: 'chart-type',
          text: 'Change Chart Type…',
          beginGroup: true,
          command: actions.changeType,
        },
        { key: 'chart-data', text: 'Select Data…', command: actions.selectData },
        { key: 'chart-move', text: 'Move Chart…', command: actions.moveChart },
        {
          key: 'chart-format',
          text: 'Format ' + (state.elementText || 'Chart Area') + '…',
          beginGroup: true,
          command: actions.formatSelection,
        },
      );
    }
    return items;
  }, [actions, drawing, state.editable, state.elementText]);

  const sheet = host.sheet();
  const engine = host.engine();
  const layer = engine ? (
    <DrawingLayer
      engine={engine}
      controller={host.grid()}
      sheet={sheet}
      drawings={drawings}
      version={version}
      screen={host.screen}
      selected={selected}
      element={element}
      onSelect={select}
      onPlace={place}
      onDelete={remove}
      onFormat={(index) => {
        select(index, element ?? { kind: 'chartArea' });
        setFormatOpen(true);
      }}
      onContextMenu={(_, x, y) => hostRef.current.menu(x, y)}
      onView={onView}
      onLeave={() => hostRef.current.grid()?.focus()}
    />
  ) : null;

  const overlays = (
    <>
      <ChartTypeDialog
        open={dialog?.kind === 'insert' || dialog?.kind === 'type'}
        mode={dialog?.kind === 'insert' ? 'insert' : 'change'}
        view={view}
        onPick={(type) =>
          dialog?.kind === 'insert' ? insertChart(type) : edit({ kind: 'type', chart: type })
        }
        onSeriesTypes={async (list) => {
          for (const entry of list) {
            await edit({
              kind: 'seriesType',
              index: entry.index,
              chart: entry.type,
              secondary: entry.secondary,
            });
          }
        }}
        onClose={() => setDialog(null)}
      />
      <SelectDataDialog
        open={dialog?.kind === 'data'}
        view={view}
        onRange={async (text) => {
          const found = await hostRef.current
            .engine()
            ?.resolveName(text.replace(/^=/, ''), hostRef.current.sheet());
          if (!found) throw new Error('That isn’t a range of cells in this workbook.');
          const { sheet: data, ...area } = found;
          await edit({ kind: 'range', sheet: data, area });
        }}
        onSeries={(list) => edit({ kind: 'series', list })}
        onSwitch={() => edit({ kind: 'switchRowCol' })}
        onClose={() => setDialog(null)}
      />
      <MoveChartDialog
        open={dialog?.kind === 'move'}
        sheets={host.worksheets}
        current={sheet}
        onMove={async (to) => {
          const index = selectedRef.current;
          if (index === null) return;
          await run({ kind: 'moveDrawing', sheet: hostRef.current.sheet(), index, to });
          select(null, null);
        }}
        onClose={() => setDialog(null)}
      />
      {formatOpen && view && element && view.kind === 'classic' && (
        <FormatPane
          view={view}
          element={element}
          themeColors={host.themeColors}
          onEdit={edit}
          onClose={() => setFormatOpen(false)}
        />
      )}
    </>
  );

  return {
    state,
    actions,
    layer,
    overlays,
    menuItems,
    /** The sheet's drawings, which the shell reads with its layout. */
    setDrawings: useCallback((list: DrawingBox[]) => {
      setDrawings(list);
      setSelected((index) =>
        index !== null && list.some((d) => d.index === index) ? index : null,
      );
    }, []),
    /** Something changed: charts read their cells again. */
    refresh: useCallback(() => setVersion((n) => n + 1), []),
    /** A click in the grid leaves the drawing. */
    deselect: useCallback(() => {
      if (selectedRef.current !== null) select(null, null);
    }, [select]),
    selected,
  };
}
