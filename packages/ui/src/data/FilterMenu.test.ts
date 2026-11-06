import { describe, expect, it } from 'vitest';
import type { FilterList } from '@cgs/engine-client';
import {
  allTicked,
  customCriteria,
  dateGroups,
  daySerial,
  filterKind,
  finalTicks,
  initialTicks,
  noneTicked,
  parseDay,
  searchMatches,
  searchTicks,
  setTicks,
  top10Criteria,
  valuesCriteria,
  visibleList,
  type Day,
} from './FilterMenu';

function list(values: string[], dates: Day[] = [], blanks = false): FilterList {
  return {
    values: values.map((text) => ({ text, checked: true })),
    dates: dates.map((date) => ({ date: [date[0], date[1], date[2]], checked: true })),
    blanks,
    blanksChecked: blanks,
    filtered: 'none',
  };
}

describe('filterKind', () => {
  it('picks numbers when most values are numbers', () => {
    expect(filterKind(list(['1', '2,500', '$3.50', 'x']))).toBe('number');
    expect(filterKind(list(['1', 'a', 'b']))).toBe('text');
    expect(filterKind(list([]))).toBe('text');
  });

  it('picks dates when the dates outnumber the rest', () => {
    expect(filterKind(list(['a'], [[2024, 1, 2]]))).toBe('date');
    expect(filterKind(list(['a', 'b'], [[2024, 1, 2]]))).toBe('text');
  });
});

describe('the values filter', () => {
  const days: Day[] = [
    [2023, 12, 31],
    [2024, 1, 5],
    [2024, 1, 6],
    [2024, 2, 1],
  ];

  it('asks for no criteria when everything is ticked', () => {
    const all = list(['a', 'b'], days, true);
    expect(valuesCriteria(all, initialTicks(all))).toBeUndefined();
  });

  it('sends the ticked values and blanks', () => {
    const all = list(['a', 'b', 'c'], [], true);
    const ticks = setTicks(initialTicks(all), { values: ['b'], blanks: true }, false);
    expect(valuesCriteria(all, ticks)).toEqual({
      kind: 'values',
      values: ['a', 'c'],
      blank: false,
      dates: [],
    });
  });

  it('groups days into whole years and months', () => {
    const all = list([], days);
    const ticks = setTicks(initialTicks(all), { dates: ['2024-2-1'] }, false);
    expect(dateGroups(days, ticks.dates)).toEqual([[2023], [2024, 1]]);
    const one = setTicks(initialTicks(all), { dates: ['2024-1-6', '2023-12-31'] }, false);
    expect(dateGroups(days, one.dates)).toEqual([
      [2024, 1, 5],
      [2024, 2],
    ]);
    expect(valuesCriteria(all, one)).toMatchObject({
      dates: [
        [2024, 1, 5],
        [2024, 2],
      ],
    });
  });

  it('reads the ticks the engine sends', () => {
    const answer: FilterList = {
      values: [
        { text: 'a', checked: true },
        { text: 'b', checked: false },
      ],
      dates: [{ date: [2024, 3, 4], checked: false }],
      blanks: true,
      blanksChecked: false,
      filtered: 'values',
    };
    const ticks = initialTicks(answer);
    expect([...ticks.values]).toEqual(['a']);
    expect(ticks.dates.size).toBe(0);
    expect(ticks.blanks).toBe(false);
    expect(allTicked(answer, ticks)).toBe(false);
    expect(noneTicked(answer, ticks)).toBe(false);
    expect(noneTicked(answer, setTicks(ticks, { values: ['a'] }, false))).toBe(true);
  });
});

describe('search', () => {
  it('matches parts of the text, with wildcards', () => {
    expect(searchMatches('Apple', 'pp')).toBe(true);
    expect(searchMatches('Apple', 'a*e')).toBe(true);
    expect(searchMatches('Apple', 'a?ple')).toBe(true);
    expect(searchMatches('Apple', 'x')).toBe(false);
    expect(searchMatches('a.b', '.')).toBe(true);
  });

  it('ticks only what it finds, unless added to the filter', () => {
    const all = list(['apple', 'banana', 'cherry'], [[2024, 1, 5]], true);
    const found = searchTicks(all, 'ban');
    expect([...found.values]).toEqual(['banana']);
    expect(visibleList(all, 'ban').blanks).toBe(false);
    expect(valuesCriteria(all, finalTicks(initialTicks(all), found, false))).toEqual({
      kind: 'values',
      values: ['banana'],
      blank: false,
      dates: [],
    });
    const base = setTicks(initialTicks(all), { values: ['banana', 'cherry'] }, false);
    expect(valuesCriteria(all, finalTicks(base, found, true))).toMatchObject({
      values: ['apple', 'banana'],
      blank: true,
      dates: [[2024]],
    });
  });

  it('finds dates by month name', () => {
    const all = list(
      [],
      [
        [2024, 1, 5],
        [2024, 3, 1],
      ],
    );
    expect([...searchTicks(all, 'march').dates]).toEqual(['2024-3-1']);
  });
});

describe('the custom filter', () => {
  it('turns the text operators into wildcards', () => {
    expect(
      customCriteria(
        'text',
        [
          { op: 'beginsWith', value: 'ab' },
          { op: 'notContains', value: 'x' },
        ],
        false,
      ),
    ).toEqual({
      kind: 'custom',
      and: false,
      conditions: [
        { op: 'equal', value: 'ab*' },
        { op: 'notEqual', value: '*x*' },
      ],
    });
    expect(customCriteria('text', [{ op: 'endsWith', value: 'z' }], true)).toMatchObject({
      conditions: [{ op: 'equal', value: '*z' }],
    });
  });

  it('leaves out a row with no operator', () => {
    expect(
      customCriteria(
        'number',
        [
          { op: 'greaterThan', value: '1,000' },
          { op: '', value: '' },
        ],
        true,
      ),
    ).toEqual({ kind: 'custom', and: true, conditions: [{ op: 'greaterThan', value: '1000' }] });
    expect(customCriteria('number', [{ op: '', value: '5' }], true)).toBeNull();
  });

  it('sends dates as serials', () => {
    expect(daySerial(2024, 1, 1)).toBe(45292);
    expect(parseDay('1/1/2024')).toBe(45292);
    expect(parseDay('2024-01-01')).toBe(45292);
    expect(parseDay('2/30/2024')).toBeNull();
    expect(parseDay('1/1/24')).toBe(45292);
    expect(customCriteria('date', [{ op: 'lessThan', value: '1/1/2024' }], true)).toMatchObject({
      conditions: [{ op: 'lessThan', value: '45292' }],
    });
    expect(customCriteria('text', [{ op: 'equal', value: '1/1/2024' }], true)).toMatchObject({
      conditions: [{ op: 'equal', value: '1/1/2024' }],
    });
  });

  it('reads percentages in number filters', () => {
    expect(customCriteria('number', [{ op: 'lessThan', value: '50%' }], true)).toMatchObject({
      conditions: [{ op: 'lessThan', value: '0.5' }],
    });
  });
});

describe('the top 10 filter', () => {
  it('builds its criteria', () => {
    expect(top10Criteria(false, '5', true)).toEqual({
      criteria: { kind: 'top10', top: false, percent: true, value: 5 },
    });
  });

  it('refuses numbers out of range', () => {
    expect(top10Criteria(true, '0', false)).toHaveProperty('error');
    expect(top10Criteria(true, '501', false)).toHaveProperty('error');
    expect(top10Criteria(true, '101', true)).toHaveProperty('error');
    expect(top10Criteria(true, '2.5', false)).toHaveProperty('error');
    expect(top10Criteria(true, '500', false)).toHaveProperty('criteria');
  });
});
