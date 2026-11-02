import { describe, expect, it } from 'vitest';
import { localeData } from './locale';

describe('localeData', () => {
  it('matches what Excel shows on an en-US machine', () => {
    const data = localeData('en-US');
    expect(data.decimal).toBe('.');
    expect(data.group).toBe(',');
    expect(data.shortDate).toBe('m/d/yyyy');
    expect(data.shortTime).toBe('h:mm AM/PM');
    expect(data.longTime).toBe('h:mm:ss AM/PM');
    expect(data.months[0]).toBe('January');
    expect(data.monthsShort[11]).toBe('Dec');
    expect(data.days[0]).toBe('Sunday');
    expect(data.daysShort[6]).toBe('Sat');
    expect(data.am).toBe('AM');
    expect(data.currency).toBe('$');
  });

  it('follows another language', () => {
    const data = localeData('de-DE');
    expect(data.decimal).toBe(',');
    expect(data.group).toBe('.');
    expect(data.shortDate).toBe('d.m.yyyy');
    expect(data.months[2]).toBe('März');
    expect(data.currency).toBe('€');
  });

  it('gives every list its full length for Arabic', () => {
    const data = localeData('ar-EG');
    expect(data.months).toHaveLength(12);
    expect(data.days).toHaveLength(7);
    expect(data.months[0]).toBe('يناير');
  });
});
