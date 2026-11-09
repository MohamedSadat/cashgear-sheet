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
  /** Text with runs of their own fonts, once a cell's text is rich or characters are formatted. */
  rich: RichEdit | null;
  /** The selected characters, from the caret's other end to it. */
  anchor: number;
}

/** A run's font as the cell editor keeps it: anything left out is the cell's. */
export interface RunStyle {
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: 'single' | 'double' | 'singleAccounting' | 'doubleAccounting';
  size?: number;
  name?: string;
  /** `RRGGBB`. */
  color?: string;
  vertAlign?: 'superscript' | 'subscript';
}

export interface RichSpan {
  text: string;
  style: RunStyle;
}

export interface RichEdit {
  spans: RichSpan[];
  /** Counts each time the spans change from outside the editor, which then draws them again. */
  version: number;
}

const sameStyle = (a: RunStyle, b: RunStyle) => JSON.stringify(a) === JSON.stringify(b);

/** Spans with neighbours of one style joined and empty ones gone. */
export function joinSpans(spans: RichSpan[]): RichSpan[] {
  const out: RichSpan[] = [];
  for (const span of spans) {
    if (!span.text) continue;
    const last = out[out.length - 1];
    if (last && sameStyle(last.style, span.style)) last.text += span.text;
    else out.push({ text: span.text, style: { ...span.style } });
  }
  return out;
}

/** A style put on characters `start` to `end` of spans, splitting them where it starts and ends. */
export function styleSpans(
  spans: RichSpan[],
  start: number,
  end: number,
  apply: (style: RunStyle) => RunStyle,
): RichSpan[] {
  const out: RichSpan[] = [];
  let at = 0;
  for (const span of spans) {
    const from = at;
    const to = at + span.text.length;
    at = to;
    const cut = [from, Math.max(from, Math.min(to, start)), Math.max(from, Math.min(to, end)), to];
    for (let piece = 0; piece < 3; piece++) {
      const text = span.text.slice(cut[piece]! - from, cut[piece + 1]! - from);
      out.push({ text, style: piece === 1 ? apply(span.style) : span.style });
    }
  }
  return joinSpans(out);
}

/** Characters `from` to `to` of spans. */
export function sliceSpans(spans: RichSpan[], from: number, to: number): RichSpan[] {
  const out: RichSpan[] = [];
  let at = 0;
  for (const span of spans) {
    const start = at;
    at += span.text.length;
    const text = span.text.slice(Math.max(0, from - start), Math.max(0, to - start));
    if (text) out.push({ text, style: span.style });
  }
  return out;
}

/** The style of the character at `at`. */
function styleAt(spans: RichSpan[], at: number): RunStyle {
  let end = 0;
  for (const span of spans) {
    end += span.text.length;
    if (at < end) return span.style;
  }
  return spans[spans.length - 1]?.style ?? {};
}

/** Runs as the engine takes them, from characters counted from 0. */
export function spanRuns(spans: RichSpan[]): { start: number; font: RunStyle }[] {
  const out: { start: number; font: RunStyle }[] = [];
  let at = 0;
  for (const span of joinSpans(spans)) {
    out.push({ start: at, font: span.style });
    at += span.text.length;
  }
  return out;
}

/** Spans from a text and the engine's runs. */
export function runSpans(
  text: string,
  runs: { start: number; font: RunStyle | null }[],
): RichSpan[] {
  const sorted = [...runs].sort((a, b) => a.start - b.start);
  const spans: RichSpan[] = [];
  if (!sorted.length || sorted[0]!.start > 0) {
    spans.push({ text: text.slice(0, sorted[0]?.start ?? text.length), style: {} });
  }
  sorted.forEach((run, index) => {
    const end = sorted[index + 1]?.start ?? text.length;
    const style: RunStyle = {};
    for (const [key, value] of Object.entries(run.font ?? {})) {
      if (value !== null && value !== false && value !== undefined) {
        (style as Record<string, unknown>)[key] = value;
      }
    }
    spans.push({ text: text.slice(run.start, end), style });
  });
  return joinSpans(spans);
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
  rich: null,
  anchor: 0,
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
      rich: null,
      anchor: text.length,
    });
  }

  /** The cell's text is rich: the editor shows its runs. */
  setRich(spans: RichSpan[]) {
    if (!this.current.active) return;
    const version = (this.current.rich?.version ?? 0) + 1;
    this.set({ ...this.current, rich: { spans: joinSpans(spans), version } });
  }

  /** What the rich editor holds after typing, which it already shows. */
  richTyped(spans: RichSpan[], anchor: number, caret: number) {
    if (!this.current.active || !this.current.rich) return;
    const text = spans.map((span) => span.text).join('');
    this.set({
      ...this.current,
      text,
      caret,
      anchor,
      picked: null,
      rich: { spans: joinSpans(spans), version: this.current.rich.version },
    });
  }

  /** The selected characters, which formatting applies to while editing. */
  select(anchor: number, caret: number) {
    if (!this.current.active) return;
    if (anchor === this.current.anchor && caret === this.current.caret) return;
    this.set({ ...this.current, anchor, caret });
  }

  /**
   * Formats the selected characters, making the text rich if it isn't. Formulas have no runs.
   * Says whether anything was selected to format.
   */
  formatSelection(apply: (style: RunStyle) => RunStyle): boolean {
    const { active, text, anchor, caret, rich } = this.current;
    if (!active || text.startsWith('=')) return false;
    const start = Math.min(anchor, caret);
    const end = Math.max(anchor, caret);
    if (start === end) return false;
    const spans = rich?.spans ?? [{ text, style: {} }];
    const version = (rich?.version ?? 0) + 1;
    this.set({ ...this.current, rich: { spans: styleSpans(spans, start, end, apply), version } });
    return true;
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
    // Text changed in the plain box (the formula bar) loses runs it no longer matches.
    const rich =
      this.current.rich && !same
        ? { spans: [{ text, style: {} }], version: this.current.rich.version + 1 }
        : this.current.rich;
    this.set({ ...this.current, text, caret, anchor: caret, picked, rich });
  }

  /** The selected characters, from a plain box. */
  setSelection(anchor: number, caret: number, text: string) {
    if (!this.current.active) return;
    if (text !== this.current.text) {
      this.setText(text, caret);
    }
    this.select(anchor, caret);
  }

  /** Changes the text from outside the text box, and puts the caret where it should go. */
  replace(text: string, caret: number) {
    if (!this.current.active) return;
    // Rich text keeps its runs for what stays the same at the start and end.
    let rich = this.current.rich;
    if (rich) {
      const old = this.current.text;
      let head = 0;
      while (head < old.length && head < text.length && old[head] === text[head]) head++;
      let tail = 0;
      while (
        tail < old.length - head &&
        tail < text.length - head &&
        old[old.length - 1 - tail] === text[text.length - 1 - tail]
      ) {
        tail++;
      }
      const inserted = text.slice(head, text.length - tail);
      // What is typed takes the style of the character before it, as in Excel.
      const style = styleAt(rich.spans, Math.max(0, head - 1));
      const kept: RichSpan[] = [
        ...sliceSpans(rich.spans, 0, head),
        { text: inserted, style },
        ...sliceSpans(rich.spans, old.length - tail, old.length),
      ];
      rich = { spans: joinSpans(kept), version: rich.version + 1 };
    }
    this.set({
      ...this.current,
      text,
      caret,
      anchor: caret,
      placed: this.current.placed + 1,
      picked: null,
      rich,
    });
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
