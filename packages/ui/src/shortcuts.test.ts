import { describe, expect, it } from 'vitest';
import { actionFor, startsTyping } from './shortcuts';

describe('actionFor', () => {
  it('knows the edit keys', () => {
    expect(actionFor({ key: 'z', ctrlKey: true })).toBe('undo');
    expect(actionFor({ key: 'Z', ctrlKey: true, shiftKey: true })).toBe('redo');
    expect(actionFor({ key: 'y', ctrlKey: true })).toBe('redo');
    expect(actionFor({ key: 'c', ctrlKey: true })).toBe('copy');
    expect(actionFor({ key: 'x', ctrlKey: true })).toBe('cut');
    expect(actionFor({ key: 'v', ctrlKey: true })).toBe('paste');
    expect(actionFor({ key: 'V', ctrlKey: true, shiftKey: true })).toBe('pasteValues');
    expect(actionFor({ key: 'd', ctrlKey: true })).toBe('fillDown');
    expect(actionFor({ key: 'r', ctrlKey: true })).toBe('fillRight');
    expect(actionFor({ key: 'Delete' })).toBe('clear');
    expect(actionFor({ key: 'F2' })).toBe('edit');
    expect(actionFor({ key: 'Escape' })).toBe('cancel');
  });

  it('knows the calculation keys', () => {
    expect(actionFor({ key: 'F9' })).toBe('calculate');
    expect(actionFor({ key: 'F9', shiftKey: true })).toBe('calculateSheet');
    expect(actionFor({ key: 'F9', ctrlKey: true, altKey: true })).toBe('calculateAll');
  });

  it('knows the keys for rows, columns and sheets', () => {
    expect(actionFor({ key: '-', ctrlKey: true })).toBe('deleteCells');
    expect(actionFor({ key: '+', ctrlKey: true, shiftKey: true })).toBe('insertCells');
    expect(actionFor({ key: '=', ctrlKey: true, shiftKey: true })).toBe('insertCells');
    expect(actionFor({ key: 'F11', shiftKey: true })).toBe('insertSheet');
    expect(actionFor({ key: 'PageUp', ctrlKey: true })).toBe('previousSheet');
    expect(actionFor({ key: 'PageDown', ctrlKey: true })).toBe('nextSheet');
    expect(actionFor({ key: ' ', ctrlKey: true })).toBe('selectColumn');
    expect(actionFor({ key: ' ', shiftKey: true })).toBe('selectRow');
    expect(actionFor({ key: 'a', ctrlKey: true })).toBe('selectAll');
  });

  it('knows the shortcuts for a date, a time and a sum', () => {
    expect(actionFor({ key: ';', ctrlKey: true })).toBe('today');
    expect(actionFor({ key: ':', ctrlKey: true, shiftKey: true })).toBe('now');
    expect(actionFor({ key: '=', altKey: true })).toBe('autoSum');
  });

  it('leaves other keys alone', () => {
    expect(actionFor({ key: 'a' })).toBeNull();
    expect(actionFor({ key: 'ArrowDown' })).toBeNull();
    expect(actionFor({ key: 'o', ctrlKey: true })).toBeNull();
    expect(actionFor({ key: 's', ctrlKey: true })).toBeNull();
    expect(actionFor({ key: '=', ctrlKey: true })).toBeNull();
  });
});

describe('startsTyping', () => {
  it('takes characters and what an input method sends', () => {
    expect(startsTyping({ key: 'a' })).toBe(true);
    expect(startsTyping({ key: 'ق' })).toBe(true);
    expect(startsTyping({ key: '=' })).toBe(true);
    expect(startsTyping({ key: 'Process' })).toBe(true);
    expect(startsTyping({ key: 'Dead' })).toBe(true);
  });

  it('leaves shortcuts and movement alone', () => {
    expect(startsTyping({ key: 'a', ctrlKey: true })).toBe(false);
    expect(startsTyping({ key: 'a', altKey: true })).toBe(false);
    expect(startsTyping({ key: 'ArrowDown' })).toBe(false);
    expect(startsTyping({ key: 'Enter' })).toBe(false);
    expect(startsTyping({ key: 'F2' })).toBe(false);
  });
});
