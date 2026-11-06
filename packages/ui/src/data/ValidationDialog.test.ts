import { describe, expect, it } from 'vitest';
import type { ValidationInfo } from '@cgs/engine-client';
import {
  buildRule,
  EMPTY_FORM,
  fieldLabels,
  formOf,
  parseDate,
  parseTime,
  shownFormula,
  storedFormula,
  type ValidationForm,
} from './ValidationDialog';

const form = (part: Partial<ValidationForm>): ValidationForm => ({ ...EMPTY_FORM, ...part });

describe('buildRule', () => {
  it('takes rules off for any value with no input message', () => {
    expect(buildRule(EMPTY_FORM)).toEqual({ rule: undefined });
    expect(buildRule(form({ errorTitle: 'Oops' }))).toEqual({ rule: undefined });
  });

  it('keeps an any value rule that has an input message', () => {
    const built = buildRule(form({ inputTitle: 'Hint', inputText: 'Type anything' }));
    expect(built).toEqual({
      rule: {
        kind: 'any',
        allowBlank: true,
        showInput: true,
        inputTitle: 'Hint',
        inputText: 'Type anything',
        showError: true,
        errorTitle: '',
        errorText: '',
        style: 'stop',
      },
    });
  });

  it('gives a between rule both bounds', () => {
    const built = buildRule(form({ kind: 'whole', op: 'between', formula1: '1', formula2: '=B1' }));
    expect(built).toMatchObject({
      rule: { kind: 'whole', op: 'between', formula1: '1', formula2: '=B1' },
    });
  });

  it('puts a one-box bound in formula1 and drops the other box', () => {
    const built = buildRule(
      form({ kind: 'decimal', op: 'lessThan', formula1: '2.5', formula2: 'left over' }),
    );
    expect(built).toMatchObject({ rule: { kind: 'decimal', op: 'lessThan', formula1: '2.5' } });
    expect('rule' in built && built.rule && 'formula2' in built.rule).toBe(false);
  });

  it('says which box is empty', () => {
    expect(buildRule(form({ kind: 'whole', formula1: '1' }))).toEqual({
      error: 'You must enter a value in the Maximum box.',
    });
    expect(buildRule(form({ kind: 'list' }))).toEqual({
      error: 'You must enter a value in the Source box.',
    });
    expect(buildRule(form({ kind: 'date', op: 'equal', formula1: ' ' }))).toEqual({
      error: 'You must enter a value in the Date box.',
    });
  });

  it('quotes a list of items, keeps a reference, and has no op', () => {
    const items = buildRule(form({ kind: 'list', formula1: 'Yes,No', dropdown: false }));
    expect(items).toMatchObject({ rule: { kind: 'list', formula1: '"Yes,No"', dropdown: false } });
    expect('rule' in items && items.rule && 'op' in items.rule).toBe(false);
    expect(buildRule(form({ kind: 'list', formula1: '=$A$1:$A$5' }))).toMatchObject({
      rule: { formula1: '=$A$1:$A$5', dropdown: true },
    });
  });

  it('turns dates and times into numbers', () => {
    expect(
      buildRule(form({ kind: 'date', op: 'greaterThan', formula1: '1/1/2020' })),
    ).toMatchObject({ rule: { formula1: '43831' } });
    expect(buildRule(form({ kind: 'time', op: 'lessThan', formula1: '12:00 PM' }))).toMatchObject({
      rule: { formula1: '0.5' },
    });
  });

  it('carries a custom formula and the error alert', () => {
    const built = buildRule(
      form({
        kind: 'custom',
        formula1: '=ISNUMBER(A1)',
        allowBlank: false,
        showError: false,
        style: 'warning',
        errorTitle: 'Careful',
        errorText: 'Numbers only',
      }),
    );
    expect(built).toEqual({
      rule: {
        kind: 'custom',
        formula1: '=ISNUMBER(A1)',
        allowBlank: false,
        showInput: true,
        inputTitle: '',
        inputText: '',
        showError: false,
        errorTitle: 'Careful',
        errorText: 'Numbers only',
        style: 'warning',
      },
    });
  });
});

describe('fieldLabels', () => {
  it('labels the boxes as Excel does', () => {
    expect(fieldLabels('any', 'between')).toEqual([]);
    expect(fieldLabels('whole', 'notBetween')).toEqual(['Minimum', 'Maximum']);
    expect(fieldLabels('decimal', 'equal')).toEqual(['Value']);
    expect(fieldLabels('textLength', 'notEqual')).toEqual(['Length']);
    expect(fieldLabels('textLength', 'greaterThanOrEqual')).toEqual(['Minimum']);
    expect(fieldLabels('date', 'between')).toEqual(['Start date', 'End date']);
    expect(fieldLabels('date', 'lessThanOrEqual')).toEqual(['End date']);
    expect(fieldLabels('time', 'greaterThan')).toEqual(['Start time']);
    expect(fieldLabels('time', 'equal')).toEqual(['Time']);
    expect(fieldLabels('list', 'between')).toEqual(['Source']);
    expect(fieldLabels('custom', 'lessThan')).toEqual(['Formula']);
  });
});

describe('formulas in the boxes', () => {
  it('shows what the engine keeps as Excel shows it', () => {
    expect(shownFormula('list', '"a,""b"",c"')).toBe('a,"b",c');
    expect(shownFormula('list', '$A$1:$A$5')).toBe('=$A$1:$A$5');
    expect(shownFormula('whole', '10')).toBe('10');
    expect(shownFormula('whole', 'B1*2')).toBe('=B1*2');
    expect(shownFormula('date', '43831')).toBe('1/1/2020');
    expect(shownFormula('date', '43831.5')).toBe('1/1/2020 12:00:00 PM');
    expect(shownFormula('time', '0.75')).toBe('6:00:00 PM');
    expect(shownFormula('custom', null)).toBe('');
  });

  it('reads dates and times both ways round', () => {
    expect(parseDate('2020-01-01')).toBe(43831);
    expect(parseDate('3/1/1900')).toBe(61);
    expect(parseDate('2/29/1900')).toBe(60);
    expect(parseDate('1/1/1900')).toBe(1);
    expect(parseDate('2/30/2021')).toBeNull();
    expect(parseDate('1/1/2020 6:00 AM')).toBe(43831.25);
    expect(parseDate('12/31/1899')).toBeNull();
    expect(parseTime('0:00')).toBe(0);
    expect(parseTime('12:00 AM')).toBe(0);
    expect(parseTime('13:30')).toBeCloseTo(13.5 / 24);
    expect(parseTime('13:30 PM')).toBeNull();
    for (const serial of [1, 59, 60, 61, 43831]) {
      expect(parseDate(shownFormula('date', String(serial)))).toBe(serial);
    }
  });

  it('leaves what it can’t read for the engine to judge', () => {
    expect(storedFormula('date', 'tomorrow')).toBe('tomorrow');
    expect(storedFormula('date', '=TODAY()')).toBe('=TODAY()');
    expect(storedFormula('whole', '  5 ')).toBe('5');
  });
});

describe('formOf', () => {
  it('starts a new cell as Excel does', () => {
    expect(formOf(null)).toEqual(EMPTY_FORM);
  });

  it('fills the dialog from a cell’s rule', () => {
    const info: ValidationInfo = {
      kind: 'list',
      op: 'between',
      formula1: '"Red,Green"',
      formula2: null,
      allowBlank: false,
      dropdown: false,
      showInput: true,
      inputTitle: 'Colour',
      inputText: 'Pick one',
      showError: true,
      errorTitle: '',
      errorText: '',
      style: 'information',
      items: ['Red', 'Green'],
      extended: false,
      areas: 'A1:A5',
    };
    expect(formOf(info)).toEqual({
      ...EMPTY_FORM,
      kind: 'list',
      formula1: 'Red,Green',
      allowBlank: false,
      dropdown: false,
      inputTitle: 'Colour',
      inputText: 'Pick one',
      style: 'information',
    });
    expect(buildRule(formOf(info))).toMatchObject({ rule: { formula1: '"Red,Green"' } });
  });
});
