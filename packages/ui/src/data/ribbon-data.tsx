// The ribbon's data tools: Home's Styles group, the Data and Review tabs, Table Design when the
// selection is in a table, and the Insert and Formulas tabs' additions. What each command does
// is in `AppShell`'s data actions; this only says what's shown.
import { useEffect, useState } from 'react';
import type { CgRibbonGroup, CgRibbonItem, CgRibbonTab } from '@cashgear/ui';
import type {
  CellStyleInfo,
  CfRuleSpec,
  TableInfo,
  TableOptions,
  TableStyleInfo,
} from '@cgs/engine-client';
import type { CfQuickKind } from './CfDialogs';
import { CellStyleGallery, TableStyleGallery } from './Galleries';

export interface DataState {
  hasBook: boolean;
  /** The table the active cell is in. */
  table: TableInfo | null;
  tableStyles: TableStyleInfo[];
  cellStyles: { group: string; styles: CellStyleInfo[] }[];
  /** Whether the sheet showing has a filter of its own. */
  filterOn: boolean;
  circled: boolean;
  hasNote: boolean;
  /** The active cell's note shows all the time. */
  noteShown: boolean;
  /** Show All Notes, which is the app's view and not saved, as in Excel. */
  allNotesShown: boolean;
  hasLink: boolean;
}

export interface DataActions {
  sort(descending: boolean): void;
  customSort(): void;
  toggleFilter(): void;
  clearFilter(): void;
  reapplyFilter(): void;
  removeDuplicates(): void;
  validation(): void;
  circleInvalid(on: boolean): void;
  cfQuick(kind: CfQuickKind): void;
  cfRule(rule: CfRuleSpec): void;
  cfClear(wholeSheet: boolean): void;
  cfManage(): void;
  formatAsTable(style: string): void;
  cellStyle(name: string): void;
  insertTable(): void;
  hyperlink(): void;
  removeLink(): void;
  nameManager(): void;
  defineName(): void;
  namesFromSelection(): void;
  editNote(): void;
  deleteNote(): void;
  nextNote(back: boolean): void;
  /** Show/Hide Note. */
  toggleNote(): void;
  showAllNotes(): void;
  /** Clear Hyperlinks: the links go, and the cells keep their look. */
  clearLinks(): void;
  tableOptions(options: TableOptions): void;
  renameTable(name: string): void;
  resizeTable(): void;
  convertToRange(): void;
}

/** Excel 2016's gradient data bars, by colour. */
export const DATA_BARS: [string, string][] = [
  ['Blue', '638EC6'],
  ['Green', '63C384'],
  ['Red', 'FF555A'],
  ['Orange', 'FFB628'],
  ['Light Blue', '008AEF'],
  ['Purple', 'D6007B'],
];

/** Excel 2016's colour scales: two or three colours from lowest to highest. */
export const COLOR_SCALES: [string, string[]][] = [
  ['Green - Yellow - Red', ['F8696B', 'FFEB84', '63BE7B']],
  ['Red - Yellow - Green', ['63BE7B', 'FFEB84', 'F8696B']],
  ['Green - White - Red', ['F8696B', 'FCFCFF', '63BE7B']],
  ['Red - White - Green', ['63BE7B', 'FCFCFF', 'F8696B']],
  ['Blue - White - Red', ['F8696B', 'FCFCFF', '5A8AC6']],
  ['Red - White - Blue', ['5A8AC6', 'FCFCFF', 'F8696B']],
  ['White - Red', ['F8696B', 'FCFCFF']],
  ['Red - White', ['FCFCFF', 'F8696B']],
  ['Green - White', ['FCFCFF', '63BE7B']],
  ['White - Green', ['63BE7B', 'FCFCFF']],
  ['Green - Yellow', ['FFEF9C', '63BE7B']],
  ['Yellow - Green', ['63BE7B', 'FFEF9C']],
];

/** Excel 2016's classic icon sets, by group. */
export const ICON_SETS: [string, [string, string][]][] = [
  [
    'Directional',
    [
      ['3 Arrows (Colored)', '3Arrows'],
      ['3 Arrows (Gray)', '3ArrowsGray'],
      ['4 Arrows (Colored)', '4Arrows'],
      ['4 Arrows (Gray)', '4ArrowsGray'],
      ['5 Arrows (Colored)', '5Arrows'],
      ['5 Arrows (Gray)', '5ArrowsGray'],
    ],
  ],
  [
    'Shapes',
    [
      ['3 Traffic Lights (Unrimmed)', '3TrafficLights1'],
      ['3 Traffic Lights (Rimmed)', '3TrafficLights2'],
      ['3 Signs', '3Signs'],
      ['4 Traffic Lights', '4TrafficLights'],
      ['Red To Black', '4RedToBlack'],
    ],
  ],
  [
    'Indicators',
    [
      ['3 Symbols (Circled)', '3Symbols'],
      ['3 Symbols (Uncircled)', '3Symbols2'],
      ['3 Flags', '3Flags'],
    ],
  ],
  [
    'Ratings',
    [
      ['4 Ratings', '4Rating'],
      ['5 Ratings', '5Rating'],
      ['5 Quarters', '5Quarters'],
    ],
  ],
];

/** A colour scale's rule from its colours, lowest first. */
export function colorScaleRule(colors: string[]): CfRuleSpec {
  const points =
    colors.length === 3
      ? [
          { kind: 'min', color: colors[0] },
          { kind: 'percentile', value: '50', color: colors[1] },
          { kind: 'max', color: colors[2] },
        ]
      : [
          { kind: 'min', color: colors[0] },
          { kind: 'max', color: colors[1] },
        ];
  return { type: 'colorScale', points };
}

function TableNameBox(props: { name: string; onRename: (name: string) => void }) {
  const [text, setText] = useState(props.name);
  useEffect(() => setText(props.name), [props.name]);
  const commit = () => {
    const name = text.trim();
    if (name && name !== props.name) props.onRename(name);
    else setText(props.name);
  };
  return (
    <label className="cgs-table-name">
      <span>Table Name:</span>
      <input
        data-testid="table-name"
        aria-label="Table Name"
        value={text}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') commit();
          if (event.key === 'Escape') setText(props.name);
          event.stopPropagation();
        }}
        style={{ width: 110 }}
      />
    </label>
  );
}

/** Home's Styles group: Conditional Formatting, Format as Table and Cell Styles. */
export function stylesGroup(state: DataState, data: DataActions): CgRibbonGroup {
  const off = !state.hasBook;
  const quick = (name: string, text: string, kind: CfQuickKind) => ({
    name,
    text,
    onClick: () => data.cfQuick(kind),
  });
  return {
    key: 'styles',
    text: 'Styles',
    priority: 3,
    icon: 'conditional-format',
    items: [
      {
        name: 'conditional-formatting',
        text: 'Conditional Formatting',
        icon: 'conditional-format',
        size: 'large',
        disabled: off,
        children: [
          {
            name: 'cf-highlight',
            text: 'Highlight Cells Rules',
            children: [
              quick('cf-greater', 'Greater Than…', 'greaterThan'),
              quick('cf-less', 'Less Than…', 'lessThan'),
              quick('cf-between', 'Between…', 'between'),
              quick('cf-equal', 'Equal To…', 'equal'),
              quick('cf-text', 'Text that Contains…', 'textContains'),
              quick('cf-date', 'A Date Occurring…', 'dateOccurring'),
              quick('cf-duplicates', 'Duplicate Values…', 'duplicates'),
            ],
          },
          {
            name: 'cf-top-bottom',
            text: 'Top/Bottom Rules',
            children: [
              quick('cf-top10', 'Top 10 Items…', 'top10'),
              quick('cf-top10p', 'Top 10%…', 'top10Percent'),
              quick('cf-bottom10', 'Bottom 10 Items…', 'bottom10'),
              quick('cf-bottom10p', 'Bottom 10%…', 'bottom10Percent'),
              quick('cf-above', 'Above Average…', 'aboveAverage'),
              quick('cf-below', 'Below Average…', 'belowAverage'),
            ],
          },
          {
            name: 'cf-data-bars',
            text: 'Data Bars',
            beginGroup: true,
            children: DATA_BARS.map(([text, color]) => ({
              name: 'cf-bar-' + color,
              text: `${text} Data Bar`,
              onClick: () =>
                data.cfRule({
                  type: 'dataBar',
                  min: { kind: 'autoMin' },
                  max: { kind: 'autoMax' },
                  color,
                }),
            })),
          },
          {
            name: 'cf-color-scales',
            text: 'Color Scales',
            children: COLOR_SCALES.map(([text, colors]) => ({
              name: 'cf-scale-' + text.replace(/\W+/g, '-'),
              text: `${text} Color Scale`,
              onClick: () => data.cfRule(colorScaleRule(colors)),
            })),
          },
          {
            name: 'cf-icon-sets',
            text: 'Icon Sets',
            children: ICON_SETS.flatMap(([group, sets], at) =>
              sets.map(([text, set], index) => ({
                name: 'cf-icons-' + set,
                text,
                beginGroup: at > 0 && index === 0,
                tooltip: group,
                onClick: () => data.cfRule({ type: 'iconSet', set }),
              })),
            ),
          },
          {
            name: 'cf-clear',
            text: 'Clear Rules',
            beginGroup: true,
            children: [
              {
                name: 'cf-clear-selection',
                text: 'Clear Rules from Selected Cells',
                onClick: () => data.cfClear(false),
              },
              {
                name: 'cf-clear-sheet',
                text: 'Clear Rules from Entire Sheet',
                onClick: () => data.cfClear(true),
              },
            ],
          },
          { name: 'cf-manage', text: 'Manage Rules…', onClick: data.cfManage },
        ],
      },
      {
        name: 'format-as-table',
        ownsKeyboard: true,
        render: () => (
          <TableStyleGallery
            styles={state.tableStyles}
            label="Format as Table"
            disabled={off}
            onPick={data.formatAsTable}
          />
        ),
      },
      {
        name: 'cell-styles',
        ownsKeyboard: true,
        render: () => (
          <CellStyleGallery groups={state.cellStyles} disabled={off} onPick={data.cellStyle} />
        ),
      },
    ],
  };
}

/** Insert's Tables and Links groups. */
export function insertGroups(state: DataState, data: DataActions): CgRibbonGroup[] {
  const off = !state.hasBook;
  return [
    {
      key: 'tables',
      text: 'Tables',
      icon: 'table-add',
      items: [
        {
          name: 'insert-table',
          text: 'Table',
          icon: 'table-add',
          size: 'large',
          disabled: off,
          onClick: data.insertTable,
        },
      ],
    },
    {
      key: 'links',
      text: 'Links',
      icon: 'link',
      items: [
        {
          name: 'insert-hyperlink',
          text: 'Hyperlink',
          icon: 'link',
          size: 'large',
          disabled: off,
          onClick: data.hyperlink,
        },
      ],
    },
  ];
}

/** Formulas' Defined Names group. */
export function namesGroup(state: DataState, data: DataActions): CgRibbonGroup {
  const off = !state.hasBook;
  return {
    key: 'defined-names',
    text: 'Defined Names',
    icon: 'formula',
    items: [
      {
        name: 'name-manager',
        text: 'Name Manager',
        icon: 'formula',
        size: 'large',
        disabled: off,
        onClick: data.nameManager,
      },
      {
        name: 'define-name',
        text: 'Define Name',
        icon: 'add',
        disabled: off,
        onClick: data.defineName,
      },
      {
        name: 'create-from-selection',
        text: 'Create from Selection',
        icon: 'table',
        disabled: off,
        onClick: data.namesFromSelection,
      },
    ],
  };
}

/** The Data and Review tabs, and Table Design while the active cell is in a table. */
export function dataTabs(state: DataState, data: DataActions): CgRibbonTab[] {
  const off = !state.hasBook;
  const table = state.table;
  const option = (
    name: string,
    text: string,
    key: keyof TableOptions,
    checked: boolean,
  ): CgRibbonItem => ({
    name,
    text,
    disabled: off,
    checked,
    onCheckedChange: (on) => data.tableOptions({ [key]: on }),
  });
  return [
    {
      key: 'data',
      text: 'Data',
      groups: [
        {
          key: 'sort-filter',
          text: 'Sort & Filter',
          icon: 'filter',
          items: [
            {
              name: 'sort-asc',
              text: 'Sort A to Z',
              icon: 'sort-ascending',
              hideText: true,
              disabled: off,
              onClick: () => data.sort(false),
            },
            {
              name: 'sort-desc',
              text: 'Sort Z to A',
              icon: 'sort-descending',
              hideText: true,
              disabled: off,
              onClick: () => data.sort(true),
            },
            {
              name: 'sort-custom',
              text: 'Sort',
              icon: 'sort',
              size: 'large',
              disabled: off,
              onClick: data.customSort,
            },
            {
              name: 'filter',
              text: 'Filter',
              icon: 'filter',
              size: 'large',
              disabled: off,
              checked: state.filterOn || Boolean(table?.filterButton),
              onCheckedChange: () => data.toggleFilter(),
            },
            {
              name: 'filter-clear',
              text: 'Clear',
              icon: 'filter-clear',
              disabled: off,
              onClick: data.clearFilter,
            },
            {
              name: 'filter-reapply',
              text: 'Reapply',
              icon: 'redo',
              disabled: off,
              onClick: data.reapplyFilter,
            },
          ],
        },
        {
          key: 'data-tools',
          text: 'Data Tools',
          icon: 'data-validation',
          items: [
            {
              name: 'remove-duplicates',
              text: 'Remove Duplicates',
              icon: 'delete',
              size: 'large',
              disabled: off,
              onClick: data.removeDuplicates,
            },
            {
              name: 'data-validation',
              text: 'Data Validation',
              icon: 'data-validation',
              size: 'large',
              disabled: off,
              onClick: data.validation,
              children: [
                { name: 'dv-open', text: 'Data Validation…', onClick: data.validation },
                {
                  name: 'dv-circle',
                  text: 'Circle Invalid Data',
                  onClick: () => data.circleInvalid(true),
                },
                {
                  name: 'dv-clear-circles',
                  text: 'Clear Validation Circles',
                  disabled: !state.circled,
                  onClick: () => data.circleInvalid(false),
                },
              ],
            },
          ],
        },
      ],
    },
    {
      key: 'review',
      text: 'Review',
      groups: [
        {
          key: 'notes',
          text: 'Notes',
          icon: 'comment',
          items: [
            {
              name: 'note-edit',
              text: state.hasNote ? 'Edit Note' : 'New Note',
              icon: 'comment',
              size: 'large',
              disabled: off,
              onClick: data.editNote,
            },
            {
              name: 'note-delete',
              text: 'Delete',
              icon: 'delete',
              disabled: off || !state.hasNote,
              onClick: data.deleteNote,
            },
            {
              name: 'note-previous',
              text: 'Previous',
              icon: 'chevron-up',
              disabled: off,
              onClick: () => data.nextNote(true),
            },
            {
              name: 'note-next',
              text: 'Next',
              icon: 'chevron-down',
              disabled: off,
              onClick: () => data.nextNote(false),
            },
            {
              name: 'note-show',
              text: 'Show/Hide Note',
              icon: 'comment',
              checked: state.noteShown,
              disabled: off || !state.hasNote,
              onClick: data.toggleNote,
            },
            {
              name: 'note-show-all',
              text: 'Show All Notes',
              icon: 'comment',
              checked: state.allNotesShown,
              disabled: off,
              onClick: data.showAllNotes,
            },
          ],
        },
      ],
    },
    {
      key: 'table-design',
      text: 'Table Design',
      visible: Boolean(table),
      groups: table
        ? [
            {
              key: 'properties',
              text: 'Properties',
              icon: 'table',
              items: [
                {
                  name: 'table-name',
                  ownsKeyboard: true,
                  render: () => (
                    <TableNameBox name={table.name} onRename={(name) => data.renameTable(name)} />
                  ),
                },
                {
                  name: 'table-resize',
                  text: 'Resize Table',
                  icon: 'table',
                  disabled: off,
                  onClick: data.resizeTable,
                },
              ],
            },
            {
              key: 'tools',
              text: 'Tools',
              icon: 'table',
              items: [
                {
                  name: 'table-remove-duplicates',
                  text: 'Remove Duplicates',
                  icon: 'delete',
                  disabled: off,
                  onClick: data.removeDuplicates,
                },
                {
                  name: 'table-convert',
                  text: 'Convert to Range',
                  icon: 'table',
                  disabled: off,
                  onClick: data.convertToRange,
                },
              ],
            },
            {
              key: 'style-options',
              text: 'Table Style Options',
              icon: 'table-style',
              layout: 'rows',
              items: [
                option('opt-header', 'Header Row', 'headerRow', table.headerRow),
                option('opt-totals', 'Total Row', 'totalsRow', table.totalsRow),
                option('opt-banded-rows', 'Banded Rows', 'rowStripes', table.rowStripes),
                option('opt-first-col', 'First Column', 'firstColumn', table.firstColumn),
                option('opt-last-col', 'Last Column', 'lastColumn', table.lastColumn),
                option('opt-banded-cols', 'Banded Columns', 'columnStripes', table.columnStripes),
                option('opt-filter', 'Filter Button', 'filterButton', table.filterButton),
              ],
            },
            {
              key: 'table-styles',
              text: 'Table Styles',
              icon: 'table-style',
              items: [
                {
                  name: 'table-style-gallery',
                  ownsKeyboard: true,
                  render: () => (
                    <TableStyleGallery
                      styles={state.tableStyles}
                      value={table.style}
                      label="Table Styles"
                      disabled={off}
                      onPick={(style) => data.tableOptions({ style })}
                    />
                  ),
                },
              ],
            },
          ]
        : [],
    },
  ];
}
