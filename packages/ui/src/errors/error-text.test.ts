import { describe, expect, it } from 'vitest';
import { defaultNoteAnchor } from '../data/useDataTools';
import { noteAuthor, readUserName, writeUserName } from '../user-name';
import { errorFix, errorText } from './error-text';
import { DEFAULT_RULES, readErrorSettings, ruleBits, writeErrorSettings } from './useErrorTools';

function memoryStorage(): () => Storage {
  const items = new Map<string, string>();
  const storage = {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
    removeItem: (key: string) => void items.delete(key),
  } as unknown as Storage;
  return () => storage;
}

describe('error checking', () => {
  it('names errors and their fixes in Excel 2016’s words', () => {
    expect(errorText({ rule: 'evaluateToError', value: '#DIV/0!' }).title).toBe(
      'Divide by Zero Error',
    );
    expect(errorText({ rule: 'numberAsText' }).title).toBe('Number Stored as Text');
    expect(errorFix({ rule: 'numberAsText' })).toBe('Convert to Number');
    expect(errorFix({ rule: 'inconsistentFormula', from: 'left' })).toBe('Copy Formula from Left');
    expect(errorFix({ rule: 'evaluateToError' })).toBeNull();
  });

  it('keeps its settings on the device, starting from Excel’s', () => {
    const storage = memoryStorage();
    expect(readErrorSettings(storage)).toEqual({ background: true, rules: DEFAULT_RULES });
    writeErrorSettings({ background: false, rules: 1 }, storage);
    expect(ruleBits(readErrorSettings(storage))).toBe(0);
    // A storage that refuses gives the defaults.
    const refusing = () => {
      throw new Error('blocked');
    };
    expect(readErrorSettings(refusing as unknown as () => Storage).rules).toBe(DEFAULT_RULES);
  });
});

describe('notes', () => {
  it('puts a new note where Excel 2016 puts it', () => {
    expect(defaultNoteAnchor(2, 2)).toEqual([2, 15, 0, 10, 4, 31, 4, 9]);
    expect(defaultNoteAnchor(1, 1)).toEqual([1, 15, 0, 10, 3, 31, 3, 9]);
  });

  it('signs notes with the user name, or Author until there is one', () => {
    const storage = memoryStorage();
    expect(noteAuthor(storage)).toBe('Author');
    writeUserName('  Ann Lee ', storage);
    expect(readUserName(storage)).toBe('Ann Lee');
    expect(noteAuthor(storage)).toBe('Ann Lee');
    writeUserName('', storage);
    expect(noteAuthor(storage)).toBe('Author');
  });
});
