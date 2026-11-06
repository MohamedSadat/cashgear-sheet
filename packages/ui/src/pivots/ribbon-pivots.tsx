// The ribbon's pivot table commands: Insert › PivotTable, and the PivotTable Analyze and Design tabs
// that show while the active cell is in a pivot table, as in Excel 2016.
import type { CgRibbonItem, CgRibbonTab } from '@cashgear/ui';
import type { PivotActions, PivotState } from './usePivotTools';

/** Insert › PivotTable, at the front of the Tables group. */
export function pivotInsertItem(state: PivotState, actions: PivotActions): CgRibbonItem {
  return {
    name: 'insert-pivot',
    text: 'PivotTable',
    icon: 'table',
    size: 'large',
    disabled: !state.hasBook,
    onClick: actions.insertPivot,
  };
}

/** Excel 2016's built-in pivot styles, by group. */
export const PIVOT_STYLES = [
  { group: 'Light', count: 28 },
  { group: 'Medium', count: 28 },
  { group: 'Dark', count: 28 },
].flatMap(({ group, count }) =>
  Array.from({ length: count }, (_, i) => ({
    name: `PivotStyle${group}${i + 1}`,
    text: `${group} ${i + 1}`,
    group,
  })),
);

export function pivotTabs(state: PivotState, actions: PivotActions): CgRibbonTab[] {
  const info = state.info;
  const shown = Boolean(info);
  const off = Boolean(info?.readOnly);
  const options = info?.options;
  const style = options?.style;
  const active = info?.cell.role === 'item' ? info.fields[info.cell.field] : undefined;
  return [
    {
      key: 'pivot-analyze',
      text: 'PivotTable Analyze',
      visible: shown,
      groups: info
        ? [
            {
              key: 'pivot-table',
              text: 'PivotTable',
              icon: 'table',
              items: [
                {
                  name: 'pivot-name',
                  text: info.name,
                  icon: 'table',
                  size: 'large',
                  disabled: off,
                  onClick: actions.options,
                },
                {
                  name: 'pivot-options',
                  text: 'Options',
                  icon: 'settings',
                  disabled: off,
                  onClick: actions.options,
                },
              ],
            },
            {
              key: 'pivot-active-field',
              text: 'Active Field',
              icon: 'add',
              items: [
                {
                  name: 'pivot-active-field-name',
                  text: active?.caption ?? 'Active Field',
                  disabled: true,
                },
                {
                  name: 'pivot-value-settings',
                  text: 'Field Settings',
                  icon: 'settings',
                  disabled: off || !info.values.length,
                  onClick: actions.valueSettings,
                },
                {
                  name: 'pivot-expand-field',
                  text: 'Expand Field',
                  icon: 'add',
                  disabled: off || info.cell.role !== 'item' || !info.cell.hasChildren,
                  onClick: () => actions.expand(true),
                },
                {
                  name: 'pivot-collapse-field',
                  text: 'Collapse Field',
                  icon: 'minus',
                  disabled: off || info.cell.role !== 'item' || !info.cell.hasChildren,
                  onClick: () => actions.expand(false),
                },
              ],
            },
            {
              key: 'pivot-data',
              text: 'Data',
              icon: 'rotate',
              items: [
                {
                  name: 'pivot-refresh',
                  text: 'Refresh',
                  icon: 'rotate',
                  size: 'large',
                  disabled: Boolean(info.noRefresh),
                  onClick: actions.refresh,
                  children: [
                    { name: 'pivot-refresh-one', text: 'Refresh', onClick: actions.refresh },
                    { name: 'pivot-refresh-all', text: 'Refresh All', onClick: actions.refreshAll },
                  ],
                },
              ],
            },
            {
              key: 'pivot-actions',
              text: 'Actions',
              icon: 'delete',
              items: [
                {
                  name: 'pivot-clear',
                  text: 'Clear All',
                  icon: 'delete',
                  size: 'large',
                  disabled: off,
                  onClick: actions.clear,
                },
              ],
            },
            {
              key: 'pivot-show',
              text: 'Show',
              icon: 'sidebar',
              items: [
                {
                  name: 'pivot-field-list',
                  text: 'Field List',
                  icon: 'sidebar',
                  size: 'large',
                  checked: state.fieldList,
                  onClick: actions.toggleFieldList,
                },
                {
                  name: 'pivot-field-headers',
                  text: 'Field Headers',
                  checked: Boolean(options?.headers),
                  disabled: off,
                  onClick: () => actions.set({ headers: !options?.headers }),
                },
              ],
            },
          ]
        : [],
    },
    {
      key: 'pivot-design',
      text: 'Design',
      visible: shown,
      groups: info
        ? [
            {
              key: 'pivot-layout',
              text: 'Layout',
              icon: 'table-style',
              items: [
                {
                  name: 'pivot-subtotals',
                  text: 'Subtotals',
                  icon: 'autosum',
                  size: 'large',
                  disabled: off,
                  children: [
                    {
                      name: 'pivot-subtotals-none',
                      text: 'Do Not Show Subtotals',
                      checked: options?.subtotals === 'none',
                      onClick: () => actions.set({ subtotals: 'none' }),
                    },
                    {
                      name: 'pivot-subtotals-bottom',
                      text: 'Show all Subtotals at Bottom of Group',
                      checked: options?.subtotals === 'bottom',
                      onClick: () => actions.set({ subtotals: 'bottom' }),
                    },
                    {
                      name: 'pivot-subtotals-top',
                      text: 'Show all Subtotals at Top of Group',
                      checked: options?.subtotals === 'top',
                      onClick: () => actions.set({ subtotals: 'top' }),
                    },
                  ],
                },
                {
                  name: 'pivot-grand-totals',
                  text: 'Grand Totals',
                  icon: 'autosum',
                  size: 'large',
                  disabled: off,
                  children: [
                    {
                      name: 'pivot-grand-off',
                      text: 'Off for Rows and Columns',
                      checked: !options?.rowGrand && !options?.colGrand,
                      onClick: () => actions.set({ grandTotals: 'off' }),
                    },
                    {
                      name: 'pivot-grand-both',
                      text: 'On for Rows and Columns',
                      checked: Boolean(options?.rowGrand && options?.colGrand),
                      onClick: () => actions.set({ grandTotals: 'both' }),
                    },
                    {
                      name: 'pivot-grand-rows',
                      text: 'On for Rows Only',
                      checked: Boolean(options?.colGrand && !options?.rowGrand),
                      onClick: () => actions.set({ grandTotals: 'cols' }),
                    },
                    {
                      name: 'pivot-grand-cols',
                      text: 'On for Columns Only',
                      checked: Boolean(options?.rowGrand && !options?.colGrand),
                      onClick: () => actions.set({ grandTotals: 'rows' }),
                    },
                  ],
                },
                {
                  name: 'pivot-report-layout',
                  text: 'Report Layout',
                  icon: 'table',
                  size: 'large',
                  disabled: off,
                  children: [
                    {
                      name: 'pivot-form-compact',
                      text: 'Show in Compact Form',
                      checked: options?.form === 'compact',
                      onClick: () => actions.set({ form: 'compact' }),
                    },
                    {
                      name: 'pivot-form-outline',
                      text: 'Show in Outline Form',
                      checked: options?.form === 'outline',
                      onClick: () => actions.set({ form: 'outline' }),
                    },
                    {
                      name: 'pivot-form-tabular',
                      text: 'Show in Tabular Form',
                      checked: options?.form === 'tabular',
                      onClick: () => actions.set({ form: 'tabular' }),
                    },
                    {
                      name: 'pivot-repeat-labels',
                      text: 'Repeat All Item Labels',
                      beginGroup: true,
                      checked: Boolean(options?.repeatLabels),
                      onClick: () => actions.set({ repeatLabels: true }),
                    },
                    {
                      name: 'pivot-no-repeat-labels',
                      text: 'Do Not Repeat Item Labels',
                      checked: !options?.repeatLabels,
                      onClick: () => actions.set({ repeatLabels: false }),
                    },
                  ],
                },
                {
                  name: 'pivot-blank-rows',
                  text: 'Blank Rows',
                  icon: 'row-insert',
                  size: 'large',
                  disabled: off,
                  children: [
                    {
                      name: 'pivot-blank-insert',
                      text: 'Insert Blank Line after Each Item',
                      checked: Boolean(options?.blankRows),
                      onClick: () => actions.set({ blankRows: true }),
                    },
                    {
                      name: 'pivot-blank-remove',
                      text: 'Remove Blank Line after Each Item',
                      checked: !options?.blankRows,
                      onClick: () => actions.set({ blankRows: false }),
                    },
                  ],
                },
              ],
            },
            {
              key: 'pivot-style-options',
              text: 'PivotTable Style Options',
              icon: 'table-style',
              items: [
                {
                  name: 'pivot-row-headers',
                  text: 'Row Headers',
                  checked: Boolean(style?.rowHeaders),
                  disabled: off,
                  onClick: () => actions.set({ rowHeaders: !style?.rowHeaders }),
                },
                {
                  name: 'pivot-col-headers',
                  text: 'Column Headers',
                  checked: Boolean(style?.colHeaders),
                  disabled: off,
                  onClick: () => actions.set({ colHeaders: !style?.colHeaders }),
                },
                {
                  name: 'pivot-row-stripes',
                  text: 'Banded Rows',
                  checked: Boolean(style?.rowStripes),
                  disabled: off,
                  onClick: () => actions.set({ rowStripes: !style?.rowStripes }),
                },
                {
                  name: 'pivot-col-stripes',
                  text: 'Banded Columns',
                  checked: Boolean(style?.colStripes),
                  disabled: off,
                  onClick: () => actions.set({ colStripes: !style?.colStripes }),
                },
              ],
            },
            {
              key: 'pivot-styles',
              text: 'PivotTable Styles',
              icon: 'table-style',
              items: [
                {
                  name: 'pivot-style',
                  text: 'PivotTable Styles',
                  icon: 'table-style',
                  size: 'large',
                  disabled: off,
                  children: [
                    {
                      name: 'pivot-style-none',
                      text: 'None',
                      checked: !style?.name,
                      onClick: () => actions.set({ style: '' }),
                    },
                    ...PIVOT_STYLES.map((entry, i) => ({
                      name: 'pivot-style-' + entry.name,
                      text: entry.text,
                      beginGroup: i > 0 && PIVOT_STYLES[i - 1]!.group !== entry.group,
                      checked: style?.name === entry.name,
                      onClick: () => actions.set({ style: entry.name }),
                    })),
                  ],
                },
              ],
            },
          ]
        : [],
    },
  ];
}
