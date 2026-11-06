import { describe, expect, it } from 'vitest';
import { CG_PIVOT_FIELD_LIST_VALUES_KEY } from '@cashgear/ui';
import type { PivotInfo } from '@cgs/engine-client';
import { fieldListLayout, layoutSpec } from './usePivotTools';
import { valueName } from './PivotDialogs';

const field = (index: number, name: string, numeric: boolean) => ({
  index,
  name,
  caption: name,
  axis: null,
  numeric,
  items: [],
  sort: 'manual' as const,
  sortBy: null,
  subtotals: ['automatic'],
  showAll: false,
  blankRow: false,
  multiple: false,
  grouped: false,
});

const info = {
  name: 'PivotTable1',
  sheet: 1,
  area: { top: 3, left: 1, bottom: 8, right: 3 },
  readOnly: null,
  noRefresh: null,
  fields: [field(0, 'Region', false), field(1, 'Product', false), field(2, 'Sales', true)],
  rows: [0],
  cols: [-2],
  pages: [{ field: 1, item: null }],
  values: [
    {
      field: 2,
      name: 'Total sales',
      summary: 'sum',
      showAs: 'percentOfTotal',
      baseField: 0,
      baseItem: 0,
      numberFormat: '0.0%',
    },
    {
      field: 2,
      name: 'Count of Sales',
      summary: 'count',
      showAs: 'normal',
      baseField: 0,
      baseItem: 0,
      numberFormat: null,
    },
  ],
  options: {
    form: 'compact',
    subtotals: 'top',
    rowGrand: true,
    colGrand: true,
    blankRows: false,
    repeatLabels: false,
    emptyText: null,
    errorText: null,
    headers: true,
    style: {
      name: 'PivotStyleLight16',
      rowHeaders: true,
      colHeaders: true,
      rowStripes: false,
      colStripes: false,
    },
  },
  cell: { role: 'other' },
  buttons: [],
} satisfies PivotInfo;

describe('the field list and the engine', () => {
  it('shows the engine’s layout, the Σ Values item where it is', () => {
    const layout = fieldListLayout(info);
    expect(layout.rows.map((i) => i.fieldKey)).toEqual(['0']);
    expect(layout.columns.map((i) => i.fieldKey)).toEqual([CG_PIVOT_FIELD_LIST_VALUES_KEY]);
    expect(layout.filters.map((i) => i.fieldKey)).toEqual(['1']);
    expect(layout.values.map((i) => i.caption)).toEqual(['Total sales', 'Count of Sales']);
  });

  it('sends a layout back with each value field’s settings kept', () => {
    const layout = fieldListLayout(info);
    const next = {
      ...layout,
      rows: [layout.rows[0]!, { id: 'rows:1:0', fieldKey: '1' }],
      filters: [],
      // The two values swapped, and a new one for Region.
      values: [layout.values[1]!, layout.values[0]!, { id: 'values:0:0', fieldKey: '0' }],
    };
    const spec = layoutSpec(info, next);
    expect(spec.rows).toEqual([0, 1]);
    expect(spec.cols).toEqual([-2]);
    expect(spec.pages).toEqual([]);
    expect(spec.values[0]).toMatchObject({ field: 2, summary: 'count', name: 'Count of Sales' });
    expect(spec.values[1]).toMatchObject({
      field: 2,
      showAs: 'percentOfTotal',
      numberFormat: '0.0%',
    });
    // Text counts, as Excel starts it.
    expect(spec.values[2]).toEqual({ field: 0, summary: 'count' });
  });

  it('names a value field as Excel does', () => {
    expect(valueName('sum', 'Sales')).toBe('Sum of Sales');
    expect(valueName('countNums', 'Sales')).toBe('Count of Sales');
  });
});
