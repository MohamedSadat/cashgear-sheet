import { describe, expect, it } from 'vitest';
import type { DefinedNameInfo } from '@cgs/engine-client';
import {
  defaultEdges,
  filterNames,
  nameKey,
  refersTo,
  setNameCommand,
  sortNames,
} from './NameDialogs';

const info = (name: string, formula: string, sheet: number | null = null): DefinedNameInfo => ({
  name,
  formula,
  sheet,
  scope: sheet === null ? 'Workbook' : 'Sheet' + (sheet + 1),
});

describe('sortNames', () => {
  it('sorts by name ignoring case, the workbook first', () => {
    const sorted = sortNames([
      info('tax', '=1', 0),
      info('Rate', '=2'),
      info('TAX', '=3'),
      info('alpha', '=4'),
    ]);
    expect(sorted.map((name) => name.name + ':' + name.sheet)).toEqual([
      'alpha:null',
      'Rate:null',
      'TAX:null',
      'tax:0',
    ]);
  });
});

describe('filterNames', () => {
  const names = [info('Sales', '=Sheet1!$A$1:$A$9'), info('Rate', '=0.15')];

  it('keeps everything for an empty filter', () => {
    expect(filterNames(names, '  ')).toBe(names);
  });

  it('matches the name or Refers to, ignoring case', () => {
    expect(filterNames(names, 'sal').map((name) => name.name)).toEqual(['Sales']);
    expect(filterNames(names, 'sheet1').map((name) => name.name)).toEqual(['Sales']);
    expect(filterNames(names, '0.15').map((name) => name.name)).toEqual(['Rate']);
    expect(filterNames(names, 'nothing')).toEqual([]);
  });
});

describe('nameKey', () => {
  it('tells the same name apart by scope', () => {
    expect(nameKey(info('Rate', '=1'))).not.toBe(nameKey(info('Rate', '=1', 0)));
    expect(nameKey(info('Rate', '=1'))).toBe(nameKey({ name: 'RATE', sheet: null }));
  });
});

describe('refersTo', () => {
  it('adds the = and trims', () => {
    expect(refersTo(' Sheet1!$A$1 ')).toBe('=Sheet1!$A$1');
    expect(refersTo('=0.15')).toBe('=0.15');
  });

  it('is null when empty', () => {
    expect(refersTo('')).toBeNull();
    expect(refersTo(' = ')).toBeNull();
  });
});

describe('setNameCommand', () => {
  it('makes a new name in the chosen scope', () => {
    expect(setNameCommand(undefined, ' Rate ', '=0.15', 1)).toEqual({
      kind: 'setName',
      name: 'Rate',
      formula: '=0.15',
      sheet: 1,
    });
  });

  it('keeps an edited name in its own scope', () => {
    expect(setNameCommand(info('Rate', '=0.1', 2), 'Tax', '=0.2', null)).toEqual({
      kind: 'setName',
      old: { name: 'Rate', sheet: 2 },
      name: 'Tax',
      formula: '=0.2',
      sheet: 2,
    });
  });
});

describe('defaultEdges', () => {
  it('ticks Top row and Left column for a block', () => {
    expect(defaultEdges({ top: 1, left: 1, bottom: 5, right: 3 })).toEqual({
      top: true,
      left: true,
      bottom: false,
      right: false,
    });
  });

  it('ticks only Left column for one row, and only Top row for one column', () => {
    expect(defaultEdges({ top: 2, left: 1, bottom: 2, right: 4 })).toMatchObject({
      top: false,
      left: true,
    });
    expect(defaultEdges({ top: 1, left: 2, bottom: 6, right: 2 })).toMatchObject({
      top: true,
      left: false,
    });
  });
});
