// The ribbon's chart commands: Insert's Charts and Illustrations groups, and the Chart Design and
// Format tabs that show while a chart is chosen, as in Excel 2016.
import {
  CgColorPicker,
  CgIcon,
  type CgRibbonGroup,
  type CgRibbonItem,
  type CgRibbonTab,
} from '@cashgear/ui';
import type { ChartView } from '@cgs/engine-client';
import { CHART_FAMILIES, chartColor } from './ChartDialogs';
import type { ChartActions, ChartState } from './useChartTools';

function familyItem(key: string, actions: ChartActions, off: boolean): CgRibbonItem {
  const family = CHART_FAMILIES.find((entry) => entry.key === key)!;
  return {
    name: 'insert-chart-' + key,
    text: family.text,
    icon: family.icon,
    disabled: off,
    onClick: () => actions.insertChart(family.types[0]!.type),
    children: [
      ...family.types.map((choice) => ({
        name: 'insert-' + choice.type,
        text: choice.text,
        onClick: () => actions.insertChart(choice.type),
      })),
      {
        name: 'insert-more-' + key,
        text: 'More Charts…',
        beginGroup: true,
        onClick: actions.moreCharts,
      },
    ],
  };
}

/** Insert's Illustrations and Charts groups. */
export function chartInsertGroups(state: ChartState, actions: ChartActions): CgRibbonGroup[] {
  const off = !state.hasBook;
  return [
    {
      key: 'illustrations',
      text: 'Illustrations',
      icon: 'image',
      items: [
        {
          name: 'insert-picture',
          text: 'Pictures',
          icon: 'image',
          size: 'large',
          disabled: off,
          onClick: actions.insertPicture,
        },
      ],
    },
    {
      key: 'charts',
      text: 'Charts',
      icon: 'chart-column',
      items: [
        {
          name: 'insert-chart',
          text: 'Recommended Charts',
          icon: 'chart-column',
          size: 'large',
          disabled: off,
          onClick: actions.moreCharts,
        },
        familyItem('column', actions, off),
        familyItem('bar', actions, off),
        familyItem('line', actions, off),
        familyItem('area', actions, off),
        familyItem('pie', actions, off),
        familyItem('scatter', actions, off),
        familyItem('combo', actions, off),
      ],
    },
  ];
}

function hasValueAxis(view: ChartView | null) {
  return Boolean(view && view.valueAxes.length);
}

const PALETTES = [
  'Colorful Palette 1',
  'Colorful Palette 2',
  'Colorful Palette 3',
  'Colorful Palette 4',
  'Monochromatic Palette 1',
  'Monochromatic Palette 2',
  'Monochromatic Palette 3',
  'Monochromatic Palette 4',
  'Monochromatic Palette 5',
  'Monochromatic Palette 6',
  'Monochromatic Palette 7',
  'Monochromatic Palette 8',
  'Monochromatic Palette 9',
  'Monochromatic Palette 10',
  'Monochromatic Palette 11',
  'Monochromatic Palette 12',
  'Monochromatic Palette 13',
];

/** Chart Design and Format, while a chart is chosen. */
/**
 * The series Add Chart Element's trendline goes on: the one selected, else the first that can
 * have one. Null when none can, as for pie charts.
 */
function trendSeries(view: ChartState['view'], element?: ChartState['element']): number | null {
  if (!view) return null;
  const can = view.series.filter((entry) => {
    const kind = view.groups[entry.group]?.kind;
    return kind === 'bar' || kind === 'line' || kind === 'scatter';
  });
  if (!can.length) return null;
  const picked =
    element?.kind === 'series' || element?.kind === 'point' || element?.kind === 'dataLabels'
      ? element.index
      : null;
  return can.find((entry) => entry.index === picked)?.index ?? can[0]!.index;
}

export function chartTabs(state: ChartState, actions: ChartActions): CgRibbonTab[] {
  const view = state.view;
  const chart = state.drawing?.kind === 'chart' && Boolean(view);
  const off = !state.editable;
  const axes = view?.valueAxes ?? [];
  const primary = axes[0];
  const category = view?.categoryAxis ?? null;
  const element = state.element ?? { kind: 'chartArea' as const };
  const inches = (px: number | undefined) => (px === undefined ? '' : (px / 96).toFixed(2) + '"');
  return [
    {
      key: 'chart-design',
      text: 'Chart Design',
      visible: chart,
      groups: chart
        ? [
            {
              key: 'chart-layouts',
              text: 'Chart Layouts',
              icon: 'chart-column',
              items: [
                {
                  name: 'chart-add-element',
                  text: 'Add Chart Element',
                  icon: 'add',
                  size: 'large',
                  disabled: off,
                  children: [
                    ...(hasValueAxis(view)
                      ? [
                          {
                            name: 'chart-axes-horizontal',
                            text: 'Primary Horizontal Axis',
                            checked: Boolean(category?.visible),
                            onClick: () => {
                              if (category)
                                actions.edit({
                                  kind: 'axisVisible',
                                  id: category.id,
                                  visible: !category.visible,
                                });
                            },
                          },
                          {
                            name: 'chart-axes-vertical',
                            text: 'Primary Vertical Axis',
                            checked: Boolean(primary?.visible),
                            onClick: () => {
                              if (primary)
                                actions.edit({
                                  kind: 'axisVisible',
                                  id: primary.id,
                                  visible: !primary.visible,
                                });
                            },
                          },
                          {
                            name: 'chart-axis-title-horizontal',
                            text: 'Primary Horizontal Axis Title',
                            beginGroup: true,
                            checked: Boolean(category?.title),
                            onClick: () => {
                              if (category)
                                actions.edit({
                                  kind: 'axisTitle',
                                  id: category.id,
                                  text: category.title ? undefined : '',
                                });
                            },
                          },
                          {
                            name: 'chart-axis-title-vertical',
                            text: 'Primary Vertical Axis Title',
                            checked: Boolean(primary?.title),
                            onClick: () => {
                              if (primary)
                                actions.edit({
                                  kind: 'axisTitle',
                                  id: primary.id,
                                  text: primary.title ? undefined : '',
                                });
                            },
                          },
                        ]
                      : []),
                    {
                      name: 'chart-title-none',
                      text: 'Chart Title: None',
                      beginGroup: true,
                      checked: !view?.title,
                      onClick: () => actions.edit({ kind: 'title' }),
                    },
                    {
                      name: 'chart-title-above',
                      text: 'Chart Title: Above Chart',
                      checked: Boolean(view?.title),
                      onClick: () => actions.edit({ kind: 'title', text: view?.title?.text ?? '' }),
                    },
                    {
                      name: 'chart-labels-none',
                      text: 'Data Labels: None',
                      beginGroup: true,
                      onClick: () => actions.edit({ kind: 'dataLabels', show: false }),
                    },
                    ...[
                      ['center', 'Center'],
                      ['insideEnd', 'Inside End'],
                      ['insideBase', 'Inside Base'],
                      ['outsideEnd', 'Outside End'],
                      ['bestFit', 'Best Fit'],
                    ].map(([position, text]) => ({
                      name: 'chart-labels-' + position,
                      text: 'Data Labels: ' + text,
                      onClick: () => actions.edit({ kind: 'dataLabels', show: true, position }),
                    })),
                    ...(hasValueAxis(view) && primary
                      ? [
                          {
                            name: 'chart-gridlines-major',
                            text: 'Gridlines: Primary Major Horizontal',
                            beginGroup: true,
                            checked: Boolean(primary.gridlines),
                            onClick: () =>
                              actions.edit({
                                kind: 'gridlines',
                                id: primary.id,
                                major: !primary.gridlines,
                                minor: Boolean(primary.minorGridlines),
                              }),
                          },
                          {
                            name: 'chart-gridlines-minor',
                            text: 'Gridlines: Primary Minor Horizontal',
                            checked: Boolean(primary.minorGridlines),
                            onClick: () =>
                              actions.edit({
                                kind: 'gridlines',
                                id: primary.id,
                                major: Boolean(primary.gridlines),
                                minor: !primary.minorGridlines,
                              }),
                          },
                        ]
                      : []),
                    ...(
                      [
                        ['none', 'None'],
                        ['right', 'Right'],
                        ['top', 'Top'],
                        ['left', 'Left'],
                        ['bottom', 'Bottom'],
                      ] as const
                    ).map(([position, text], at) => ({
                      name: 'chart-legend-' + position,
                      text: 'Legend: ' + text,
                      beginGroup: at === 0,
                      checked:
                        position === 'none' ? !view?.legend : view?.legend?.position === position,
                      onClick: () =>
                        actions.edit({
                          kind: 'legend',
                          position: position === 'none' ? undefined : position,
                        }),
                    })),
                    ...(trendSeries(view) === null
                      ? []
                      : (
                          [
                            ['none', 'None'],
                            ['linear', 'Linear'],
                            ['exponential', 'Exponential'],
                            ['forecast', 'Linear Forecast'],
                            ['movingAverage', 'Moving Average'],
                          ] as const
                        ).map(([trend, text], at) => ({
                          name: 'chart-trendline-' + trend,
                          text: 'Trendline: ' + text,
                          beginGroup: at === 0,
                          onClick: () =>
                            actions.edit({
                              kind: 'trendline',
                              index: trendSeries(view, state.element) ?? 0,
                              ...(trend === 'none' ? {} : { trend }),
                            }),
                        }))),
                  ],
                },
                {
                  name: 'chart-quick-layout',
                  text: 'Quick Layout',
                  icon: 'table-style',
                  size: 'large',
                  disabled: off,
                  children: Array.from({ length: 11 }, (_, at) => ({
                    name: 'chart-layout-' + (at + 1),
                    text: 'Layout ' + (at + 1),
                    onClick: () => actions.edit({ kind: 'quickLayout', layout: at + 1 }),
                  })),
                },
              ],
            },
            {
              key: 'chart-styles',
              text: 'Chart Styles',
              icon: 'format-painter',
              items: [
                {
                  name: 'chart-colors',
                  text: 'Change Colors',
                  icon: 'fill-color',
                  size: 'large',
                  disabled: off,
                  children: PALETTES.map((text, at) => ({
                    name: 'chart-colors-' + (at + 1),
                    text,
                    beginGroup: at === 4,
                    onClick: () => actions.edit({ kind: 'colors', palette: at + 1 }),
                  })),
                },
                {
                  name: 'chart-style',
                  text: 'Chart Styles',
                  icon: 'table-style',
                  size: 'large',
                  disabled: off,
                  children: Array.from({ length: 8 }, (_, at) => ({
                    name: 'chart-style-' + (at + 1),
                    text: 'Style ' + (at + 1),
                    onClick: () => actions.edit({ kind: 'style', style: at + 1 }),
                  })),
                },
              ],
            },
            {
              key: 'chart-data',
              text: 'Data',
              icon: 'table',
              items: [
                {
                  name: 'chart-switch',
                  text: 'Switch Row/Column',
                  icon: 'replace',
                  size: 'large',
                  disabled: off,
                  onClick: actions.switchRowCol,
                },
                {
                  name: 'chart-select-data',
                  text: 'Select Data',
                  icon: 'table',
                  size: 'large',
                  disabled: off,
                  onClick: actions.selectData,
                },
              ],
            },
            {
              key: 'chart-type',
              text: 'Type',
              icon: 'chart-column',
              items: [
                {
                  name: 'chart-change-type',
                  text: 'Change Chart Type',
                  icon: 'chart-column',
                  size: 'large',
                  disabled: view?.kind !== 'classic',
                  onClick: actions.changeType,
                },
              ],
            },
            {
              key: 'chart-location',
              text: 'Location',
              icon: 'workbook',
              items: [
                {
                  name: 'chart-move',
                  text: 'Move Chart',
                  icon: 'workbook',
                  size: 'large',
                  onClick: actions.moveChart,
                },
              ],
            },
          ]
        : [],
    },
    {
      key: 'chart-format',
      text: 'Format',
      visible: chart,
      groups: chart
        ? [
            {
              key: 'chart-current',
              text: 'Current Selection',
              icon: 'more',
              items: [
                {
                  name: 'chart-elements',
                  text: state.elementText || 'Chart Area',
                  disabled: off,
                  children: state.elements.map((entry) => ({
                    name: 'chart-element-' + JSON.stringify(entry.element),
                    text: entry.text,
                    onClick: () => actions.selectElement(entry.element),
                  })),
                },
                {
                  name: 'chart-format-selection',
                  text: 'Format Selection',
                  icon: 'settings',
                  disabled: off,
                  onClick: actions.formatSelection,
                },
              ],
            },
            {
              key: 'chart-shape-styles',
              text: 'Shape Styles',
              icon: 'fill-color',
              items: [
                {
                  name: 'chart-shape-fill',
                  text: 'Shape Fill',
                  icon: <CgIcon name="fill-color" />,
                  disabled: off,
                  renderFlyout: ({ close }) => (
                    <CgColorPicker
                      themeColors={state.themeColors}
                      allowNone
                      allowCustomColor
                      labels={{ none: 'No Fill' }}
                      onValueChange={(value) => {
                        actions.edit({
                          kind: 'fill',
                          element,
                          color: chartColor(value),
                          none: value.kind === 'none',
                        });
                        close();
                      }}
                    />
                  ),
                },
                {
                  name: 'chart-shape-outline',
                  text: 'Shape Outline',
                  icon: <CgIcon name="line-color" />,
                  disabled: off,
                  renderFlyout: ({ close }) => (
                    <CgColorPicker
                      themeColors={state.themeColors}
                      allowNone
                      allowCustomColor
                      labels={{ none: 'No Outline' }}
                      onValueChange={(value) => {
                        actions.edit({
                          kind: 'outline',
                          element,
                          color: chartColor(value),
                          none: value.kind === 'none',
                        });
                        close();
                      }}
                    />
                  ),
                },
                {
                  name: 'chart-outline-weight',
                  text: 'Weight',
                  disabled: off,
                  children: [0.25, 0.5, 0.75, 1, 1.5, 2.25, 3, 4.5, 6].map((width) => ({
                    name: 'chart-weight-' + width,
                    text: width + ' pt',
                    onClick: () => actions.edit({ kind: 'outline', element, width }),
                  })),
                },
              ],
            },
            {
              key: 'chart-wordart',
              text: 'WordArt Styles',
              icon: 'text-color',
              items: [
                {
                  name: 'chart-text-fill',
                  text: 'Text Fill',
                  icon: <CgIcon name="text-color" />,
                  disabled: off,
                  renderFlyout: ({ close }) => (
                    <CgColorPicker
                      themeColors={state.themeColors}
                      allowCustomColor
                      onValueChange={(value) => {
                        const color = chartColor(value);
                        if (color) actions.edit({ kind: 'font', element, color });
                        close();
                      }}
                    />
                  ),
                },
              ],
            },
            {
              key: 'chart-size',
              text: 'Size',
              icon: 'column-width',
              items: [
                {
                  name: 'chart-height',
                  text: 'Height: ' + inches(state.drawing?.height),
                  children: [2, 2.5, 3, 3.5, 4, 5].map((value) => ({
                    name: 'chart-height-' + value,
                    text: value + '"',
                    onClick: () => actions.resize(null, value),
                  })),
                },
                {
                  name: 'chart-width',
                  text: 'Width: ' + inches(state.drawing?.width),
                  children: [3, 4, 5, 6, 7, 8].map((value) => ({
                    name: 'chart-width-' + value,
                    text: value + '"',
                    onClick: () => actions.resize(value, null),
                  })),
                },
              ],
            },
          ]
        : [],
    },
    {
      key: 'picture-format',
      text: 'Picture Format',
      visible: state.drawing?.kind === 'picture',
      groups:
        state.drawing?.kind === 'picture'
          ? [
              {
                key: 'picture-adjust',
                text: 'Adjust',
                icon: 'image-edit',
                items: [
                  {
                    name: 'picture-reset',
                    text: 'Reset Picture',
                    icon: 'image-edit',
                    size: 'large',
                    disabled: !state.hasBook,
                    onClick: actions.resetPicture,
                  },
                ],
              },
              {
                key: 'picture-accessibility',
                text: 'Accessibility',
                icon: 'comment',
                items: [
                  {
                    name: 'picture-alt-text',
                    text: 'Alt Text',
                    icon: 'comment',
                    size: 'large',
                    disabled: !state.hasBook,
                    onClick: actions.altText,
                  },
                ],
              },
              {
                key: 'picture-arrange',
                text: 'Arrange',
                icon: 'rotate',
                items: [
                  {
                    name: 'picture-forward',
                    text: 'Bring Forward',
                    disabled: !state.hasBook,
                    onClick: () => actions.picture({ order: 'forward' }),
                    children: [
                      {
                        name: 'picture-forward-one',
                        text: 'Bring Forward',
                        onClick: () => actions.picture({ order: 'forward' }),
                      },
                      {
                        name: 'picture-front',
                        text: 'Bring to Front',
                        onClick: () => actions.picture({ order: 'front' }),
                      },
                    ],
                  },
                  {
                    name: 'picture-backward',
                    text: 'Send Backward',
                    disabled: !state.hasBook,
                    onClick: () => actions.picture({ order: 'backward' }),
                    children: [
                      {
                        name: 'picture-backward-one',
                        text: 'Send Backward',
                        onClick: () => actions.picture({ order: 'backward' }),
                      },
                      {
                        name: 'picture-back',
                        text: 'Send to Back',
                        onClick: () => actions.picture({ order: 'back' }),
                      },
                    ],
                  },
                  {
                    name: 'picture-rotate',
                    text: 'Rotate',
                    icon: 'rotate',
                    disabled: !state.hasBook,
                    children: [
                      {
                        name: 'picture-rotate-right',
                        text: 'Rotate Right 90°',
                        onClick: () => actions.picture({ rotate: 90 }),
                      },
                      {
                        name: 'picture-rotate-left',
                        text: 'Rotate Left 90°',
                        onClick: () => actions.picture({ rotate: -90 }),
                      },
                      {
                        name: 'picture-flip-vertical',
                        text: 'Flip Vertical',
                        onClick: () => actions.picture({ flip: 'vertical' }),
                      },
                      {
                        name: 'picture-flip-horizontal',
                        text: 'Flip Horizontal',
                        onClick: () => actions.picture({ flip: 'horizontal' }),
                      },
                    ],
                  },
                ],
              },
              {
                key: 'picture-size',
                text: 'Size',
                icon: 'crop',
                items: [
                  {
                    name: 'picture-height',
                    text: 'Height: ' + inches(state.drawing?.height),
                    disabled: !state.hasBook,
                    children: [0.5, 1, 1.5, 2, 3, 4].map((value) => ({
                      name: 'picture-height-' + value,
                      text: value + '"',
                      onClick: () => actions.resizePicture(null, value),
                    })),
                  },
                  {
                    name: 'picture-width',
                    text: 'Width: ' + inches(state.drawing?.width),
                    disabled: !state.hasBook,
                    children: [0.5, 1, 1.5, 2, 3, 4].map((value) => ({
                      name: 'picture-width-' + value,
                      text: value + '"',
                      onClick: () => actions.resizePicture(value, null),
                    })),
                  },
                ],
              },
            ]
          : [],
    },
  ];
}
