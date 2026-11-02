import { describe, expect, it } from 'vitest';
import { shortenGeneral, singleLine, wrapLines } from './text-fit';

const within = (chars: number) => (candidate: string) => candidate.length <= chars;

describe('shortenGeneral', () => {
  it('leaves what fits alone', () => {
    expect(shortenGeneral('3.14159', within(10))).toBe('3.14159');
  });

  it('gives up decimals first, rounding as it goes', () => {
    expect(shortenGeneral('3.14159', within(4))).toBe('3.14');
    expect(shortenGeneral('2.5999', within(3))).toBe('2.6');
    expect(shortenGeneral('1234.5678', within(4))).toBe('1235');
  });

  it('turns to scientific when the whole part does not fit', () => {
    expect(shortenGeneral('123456789012', within(8))).toBe('1.23E+11');
    expect(shortenGeneral('123456789012', within(5))).toBe('1E+11');
  });

  it('shortens what is already scientific', () => {
    expect(shortenGeneral('1.23457E+11', within(8))).toBe('1.23E+11');
  });

  it('keeps a small number meaningful', () => {
    expect(shortenGeneral('0.000123457', within(8))).toBe('0.000123');
    expect(shortenGeneral('0.000123457', within(5))).toBe('1E-04');
  });

  it('keeps a decimal comma', () => {
    expect(shortenGeneral('3,14159', within(4))).toBe('3,14');
  });

  it('says so when nothing fits', () => {
    expect(shortenGeneral('123456', within(3))).toBeNull();
  });
});

describe('wrapLines', () => {
  it('breaks at spaces and keeps line breaks', () => {
    expect(wrapLines('one two three\nfour', within(7))).toEqual(['one two', 'three', 'four']);
  });

  it('splits a word only when it must', () => {
    expect(wrapLines('abcdefgh', within(3))).toEqual(['abc', 'def', 'gh']);
  });

  it('keeps empty lines', () => {
    expect(wrapLines('a\n\nb', within(3))).toEqual(['a', '', 'b']);
  });
});

describe('singleLine', () => {
  it('drops what a single line never shows', () => {
    expect(singleLine('a\nb\tc')).toBe('abc');
    expect(singleLine('plain')).toBe('plain');
  });
});
