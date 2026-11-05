import { describe, expect, it } from 'vitest';
import {
  canPointAt,
  cycleAbsolute,
  findReferences,
  isFormula,
  MAX_COL,
  MAX_ROW,
  nameAtCaret,
  quoteSheet,
  referenceText,
} from './formula-text';

describe('findReferences', () => {
  const areas = (text: string) =>
    findReferences(text).map((ref) => [text.slice(ref.start, ref.end), ref.color]);

  it('finds cells and ranges, and gives the same text the same colour', () => {
    expect(areas('=A1+SUM(B2:C3)+$A$1')).toEqual([
      ['A1', 0],
      ['B2:C3', 1],
      ['$A$1', 0],
    ]);
    const [range] = findReferences('=B2:C3');
    expect(range).toMatchObject({ top: 1, left: 1, bottom: 2, right: 2, sheet: null });
  });

  it('reads sheets, quoted or not', () => {
    const [one, two] = findReferences("=Data!A1+'My Sheet'!B2");
    expect(one).toMatchObject({ sheet: 'Data', top: 0, left: 0 });
    expect(two).toMatchObject({ sheet: 'My Sheet', top: 1, left: 1 });
    expect(findReferences("='it''s'!A1")[0]!.sheet).toBe("it's");
  });

  it('reads whole rows and columns', () => {
    expect(findReferences('=SUM(B:C)')[0]).toMatchObject({
      top: 0,
      bottom: MAX_ROW - 1,
      left: 1,
      right: 2,
    });
    expect(findReferences('=SUM(2:3)')[0]).toMatchObject({
      left: 0,
      right: MAX_COL - 1,
      top: 1,
      bottom: 2,
    });
  });

  it('leaves text, functions, names and tables alone', () => {
    for (const text of ['="A1"', '=LOG10(2)', '=Rate1Year', '=T1[A1]', '=TRUE', 'A1', '=1.5E+3']) {
      expect(areas(text), text).toEqual([]);
    }
  });
});

describe('canPointAt', () => {
  it('says yes after an operator or an open bracket', () => {
    for (const text of ['=', '=A1+', '=SUM(', '=SUM(A1,', '=SUM(A1, ', '=A1:']) {
      expect(canPointAt(text, text.length), text).toBe(true);
    }
  });

  it('says no in the middle of something, or in text', () => {
    for (const text of ['=A1', '=SUM', '="a(', 'plain(', '=SUM(A1)']) {
      expect(canPointAt(text, text.length), text).toBe(false);
    }
    expect(canPointAt('=+A1', 2)).toBe(false);
  });
});

describe('nameAtCaret', () => {
  it('finds what is being typed', () => {
    expect(nameAtCaret('=SU', 3)).toEqual({ start: 1, prefix: 'SU' });
    expect(nameAtCaret('=A1+SUM', 7)).toEqual({ start: 4, prefix: 'SUM' });
    expect(nameAtCaret('=T.DI', 5)).toEqual({ start: 1, prefix: 'T.DI' });
  });

  it('finds nothing in text, after a sheet, or for a number', () => {
    expect(nameAtCaret('SU', 2)).toBeNull();
    expect(nameAtCaret('="SU', 4)).toBeNull();
    expect(nameAtCaret('=Data!A', 7)).toBeNull();
    expect(nameAtCaret('=12', 3)).toBeNull();
    expect(nameAtCaret('=', 1)).toBeNull();
  });
});

describe('cycleAbsolute', () => {
  it('goes round the four forms', () => {
    let state = { text: '=A1+1', caret: 3 };
    state = cycleAbsolute(state.text, state.caret);
    expect(state).toEqual({ text: '=$A$1+1', caret: 5 });
    state = cycleAbsolute(state.text, state.caret);
    expect(state.text).toBe('=A$1+1');
    state = cycleAbsolute(state.text, state.caret);
    expect(state.text).toBe('=$A1+1');
    state = cycleAbsolute(state.text, state.caret);
    expect(state.text).toBe('=A1+1');
  });

  it('keeps the sheet and does both ends of a range', () => {
    expect(cycleAbsolute('=S!A1:B2', 8).text).toBe('=S!$A$1:$B$2');
    expect(cycleAbsolute('=1+2', 2).text).toBe('=1+2');
  });
});

describe('referenceText', () => {
  it('writes what a click would mean', () => {
    expect(referenceText(0, 0, 0, 0, null)).toBe('A1');
    expect(referenceText(1, 1, 2, 2, null)).toBe('B2:C3');
    expect(referenceText(0, 27, 0, 27, 'Data')).toBe('Data!AB1');
    expect(referenceText(0, 0, 0, 0, 'My Sheet')).toBe("'My Sheet'!A1");
    expect(referenceText(0, 2, MAX_ROW - 1, 3, null)).toBe('C:D');
    expect(referenceText(4, 0, 5, MAX_COL - 1, null)).toBe('5:6');
  });

  it('quotes sheets as a formula needs them', () => {
    expect(quoteSheet('Sheet1')).toBe('Sheet1');
    expect(quoteSheet('A1')).toBe("'A1'");
    expect(quoteSheet('R2')).toBe("'R2'");
    expect(quoteSheet("it's")).toBe("'it''s'");
    expect(quoteSheet('ورقة')).toBe('ورقة');
  });
});

describe('isFormula', () => {
  it('goes by the first character', () => {
    expect(isFormula('=1')).toBe(true);
    expect(isFormula('+A1')).toBe(true);
    expect(isFormula('abc')).toBe(false);
  });
});
