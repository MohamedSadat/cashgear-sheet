import { describe, expect, it } from 'vitest';
import type { ChartAxisView, ChartSeriesView, ChartView } from '@cgs/engine-client';
import { chartElement, chartProps, elementLabel, engineElement, uniqueNames } from './chartProps';
import { dragged } from './DrawingLayer';

const axis = (id: number, position: ChartAxisView['position'], value: boolean): ChartAxisView => ({
  id,
  visible: true,
  position,
  reversed: false,
  title: null,
  labels: { text: '', size: 12, color: '#595959', bold: false, italic: false, rotation: 0 },
  line: value ? null : { color: '#D9D9D9', width: 1, dash: 'solid' },
  gridlines: value ? { color: '#D9D9D9', width: 1, dash: 'solid' } : null,
  minorGridlines: null,
  majorTick: 'none',
  minorTick: 'none',
  min: value ? 0 : null,
  max: value ? 35 : null,
  unit: value ? 5 : null,
  ticks: value ? [0, 5, 10, 15, 20, 25, 30, 35].map((v) => ({ value: v, text: String(v) })) : [],
  between: true,
  labelSkip: 1,
  crossesAt: null,
  crosses: 'autoZero',
});

const series = (name: string, index: number, values: number[], group = 0): ChartSeriesView => ({
  trendlines: [],
  name,
  group,
  values,
  x: null,
  fill: index === 0 ? '#4472C4' : '#ED7D31',
  line: null,
  marker: null,
  smooth: false,
  pointFills: [],
  pointLines: [],
  labels: null,
  index,
  explosion: 0,
  nameRef: null,
  categoriesRef: null,
  valuesRef: null,
});

const column = (): ChartView => ({
  kind: 'classic',
  extended: null,
  editable: true,
  font: 'Calibri',
  background: '#FFFFFF',
  border: { color: '#D9D9D9', width: 1, dash: 'solid' },
  rounded: false,
  plotBackground: null,
  plotBorder: null,
  plotLayout: null,
  title: { text: 'Sales', size: 18.67, color: '#595959', bold: false, italic: false, rotation: 0 },
  legend: {
    position: 'bottom',
    overlay: false,
    look: { text: '', size: 12, color: '#595959', bold: false, italic: false, rotation: 0 },
    hidden: [],
    fill: null,
    border: null,
  },
  categories: ['North', 'North', 'East'],
  categoryAxis: axis(1, 'bottom', false),
  valueAxes: [axis(2, 'left', true)],
  groups: [
    {
      kind: 'bar',
      fileKind: 'barChart',
      horizontal: false,
      stacking: 'none',
      gapWidth: 219,
      overlap: -27,
      hole: 0,
      firstSlice: 0,
      varyColors: false,
      axis: 0,
    },
  ],
  series: [series('Sales', 0, [10, 20, 30]), series('Sales', 1, [4, 8, 12])],
  approximate: false,
  blanks: 'gap',
});

describe('chartProps', () => {
  it('keeps categories that share a name apart, and series names unique', () => {
    const view = column();
    const props = chartProps(view, 480, 288);
    expect(props.data).toHaveLength(3);
    expect(props.series.map((s) => s.name)).toEqual(uniqueNames(view.series));
    expect(new Set(props.series.map((s) => s.name)).size).toBe(2);
    const formatter = props.argumentAxis?.labelFormatter;
    expect(formatter?.(1)).toBe('North');
    expect(props.argumentAxis?.categoryPlacement).toBe('betweenTicks');
  });

  it('passes the engine’s scale, gap and look through as they are', () => {
    const props = chartProps(column(), 480, 288);
    const value = props.valueAxes?.[0];
    expect(value?.minimum).toBe(0);
    expect(value?.maximum).toBe(35);
    expect(value?.tickInterval).toBe(5);
    expect(value?.barGapWidth).toBe(219);
    expect(value?.barOverlap).toBe(-27);
    expect(props.appearance?.chartArea?.fill).toBe('#FFFFFF');
    expect(props.legend?.position).toBe('bottom');
    expect(props.interactive).toBe(false);
    expect(props.direction).toBe('ltr');
    const first = props.series[0]!;
    expect(first.type).toBe('bar');
    expect(first.value({ i: 2 })).toBe(30);
  });

  it('adds up stacked lines, as CgChart draws lines unstacked', () => {
    const view = column();
    view.groups[0] = { ...view.groups[0]!, kind: 'line', stacking: 'stacked' };
    const props = chartProps(view, 480, 288);
    expect(props.series[1]!.value({ i: 0 })).toBe(14);
    view.groups[0] = { ...view.groups[0]!, stacking: 'percent' };
    const percent = chartProps(view, 480, 288);
    expect(percent.series[1]!.value({ i: 2 })).toBeCloseTo(1);
  });

  it('runs a bar chart’s categories up from the bottom, as Excel does', () => {
    const view = column();
    view.groups[0] = { ...view.groups[0]!, horizontal: true };
    const props = chartProps(view, 480, 288);
    expect(props.orientation).toBe('horizontal');
    expect(props.argumentAxis?.reversed).toBe(true);
  });

  it('starts a pie where Excel does', () => {
    const view = column();
    view.groups[0] = { ...view.groups[0]!, kind: 'pie', firstSlice: 90 };
    view.series = [view.series[0]!];
    view.categoryAxis = null;
    view.valueAxes = [];
    const props = chartProps(view, 480, 288);
    const pie = props.series[0] as { startAngleDegrees?: number; type: string };
    expect(pie.type).toBe('pie');
    expect(pie.startAngleDegrees).toBe(0);
  });

  it('names elements both ways', () => {
    const view = column();
    const series = engineElement(view, {
      kind: 'series',
      seriesName: uniqueNames(view.series)[1]!,
    });
    expect(series).toEqual({ kind: 'series', index: 1 });
    expect(chartElement(view, series)).toEqual({
      kind: 'series',
      seriesName: uniqueNames(view.series)[1],
    });
    expect(engineElement(view, { kind: 'axis', axis: 'value', axisName: 'v2' })).toEqual({
      kind: 'axis',
      id: 2,
    });
    expect(elementLabel(view, { kind: 'axis', id: 2 })).toBe('Vertical (Value) Axis');
    expect(elementLabel(view, { kind: 'gridlines', id: 2 })).toBe(
      'Vertical (Value) Axis Major Gridlines',
    );
  });
});

describe('dragged', () => {
  const start = { x: 100, y: 50, width: 400, height: 200 };
  const drag = (handle: Parameters<typeof dragged>[0]['handle'], keepRatio = false) => ({
    index: 0,
    handle,
    startX: 0,
    startY: 0,
    start,
    zoom: 2,
    moved: true,
    keepRatio,
  });

  it('moves by the pointer’s travel at the sheet’s zoom', () => {
    expect(dragged(drag(null), 40, -20, false)).toEqual({ x: 120, y: 40, width: 400, height: 200 });
  });

  it('sizes from the handle that is dragged', () => {
    expect(dragged(drag('se'), 100, 50, false)).toEqual({ x: 100, y: 50, width: 450, height: 225 });
    expect(dragged(drag('w'), 100, 0, false)).toEqual({ x: 150, y: 50, width: 350, height: 200 });
  });

  it('keeps a picture’s proportions from a corner', () => {
    const out = dragged(drag('se', true), 200, 0, false);
    expect(out.width / out.height).toBeCloseTo(2);
  });
});
