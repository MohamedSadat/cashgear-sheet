import { describe, expect, it, vi } from 'vitest';
import { EditSession } from './editing';

function started(text: string) {
  const session = new EditSession();
  session.begin(0, 0, 0, text, 'enter', 'cell');
  return session;
}

describe('EditSession', () => {
  it('tells its listeners when anything changes', () => {
    const session = new EditSession();
    const listener = vi.fn();
    session.subscribe(listener);
    session.begin(1, 2, 3, 'x', 'edit', 'bar');
    expect(listener).toHaveBeenCalledTimes(1);
    expect(session.snapshot()).toMatchObject({ active: true, sheet: 1, row: 2, col: 3, caret: 1 });
    session.setText('xy', 2);
    expect(session.snapshot().text).toBe('xy');
    session.end();
    expect(session.snapshot().active).toBe(false);
  });

  it('puts a picked cell in at the caret, and a second pick replaces it', () => {
    const session = started('=SUM(');
    expect(session.canPoint()).toBe(true);
    session.pick(0, 0, 0, 0, null);
    expect(session.snapshot().text).toBe('=SUM(A1');
    session.pick(1, 1, 1, 1, null);
    expect(session.snapshot().text).toBe('=SUM(B2');
    session.pick(1, 1, 3, 2, null);
    expect(session.snapshot().text).toBe('=SUM(B2:C4');
    expect(session.snapshot().caret).toBe(10);
  });

  it('starts a new reference after typing', () => {
    const session = started('=');
    session.pick(0, 0, 0, 0, null);
    session.setText('=A1+', 4);
    expect(session.snapshot().picked).toBeNull();
    session.pick(1, 0, 1, 0, 'Data');
    expect(session.snapshot().text).toBe('=A1+Data!A2');
  });

  it('stops pointing when the caret moves away', () => {
    const session = started('=');
    session.pick(0, 0, 0, 0, null);
    session.setText('=A1', 1);
    expect(session.snapshot().picked).toBeNull();
    expect(session.canPoint()).toBe(false);
  });

  it('has nothing to point at in text', () => {
    expect(started('abc').canPoint()).toBe(false);
  });

  it('completes a function name', () => {
    const session = started('=A1+SU');
    session.complete('SUM');
    expect(session.snapshot().text).toBe('=A1+SUM(');
    expect(session.snapshot().caret).toBe(8);
  });
});
