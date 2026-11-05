// What the ribbon, the shortcuts and the context menus do to a selection. It all leaves here as engine
// commands: nothing here sees a cell's value.
import type {
  BorderChange,
  CellArea,
  CellFormat,
  Command,
  FormatChange,
  FormatColor,
  StoredColor,
} from '@cgs/engine-client';
import type { CgBorderApplyDetails, CgColorPickerValue } from '@cashgear/ui';
import type { CellRange } from './grid/controller';
import { MAX_COLS, MAX_ROWS } from './grid/geometry';
import { toArea, type EditingHost, type SheetEditing } from './sheet-editing';

/** A colour picker's choice as a format wants it. Theme keys are the theme's own numbers. */
export function pickedColor(value: CgColorPickerValue): FormatColor {
  switch (value.kind) {
    case 'automatic':
    case 'none':
      return { kind: 'auto' };
    case 'theme':
      return { kind: 'theme', theme: Number(value.key), tint: value.tint };
    case 'rgb':
      return { kind: 'rgb', rgb: value.color.replace('#', '').toUpperCase() };
  }
}

/** A cell's colour as a colour picker shows it. */
export function shownColor(color: StoredColor): CgColorPickerValue {
  switch (color.kind) {
    case 'auto':
      return { kind: 'automatic' };
    case 'theme':
      return { kind: 'theme', key: String(color.theme), tint: color.tint, color: color.color };
    case 'rgb':
      return { kind: 'rgb', color: color.color };
  }
}

const LINE_NAMES: Record<string, string> = {
  thin: 'thin',
  medium: 'medium',
  thick: 'thick',
  dashed: 'dashed',
  dotted: 'dotted',
  double: 'double',
};

/** What a border picker's choice does to a selection. */
export function bordersFromPicker(details: CgBorderApplyDetails, lineStyle: string): BorderChange {
  const color = pickedColor(details.color);
  const none = { style: 'none' };
  if (details.clear) {
    return {
      top: none,
      bottom: none,
      left: none,
      right: none,
      insideHorizontal: none,
      insideVertical: none,
    };
  }
  const change: BorderChange = {};
  for (const { edge, lineStyle: fixed } of details.edges) {
    const line = { style: LINE_NAMES[fixed ?? lineStyle] ?? 'thin', color };
    switch (edge) {
      case 'top':
        change.top = line;
        break;
      case 'bottom':
        change.bottom = line;
        break;
      case 'left':
        change.left = line;
        break;
      case 'right':
        change.right = line;
        break;
      case 'inside-horizontal':
        change.insideHorizontal = line;
        break;
      case 'inside-vertical':
        change.insideVertical = line;
        break;
    }
  }
  return change;
}

export type HideAxis = 'row' | 'col';

/** The format painter: what it copies from, and whether it stays on. */
interface Painter {
  sheet: number;
  range: CellRange;
  sticky: boolean;
}

export class SheetFormat {
  private painter: Painter | null = null;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly host: EditingHost,
    private readonly editing: SheetEditing,
    private readonly current: () => CellFormat | null,
  ) {}

  /** Tells whoever shows the painter's state that it changed. */
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly painterOn = () => this.painter !== null;

  private send(command: Command) {
    this.host.notice(null);
    this.host.run(command).catch((error) => {
      this.host.notice(error instanceof Error ? error.message : String(error));
    });
  }

  /** What's typed so far goes in first, so a format applies to it too. */
  private target() {
    if (this.editing.session.snapshot().active) this.editing.commit();
    const grid = this.host.grid();
    if (!grid || !this.host.canEdit()) return null;
    return grid.selection();
  }

  format(change: FormatChange) {
    const selection = this.target();
    if (!selection) return;
    this.send({
      kind: 'format',
      sheet: selection.sheet,
      areas: selection.areas.map(toArea),
      change,
      active: [selection.row + 1, selection.col + 1],
    });
  }

  toggleBold = () => this.format({ bold: !this.current()?.bold });
  toggleItalic = () => this.format({ italic: !this.current()?.italic });
  toggleStrike = () => this.format({ strike: !this.current()?.strike });
  toggleUnderline = (kind: 'single' | 'double' = 'single') => {
    this.format({ underline: this.current()?.underline === kind ? 'none' : kind });
  };
  toggleWrap = () => this.format({ wrap: !this.current()?.wrap });

  /** Clicking the alignment that's already on goes back to General, as in Excel. */
  toggleHorizontal = (value: string) => {
    this.format({ horizontal: this.current()?.horizontal === value ? 'general' : value });
  };

  /** Lines around the selection, by a shortcut. */
  outline(on: boolean) {
    const line = on ? { style: 'thin' } : { style: 'none' };
    this.format({
      border: on
        ? { top: line, bottom: line, left: line, right: line }
        : {
            top: line,
            bottom: line,
            left: line,
            right: line,
            insideHorizontal: line,
            insideVertical: line,
          },
    });
  }

  /** The rows or columns a selection covers, as stretches. */
  private spans(axis: HideAxis) {
    const selection = this.target();
    if (!selection) return null;
    const list = selection.areas.map((area) =>
      axis === 'row' ? [area.top + 1, area.bottom + 1] : [area.left + 1, area.right + 1],
    ) as [number, number][];
    return { sheet: selection.sheet, list };
  }

  setSize(axis: HideAxis, px: number) {
    const spans = this.spans(axis);
    if (!spans) return;
    for (const [first, last] of spans.list) {
      this.send({ kind: 'lines', sheet: spans.sheet, axis, first, last, size: px });
    }
  }

  /** Hides or shows what the selection covers. */
  setHidden(axis: HideAxis, hidden: boolean) {
    const spans = this.spans(axis);
    if (!spans) return;
    for (const [first, last] of spans.list) {
      this.send({ kind: 'lines', sheet: spans.sheet, axis, first, last, hidden });
    }
  }

  autoFit(axis: HideAxis, lines?: [number, number]) {
    const spans = this.spans(axis);
    if (!spans) return;
    const list = lines ? [lines] : spans.list;
    for (const [first, last] of list) {
      if (axis === 'row') {
        this.send({ kind: 'autoFitRows', sheet: spans.sheet, first, last });
      } else {
        this.host.fitColumns(spans.sheet, first, last);
      }
    }
  }

  /** Merges what's selected. A merge that throws something away asks first, through `confirm`. */
  async merge(kind: 'center' | 'across' | 'cells', confirm: () => Promise<boolean>) {
    const selection = this.target();
    const engine = this.host.engine();
    if (!selection || !engine) return;
    await this.host.settled();
    const areas: CellArea[] = selection.areas.map(toArea);
    const across = kind === 'across';
    const discards = await engine.mergeDiscards(selection.sheet, areas, across);
    if (discards && !(await confirm())) return;
    this.send({ kind: 'merge', sheet: selection.sheet, areas, center: kind === 'center', across });
  }

  unmerge() {
    const selection = this.target();
    if (!selection) return;
    this.send({ kind: 'unmerge', sheet: selection.sheet, areas: selection.areas.map(toArea) });
  }

  /** Freezes above and left of the active cell, or unfreezes. */
  freeze(kind: 'panes' | 'row' | 'column' | 'off') {
    const grid = this.host.grid();
    const selection = this.target();
    if (!grid || !selection) return;
    const { sheet } = selection;
    if (kind === 'off') {
      this.send({ kind: 'freeze', sheet, rows: 0, cols: 0 });
      return;
    }
    const corner = grid.visibleCorner();
    if (kind === 'row') {
      this.send({ kind: 'freeze', sheet, rows: 1, cols: 0, top: corner.row + 1 });
    } else if (kind === 'column') {
      this.send({ kind: 'freeze', sheet, rows: 0, cols: 1, left: corner.col + 1 });
    } else {
      const rows = Math.max(0, selection.row - corner.row);
      const cols = Math.max(0, selection.col - corner.col);
      this.send({
        kind: 'freeze',
        sheet,
        rows,
        cols,
        top: corner.row + 1,
        left: corner.col + 1,
      });
    }
  }

  /** Inserts or deletes cells that shift others along. */
  shift(down: boolean, remove: boolean) {
    const selection = this.target();
    if (!selection) return;
    this.send({
      kind: 'shiftCells',
      sheet: selection.sheet,
      area: toArea(selection.range),
      down,
      delete: remove,
    });
  }

  /** Whether what's selected is whole rows or columns, which inserting and deleting go by. */
  wholeLines(): 'row' | 'col' | null {
    const selection = this.host.grid()?.selection();
    if (!selection) return null;
    const { range } = selection;
    if (range.left === 0 && range.right === MAX_COLS - 1) return 'row';
    if (range.top === 0 && range.bottom === MAX_ROWS - 1) return 'col';
    return null;
  }

  /** Picks up what's selected, to paint it on what's selected next. */
  startPainter(sticky: boolean) {
    const selection = this.target();
    const grid = this.host.grid();
    if (!selection || !grid) return;
    this.painter = { sheet: selection.sheet, range: selection.range, sticky };
    grid.setCopied({ sheet: selection.sheet, range: selection.range });
    grid.setPainter(true);
    this.changed();
  }

  stopPainter() {
    if (!this.painter) return;
    this.painter = null;
    const grid = this.host.grid();
    grid?.setCopied(null);
    grid?.setPainter(false);
    this.changed();
  }

  /** A drag on the sheet finished. With the painter on, that's where it paints. */
  readonly selectionDone = () => {
    const painter = this.painter;
    const grid = this.host.grid();
    if (!painter || !grid) return;
    const selection = grid.selection();
    this.send({
      kind: 'paste',
      fromSheet: painter.sheet,
      from: toArea(painter.range),
      sheet: selection.sheet,
      to: toArea(selection.range),
      mode: 'formats',
    });
    if (!painter.sticky) this.stopPainter();
  };

  private changed() {
    for (const listener of this.listeners) listener();
  }
}
