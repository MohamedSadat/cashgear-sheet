import { describe, expect, it } from 'vitest';
import {
  absoluteRange,
  columnHeading,
  duplicatesMessage,
  linkLocation,
  mailtoAddress,
  readLocation,
  readMailto,
} from './SmallDialogs';

describe('Remove Duplicates', () => {
  it('says what Excel says', () => {
    expect(duplicatesMessage(0, 9)).toBe('No duplicate values found.');
    expect(duplicatesMessage(3, 9)).toBe(
      '3 duplicate values found and removed; 6 unique values remain.',
    );
  });

  it('names columns by heading or letter', () => {
    expect(columnHeading(1, 'Name', true)).toBe('Name');
    expect(columnHeading(1, 'Name', false)).toBe('Column A');
    expect(columnHeading(28, '  ', true)).toBe('Column AB');
  });
});

describe('Create Table', () => {
  it('shows the range with dollars', () => {
    expect(absoluteRange({ top: 1, left: 1, bottom: 9, right: 4 })).toBe('$A$1:$D$9');
    expect(absoluteRange({ top: 3, left: 2, bottom: 3, right: 2 })).toBe('$B$3');
  });
});

describe('Insert Hyperlink', () => {
  it('makes mail links', () => {
    expect(mailtoAddress('a@b.com', '')).toBe('mailto:a@b.com');
    expect(mailtoAddress(' mailto:a@b.com ', 'Hello there & more')).toBe(
      'mailto:a@b.com?subject=Hello%20there%20%26%20more',
    );
  });

  it('reads mail links back', () => {
    expect(readMailto('mailto:a@b.com')).toEqual({ address: 'a@b.com', subject: '' });
    expect(readMailto('mailto:a@b.com?subject=Hello%20there%20%26%20more')).toEqual({
      address: 'a@b.com',
      subject: 'Hello there & more',
    });
    expect(readMailto('MAILTO:a@b.com?cc=c@d.com&subject=1+1')).toEqual({
      address: 'a@b.com',
      subject: '1+1',
    });
  });

  it('quotes a sheet only when it needs it', () => {
    expect(linkLocation('Sheet1', 'A1')).toBe('Sheet1!A1');
    expect(linkLocation('Sheet 2', ' B3 ')).toBe("'Sheet 2'!B3");
    expect(linkLocation("it's", 'C4')).toBe("'it''s'!C4");
    expect(linkLocation(null, 'Totals')).toBe('Totals');
  });

  it('reads places back', () => {
    expect(readLocation('Sheet1!A1')).toEqual({ sheet: 'Sheet1', reference: 'A1' });
    expect(readLocation("'Sheet 2'!B3:C4")).toEqual({ sheet: 'Sheet 2', reference: 'B3:C4' });
    expect(readLocation("'it''s'!C4")).toEqual({ sheet: "it's", reference: 'C4' });
    expect(readLocation('Totals')).toEqual({ sheet: null, reference: 'Totals' });
  });
});
