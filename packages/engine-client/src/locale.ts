// Regional conventions from `Intl`, which `crates/cgs-numfmt` follows for built-in formats.
// The browser exposes its language, not the Windows region, so the two can differ.
import type { LocaleData } from './types';

function monthNames(locale: string, style: 'long' | 'short') {
  const format = new Intl.DateTimeFormat(locale, { month: style, timeZone: 'UTC' });
  return Array.from({ length: 12 }, (_, month) => format.format(Date.UTC(2024, month, 15)));
}

/** Sunday first, as format codes count them. */
function dayNames(locale: string, style: 'long' | 'short') {
  const format = new Intl.DateTimeFormat(locale, { weekday: style, timeZone: 'UTC' });
  // 7 January 2024 was a Sunday.
  return Array.from({ length: 7 }, (_, day) => format.format(Date.UTC(2024, 0, 7 + day)));
}

/** A locale's date style as a format code, such as `m/d/yyyy`. */
export function dateCode(locale: string, style: 'short' | 'long'): string {
  const options: Intl.DateTimeFormatOptions =
    style === 'short'
      ? { year: 'numeric', month: 'numeric', day: 'numeric', timeZone: 'UTC' }
      : { year: 'numeric', month: 'long', day: 'numeric', weekday: 'long', timeZone: 'UTC' };
  const parts = new Intl.DateTimeFormat(locale, options).formatToParts(Date.UTC(2024, 10, 22));
  return parts
    .map((part) => {
      switch (part.type) {
        case 'year':
          return part.value.length === 2 ? 'yy' : 'yyyy';
        case 'month':
          if (style === 'long') return 'mmmm';
          return part.value.length === 2 && part.value.startsWith('0') ? 'mm' : 'm';
        case 'day':
          return part.value.length === 2 && part.value.startsWith('0') ? 'dd' : 'd';
        case 'weekday':
          return 'dddd';
        default:
          return /^[\s,./-]+$/.test(part.value) ? part.value : '"' + part.value + '"';
      }
    })
    .join('');
}

function timeCode(locale: string, withSeconds: boolean): string {
  const parts = new Intl.DateTimeFormat(locale, {
    hour: 'numeric',
    minute: 'numeric',
    ...(withSeconds ? { second: 'numeric' } : {}),
    timeZone: 'UTC',
  }).formatToParts(Date.UTC(2024, 0, 1, 15, 4, 5));
  return parts
    .map((part) => {
      switch (part.type) {
        case 'hour':
          return part.value.length === 2 && part.value.startsWith('0') ? 'hh' : 'h';
        case 'minute':
          return 'mm';
        case 'second':
          return 'ss';
        case 'dayPeriod':
          return 'AM/PM';
        default:
          // Intl separates the time from AM/PM with a narrow space Excel doesn't use.
          return /^[\s:.]+$/.test(part.value)
            ? part.value.replace(/\s/g, ' ')
            : '"' + part.value + '"';
      }
    })
    .join('');
}

function period(locale: string, hour: number) {
  const parts = new Intl.DateTimeFormat(locale, {
    hour: 'numeric',
    hour12: true,
    timeZone: 'UTC',
  }).formatToParts(Date.UTC(2024, 0, 1, hour));
  return parts.find((part) => part.type === 'dayPeriod')?.value ?? (hour < 12 ? 'AM' : 'PM');
}

export function localeData(locale: string): LocaleData {
  const number = new Intl.NumberFormat(locale, { useGrouping: true }).formatToParts(1234567.5);
  const separator = (type: 'decimal' | 'group', fallback: string) =>
    number.find((part) => part.type === type)?.value ?? fallback;
  const date = new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    timeZone: 'UTC',
  })
    .formatToParts(Date.UTC(2024, 10, 22))
    .find((part) => part.type === 'literal');
  const time = new Intl.DateTimeFormat(locale, {
    hour: 'numeric',
    minute: 'numeric',
    timeZone: 'UTC',
  })
    .formatToParts(Date.UTC(2024, 0, 1, 15, 4))
    .find((part) => part.type === 'literal');
  const currency = new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: currencyFor(locale),
    currencyDisplay: 'narrowSymbol',
  })
    .formatToParts(1)
    .find((part) => part.type === 'currency');
  return {
    decimal: separator('decimal', '.'),
    group: separator('group', ','),
    shortDate: dateCode(locale, 'short'),
    longDate: dateCode(locale, 'long'),
    shortTime: timeCode(locale, false),
    longTime: timeCode(locale, true),
    am: period(locale, 9),
    pm: period(locale, 15),
    months: monthNames(locale, 'long'),
    monthsShort: monthNames(locale, 'short'),
    days: dayNames(locale, 'long'),
    daysShort: dayNames(locale, 'short'),
    currency: currency?.value ?? '$',
    dateSeparator: date?.value.trim() || '/',
    timeSeparator: time?.value.trim() || ':',
  };
}

const CURRENCY_BY_REGION: Record<string, string> = {
  AE: 'AED',
  DE: 'EUR',
  EG: 'EGP',
  ES: 'EUR',
  FR: 'EUR',
  GB: 'GBP',
  JP: 'JPY',
  KW: 'KWD',
  SA: 'SAR',
};

function currencyFor(locale: string) {
  const region = new Intl.Locale(locale).maximize().region ?? 'US';
  return CURRENCY_BY_REGION[region] ?? 'USD';
}
