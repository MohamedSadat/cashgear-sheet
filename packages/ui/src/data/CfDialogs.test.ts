import { describe, expect, it } from 'vitest';
import type { CfRuleInfo } from '@cgs/engine-client';
import {
  CF_QUICK_DEFAULTS,
  cfQuickRule,
  cfValueFormula,
  managedCfRules,
  moveCfRow,
  splitAppliesTo,
  type CfManagerRow,
  type CfQuickInputs,
} from './CfDialogs';

const red = { fill: 'FFC7CE', color: '9C0006' };

function inputs(change: Partial<CfQuickInputs>): CfQuickInputs {
  return { ...CF_QUICK_DEFAULTS, ...change };
}

describe('cfValueFormula', () => {
  it('keeps numbers and formulas, and quotes text as Excel does', () => {
    expect(cfValueFormula('10')).toBe('10');
    expect(cfValueFormula(' -1.5e3 ')).toBe('-1.5e3');
    expect(cfValueFormula('25%')).toBe('25%');
    expect(cfValueFormula('=$A$1')).toBe('=$A$1');
    expect(cfValueFormula('true')).toBe('true');
    expect(cfValueFormula('abc')).toBe('"abc"');
    expect(cfValueFormula('say "hi"')).toBe('"say ""hi"""');
    expect(cfValueFormula('  ')).toBeNull();
  });
});

describe('cfQuickRule', () => {
  it('compares cell values', () => {
    expect(cfQuickRule('greaterThan', inputs({ first: '5' }))).toEqual({
      rule: { type: 'cellIs', op: 'greaterThan', formulas: ['5'], format: red },
    });
    expect(cfQuickRule('equal', inputs({ first: 'yes', preset: 'redBorder' }))).toEqual({
      rule: { type: 'cellIs', op: 'equal', formulas: ['"yes"'], format: { border: 'FF0000' } },
    });
    expect(cfQuickRule('between', inputs({ first: '1', second: '=B1' }))).toEqual({
      rule: { type: 'cellIs', op: 'between', formulas: ['1', '=B1'], format: red },
    });
  });

  it('wants every box filled', () => {
    expect(cfQuickRule('lessThan', inputs({}))).toEqual({ error: 'Enter a value for each box.' });
    expect(cfQuickRule('between', inputs({ first: '1' }))).toEqual({
      error: 'Enter a value for each box.',
    });
    expect(cfQuickRule('textContains', inputs({}))).toHaveProperty('error');
  });

  it('builds text, date, duplicate and average rules', () => {
    expect(cfQuickRule('textContains', inputs({ first: 'ab', preset: 'green' }))).toEqual({
      rule: {
        type: 'text',
        op: 'contains',
        text: 'ab',
        format: { fill: 'C6EFCE', color: '006100' },
      },
    });
    expect(cfQuickRule('dateOccurring', inputs({ period: 'lastMonth' }))).toEqual({
      rule: { type: 'timePeriod', period: 'lastMonth', format: red },
    });
    expect(cfQuickRule('duplicates', inputs({}))).toEqual({
      rule: { type: 'duplicates', format: red },
    });
    expect(cfQuickRule('duplicates', inputs({ unique: true }))).toEqual({
      rule: { type: 'duplicates', unique: true, format: red },
    });
    expect(cfQuickRule('belowAverage', inputs({ preset: 'redText' }))).toEqual({
      rule: { type: 'average', above: false, format: { color: '9C0006' } },
    });
  });

  it('ranks the top and bottom, by count or percent', () => {
    expect(cfQuickRule('top10', inputs({}))).toEqual({
      rule: { type: 'top10', rank: 10, format: red },
    });
    expect(cfQuickRule('bottom10Percent', inputs({ rank: '25' }))).toEqual({
      rule: { type: 'top10', bottom: true, percent: true, rank: 25, format: red },
    });
    expect(cfQuickRule('top10Percent', inputs({ rank: '101' }))).toEqual({
      error: 'Enter a whole number from 1 to 100.',
    });
    expect(cfQuickRule('bottom10', inputs({ rank: '2.5' }))).toEqual({
      error: 'Enter a whole number from 1 to 1000.',
    });
    expect(cfQuickRule('top10', inputs({ rank: '0' }))).toHaveProperty('error');
  });
});

describe('splitAppliesTo', () => {
  it('splits at spaces and commas, but not inside a quoted sheet name', () => {
    expect(splitAppliesTo('A1:B5 D1')).toEqual(['A1:B5', 'D1']);
    expect(splitAppliesTo('=$A$1:$B$5,$D$1')).toEqual(['$A$1:$B$5', '$D$1']);
    expect(splitAppliesTo("'My Sheet'!A1, C3")).toEqual(["'My Sheet'!A1", 'C3']);
    expect(splitAppliesTo('  ')).toEqual([]);
  });
});

function row(block: number, kind: string, change: Partial<CfRuleInfo> = {}): CfManagerRow {
  const info: CfRuleInfo = {
    block,
    rule: 0,
    priority: block + 1,
    stop: false,
    extended: false,
    kind,
    description: kind,
    areas: 'A1',
    fill: null,
    color: null,
    bold: null,
    ...change,
  };
  return { info, appliesTo: info.areas, stop: info.stop };
}

describe('the rules manager', () => {
  it('moves rows within the list', () => {
    expect(moveCfRow(['a', 'b', 'c'], 2, -1)).toEqual(['a', 'c', 'b']);
    expect(moveCfRow(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c']);
    const same = ['a', 'b'];
    expect(moveCfRow(same, 0, -1)).toBe(same);
    expect(moveCfRow(same, 1, 1)).toBe(same);
  });

  it('sends the classic rules in order, and leaves extended ones to the engine', () => {
    const rows = [
      { ...row(2, 'cellIs'), stop: true },
      row(0, 'dataBar', { extended: true }),
      { ...row(1, 'colorScale'), stop: true },
    ];
    const area = { top: 1, left: 1, bottom: 5, right: 2 };
    expect(managedCfRules(rows, [[area], [], [area]])).toEqual([
      { block: 2, rule: 0, areas: [area], stop: true },
      { block: 1, rule: 0, areas: [area], stop: false },
    ]);
  });
});
