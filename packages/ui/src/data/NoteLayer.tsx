// Notes as Excel 2016 shows them: a yellow box at its anchor beside the cell, with a line to the
// cell's corner. Boxes that Show/Hide Note or Show All Notes keep showing, and the one being
// written in, which takes the keyboard until a click elsewhere or Esc ends it.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { GridController } from '../grid/controller';

export interface ShownNote {
  /** 1-based. */
  row: number;
  col: number;
  author: string;
  text: string;
  /** Show/Hide Note. */
  visible: boolean;
  /** Columns, rows and pixels at 100%, counted from 0: see `Note::anchor` in cgs-model. */
  anchor: number[];
}

export interface NoteEdit {
  row: number;
  col: number;
  text: string;
  anchor: number[];
}

interface Placed {
  key: string;
  left: number;
  top: number;
  width: number;
  height: number;
  /** The cell's corner the line starts from. */
  fromX: number;
  fromY: number;
}

/** Where a note's box shows on the page, if its cells do. */
function place(grid: GridController, row: number, col: number, anchor: number[]): Placed | null {
  const [c1, x1, r1, y1, c2, x2, r2, y2] = anchor as [
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  const start = grid.cellBox(r1, c1);
  const end = grid.cellBox(r2, c2);
  const cell = grid.cellBox(row - 1, col - 1);
  if (!start || !end || !cell) return null;
  const zoom = grid.state().zoom / 100;
  // Offsets run from each cell's leading edge: its right on a right-to-left sheet.
  const rtl = grid.rightToLeft;
  const xOf = (box: { left: number; width: number }, dx: number) =>
    rtl ? box.left + box.width - dx * zoom : box.left + dx * zoom;
  const left = Math.min(xOf(start, x1), xOf(end, x2));
  const right = Math.max(xOf(start, x1), xOf(end, x2));
  const top = start.top + y1 * zoom;
  const bottom = end.top + y2 * zoom;
  return {
    key: `${row}:${col}`,
    left,
    top,
    width: Math.max(8, right - left),
    height: Math.max(8, bottom - top),
    fromX: rtl ? cell.left : cell.left + cell.width,
    fromY: cell.top,
  };
}

/** A note's text with its author's name bold, as Excel's box shows the line it starts with. */
function NoteText({ author, text }: { author: string; text: string }) {
  const head = author + ':';
  if (author && text.startsWith(head)) {
    return (
      <>
        <strong>{head}</strong>
        {text.slice(head.length)}
      </>
    );
  }
  return <>{text}</>;
}

export interface NoteLayerProps {
  grid: () => GridController | null;
  sheet: number;
  notes: ShownNote[];
  editing: NoteEdit | null;
  onEditText: (text: string) => void;
  onDone: () => void;
  /** A click in a shown note's box starts editing it, as in Excel. */
  onOpen: (note: ShownNote) => void;
}

export function NoteLayer(props: NoteLayerProps) {
  const { notes, editing } = props;
  const [placed, setPlaced] = useState<Map<string, Placed>>(new Map());
  const latest = useRef(props);
  latest.current = props;
  const text = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    const grid = props.grid();
    if (!grid) return;
    const update = () => {
      const { notes: list, editing: edit } = latest.current;
      const next = new Map<string, Placed>();
      for (const note of list) {
        const at = place(grid, note.row, note.col, note.anchor);
        if (at) next.set(at.key, at);
      }
      if (edit) {
        const at = place(grid, edit.row, edit.col, edit.anchor);
        if (at) next.set('edit', at);
      }
      setPlaced((old) => (JSON.stringify([...old]) === JSON.stringify([...next]) ? old : next));
    };
    update();
    return grid.onFrame(update);
    // The grid calls back after each frame; the lists are read from `latest`.
  }, [props.grid, props.sheet, notes, editing]);

  // The box being written in has the keyboard, with the caret at the end, as in Excel.
  const editKey = editing ? `${editing.row}:${editing.col}` : '';
  useLayoutEffect(() => {
    const area = text.current;
    if (!area || !editKey) return;
    area.focus();
    area.setSelectionRange(area.value.length, area.value.length);
  }, [editKey, placed.has('edit')]);

  const boxStyle = (at: Placed) =>
    ({
      position: 'fixed',
      left: at.left,
      top: at.top,
      width: at.width,
      height: at.height,
      boxSizing: 'border-box',
      padding: '2px 4px',
      background: '#ffffe1',
      color: '#000',
      border: '1px solid #767676',
      boxShadow: '1px 1px 3px rgba(0,0,0,0.35)',
      font: '12px Tahoma, "Segoe UI", sans-serif',
      whiteSpace: 'pre-wrap',
      overflow: 'hidden',
      zIndex: 35,
    }) as const;
  const line = (at: Placed, key: string) => {
    const toX = at.fromX < at.left ? at.left : at.left + at.width;
    const minX = Math.min(at.fromX, toX);
    const minY = Math.min(at.fromY, at.top);
    return (
      <svg
        key={'line-' + key}
        style={{
          position: 'fixed',
          left: minX,
          top: minY,
          width: Math.abs(toX - at.fromX) + 1,
          height: Math.abs(at.top - at.fromY) + 1,
          overflow: 'visible',
          pointerEvents: 'none',
          zIndex: 34,
        }}
      >
        <line
          x1={at.fromX - minX}
          y1={at.fromY - minY}
          x2={toX - minX}
          y2={at.top - minY}
          stroke="#000"
          strokeWidth={1}
        />
      </svg>
    );
  };

  const editingAt = placed.get('edit');
  return (
    <>
      {notes.map((note) => {
        const key = `${note.row}:${note.col}`;
        const at = placed.get(key);
        if (!at || editKey === key) return null;
        return [
          line(at, key),
          <div
            key={key}
            data-testid={`note-box-${note.row}-${note.col}`}
            style={boxStyle(at)}
            onMouseDown={(event) => {
              event.preventDefault();
              props.onOpen(note);
            }}
          >
            <NoteText author={note.author} text={note.text} />
          </div>,
        ];
      })}
      {editing &&
        editingAt && [
          line(editingAt, 'edit'),
          <textarea
            key="edit"
            ref={text}
            data-testid="note-editor"
            aria-label="Note"
            value={editing.text}
            spellCheck={false}
            style={{ ...boxStyle(editingAt), resize: 'none', outline: '2px solid #217346' }}
            onChange={(event) => props.onEditText(event.target.value)}
            onBlur={props.onDone}
            onKeyDown={(event) => {
              // Esc ends the note keeping what it says, as in Excel.
              if (event.key === 'Escape') {
                event.preventDefault();
                props.onDone();
              }
              event.stopPropagation();
            }}
          />,
        ]}
    </>
  );
}
