// What the embed lists of a workbook and how views find their part of it (Track S3, S4).
import { describe, expect, it } from 'vitest';
import type { BookSummary, CellArea, ChartView, DrawingBox, SheetLayout } from '@cgs/engine-client';
import { catalogue, chartIds, chartTitle, resolveView, type BookEngine } from './book';
import { splitCopied } from './cells';
import { EmbedFailure } from './source';

const layout = (rightToLeft = false): SheetLayout =>
  ({
    defaultRowPx: 20,
    defaultColPx: 64,
    rightToLeft,
    showGrid: true,
    rows: new Uint32Array(),
    cols: new Uint32Array(),
    merges: new Uint32Array(),
  }) as unknown as SheetLayout;

const sheet = (name: string, kind: 'worksheet' | 'chart' = 'worksheet', state = 'visible') => ({
  name,
  kind,
  state,
  loaded: true,
  deleted: false,
  protection: null,
});

const chart = (index: number, name: string): DrawingBox => ({
  kind: 'chart',
  name,
  index,
  x: 0,
  y: 0,
  width: 480,
  height: 288,
  chart: index,
});

/** Budget (2 charts named alike), Hidden, Notes (right to left), a chart sheet; tabs reordered. */
function fakeBook(used: Record<number, CellArea | null> = {}): {
  engine: BookEngine;
  summary: BookSummary;
} {
  const summary = {
    active: 0,
    sheets: [
      sheet('Budget'),
      sheet('Hidden', 'worksheet', 'hidden'),
      sheet('Notes'),
      sheet('Chart1', 'chart'),
    ],
    order: [2, 0, 1, 3],
    structureLocked: false,
    structurePassword: false,
  } as unknown as BookSummary;
  const drawings: Record<number, DrawingBox[]> = {
    0: [chart(0, 'Sales'), { ...chart(1, 'Logo'), kind: 'picture' }, chart(2, 'sales')],
    1: [chart(0, 'Secret')],
    2: [],
    3: [{ ...chart(0, 'Chart 1'), width: 0, height: 0 }],
  };
  const engine: BookEngine = {
    layout: async (index) => layout(index === 2),
    drawings: async (index) => drawings[index] ?? [],
    chart: async (index, drawing) =>
      ({
        title: index === 0 && drawing === 0 ? { text: ' Sales\nby  month ' } : null,
      }) as unknown as ChartView,
    query: (async (question: { kind: string; sheet: number }) =>
      question.kind === 'usedRange' ? (used[question.sheet] ?? null) : null) as BookEngine['query'],
  };
  return { engine, summary };
}

describe('the catalogue', () => {
  it('lists visible worksheets in tab order, with their direction', async () => {
    const { engine, summary } = fakeBook();
    const book = await catalogue(engine, summary);
    expect(book.sheets.map(({ name, direction }) => ({ name, direction }))).toEqual([
      { name: 'Notes', direction: 'rtl' },
      { name: 'Budget', direction: 'ltr' },
    ]);
  });

  it('lists charts by their names, a repeat on one sheet numbered, and chart sheets’', async () => {
    const { engine, summary } = fakeBook();
    const book = await catalogue(engine, summary);
    expect(book.charts.map(({ id, title, sheet }) => ({ id, title, sheet }))).toEqual([
      { id: 'Sales', title: 'Sales by month', sheet: 'Budget' },
      { id: 'sales (2)', title: 'sales (2)', sheet: 'Budget' },
      { id: 'Chart 1', title: 'Chart 1', sheet: 'Chart1' },
    ]);
    // A chart sheet's chart with no size of its own takes one.
    expect(book.charts[2]!.drawing.width).toBeGreaterThan(0);
  });

  it('names charts within 128 characters', () => {
    const long = 'x'.repeat(200);
    const ids = chartIds([long, long, '']);
    expect(ids[0]).toHaveLength(128);
    expect(ids[1]).toHaveLength(128);
    expect(ids[1]!.endsWith(' (2)')).toBe(true);
    expect(ids[2]).toBe('Chart 3');
    expect(chartTitle(null, 'Chart 3')).toBe('Chart 3');
  });
});

describe('resolving a view', () => {
  async function resolve(view: Parameters<typeof resolveView>[2], used = {}) {
    const { engine, summary } = fakeBook(used);
    return resolveView(engine, await catalogue(engine, summary), view);
  }
  const failure = async (promise: Promise<unknown>) => {
    try {
      await promise;
    } catch (error) {
      return error instanceof EmbedFailure ? error.code : String(error);
    }
    return 'resolved';
  };

  it('takes the first visible sheet for a null sheet, and its used range for a null range', async () => {
    const resolved = await resolve(
      { kind: 'range', sheet: null, range: null },
      { 2: { top: 3, left: 2, bottom: 9, right: 4 } },
    );
    expect(resolved.drawn).toEqual({ kind: 'range', sheet: 'Notes', range: 'B3:D9' });
    expect(resolved.kind === 'range' && resolved.capped).toBe(false);
  });

  it('cuts a large used range, and draws A1 of an empty sheet', async () => {
    const big = await resolve(
      { kind: 'range', sheet: 'budget', range: null },
      { 0: { top: 1, left: 1, bottom: 20_000, right: 10 } },
    );
    expect(big.drawn).toEqual({ kind: 'range', sheet: 'Budget', range: 'A1:J500' });
    expect(big.kind === 'range' && big.capped).toBe(true);
    const empty = await resolve({ kind: 'range', sheet: 'Budget', range: null });
    expect(empty.drawn).toEqual({ kind: 'range', sheet: 'Budget', range: 'A1' });
  });

  it('draws a range given, whatever its corners’ order, up to 5,000 cells', async () => {
    const resolved = await resolve({ kind: 'range', sheet: 'Budget', range: 'E1000:A1' });
    expect(resolved.drawn).toEqual({ kind: 'range', sheet: 'Budget', range: 'A1:E1000' });
    expect(await failure(resolve({ kind: 'range', sheet: 'Budget', range: 'A1:E1001' }))).toBe(
      'range.tooLarge',
    );
  });

  it('finds no hidden sheet, chart sheet or missing name', async () => {
    for (const name of ['Hidden', 'Chart1', 'Nowhere']) {
      expect(await failure(resolve({ kind: 'range', sheet: name, range: 'A1' }))).toBe(
        'view.notFound',
      );
    }
  });

  it('finds a chart by its id, on its sheet or the first anywhere', async () => {
    expect((await resolve({ kind: 'chart', sheet: null, chartId: 'Chart 1' })).drawn).toEqual({
      kind: 'chart',
      sheet: 'Chart1',
      chartId: 'Chart 1',
    });
    expect((await resolve({ kind: 'chart', sheet: 'Budget', chartId: 'SALES (2)' })).drawn).toEqual(
      {
        kind: 'chart',
        sheet: 'Budget',
        chartId: 'sales (2)',
      },
    );
    expect(await failure(resolve({ kind: 'chart', sheet: 'Notes', chartId: 'Sales' }))).toBe(
      'view.notFound',
    );
    expect(await failure(resolve({ kind: 'chart', sheet: null, chartId: 'Secret' }))).toBe(
      'view.notFound',
    );
  });
});

describe('the cells’ text for screen readers', () => {
  it('splits a copy into rows and cells, with Excel’s quoting', () => {
    expect(splitCopied('a\tb\r\n"two\nlines"\t"say ""hi"""\r\n')).toEqual([
      ['a', 'b'],
      ['two\nlines', 'say "hi"'],
    ]);
    expect(splitCopied('one')).toEqual([['one']]);
  });
});
