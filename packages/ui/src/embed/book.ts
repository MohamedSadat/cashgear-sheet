// What the embed offers of an open workbook and how a view finds its part of it (Track S3, S4):
// `sheets` are the visible worksheets in tab order; `charts` are the charts on them and each visible
// chart sheet's, named as the file names them (decisions, 2026-11-14).
import type {
  BookSummary,
  CellArea,
  ChartScreen,
  ChartView,
  DrawingBox,
  EngineClient,
  SheetLayout,
} from '@cgs/engine-client';
import {
  MAX_CELLS,
  chartTitleText,
  type ChartEntry,
  type DrawnView,
  type SheetEntry,
  type View,
} from './contract';
import { capArea, cellCount, parseRange, rangeText } from './range';
import { EmbedFailure } from './source';

/** Charts in the file's own colours, as pages and previews show them. */
export const PAPER: ChartScreen = { dark: false, background: 0xffffff, text: 0x000000 };

/** A chart sheet's chart when its drawing has no size: Excel's default chart, doubled. */
const CHART_SHEET = { width: 960, height: 576 };

export type BookEngine = Pick<EngineClient, 'layout' | 'drawings' | 'chart' | 'query'>;

export interface ListedSheet extends SheetEntry {
  index: number;
}

export interface ListedChart extends ChartEntry {
  /** The sheet it is on, by index. */
  index: number;
  drawing: DrawingBox;
}

export interface Catalogue {
  sheets: ListedSheet[];
  charts: ListedChart[];
}

/** Names shared on one sheet get ` (2)`, ` (3)` after them, in drawing order; at most 128. */
export function chartIds(names: string[]): string[] {
  const seen = new Map<string, number>();
  return names.map((name, index) => {
    const base = (name.trim() || `Chart ${index + 1}`).slice(0, 128);
    const count = (seen.get(base.toLowerCase()) ?? 0) + 1;
    seen.set(base.toLowerCase(), count);
    if (count === 1) return base;
    const suffix = ` (${count})`;
    return base.slice(0, 128 - suffix.length) + suffix;
  });
}

/** The chart's title text, or its name when it has none; at most 255 characters. */
export function chartTitle(view: ChartView | null, name: string): string {
  const text = view?.title?.text.replace(/\s+/g, ' ').trim();
  return chartTitleText(text || name);
}

/** The workbook's visible sheets in tab order. */
function tabOrder(summary: BookSummary): number[] {
  const order = summary.order.length ? summary.order : summary.sheets.map((_, index) => index);
  return order.filter((index) => {
    const sheet = summary.sheets[index];
    return sheet && !sheet.deleted && sheet.state === 'visible';
  });
}

export async function catalogue(engine: BookEngine, summary: BookSummary): Promise<Catalogue> {
  const sheets: ListedSheet[] = [];
  const charts: ListedChart[] = [];
  for (const index of tabOrder(summary)) {
    const sheet = summary.sheets[index]!;
    if (sheet.kind !== 'worksheet' && sheet.kind !== 'chart') continue;
    if (sheet.kind === 'worksheet') {
      const layout = await engine.layout(index);
      sheets.push({ index, name: sheet.name, direction: layout.rightToLeft ? 'rtl' : 'ltr' });
    }
    const drawings = (await engine.drawings(index)).filter((drawing) => drawing.kind === 'chart');
    // A chart sheet's chart fills its tab, so it takes a size of its own when the file gives none.
    const shown =
      sheet.kind === 'chart'
        ? drawings
            .slice(0, 1)
            .map((drawing) =>
              drawing.width >= 16 && drawing.height >= 16
                ? drawing
                : { ...drawing, width: CHART_SHEET.width, height: CHART_SHEET.height },
            )
        : drawings;
    const ids = chartIds(shown.map((drawing) => drawing.name));
    for (const [at, drawing] of shown.entries()) {
      const view = await engine.chart(index, drawing.index, PAPER).catch(() => null);
      charts.push({
        id: ids[at]!,
        title: chartTitle(view, ids[at]!),
        sheet: sheet.name,
        index,
        drawing,
      });
    }
  }
  return { sheets, charts };
}

/** A range view, resolved: its sheet, layout and cells. */
export interface RangeResolved {
  kind: 'range';
  sheet: ListedSheet;
  layout: SheetLayout;
  area: CellArea;
  capped: boolean;
  drawn: DrawnView;
}

export interface ChartResolved {
  kind: 'chart';
  chart: ListedChart;
  drawn: DrawnView;
}

export type Resolved = RangeResolved | ChartResolved;

const sameName = (a: string, b: string) => a.toLocaleLowerCase() === b.toLocaleLowerCase();

/** Finds what a view shows, or fails `view.notFound` or `range.tooLarge`. */
export async function resolveView(
  engine: BookEngine,
  book: Catalogue,
  view: View,
  /** Fits a sheet's rows before it first shows, as the editor does. */
  prepare: (sheet: number) => Promise<void> = async () => undefined,
): Promise<Resolved> {
  const missing = () =>
    new EmbedFailure('view.notFound', 'The sheet or chart isn’t in the workbook.');
  if (view.kind === 'chart') {
    const named = book.charts.filter(
      (chart) => view.sheet === null || sameName(chart.sheet, view.sheet),
    );
    const chart =
      named.find((entry) => entry.id === view.chartId) ??
      named.find((entry) => sameName(entry.id, view.chartId));
    if (!chart) throw missing();
    return {
      kind: 'chart',
      chart,
      drawn: { kind: 'chart', sheet: chart.sheet, chartId: chart.id },
    };
  }
  const sheet =
    view.sheet === null
      ? book.sheets[0]
      : book.sheets.find((entry) => sameName(entry.name, view.sheet!));
  if (!sheet) throw missing();
  let area: CellArea;
  let capped = false;
  if (view.range !== null) {
    const given = parseRange(view.range);
    if (!given) throw missing();
    if (cellCount(given) > MAX_CELLS) {
      throw new EmbedFailure('range.tooLarge', 'The range is over 5,000 cells.');
    }
    area = given;
  } else {
    const used = await engine.query({ kind: 'usedRange', sheet: sheet.index });
    const cut = capArea(used ?? { top: 1, left: 1, bottom: 1, right: 1 });
    area = cut.area;
    capped = cut.capped;
  }
  await prepare(sheet.index);
  const layout = await engine.layout(sheet.index);
  return {
    kind: 'range',
    sheet,
    layout,
    area,
    capped,
    drawn: { kind: 'range', sheet: sheet.name, range: rangeText(area) },
  };
}

/** The colours charts take in the theme the frame shows. */
export function chartScreen(palette: {
  dark: boolean;
  background: string;
  text: string;
}): ChartScreen {
  return {
    dark: palette.dark,
    background: parseInt(palette.background.slice(1), 16) || 0xffffff,
    text: parseInt(palette.text.slice(1), 16) || 0,
  };
}
