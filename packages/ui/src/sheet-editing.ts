// What the keyboard and mouse do to a workbook: typing, committing, pointing at cells, the clipboard,
// fills and the shortcut map. Everything that changes cells leaves here as an engine command. A key
// never waits for the engine: the selection moves at once, so that typing keeps up however long a
// calculation takes, and a command the engine refuses comes back as an open editor and a message.
import type { Applied, CellArea, Command, EngineClient } from '@cgs/engine-client';
import type { KeyboardEvent } from 'react';
import { EditSession, runSpans, spanRuns } from './editing';
import { cycleAbsolute, findReferences, MAX_COL, MAX_ROW } from './formula-text';
import type { CellRange, GridController } from './grid/controller';
import type { GridAction } from './shortcuts';

export interface EditingHost {
  engine(): EngineClient | null;
  grid(): GridController | null;
  /** Runs commands one after another, the first making the source safe. */
  run(command: Command): Promise<Applied>;
  /** Commands sent so far have run. A question about cells has to wait for that. */
  settled(): Promise<unknown>;
  /** Whether cells can change now. */
  canEdit(): boolean;
  /** Whether this cell may be edited, saying why not when it may not. */
  mayEdit(sheet: number, row: number, col: number): boolean;
  /** The active cell's text, as the formula bar shows it. */
  activeText(): string;
  sheetName(index: number): string;
  sheetIndex(name: string): number | null;
  /** Shows a sheet, and resolves once it is showing. */
  showSheet(index: number): Promise<void>;
  /** The tab next to the one showing. */
  nextSheet(step: number): number | null;
  notice(message: string | null): void;
  focusEditor(): void;
  editorElement(): HTMLElement | null;
  insertSheet(): void;
  calculate(kind: 'now' | 'sheet' | 'all'): void;
  /** Autocomplete, which has first refusal on some keys. */
  completionKey(event: KeyboardEvent<HTMLElement>): boolean;
  /** Fits columns to what they hold, through the same queue as commands. */
  fitColumns(sheet: number, first: number, last: number): void;
  /** A formatting shortcut, which the shell knows what to do with. */
  formatAction(action: GridAction): void;
}

export function toArea(range: CellRange): CellArea {
  return {
    top: range.top + 1,
    left: range.left + 1,
    bottom: range.bottom + 1,
    right: range.right + 1,
  };
}

export function fromArea(area: CellArea): CellRange {
  return {
    top: area.top - 1,
    left: area.left - 1,
    bottom: area.bottom - 1,
    right: area.right - 1,
  };
}

/** Browsers can change line endings on the way through the clipboard. */
function clipboardKey(text: string) {
  return text.replace(/\r\n?/g, '\n').replace(/\n$/, '');
}

function latinLocale() {
  try {
    return new Intl.Locale(navigator.language || 'en-US', { numberingSystem: 'latn' }).toString();
  } catch {
    return 'en-US';
  }
}

interface Clip {
  sheet: number;
  range: CellRange;
  cut: boolean;
  /** What went to the clipboard, once the engine has said. */
  text: string | null;
}

export class SheetEditing {
  readonly session = new EditSession();
  private clip: Clip | null = null;
  private pasteValues = false;
  private pointStart: { row: number; col: number } | null = null;
  private wasActive = false;

  constructor(private readonly host: EditingHost) {
    this.session.subscribe(this.onSession);
  }

  private readonly onSession = () => {
    const grid = this.host.grid();
    const state = this.session.snapshot();
    if (!grid) return;
    if (state.active !== this.wasActive) {
      this.wasActive = state.active;
      grid.setEditing(
        state.active ? { sheet: state.sheet, row: state.row, col: state.col } : null,
        this.host.editorElement(),
      );
    }
    const highlights = [];
    if (state.active) {
      for (const reference of findReferences(state.text)) {
        const sheet =
          reference.sheet === null ? state.sheet : this.host.sheetIndex(reference.sheet);
        if (sheet !== null) {
          highlights.push({
            sheet,
            color: reference.color,
            range: {
              top: reference.top,
              left: reference.left,
              bottom: reference.bottom,
              right: reference.right,
            },
          });
        }
      }
    }
    grid.setHighlights(highlights);
  };

  /** Starts the editor, unless the cell refuses it, as a locked cell on a protected sheet does. */
  private begin(...args: Parameters<EditSession['begin']>): boolean {
    const [sheet, row, col] = args;
    if (!this.host.mayEdit(sheet, row, col)) return false;
    this.session.begin(...args);
    return true;
  }

  /** A key that types went down: the editor takes focus, and so the key's character. */
  readonly type = () => {
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit()) return;
    const { sheet, row, col } = grid.selection();
    if (!this.begin(sheet, row, col, '', 'enter', 'cell')) return;
    this.host.focusEditor();
  };

  /** Starts editing what the active cell holds. */
  async edit(place: 'cell' | 'bar' = 'cell') {
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit() || this.session.snapshot().active) return;
    const { sheet, row, col } = grid.selection();
    if (place === 'bar') {
      if (!this.begin(sheet, row, col, this.host.activeText(), 'edit', 'bar')) return;
      return;
    }
    await this.host.settled();
    const engine = this.host.engine();
    const [text, runs] = await Promise.all([
      engine?.cellText(sheet, row + 1, col + 1),
      engine?.query({ kind: 'richAt', sheet, row: row + 1, col: col + 1 }),
    ]);
    if (this.session.snapshot().active) return;
    if (!this.begin(sheet, row, col, text?.edit ?? '', 'edit', place)) return;
    // A rich cell is edited with its runs, as in Excel.
    if (runs && text) this.session.setRich(runSpans(text.edit, runs));
    this.host.focusEditor();
  }

  /** Runs a command for its own sake. A refusal only needs saying. */
  private send(command: Command) {
    this.host.notice(null);
    this.host.run(command).catch((error) => this.refused(error));
  }

  private refused(error: unknown) {
    this.host.notice(error instanceof Error ? error.message : String(error));
  }

  /** Puts what was typed into the cell, and moves on. */
  commit(rows = 0, cols = 0, scope: 'cell' | 'selection' | 'array' = 'cell') {
    const state = this.session.snapshot();
    const grid = this.host.grid();
    if (!state.active || !grid) return;
    const selection = grid.selection();
    let command: Command;
    const rich = state.rich && !state.text.startsWith('=') ? state.rich : null;
    if (rich && scope === 'cell' && rich.spans.some((span) => Object.keys(span.style).length)) {
      // Text with runs of its own fonts, as the cell editor made it.
      command = {
        kind: 'richInput',
        sheet: state.sheet,
        row: state.row + 1,
        col: state.col + 1,
        text: state.text,
        runs: spanRuns(rich.spans),
      };
    } else if (scope === 'array') {
      command = {
        kind: 'arrayInput',
        sheet: state.sheet,
        area: toArea(selection.range),
        text: state.text,
      };
    } else {
      command = {
        kind: 'input',
        sheet: state.sheet,
        row: state.row + 1,
        col: state.col + 1,
        text: state.text,
        ...(scope === 'selection' ? { areas: selection.areas.map(toArea) } : {}),
      };
    }
    this.session.end();
    this.letGoOfClip();
    this.pointStart = null;
    if (grid.currentSheet() !== state.sheet) void this.host.showSheet(state.sheet);
    if (rows || cols) grid.moveBy(rows, cols);
    grid.focus();
    this.host.notice(null);
    this.host.run(command).catch((error) => {
      this.refused(error);
      // It stays open on what was typed, so that nothing has to be typed again.
      if (this.session.snapshot().active) return;
      void this.host.showSheet(state.sheet).then(() => {
        grid.selectArea(
          { top: state.row, left: state.col, bottom: state.row, right: state.col },
          state.row,
          state.col,
        );
        if (!this.begin(state.sheet, state.row, state.col, state.text, 'edit', 'cell')) return;
        this.host.focusEditor();
      });
    });
  }

  /** Esc: leaves the cell as it was. */
  cancel() {
    const state = this.session.snapshot();
    const grid = this.host.grid();
    if (!state.active) return;
    this.session.end();
    this.pointStart = null;
    if (grid && grid.currentSheet() !== state.sheet) void this.host.showSheet(state.sheet);
    grid?.focus();
  }

  /** A key in the cell editor or the formula bar. */
  readonly key = (event: KeyboardEvent<HTMLElement>) => {
    if (this.host.completionKey(event)) return;
    const state = this.session.snapshot();
    if (!state.active) return;
    const ctrl = event.ctrlKey || event.metaKey;
    switch (event.key) {
      case 'Enter':
        event.preventDefault();
        if (event.altKey) {
          const { text, caret } = state;
          this.session.replace(text.slice(0, caret) + '\n' + text.slice(caret), caret + 1);
        } else if (ctrl && event.shiftKey) {
          this.commit(0, 0, 'array');
        } else if (ctrl) {
          this.commit(0, 0, 'selection');
        } else {
          this.commit(event.shiftKey ? -1 : 1, 0);
        }
        return;
      case 'Tab':
        event.preventDefault();
        this.commit(0, event.shiftKey ? -1 : 1);
        return;
      case 'Escape':
        event.preventDefault();
        this.cancel();
        return;
      case 'F2':
        event.preventDefault();
        this.session.setMode(state.mode === 'enter' ? 'edit' : 'enter');
        return;
      case 'F4': {
        event.preventDefault();
        const next = cycleAbsolute(state.text, state.caret);
        this.session.replace(next.text, next.caret);
        return;
      }
      case 'ArrowUp':
      case 'ArrowDown':
      case 'ArrowLeft':
      case 'ArrowRight': {
        if (state.mode === 'edit' || state.place === 'bar') return;
        event.preventDefault();
        const rows = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
        const sideways = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
        // Arrows go the way they point, towards column A on a right-to-left sheet.
        const cols = this.host.grid()?.columnsFor(event.key) ?? sideways;
        if (this.session.canPoint()) this.pointBy(rows, cols, event.shiftKey);
        else this.commit(rows, cols);
        return;
      }
      default:
        return;
    }
  };

  private prefix(state: { sheet: number }) {
    const grid = this.host.grid();
    const showing = grid ? grid.currentSheet() : state.sheet;
    return showing === state.sheet ? null : this.host.sheetName(showing);
  }

  /** An arrow key while pointing moves the reference, and Shift stretches it. */
  private pointBy(rows: number, cols: number, extend: boolean) {
    const state = this.session.snapshot();
    const grid = this.host.grid();
    const from = state.picked ?? {
      row: state.row,
      col: state.col,
      endRow: state.row,
      endCol: state.col,
    };
    const clampRow = (row: number) => Math.max(0, Math.min(MAX_ROW - 1, row));
    const clampCol = (col: number) => Math.max(0, Math.min(MAX_COL - 1, col));
    const endRow = clampRow(from.endRow + rows);
    const endCol = clampCol(from.endCol + cols);
    const row = extend ? from.row : endRow;
    const col = extend ? from.col : endCol;
    this.session.pick(row, col, endRow, endCol, this.prefix(state));
    grid?.revealCell(endRow, endCol);
  }

  /** A click on the sheet while editing. */
  readonly editClick = (row: number, col: number) => {
    const state = this.session.snapshot();
    if (!state.active) return false;
    if (!this.session.canPoint()) {
      this.commit();
      return false;
    }
    this.pointStart = { row, col };
    this.session.pick(row, col, row, col, this.prefix(state));
    return true;
  };

  readonly editDrag = (row: number, col: number) => {
    const state = this.session.snapshot();
    if (!state.active || !this.pointStart) return;
    this.session.pick(this.pointStart.row, this.pointStart.col, row, col, this.prefix(state));
  };

  /** Whether choosing a sheet now is part of typing a formula, rather than leaving the cell. */
  beforeSheetChange() {
    const state = this.session.snapshot();
    if (!state.active || this.session.canPoint()) return;
    this.commit();
  }

  /** Ctrl+arrow. */
  readonly jump = (rows: number, cols: number, extend: boolean) => {
    const grid = this.host.grid();
    const engine = this.host.engine();
    if (!grid || !engine) return;
    const from = extend ? grid.movingCorner() : grid.selection();
    const sheet = grid.currentSheet();
    void this.host
      .settled()
      .then(() => engine.jumpTarget(sheet, from.row + 1, from.col + 1, rows, cols))
      .then(([row, col]) => {
        if (grid.currentSheet() !== sheet) return;
        if (extend) grid.extendSelectionTo(row - 1, col - 1);
        else grid.selectArea({ top: row - 1, left: col - 1, bottom: row - 1, right: col - 1 });
      })
      .catch((error) => this.refused(error));
  };

  /** A fill handle dragged, or double-clicked with no `to`. */
  readonly fill = (from: CellRange, to: CellRange | null) => {
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit()) return;
    const sheet = grid.currentSheet();
    if (to) {
      this.fillTo(sheet, from, to);
      return;
    }
    void this.fillDownTo(sheet, from).catch((error) => this.refused(error));
  };

  private fillTo(sheet: number, from: CellRange, to: CellRange) {
    if (to.bottom < from.bottom || to.right < from.right) {
      // Dragged back into itself: what it leaves behind goes.
      const area: CellRange =
        to.bottom < from.bottom ? { ...from, top: to.bottom + 1 } : { ...from, left: to.right + 1 };
      this.send({ kind: 'clear', sheet, areas: [toArea(area)] });
      return;
    }
    this.send({ kind: 'fill', sheet, from: toArea(from), to: toArea(to) });
  }

  /** How far a double-click fills: as far as the column beside it has cells. */
  private async fillDownTo(sheet: number, from: CellRange) {
    const engine = this.host.engine();
    const grid = this.host.grid();
    if (!engine || !grid) return;
    await this.host.settled();
    const has = async (row: number, col: number) =>
      col >= 0 && col < MAX_COL && (await engine.cellText(sheet, row + 1, col + 1)).display !== '';
    for (const col of [from.left - 1, from.right + 1]) {
      if (!(await has(from.bottom, col)) || !(await has(from.bottom + 1, col))) continue;
      const [end] = await engine.jumpTarget(sheet, from.bottom + 1, col + 1, 1, 0);
      const to = { ...from, bottom: end - 1 };
      this.send({ kind: 'fill', sheet, from: toArea(from), to: toArea(to) });
      grid.selectArea(to, grid.selection().row, grid.selection().col);
      return;
    }
  }

  /** Ctrl+C and Ctrl+X. Clipboard writes need the key press still current, so this can't wait. */
  copy(cut: boolean) {
    const grid = this.host.grid();
    const engine = this.host.engine();
    if (!grid || !engine) return;
    const { sheet, range, areas } = grid.selection();
    if (areas.length > 1) {
      this.host.notice('That can’t be copied: select one area.');
      return;
    }
    const clip: Clip = { sheet, range, cut, text: null };
    this.clip = clip;
    grid.setCopied({ sheet, range });
    const texts = engine.copyText(sheet, toArea(range));
    void texts.then(({ text }) => {
      clip.text = clipboardKey(text);
    });
    const blob = (type: string, pick: (value: { text: string; html: string }) => string) =>
      texts.then((value) => new Blob([pick(value)], { type }));
    const fallback = () =>
      texts
        .then(({ text }) => navigator.clipboard.writeText(text))
        .catch((error) => {
          console.warn('The clipboard refused:', error);
        });
    if (typeof ClipboardItem === 'function' && navigator.clipboard?.write) {
      navigator.clipboard
        .write([
          new ClipboardItem({
            'text/plain': blob('text/plain', (value) => value.text),
            'text/html': blob('text/html', (value) => value.html),
          }),
        ])
        .catch(fallback);
    } else {
      void fallback();
    }
  }

  /** What the clipboard holds, from a paste event or from asking for it. */
  paste(text: string, html: string | undefined) {
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit()) return;
    const { sheet, row, col, range } = grid.selection();
    const clip = this.clip;
    const ours = clip && (clip.text === null || clip.text === clipboardKey(text));
    const values = this.pasteValues;
    this.pasteValues = false;
    if (clip && ours && clip.cut) {
      this.send({
        kind: 'move',
        fromSheet: clip.sheet,
        from: toArea(clip.range),
        sheet,
        row: row + 1,
        col: col + 1,
      });
      this.letGoOfClip();
    } else if (clip && ours) {
      this.send({
        kind: 'paste',
        fromSheet: clip.sheet,
        from: toArea(clip.range),
        sheet,
        to: toArea(range),
        ...(values ? { mode: 'values' as const } : {}),
      });
    } else {
      this.letGoOfClip();
      this.send({
        kind: 'pasteExternal',
        sheet,
        row: row + 1,
        col: col + 1,
        text,
        ...(html && !values ? { html } : {}),
      });
    }
  }

  /** Paste of one kind, from the ribbon. Only cells copied here can do it in full. */
  pasteMode(mode: 'values' | 'formulas' | 'formats') {
    const grid = this.host.grid();
    const clip = this.clip;
    if (!grid || !this.host.canEdit()) return;
    if (clip) {
      const { sheet, range } = grid.selection();
      this.send({
        kind: 'paste',
        fromSheet: clip.sheet,
        from: toArea(clip.range),
        sheet,
        to: toArea(range),
        mode,
      });
      return;
    }
    if (mode === 'formats') {
      this.host.notice('Copy cells in this workbook first, to paste their formatting.');
      return;
    }
    this.pasteValues = true;
    void this.pasteFromMenu();
  }

  /** Paste from a menu, which has to ask for the clipboard. */
  async pasteFromMenu() {
    try {
      const items = await navigator.clipboard.read();
      let text = '';
      let html: string | undefined;
      for (const item of items) {
        if (item.types.includes('text/plain')) {
          text = await (await item.getType('text/plain')).text();
        }
        if (item.types.includes('text/html')) {
          html = await (await item.getType('text/html')).text();
        }
      }
      this.paste(text, html);
    } catch {
      this.host.notice('The browser kept the clipboard private. Press Ctrl+V to paste.');
    }
  }

  private letGoOfClip() {
    if (!this.clip) return;
    this.clip = null;
    this.host.grid()?.setCopied(null);
  }

  /** Runs a row or column change over the selection. */
  axis(axis: 'row' | 'col', remove: boolean) {
    const grid = this.host.grid();
    if (!grid) return;
    const { sheet, range } = grid.selection();
    this.send({
      kind: 'axis',
      sheet,
      axis,
      at: (axis === 'row' ? range.top : range.left) + 1,
      count: axis === 'row' ? range.bottom - range.top + 1 : range.right - range.left + 1,
      delete: remove,
    });
  }

  /** Ctrl+D and Ctrl+R copy the first line, or the line before a single one. */
  private fillKey(down: boolean) {
    const grid = this.host.grid();
    if (!grid) return;
    const { sheet, range } = grid.selection();
    const single = down ? range.top === range.bottom : range.left === range.right;
    let from: CellRange;
    let to = range;
    if (down) {
      if (single && range.top === 0) return;
      from = single
        ? { ...range, top: range.top - 1, bottom: range.top - 1 }
        : { ...range, bottom: range.top };
      if (single) to = { ...range, top: range.top - 1 };
    } else {
      if (single && range.left === 0) return;
      from = single
        ? { ...range, left: range.left - 1, right: range.left - 1 }
        : { ...range, right: range.left };
      if (single) to = { ...range, left: range.left - 1 };
    }
    this.send({ kind: 'fill', sheet, from: toArea(from), to: toArea(to), copy: true });
  }

  /** Alt+=: a sum over the numbers above, or else to the left. */
  private async autoSum() {
    const grid = this.host.grid();
    const engine = this.host.engine();
    if (!grid || !engine || !this.host.canEdit()) return;
    const { sheet, row, col } = grid.selection();
    await this.host.settled();
    const filled = async (r: number, c: number) =>
      r >= 0 && c >= 0 && (await engine.cellText(sheet, r + 1, c + 1)).display !== '';
    let body = '';
    const name = (r: number, c: number, r2: number, c2: number) =>
      grid.addressOf({ top: r, left: c, bottom: r2, right: c2 });
    if (await filled(row - 1, col)) {
      const top = (await filled(row - 2, col))
        ? (await engine.jumpTarget(sheet, row, col + 1, -1, 0))[0] - 1
        : row - 1;
      body = name(top, col, row - 1, col);
    } else if (await filled(row, col - 1)) {
      const left = (await filled(row, col - 2))
        ? (await engine.jumpTarget(sheet, row + 1, col, 0, -1))[1] - 1
        : col - 1;
      body = name(row, left, row, col - 1);
    }
    const text = `=SUM(${body})`;
    if (!this.begin(sheet, row, col, text, 'edit', 'cell')) return;
    if (!body) this.session.replace(text, text.length - 1);
    this.host.focusEditor();
  }

  private startWith(text: string) {
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit()) return;
    const { sheet, row, col } = grid.selection();
    if (!this.begin(sheet, row, col, text, 'edit', 'cell')) return;
    this.host.focusEditor();
  }

  /** The shortcut map's actions, away from the editor. */
  readonly action = (action: GridAction) => {
    const grid = this.host.grid();
    if (!grid) return;
    const { sheet, areas } = grid.selection();
    switch (action) {
      case 'undo':
        this.send({ kind: 'undo' });
        break;
      case 'redo':
        this.send({ kind: 'redo' });
        break;
      case 'copy':
        this.copy(false);
        break;
      case 'cut':
        this.copy(true);
        break;
      case 'paste':
        this.pasteValues = false;
        break;
      case 'pasteValues':
        this.pasteValues = true;
        break;
      case 'clear':
        if (this.host.canEdit()) this.send({ kind: 'clear', sheet, areas: areas.map(toArea) });
        break;
      case 'fillDown':
        this.fillKey(true);
        break;
      case 'fillRight':
        this.fillKey(false);
        break;
      case 'cancel':
        this.letGoOfClip();
        this.host.notice(null);
        this.host.formatAction(action);
        break;
      case 'insertSheet':
        this.host.insertSheet();
        break;
      case 'previousSheet':
      case 'nextSheet': {
        const next = this.host.nextSheet(action === 'nextSheet' ? 1 : -1);
        if (next !== null) void this.host.showSheet(next);
        break;
      }
      case 'today':
        this.startWith(new Date().toLocaleDateString(latinLocale()));
        break;
      case 'now':
        this.startWith(
          new Date().toLocaleTimeString(latinLocale(), { hour: '2-digit', minute: '2-digit' }),
        );
        break;
      case 'autoSum':
        void this.autoSum().catch((error) => this.refused(error));
        break;
      case 'selectColumn': {
        const { range, row, col } = grid.selection();
        grid.selectArea(
          { top: 0, left: range.left, bottom: MAX_ROW - 1, right: range.right },
          row,
          col,
        );
        break;
      }
      case 'selectRow': {
        const { range, row, col } = grid.selection();
        grid.selectArea(
          { top: range.top, left: 0, bottom: range.bottom, right: MAX_COL - 1 },
          row,
          col,
        );
        break;
      }
      case 'selectAll': {
        const { row, col } = grid.selection();
        grid.selectArea({ top: 0, left: 0, bottom: MAX_ROW - 1, right: MAX_COL - 1 }, row, col);
        break;
      }
      case 'calculate':
        this.host.calculate('now');
        break;
      case 'calculateSheet':
        this.host.calculate('sheet');
        break;
      case 'calculateAll':
        this.host.calculate('all');
        break;
      case 'edit':
        void this.edit().catch((error) => this.refused(error));
        break;
      default:
        this.host.formatAction(action);
    }
  };
}
