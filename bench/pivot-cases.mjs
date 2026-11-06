// Pivot tables for Excel 2016 to make over COM, and the edits to their source that a refresh must
// follow. `bench/excel-make-pivots.ps1` builds them; `bench/run-pivot-check.mjs` compares the app's
// refresh with Excel's. One sheet of generated sales feeds them all.

/** A small deterministic generator, so every run builds the same data. */
function generator(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

const REGIONS = ['North', 'South', 'East'];
const PRODUCTS = ['Apple', 'Banana', 'Cherry', 'Date'];
const REPS = ['Ann', 'Bob', 'Cy', 'Di'];

export function salesRows(count = 48) {
  const random = generator(7);
  const rows = [['Region', 'Product', 'Rep', 'Date', 'Sales', 'Units', 'Score']];
  for (let i = 0; i < count; i++) {
    const region = REGIONS[Math.floor(random() * REGIONS.length)];
    const product = PRODUCTS[Math.floor(random() * PRODUCTS.length)];
    const rep = REPS[Math.floor(random() * REPS.length)];
    const date = 44927 + Math.floor(random() * 700);
    const sales = Math.round(random() * 100000) / 100;
    const units = 1 + Math.floor(random() * 40);
    const score = Math.round(random() * 1000) / 10;
    rows.push([region, product, rep, `date:${date}`, sales, units, score]);
  }
  // A blank region, a blank sale, text among the units and an error among the scores.
  rows[10][0] = null;
  rows[20][4] = null;
  rows[25][5] = 'n/a';
  rows[30][6] = '=NA()';
  return rows;
}

const sum = (field, extra = {}) => ({
  field,
  summary: 'sum',
  caption: `Sum of ${field}`,
  ...extra,
});

/** Each case is one pivot table on a sheet of its own. Names are sheet names. */
export const CASES = [
  { name: 'Rows', fields: [{ name: 'Region', axis: 'row' }], values: [sum('Sales')] },
  {
    name: 'Two rows',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Three rows',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
      { name: 'Rep', axis: 'row' },
    ],
    values: [sum('Units')],
  },
  {
    name: 'Rows and columns',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Two columns',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
      { name: 'Rep', axis: 'col' },
    ],
    values: [sum('Units')],
  },
  {
    name: 'Columns only',
    fields: [{ name: 'Product', axis: 'col' }],
    values: [sum('Sales')],
  },
  {
    name: 'Values only',
    fields: [],
    values: [sum('Sales'), { field: 'Units', summary: 'count', caption: 'Count of Units' }],
  },
  {
    name: 'Tabular',
    layout: 'tabular',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
      { name: 'Rep', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Outline',
    layout: 'outline',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Values on columns',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales'), { field: 'Units', summary: 'count', caption: 'Count of Units' }],
  },
  {
    name: 'Values on rows',
    valuesAxis: 'row',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [sum('Sales'), { field: 'Units', summary: 'average', caption: 'Average of Units' }],
  },
  {
    name: 'Values with columns',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Rep', axis: 'col' },
    ],
    values: [sum('Sales'), { field: 'Units', summary: 'max', caption: 'Max of Units' }],
  },
  {
    name: 'Every summary',
    fields: [{ name: 'Region', axis: 'row' }],
    values: [
      'sum',
      'count',
      'average',
      'max',
      'min',
      'product',
      'countNums',
      'stdDev',
      'stdDevp',
      'var',
      'varp',
    ].map((summary) => ({ field: 'Units', summary, caption: `${summary} of Units` })),
  },
  {
    name: 'Summaries of scores',
    fields: [{ name: 'Product', axis: 'row' }],
    values: ['sum', 'count', 'average', 'max', 'countNums'].map((summary) => ({
      field: 'Score',
      summary,
      caption: `${summary} of Score`,
    })),
  },
  {
    name: 'Show values as',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [
      sum('Sales', { showAs: 'percentOfTotal', caption: 'Total %' }),
      sum('Sales', { showAs: 'percentOfRow', caption: 'Row %' }),
      sum('Sales', { showAs: 'percentOfCol', caption: 'Column %' }),
    ],
  },
  {
    name: 'Show values as base',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
    ],
    values: [
      sum('Sales', {
        showAs: 'difference',
        baseField: 'Product',
        baseItem: 'Apple',
        caption: 'Diff',
      }),
      sum('Sales', {
        showAs: 'percent',
        baseField: 'Product',
        baseItem: 'previous',
        caption: 'Pct',
      }),
      sum('Sales', {
        showAs: 'percentDiff',
        baseField: 'Region',
        baseItem: 'North',
        caption: 'PctDiff',
      }),
      sum('Sales', { showAs: 'runTotal', baseField: 'Product', caption: 'Running' }),
    ],
  },
  {
    name: 'Show values as newer',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
      { name: 'Rep', axis: 'col' },
    ],
    values: [
      sum('Units', { showAs: 'index', caption: 'Index' }),
      sum('Units', { showAs: 'percentOfParentRow', caption: 'Parent row %' }),
      sum('Units', { showAs: 'percentOfParent', baseField: 'Region', caption: 'Parent %' }),
      sum('Units', { showAs: 'percentOfRunningTotal', baseField: 'Product', caption: 'Running %' }),
      sum('Units', { showAs: 'rankDescending', baseField: 'Product', caption: 'Rank' }),
    ],
  },
  {
    name: 'Report filter',
    at: 'R5C1',
    fields: [
      { name: 'Rep', axis: 'page', page: 'Bob' },
      { name: 'Region', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Two report filters',
    at: 'R6C1',
    fields: [
      { name: 'Rep', axis: 'page', pages: ['Ann', 'Cy'] },
      { name: 'Region', axis: 'page' },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Hidden items',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col', hide: ['Banana'] },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Sort descending',
    fields: [
      { name: 'Region', axis: 'row', sort: 'descending' },
      { name: 'Product', axis: 'row', sort: 'descending' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Sort by value',
    fields: [
      { name: 'Product', axis: 'row', sortBy: 'Sum of Sales', sortOrder: 'descending' },
      { name: 'Region', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Group dates',
    fields: [{ name: 'Date', axis: 'row', group: 'dates', periods: [6, 5, 4] }],
    values: [sum('Sales')],
  },
  {
    name: 'Group months',
    fields: [
      { name: 'Date', axis: 'col', group: 'dates', periods: [4] },
      { name: 'Region', axis: 'row' },
    ],
    values: [sum('Units')],
  },
  {
    name: 'Group numbers',
    fields: [{ name: 'Sales', axis: 'row', group: 'numbers', start: 0, end: 1000, by: 250 }],
    values: [{ field: 'Units', summary: 'count', caption: 'Count of Units' }],
  },
  {
    name: 'No grand totals',
    rowGrand: false,
    colGrand: false,
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Column grand only',
    rowGrand: false,
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Subtotals at bottom',
    layout: 'outline',
    fields: [
      { name: 'Region', axis: 'row', subtotalBottom: true },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'No subtotals',
    layout: 'tabular',
    fields: [
      { name: 'Region', axis: 'row', subtotals: [] },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Custom subtotals',
    layout: 'tabular',
    fields: [
      { name: 'Region', axis: 'row', subtotals: [1, 2, 4] },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Blank rows',
    fields: [
      { name: 'Region', axis: 'row', blankLine: true },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Repeat labels',
    layout: 'tabular',
    repeatAll: true,
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Items with no data',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'row', showAll: true },
      { name: 'Rep', axis: 'page', page: 'Di' },
    ],
    at: 'R5C1',
    values: [sum('Sales')],
  },
  {
    name: 'Collapsed',
    fields: [
      { name: 'Region', axis: 'row', collapse: ['South'] },
      { name: 'Product', axis: 'row' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Mixed forms',
    fields: [
      { name: 'Region', axis: 'row', form: 'tabular' },
      { name: 'Product', axis: 'row', form: 'compact' },
      { name: 'Rep', axis: 'row' },
    ],
    values: [sum('Units')],
  },
  {
    name: 'Captions',
    grandName: 'Everything',
    rowHeader: 'Where',
    fields: [
      { name: 'Region', axis: 'row', rename: { North: 'Up north' } },
      { name: 'Product', axis: 'col', caption: 'Fruit' },
    ],
    values: [sum('Sales', { caption: 'Takings' })],
  },
  {
    name: 'Number format',
    fields: [{ name: 'Region', axis: 'row' }],
    values: [
      sum('Sales', { format: '#,##0.00' }),
      { field: 'Units', summary: 'average', caption: 'Avg', format: '0.0' },
    ],
  },
  {
    name: 'Empty and error text',
    emptyText: '-',
    errorText: 'bad',
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Rep', axis: 'col' },
    ],
    values: [{ field: 'Score', summary: 'sum', caption: 'Sum of Score' }],
  },
  {
    name: 'No headers',
    headers: false,
    fields: [
      { name: 'Region', axis: 'row' },
      { name: 'Product', axis: 'col' },
    ],
    values: [sum('Sales')],
  },
  {
    name: 'Placed at C4',
    at: 'R4C3',
    style: 'PivotStyleMedium9',
    fields: [
      { name: 'Product', axis: 'row' },
      { name: 'Region', axis: 'col' },
    ],
    values: [sum('Units')],
  },
];

/** Source edits made before the refresh: new items, changed values, a blank. 1-based. */
export function sourceEdits(rows) {
  return [
    { row: 3, col: 1, value: 'West' },
    { row: 4, col: 2, value: 'Elderberry' },
    { row: 5, col: 5, value: 12345.5 },
    { row: 6, col: 6, value: 99 },
    { row: 7, col: 2, value: null },
    { row: 8, col: 4, value: 'date:45700' },
    { row: rows.length, col: 3, value: 'Eve' },
  ];
}

export function pivotSpec() {
  const rows = salesRows();
  return {
    rows,
    source: `Data!R1C1:R${rows.length}C${rows[0].length}`,
    cases: CASES,
    edits: sourceEdits(rows),
  };
}
