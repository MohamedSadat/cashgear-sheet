// Turns the engine's chart view into CgChart's props. The engine has already worked out
// everything Excel decides (scales, labels, colours, fonts), so this only maps names. Pure, so
// tests can check it without a browser.
import type {
  CgChartAppearance,
  CgChartArgumentAxisDescriptor,
  CgChartDashStyle,
  CgChartElementRef,
  CgChartMarkerSymbol,
  CgChartPointLabelPosition,
  CgChartProps,
  CgChartSeriesDescriptor,
  CgChartStrokeStyle,
  CgChartTextStyle,
  CgChartTickMarkStyle,
  CgChartValueAxisDescriptor,
} from '@cashgear/ui';
import type {
  ChartAxisView,
  ChartGroupView,
  ChartLineView,
  ChartSeriesView,
  ChartTextView,
  ChartView,
} from '@cgs/engine-client';

/** One category of a category chart, by its place: categories with the same text stay apart. */
export interface ChartItem {
  i: number;
  x?: number | null;
  y?: number | null;
}

/** Excel's fallback fonts after the chart's own, as Calibri is not on every system. */
const FONT_FALLBACK = ', Carlito, "Segoe UI", Arial, sans-serif';

export function stroke(line: ChartLineView | null): CgChartStrokeStyle {
  if (!line) return { color: 'none' };
  return { color: line.color, width: line.width, dashStyle: line.dash as CgChartDashStyle };
}

export function textStyle(look: ChartTextView | null | undefined): CgChartTextStyle | undefined {
  if (!look) return undefined;
  return { fontSize: look.size, color: look.color, bold: look.bold, italic: look.italic };
}

const TICKS: Record<ChartAxisView['majorTick'], CgChartTickMarkStyle> = {
  none: 'none',
  in: 'inside',
  out: 'outside',
  cross: 'cross',
};

const MARKERS: Record<string, CgChartMarkerSymbol> = {
  circle: 'circle',
  square: 'square',
  diamond: 'diamond',
  triangle: 'triangle',
  x: 'cross',
  plus: 'cross',
  star: 'cross',
  dash: 'square',
  dot: 'circle',
  picture: 'square',
  none: 'none',
};

const LABEL_POSITIONS: Record<string, CgChartPointLabelPosition> = {
  bestFit: 'bestFit',
  center: 'center',
  insideEnd: 'insideEnd',
  insideBase: 'insideBase',
  outsideEnd: 'outsideEnd',
  above: 'above',
  below: 'below',
  left: 'left',
  right: 'right',
};

/** Series names that are unique, as CgChart needs, though Excel lets two series share one. */
export function uniqueNames(series: ChartSeriesView[]): string[] {
  const seen = new Map<string, number>();
  return series.map((entry) => {
    const name = entry.name.trim() ? entry.name : `Series${entry.index + 1}`;
    const count = seen.get(name) ?? 0;
    seen.set(name, count + 1);
    return count === 0 ? name : name + '​'.repeat(count);
  });
}

function valueAxisName(axis: ChartAxisView) {
  return 'v' + axis.id;
}

function legendPosition(view: ChartView) {
  if (!view.legend) return 'none' as const;
  switch (view.legend.position) {
    case 'left':
      return 'start' as const;
    case 'right':
      return 'end' as const;
    case 'top':
      return 'top' as const;
    case 'topRight':
      return 'topEnd' as const;
    default:
      return 'bottom' as const;
  }
}

/** A stacked or 100% line series has no stacking in CgChart, so its values are added up here. */
function stackedLines(view: ChartView): Map<number, (number | null)[]> {
  const out = new Map<number, (number | null)[]>();
  view.groups.forEach((group, groupIndex) => {
    if (group.kind !== 'line' || group.stacking === 'none') return;
    const members = view.series
      .map((series, index) => ({ series, index }))
      .filter(({ series }) => series.group === groupIndex);
    const length = Math.max(0, ...members.map(({ series }) => series.values.length));
    const totals = Array.from({ length }, (_, at) =>
      members.reduce((sum, { series }) => sum + Math.abs(series.values[at] ?? 0), 0),
    );
    const running = new Array<number>(length).fill(0);
    for (const { series, index } of members) {
      out.set(
        index,
        series.values.map((value, at) => {
          if (value === null) return null;
          running[at] = (running[at] ?? 0) + value;
          const total = totals[at] ?? 0;
          return group.stacking === 'percent' ? (total ? running[at]! / total : 0) : running[at]!;
        }),
      );
    }
  });
  return out;
}

function valueAxis(
  axis: ChartAxisView,
  index: number,
  bars: ChartGroupView | undefined,
): CgChartValueAxisDescriptor {
  return {
    name: valueAxisName(axis),
    visible: axis.visible,
    position:
      axis.position === 'right' || axis.position === 'top' ? 'end' : index === 0 ? 'start' : 'end',
    minimum: axis.min ?? undefined,
    maximum: axis.max ?? undefined,
    tickInterval: axis.unit ?? undefined,
    ticks: axis.ticks.length ? axis.ticks : undefined,
    reversed: axis.reversed,
    title: axis.title?.text,
    titleStyle: textStyle(axis.title),
    showGridLines: Boolean(axis.gridlines),
    majorGridLineStyle: axis.gridlines ? stroke(axis.gridlines) : undefined,
    showMinorGridLines: Boolean(axis.minorGridlines),
    minorGridLineStyle: axis.minorGridlines ? stroke(axis.minorGridlines) : undefined,
    axisLineStyle: stroke(axis.line),
    showAxisLine: Boolean(axis.line),
    majorTickMarks: TICKS[axis.majorTick],
    minorTickMarks: TICKS[axis.minorTick],
    labelPosition: axis.labels ? 'nextToAxis' : 'none',
    labelStyle: textStyle(axis.labels),
    includeZero: false,
    showZeroLine: false,
    argumentAxisCrossesAt:
      axis.crosses === 'min'
        ? 'minimum'
        : axis.crosses === 'max'
          ? 'maximum'
          : axis.crossesAt !== null
            ? axis.crossesAt
            : 'auto',
    barGapWidth: bars?.gapWidth,
    barOverlap: bars ? (bars.stacking === 'none' ? bars.overlap : 100) : undefined,
  };
}

/** CgChart's props for a chart the engine described, drawn at `width` by `height` pixels. */
export function chartProps(
  view: ChartView,
  width: number,
  height: number,
  locale = 'en-US',
): CgChartProps<ChartItem> {
  const names = uniqueNames(view.series);
  const round =
    view.groups.length > 0 && view.groups.every((g) => g.kind === 'pie' || g.kind === 'doughnut');
  const scatter = view.groups[0]?.kind === 'scatter';
  const horizontal = view.groups.some((g) => g.kind === 'bar' && g.horizontal);
  const count = Math.max(view.categories.length, ...view.series.map((s) => s.values.length), 0);
  const data: ChartItem[] = Array.from({ length: count }, (_, i) => ({ i }));
  const stacked = stackedLines(view);
  const missing = view.blanks === 'zero' ? 'zero' : view.blanks === 'span' ? 'skip' : 'gap';
  const axisOf = (group: ChartGroupView) => {
    const axis = view.valueAxes[group.axis] ?? view.valueAxes[0];
    return axis ? valueAxisName(axis) : undefined;
  };
  const pieLabels = view.categories.map((text, at) => {
    const before = view.categories.slice(0, at).filter((other) => other === text).length;
    return (text || String(at + 1)) + '​'.repeat(before);
  });

  const series: CgChartSeriesDescriptor<ChartItem>[] = view.series.map((entry, index) => {
    const group = view.groups[entry.group] ?? view.groups[0]!;
    const name = names[index]!;
    const values = stacked.get(index) ?? entry.values;
    const value = (item: ChartItem) => values[item.i] ?? null;
    const labels = entry.labels;
    const common = {
      name,
      missingValueMode: missing,
      showPointLabels: Boolean(labels),
      pointLabelPosition: labels ? LABEL_POSITIONS[labels.position] : undefined,
      pointLabelStyle: labels ? textStyle(labels.look) : undefined,
      tooltipEnabled: false,
    } as const;
    const marker = entry.marker
      ? {
          showMarkers: entry.marker.symbol !== 'none',
          markerSymbol: MARKERS[entry.marker.symbol] ?? 'circle',
          markerSize: entry.marker.size,
          markerFillColor: entry.marker.fill ?? 'none',
          markerOutlineColor: entry.marker.line?.color,
          markerOutlineWidth: entry.marker.line?.width,
        }
      : { showMarkers: false };
    switch (group.kind) {
      case 'bar':
        return {
          ...common,
          type: 'bar',
          argument: (item) => item.i,
          value,
          valueAxisName: axisOf(group),
          color: entry.fill ?? entry.line?.color ?? undefined,
          fillColor: entry.fill ?? 'none',
          outlineColor: entry.line?.color ?? undefined,
          outlineWidth: entry.line?.width,
          outlineDashStyle: entry.line?.dash as CgChartDashStyle | undefined,
          stacking:
            group.stacking === 'none'
              ? 'none'
              : group.stacking === 'percent'
                ? 'fullStacked'
                : 'stacked',
          stackName: group.stacking === 'none' ? undefined : 'g' + entry.group,
        };
      case 'area':
        return {
          ...common,
          type: 'area',
          argument: (item) => item.i,
          value,
          valueAxisName: axisOf(group),
          color: entry.fill ?? undefined,
          fillColor: entry.fill ?? 'none',
          outlineColor: entry.line?.color ?? 'none',
          lineWidth: entry.line?.width,
          areaOpacity: 1,
          stacking:
            group.stacking === 'none'
              ? 'none'
              : group.stacking === 'percent'
                ? 'fullStacked'
                : 'stacked',
          stackName: group.stacking === 'none' ? undefined : 'g' + entry.group,
          ...marker,
        };
      case 'line':
        return {
          ...common,
          type: 'line',
          argument: (item) => item.i,
          value,
          valueAxisName: axisOf(group),
          color: entry.line?.color ?? entry.marker?.fill ?? undefined,
          lineColor: entry.line?.color ?? 'none',
          lineWidth: entry.line?.width ?? 3,
          dashStyle: (entry.line?.dash ?? 'solid') as CgChartDashStyle,
          lineStyle: entry.smooth ? 'smooth' : 'straight',
          ...marker,
        };
      case 'scatter': {
        const points: ChartItem[] = entry.values.map((y, i) => ({
          i,
          x: entry.x?.[i] ?? i + 1,
          y,
        }));
        return {
          ...common,
          type: 'scatter',
          data: points,
          argument: (item) => item.x ?? null,
          value: (item) => item.y ?? null,
          valueAxisName: axisOf(group),
          color: entry.marker?.fill ?? entry.line?.color ?? undefined,
          showLine: Boolean(entry.line),
          lineColor: entry.line?.color ?? 'none',
          lineWidth: entry.line?.width,
          dashStyle: (entry.line?.dash ?? 'solid') as CgChartDashStyle,
          lineStyle: entry.smooth ? 'smooth' : 'straight',
          ...marker,
        };
      }
      default:
        return {
          ...common,
          type: group.kind === 'doughnut' ? 'donut' : 'pie',
          argument: (item) => pieLabels[item.i] ?? String(item.i + 1),
          value,
          innerRadiusRatio:
            group.kind === 'doughnut' ? Math.min(0.9, Math.max(0.1, group.hole / 100)) : undefined,
          startAngleDegrees: group.firstSlice - 90,
          outlineColor: entry.pointLines.find(Boolean)?.color ?? entry.line?.color ?? undefined,
          outlineWidth: entry.pointLines.find(Boolean)?.width ?? entry.line?.width,
          explosion: entry.explosion ? entry.explosion / 100 : undefined,
        };
    }
  });

  // Trendlines, drawn as Excel draws them: a line over the series' points, in its legend too.
  view.series.forEach((entry) => {
    const group = view.groups[entry.group] ?? view.groups[0]!;
    for (const trend of entry.trendlines ?? []) {
      const line = {
        name: trend.name,
        missingValueMode: 'gap' as const,
        tooltipEnabled: false,
        valueAxisName: axisOf(group),
        color: trend.line?.color ?? undefined,
        lineColor: trend.line?.color ?? undefined,
        lineWidth: trend.line?.width ?? 2,
        dashStyle: (trend.line?.dash ?? 'solid') as CgChartDashStyle,
        lineStyle: 'straight' as const,
        showMarkers: false,
      };
      if (group.kind === 'scatter') {
        const points: ChartItem[] = trend.values
          .map((y, i) => ({ i, x: entry.x?.[i] ?? i + 1, y }))
          .filter((point) => point.y !== null)
          .sort((a, b) => (a.x ?? 0) - (b.x ?? 0));
        series.push({
          ...line,
          type: 'scatter',
          data: points,
          argument: (item) => item.x ?? null,
          value: (item) => item.y ?? null,
          showLine: true,
        });
      } else {
        series.push({
          ...line,
          type: 'line',
          argument: (item) => item.i,
          value: (item) => trend.values[item.i] ?? null,
        });
      }
    }
  });

  const categoryAxis = view.categoryAxis;
  const primaryBars = view.groups.find((g) => g.kind === 'bar' && g.axis === 0);
  const secondaryBars = view.groups.find((g) => g.kind === 'bar' && g.axis === 1);
  const valueAxes = round
    ? undefined
    : view.valueAxes.map((axis, index) =>
        valueAxis(axis, index, index === 0 ? primaryBars : secondaryBars),
      );

  let argumentAxis: CgChartArgumentAxisDescriptor | undefined;
  if (!round && categoryAxis) {
    const shared = {
      visible: categoryAxis.visible,
      title: categoryAxis.title?.text,
      titleStyle: textStyle(categoryAxis.title),
      // CgChart runs a bar chart's categories down from the top; Excel runs them up.
      reversed: horizontal ? !categoryAxis.reversed : categoryAxis.reversed,
      valueAxisCrossesAt: 'auto' as const,
      showGridLines: Boolean(categoryAxis.gridlines),
      majorGridLineStyle: categoryAxis.gridlines ? stroke(categoryAxis.gridlines) : undefined,
      showMinorGridLines: Boolean(categoryAxis.minorGridlines),
      minorGridLineStyle: categoryAxis.minorGridlines
        ? stroke(categoryAxis.minorGridlines)
        : undefined,
      axisLineStyle: stroke(categoryAxis.line),
      showAxisLine: Boolean(categoryAxis.line),
      majorTickMarks: TICKS[categoryAxis.majorTick],
      minorTickMarks: TICKS[categoryAxis.minorTick],
      labelPosition: categoryAxis.labels ? ('nextToAxis' as const) : ('none' as const),
      labelStyle: textStyle(categoryAxis.labels),
      labelOverlapMode: 'auto' as const,
    };
    argumentAxis = scatter
      ? {
          ...shared,
          valueType: 'numeric',
          minimum: categoryAxis.min ?? undefined,
          maximum: categoryAxis.max ?? undefined,
          tickInterval: categoryAxis.unit ?? undefined,
          ticks: categoryAxis.ticks.length ? categoryAxis.ticks : undefined,
        }
      : {
          ...shared,
          valueType: 'category',
          labelFormatter: (value) => view.categories[Number(value)] ?? '',
          labelInterval: categoryAxis.labelSkip > 1 ? categoryAxis.labelSkip : undefined,
          categoryPlacement: categoryAxis.between ? 'betweenTicks' : 'onTicks',
        };
  }

  const font = (view.font || 'Calibri') + FONT_FALLBACK;
  const firstAxisLabels = view.valueAxes[0]?.labels ?? categoryAxis?.labels ?? null;
  const appearance: CgChartAppearance = {
    fontFamily: font,
    chartArea: {
      fill: view.background ?? 'none',
      border: view.border ? stroke(view.border) : 'none',
      cornerRadius: view.rounded ? 8 : 0,
      padding: 7,
    },
    plotArea: {
      fill: view.plotBackground ?? 'none',
      border: view.plotBorder ? stroke(view.plotBorder) : 'none',
      layout: view.plotLayout
        ? {
            x: view.plotLayout[0],
            y: view.plotLayout[1],
            width: view.plotLayout[2],
            height: view.plotLayout[3],
          }
        : undefined,
    },
    title: view.title ? { ...textStyle(view.title), maxLines: 2 } : undefined,
    axisLabels: textStyle(firstAxisLabels),
    axisTitles: textStyle(view.valueAxes[0]?.title ?? categoryAxis?.title),
    legend: textStyle(view.legend?.look),
  };

  const pointColors = view.series.map((entry) => entry.pointFills);
  const labelTexts = view.series.map((entry) => entry.labels?.texts ?? null);
  const nameIndex = new Map(names.map((name, index) => [name, index]));
  return {
    data,
    series,
    argumentAxis,
    valueAxes,
    orientation: horizontal ? 'horizontal' : 'vertical',
    title: view.title?.text,
    legend: { position: legendPosition(view), mode: 'static' },
    tooltip: { enabled: false },
    appearance,
    interactive: false,
    animation: 'none',
    width,
    height,
    locale,
    maxPointsPerSeries: 32000,
    // The grid stays left to right, and so do its charts.
    direction: 'ltr',
    enableKeyboardNavigation: false,
    customizePoint: (context) => {
      const index = nameIndex.get(context.seriesName);
      if (index === undefined) return null;
      const color = pointColors[index]?.[context.pointIndex];
      const text = labelTexts[index]?.[context.pointIndex];
      if (!color && text === undefined) return null;
      return { ...(color ? { color } : {}), ...(text !== undefined ? { labelText: text } : {}) };
    },
  };
}

/** What the engine calls a chart element, from what CgChart reports under the pointer. */
export type EngineElement =
  | { kind: 'chartArea' }
  | { kind: 'plotArea' }
  | { kind: 'title' }
  | { kind: 'legend' }
  | { kind: 'axis'; id: number }
  | { kind: 'axisTitle'; id: number }
  | { kind: 'gridlines'; id: number }
  | { kind: 'series'; index: number }
  | { kind: 'point'; index: number; point: number }
  | { kind: 'dataLabels'; index: number };

export function engineElement(view: ChartView, element: CgChartElementRef): EngineElement {
  const names = uniqueNames(view.series);
  const series = (name: string) => view.series[names.indexOf(name)]?.index ?? 0;
  const axisId = (ref: { axis: 'argument' } | { axis: 'value'; axisName: string }) =>
    ref.axis === 'argument'
      ? (view.categoryAxis?.id ?? 0)
      : Number(ref.axisName.slice(1)) || (view.valueAxes[0]?.id ?? 0);
  switch (element.kind) {
    case 'chartArea':
    case 'plotArea':
    case 'title':
    case 'legend':
      return { kind: element.kind };
    case 'legendEntry':
      return { kind: 'series', index: series(element.seriesName) };
    case 'axis':
      return { kind: 'axis', id: axisId(element) };
    case 'axisTitle':
      return { kind: 'axisTitle', id: axisId(element) };
    case 'majorGridLines':
    case 'minorGridLines':
      return { kind: 'gridlines', id: axisId(element) };
    case 'series':
      return { kind: 'series', index: series(element.seriesName) };
    case 'point':
      return { kind: 'point', index: series(element.seriesName), point: element.pointIndex };
    case 'pointLabels':
      return { kind: 'dataLabels', index: series(element.seriesName) };
  }
}

/** CgChart's name for an element the engine names, to draw it as selected. */
export function chartElement(
  view: ChartView,
  element: EngineElement | null,
): CgChartElementRef | null {
  if (!element) return null;
  const names = uniqueNames(view.series);
  const name = (index: number) => names[view.series.findIndex((s) => s.index === index)] ?? '';
  const axis = (id: number) =>
    view.categoryAxis?.id === id
      ? ({ axis: 'argument' } as const)
      : ({ axis: 'value', axisName: 'v' + id } as const);
  switch (element.kind) {
    case 'chartArea':
    case 'plotArea':
    case 'title':
    case 'legend':
      return { kind: element.kind };
    case 'axis':
      return { kind: 'axis', ...axis(element.id) };
    case 'axisTitle':
      return { kind: 'axisTitle', ...axis(element.id) };
    case 'gridlines':
      return { kind: 'majorGridLines', ...axis(element.id) };
    case 'series':
      return { kind: 'series', seriesName: name(element.index) };
    case 'point':
      return { kind: 'point', seriesName: name(element.index), pointIndex: element.point };
    case 'dataLabels':
      return { kind: 'pointLabels', seriesName: name(element.index) };
  }
}

/** The element's name as Excel's Format tab lists it. */
export function elementLabel(view: ChartView, element: EngineElement): string {
  const seriesName = (index: number) =>
    view.series.find((s) => s.index === index)?.name ?? 'Series';
  const axisName = (id: number) => {
    if (view.categoryAxis?.id === id)
      return view.groups[0]?.kind === 'scatter'
        ? 'Horizontal (Value) Axis'
        : 'Horizontal (Category) Axis';
    const index = view.valueAxes.findIndex((a) => a.id === id);
    return index > 0 ? 'Secondary Vertical (Value) Axis' : 'Vertical (Value) Axis';
  };
  switch (element.kind) {
    case 'chartArea':
      return 'Chart Area';
    case 'plotArea':
      return 'Plot Area';
    case 'title':
      return 'Chart Title';
    case 'legend':
      return 'Legend';
    case 'axis':
      return axisName(element.id);
    case 'axisTitle':
      return axisName(element.id) + ' Title';
    case 'gridlines':
      return axisName(element.id) + ' Major Gridlines';
    case 'series':
      return 'Series "' + seriesName(element.index) + '"';
    case 'point':
      return 'Series "' + seriesName(element.index) + '" Point ' + (element.point + 1);
    case 'dataLabels':
      return 'Series "' + seriesName(element.index) + '" Data Labels';
  }
}
