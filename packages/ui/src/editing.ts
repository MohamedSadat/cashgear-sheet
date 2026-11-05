// What is being typed, shared by the cell editor and the formula bar, outside React's tree so that a
// keystroke only redraws those two. It knows nothing about what any cell holds until a commit.
import { canPointAt, nameAtCaret, referenceText } from './formula-text';

export interface EditState {
  active: boolean;
  sheet: number;
  /** 0-based. */
  row: number;
  col: number;
  text: string;
  caret: number;
  /** Counts each time the caret was put somewhere, rather than the text box saying where it is. */
  placed: number;
  /** Typing over the cell, where arrow keys leave it, or editing in it, where they move the caret. */
  mode: 'enter' | 'edit';
  /** Which of the two boxes has the keyboard. */
  place: 'cell' | 'bar';
  /** The reference a click or arrow key last put in, which the next one replaces. */
  picked: {
    start: number;
    end: number;
    row: number;
    col: number;
    endRow: number;
    endCol: number;
  } | null;
}

const idle: EditState = {
  active: false,
  sheet: 0,
  row: 0,
  col: 0,
  text: '',
  caret: 0,
  placed: 0,
  mode: 'enter',
  place: 'cell',
  picked: null,
};

export class EditSession {
  private current: EditState = idle;
  private readonly listeners = new Set<() => void>();

  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly snapshot = () => this.current;

  private set(next: EditState) {
    this.current = next;
    for (const listener of this.listeners) listener();
  }

  begin(
    sheet: number,
    row: number,
    col: number,
    text: string,
    mode: EditState['mode'],
    place: EditState['place'],
  ) {
    this.set({
      active: true,
      sheet,
      row,
      col,
      text,
      caret: text.length,
      placed: this.current.placed + 1,
      mode,
      place,
      picked: null,
    });
  }

  /** The user typed, or moved the caret. */
  setText(text: string, caret: number) {
    if (!this.current.active) return;
    const same = text === this.current.text;
    if (same && caret === this.current.caret) return;
    // Typing ends what a click put in, and moving away from it does too.
    const picked =
      same && this.current.picked?.end === caret && this.current.picked.end !== undefined
        ? this.current.picked
        : null;
    this.set({ ...this.current, text, caret, picked });
  }

  /** Changes the text from outside the text box, and puts the caret where it should go. */
  replace(text: string, caret: number) {
    if (!this.current.active) return;
    this.set({ ...this.current, text, caret, placed: this.current.placed + 1, picked: null });
  }

  setPlace(place: EditState['place']) {
    if (!this.current.active || this.current.place === place) return;
    this.set({ ...this.current, place, mode: place === 'bar' ? 'edit' : this.current.mode });
  }

  setMode(mode: EditState['mode']) {
    if (this.current.active) this.set({ ...this.current, mode });
  }

  end() {
    if (this.current.active) this.set(idle);
  }

  /** Whether a cell picked now would go in as a reference. */
  canPoint() {
    const { active, text, caret, picked } = this.current;
    return active && (!!picked || canPointAt(text, caret));
  }

  /** Puts a reference in at the caret, or in place of the one last picked. */
  pick(row: number, col: number, endRow: number, endCol: number, sheetName: string | null) {
    const { text, caret, picked } = this.current;
    const top = Math.min(row, endRow);
    const bottom = Math.max(row, endRow);
    const left = Math.min(col, endCol);
    const right = Math.max(col, endCol);
    const reference = referenceText(top, left, bottom, right, sheetName);
    const start = picked ? picked.start : caret;
    const end = picked ? picked.end : caret;
    const next = text.slice(0, start) + reference + text.slice(end);
    this.set({
      ...this.current,
      text: next,
      caret: start + reference.length,
      placed: this.current.placed + 1,
      picked: { start, end: start + reference.length, row, col, endRow, endCol },
    });
  }

  /** Puts a function's name in, in place of what has been typed of it. */
  complete(name: string) {
    const { text, caret } = this.current;
    const found = nameAtCaret(text, caret);
    if (!found) return;
    const insert = name + '(';
    const next = text.slice(0, found.start) + insert + text.slice(caret);
    this.replace(next, found.start + insert.length);
  }
}
