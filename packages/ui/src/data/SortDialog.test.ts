import { describe, expect, it } from 'vitest';
import {
  CUSTOM_LISTS,
  keyColor,
  lineLabel,
  listLabel,
  newLevel,
  parseListEntries,
  sortCommand,
  type SortLevel,
  type SortSettings,
} from './SortDialog';

const area = { top: 1, left: 2, bottom: 10, right: 5 };

function settings(levels: Partial<SortLevel>[], more: Partial<SortSettings> = {}): SortSettings {
  return {
    sheet: 0,
    area,
    headers: true,
    caseSensitive: false,
    byColumns: false,
    levels: levels.map((level) => ({ ...newLevel(), ...level })),
    label: (index) => lineLabel(index, false),
    ...more,
  };
}

describe('custom lists', () => {
  it('are Excel’s four built-in ones', () => {
    expect(CUSTOM_LISTS.map((list) => listLabel(list))).toEqual([
      'Sun, Mon, Tue, Wed, Thu, Fri, Sat',
      'Sunday, Monday, Tuesday, Wednesday, Thursday, Friday, Saturday',
      'Jan, Feb, Mar, Apr, May, Jun, Jul, Aug, Sep, Oct, Nov, Dec',
      'January, February, March, April, May, June, July, August, September, October, November, December',
    ]);
  });

  it('read backwards for a descending sort', () => {
    expect(listLabel(['High', 'Medium', 'Low'], true)).toBe('Low, Medium, High');
  });

  it('take new entries a line each or split by commas', () => {
    expect(parseListEntries('High\r\nMedium, Low\n\n , ')).toEqual(['High', 'Medium', 'Low']);
    expect(parseListEntries('  ')).toEqual([]);
  });
});

describe('lineLabel', () => {
  it('uses the heading, or the column’s letter, or the row', () => {
    expect(lineLabel(3, false, 'Region')).toBe('Region');
    expect(lineLabel(3, false, ' ')).toBe('Column C');
    expect(lineLabel(28, false)).toBe('Column AB');
    expect(lineLabel(4, true, 'Region')).toBe('Row 4');
  });
});

describe('sortCommand', () => {
  it('builds a key for each level, in order', () => {
    const built = sortCommand(
      settings([
        { index: 3 },
        { index: 2, descending: true },
        { index: 4, on: 'cellColor', color: 'FF0000' },
        { index: 5, on: 'fontColor', color: null, descending: true },
        { index: 4, list: [...CUSTOM_LISTS[2]], descending: true },
      ]),
    );
    expect(built).toEqual({
      command: {
        kind: 'sort',
        sheet: 0,
        area,
        header: true,
        caseSensitive: false,
        byColumns: false,
        keys: [
          { index: 3, descending: false, by: { kind: 'value' } },
          { index: 2, descending: true, by: { kind: 'value' } },
          { index: 4, descending: false, by: { kind: 'cellColor', color: 'FF0000' } },
          { index: 5, descending: true, by: { kind: 'fontColor', color: null } },
          {
            index: 4,
            descending: true,
            by: { kind: 'value' },
            list: [...CUSTOM_LISTS[2]],
          },
        ],
      },
    });
  });

  it('has no headings left to right, and passes the options', () => {
    const built = sortCommand(
      settings([{ index: 1 }], { byColumns: true, caseSensitive: true, headers: true }),
    );
    expect(built).toMatchObject({
      command: { header: false, byColumns: true, caseSensitive: true },
    });
  });

  it('refuses a level with no column', () => {
    expect(sortCommand(settings([{ index: 3 }, {}]))).toEqual({
      error:
        'All sort criteria must have a column specified. Check the selected sort criteria and try again.',
    });
  });

  it('refuses sorting one column by values twice', () => {
    expect(sortCommand(settings([{ index: 3 }, { index: 3, descending: true }]))).toEqual({
      error:
        'Column C is being sorted by values more than once. Delete the duplicate sort criteria and try again.',
    });
  });

  it('allows one column by two colours, but not the same colour twice', () => {
    const two = settings([
      { index: 3, on: 'cellColor', color: 'FF0000' },
      { index: 3, on: 'cellColor', color: '00FF00' },
      { index: 3 },
    ]);
    expect(sortCommand(two)).toHaveProperty('command');
    const twice = settings([
      { index: 3, on: 'fontColor', color: 'FF0000' },
      { index: 3, on: 'fontColor', color: 'FF0000', descending: true },
    ]);
    expect(sortCommand(twice)).toHaveProperty('error');
  });

  it('copies a level’s list rather than sharing it', () => {
    const level = { ...newLevel(), index: 2, list: ['a', 'b'] };
    const copy = newLevel(level);
    expect(copy.id).not.toBe(level.id);
    expect(copy.list).toEqual(['a', 'b']);
    expect(copy.list).not.toBe(level.list);
  });
});

describe('keyColor', () => {
  it('turns the picker’s colour into RRGGBB, and no colour into null', () => {
    expect(keyColor({ kind: 'rgb', color: '#ff00aa' })).toBe('FF00AA');
    expect(keyColor({ kind: 'theme', key: '4', tint: 0, color: '#4472c4' })).toBe('4472C4');
    expect(keyColor({ kind: 'none' })).toBeNull();
    expect(keyColor({ kind: 'automatic' })).toBeNull();
  });
});
