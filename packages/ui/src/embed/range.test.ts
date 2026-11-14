import { describe, expect, it } from 'vitest';
import { capArea, cellCount, parseRange, rangeText } from './range';

describe('ranges', () => {
  it('read either way round and answer top-left first', () => {
    expect(parseRange('D20:A1')).toEqual({ top: 1, left: 1, bottom: 20, right: 4 });
    expect(rangeText(parseRange('D20:A1')!)).toBe('A1:D20');
    expect(rangeText(parseRange('B7')!)).toBe('B7');
    expect(rangeText(parseRange('XFD1048576')!)).toBe('XFD1048576');
    expect(parseRange('A0')).toBeNull();
  });

  it('count their cells', () => {
    expect(cellCount(parseRange('A1:D20')!)).toBe(80);
  });
});

describe('a used range cut to 5,000 cells', () => {
  const cut = (text: string) => {
    const { area, capped } = capArea(parseRange(text)!);
    return { range: rangeText(area), capped, cells: cellCount(area) };
  };

  it('is left alone when it fits', () => {
    expect(cut('A1:E1000')).toEqual({ range: 'A1:E1000', capped: false, cells: 5000 });
  });

  it('keeps every column when there are at most 50', () => {
    expect(cut('A1:Z1000')).toEqual({ range: 'A1:Z192', capped: true, cells: 4992 });
    expect(cut('C3:AZ1000')).toEqual({ range: 'C3:AZ102', capped: true, cells: 5000 });
  });

  it('keeps 50 columns of a wide sheet, or more when its rows are few', () => {
    expect(cut('A1:ZZ1000')).toEqual({ range: 'A1:AX100', capped: true, cells: 5000 });
    expect(cut('A1:CV90')).toEqual({ range: 'A1:BC90', capped: true, cells: 4950 });
    expect(cut('A1:NTP2')).toEqual({ range: 'A1:CRD2', capped: true, cells: 5000 });
    expect(cut('A1:XFD1')).toEqual({ range: 'A1:GJH1', capped: true, cells: 5000 });
  });
});
