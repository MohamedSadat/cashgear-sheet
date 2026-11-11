// The data tools as the shell runs them: what the ribbon shows about the selection, the dialogs
// and menus, the grid's filter and list buttons, notes and links under the pointer, and Excel's
// message when validation refuses what was typed. Everything is asked of the engine, and every
// change is a command through the shell's queue.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { linkToOpen } from './links';
import { CgButton, CgWindow } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  CellFormat,
  CellStyleInfo,
  Command,
  EngineClient,
  FormatChange,
  StyleIncludes,
  FilterInfo,
  TableInfo,
  TableStyleInfo,
} from '@cgs/engine-client';
import type { CellButton, ClientBox, GridController, GridSelection } from '../grid/controller';
import { quoteSheet } from '../formula-text';
import { toArea } from '../sheet-editing';
import type { CfQuickKind } from './CfDialogs';
import { CfManager, CfQuickRule } from './CfDialogs';
import { FilterMenu } from './FilterMenu';
import { CreateNamesDialog, DefineNameDialog, NameManager } from './NameDialogs';
import type { DataActions, DataState } from './ribbon-data';
import {
  absoluteRange,
  CreateTableDialog,
  HyperlinkDialog,
  NoteDialog,
  RemoveDuplicatesDialog,
} from './SmallDialogs';
import { NoteLayer, type NoteEdit, type ShownNote } from './NoteLayer';
import { SortDialog } from './SortDialog';
import { ValidationDialog } from './ValidationDialog';
import { ALL_INCLUDED, StyleDialog } from './StyleDialog';

/** The cell editor as far as a refusal needs it. */
export interface RefusedEditor {
  snapshot(): { active: boolean; sheet: number; row: number; col: number; text: string };
  cancel(): void;
}

export interface DataToolsHost {
  engine: () => EngineClient | null;
  grid: () => GridController | null;
  /** The sheet showing. */
  sheet: () => number;
  selection: () => GridSelection | null;
  hasBook: boolean;
  /** The workbook's sheet names, by index. */
  sheetNames: string[];
  run: (command: Command) => Promise<Applied>;
  notice: (message: string | null) => void;
  /** Shows a sheet and selects an area on it. 1-based. */
  goTo: (sheet: number, area: CellArea) => Promise<void>;
  editor: () => RefusedEditor;
  /** Other buttons the grid shows beside the data tools' own, such as pivot tables'. */
  extraButtons?: () => CellButton[];
  /** Who a new note says wrote it. */
  userName: () => string;
  /** The active cell's format, which New Cell Style takes by example. */
  cellFormat?: CellFormat | null;
  /** Format Cells on a style's look, for Modify's Format…. */
  formatStyle?: (format: CellFormat, done: (change: FormatChange) => void) => void;
  /** What hovering over a cell with no note or link shows, such as an ERP field's details. */
  hoverDetails?: (
    sheet: number,
    row: number,
    col: number,
  ) => Promise<{ title?: string; text: string } | null>;
}

/** Where Excel 2016 puts a new note's box: see `Note::default_anchor` in cgs-model. 1-based. */
export function defaultNoteAnchor(row: number, col: number): number[] {
  const r = row - 1;
  const c = col - 1;
  return [c + 1, 15, Math.max(0, r - 1), 10, c + 3, 31, r + 3, 9];
}

type DataDialog =
  | { kind: 'sort'; area: CellArea; headers: boolean }
  | { kind: 'validation' }
  | { kind: 'names' }
  | { kind: 'defineName' }
  | { kind: 'createNames'; area: CellArea }
  | { kind: 'cfQuick'; quick: CfQuickKind }
  | { kind: 'cfManage' }
  | { kind: 'duplicates'; area: CellArea; headers: boolean }
  | { kind: 'hyperlink'; area: CellArea }
  | { kind: 'createTable'; area: CellArea; headers: boolean; style?: string; resize?: string }
  | { kind: 'note'; row: number; col: number }
  | { kind: 'message'; title: string; text: string }
  | { kind: 'convert'; table: string }
  | { kind: 'refused'; style: string; title: string; text: string }
  | {
      kind: 'cellStyle';
      mode: 'new' | 'modify';
      name: string;
      includes: StyleIncludes;
      format: CellFormat | null;
      /** What Format… changed, sent with OK. */
      change?: FormatChange;
    };

/** Excel's message when validation refuses what was typed, from the engine's error. */
export function readRefusal(message: string) {
  const parts = message.split('\u0001');
  if (parts[0] !== 'VALIDATION' || parts.length < 4) return null;
  return { style: parts[1] || 'stop', title: parts[2] || 'Microsoft Excel', text: parts[3] ?? '' };
}

const DEFAULT_REFUSAL =
  'This value doesn’t match the data validation restrictions defined for this cell.';

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

interface ListMenu {
  sheet: number;
  row: number;
  col: number;
  items: string[];
  /** An ERP lookup's labels beside its codes. */
  labels?: string[];
  anchor: ClientBox;
}

interface Tip {
  x: number;
  y: number;
  /** `x` is the tip's right edge: it opens to the left, as on a right-to-left sheet. */
  leftward?: boolean;
  title?: string;
  text: string;
}

export function useDataTools(host: DataToolsHost) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [dialog, setDialog] = useState<DataDialog | null>(null);
  const [table, setTable] = useState<TableInfo | null>(null);
  const [tableStyles, setTableStyles] = useState<TableStyleInfo[]>([]);
  const [cellStyles, setCellStyles] = useState<{ group: string; styles: CellStyleInfo[] }[]>([]);
  const [filters, setFilters] = useState<FilterInfo[]>([]);
  const [circled, setCircled] = useState(false);
  const [hasNote, setHasNote] = useState(false);
  const [noteShown, setNoteShown] = useState(false);
  const [allNotesShown, setAllNotesShown] = useState(false);
  const [sheetNotes, setSheetNotes] = useState<ShownNote[]>([]);
  const [noteEdit, setNoteEdit] = useState<
    (NoteEdit & { sheet: number; author: string; was: string | null }) | null
  >(null);
  const [hasLink, setHasLink] = useState(false);
  const [filterMenu, setFilterMenu] = useState<{
    table: string | null;
    area: CellArea;
    col: number;
    anchor: ClientBox;
  } | null>(null);
  const [listMenu, setListMenu] = useState<ListMenu | null>(null);
  const [hoverTip, setHoverTip] = useState<Tip | null>(null);
  const [inputTip, setInputTip] = useState<Tip | null>(null);
  const token = useRef(0);
  const listButton = useRef<CellButton | null>(null);
  const filterButtons = useRef<CellButton[]>([]);
  // A click on the button of the menu that's open closes it, rather than opening it again.
  const menuClosed = useRef({ col: -1, at: 0 });

  const allNotesRef = useRef(false);
  allNotesRef.current = allNotesShown;
  const noteEditRef = useRef(noteEdit);
  noteEditRef.current = noteEdit;
  const run = useCallback(async (command: Command) => hostRef.current.run(command), []);
  const send = useCallback((command: Command) => {
    hostRef.current.notice(null);
    hostRef.current.run(command).catch((error) => hostRef.current.notice(errorText(error)));
  }, []);

  const placeButtons = useCallback(() => {
    const list = listButton.current ? [listButton.current] : [];
    const extra = hostRef.current.extraButtons?.() ?? [];
    hostRef.current.grid()?.setButtons([...filterButtons.current, ...list, ...extra]);
  }, []);

  // The galleries, once a workbook is open: its theme decides the table styles' colours.
  useEffect(() => {
    const engine = hostRef.current.engine();
    if (!host.hasBook || !engine) return;
    engine
      .query({ kind: 'tableStyles' })
      .then(setTableStyles)
      .catch(() => setTableStyles([]));
    loadCellStyles();
  }, [host.hasBook]);

  const loadCellStyles = useCallback(() => {
    hostRef.current
      .engine()
      ?.query({ kind: 'cellStyles' })
      .then(setCellStyles)
      .catch(() => setCellStyles([]));
  }, []);

  /** What the ribbon and the grid show about the selection and the sheet. */
  const refresh = useCallback(() => {
    const { engine: getEngine, selection: getSelection, sheet: getSheet } = hostRef.current;
    const engine = getEngine();
    const selection = getSelection();
    if (!engine || !selection) return;
    const mine = ++token.current;
    const sheet = getSheet();
    const row = selection.row + 1;
    const col = selection.col + 1;
    void Promise.all([
      engine.query({ kind: 'filters', sheet }),
      engine.query({ kind: 'tableAt', sheet, row, col }),
      engine.query({ kind: 'validationAt', sheet, row, col }),
      engine.query({ kind: 'noteAt', sheet, row, col }),
      engine.query({ kind: 'linkAt', sheet, row, col }),
      engine.query({ kind: 'notes', sheet }),
    ])
      .then(([nextFilters, nextTable, validation, note, link, notes]) => {
        if (mine !== token.current) return;
        setFilters(nextFilters);
        setTable((old) => (JSON.stringify(old) === JSON.stringify(nextTable) ? old : nextTable));
        setHasNote(Boolean(note && !note.threaded));
        setNoteShown(Boolean(note?.visible));
        setSheetNotes((old) => (JSON.stringify(old) === JSON.stringify(notes) ? old : notes));
        setHasLink(Boolean(link));
        filterButtons.current = nextFilters.flatMap((filter) => {
          const cols: CellButton[] = [];
          for (let at = filter.area.left; at <= filter.area.right; at++) {
            if (filter.hiddenButtons.includes(at)) continue;
            const sorted = filter.sorted?.col === at ? filter.sorted : null;
            cols.push({
              kind: 'filter',
              row: filter.area.top - 1,
              col: at - 1,
              look: filter.filtered.includes(at)
                ? 'filtered'
                : sorted
                  ? sorted.descending
                    ? 'sortedDown'
                    : 'sortedUp'
                  : 'arrow',
            });
          }
          return cols;
        });
        listButton.current =
          validation?.dropdown && validation.items.length
            ? { kind: 'list', row: row - 1, col: col - 1, look: 'arrow' }
            : null;
        placeButtons();
        // The input message shows beside the active cell, as Excel shows it.
        const grid = hostRef.current.grid();
        const box = grid?.cellBox(row - 1, col - 1);
        // Beside the cell after it: to its left on a right-to-left sheet.
        const leftward = grid?.rightToLeft ?? false;
        setInputTip(
          validation?.showInput && (validation.inputTitle || validation.inputText) && box
            ? {
                x: leftward ? box.left - 6 : box.left + box.width + 6,
                leftward,
                y: box.top + box.height + 4,
                title: validation.inputTitle,
                text: validation.inputText,
              }
            : null,
        );
      })
      .catch(() => undefined);
  }, [placeButtons]);

  /** The selection as one area, 1-based, and its sheet. */
  const selected = useCallback(() => {
    const selection = hostRef.current.selection();
    if (!selection) return null;
    return { sheet: hostRef.current.sheet(), area: toArea(selection.range), selection };
  }, []);

  /** What a data tool works on: the selection, or the region around a single cell. */
  const region = useCallback(async () => {
    const here = selected();
    const engine = hostRef.current.engine();
    if (!here || !engine) return null;
    const guess = await engine.query({ kind: 'guessTable', sheet: here.sheet, area: here.area });
    return { sheet: here.sheet, area: guess.area, headers: guess.headers, here };
  }, [selected]);

  /**
   * Excel's note box beside the cell: a new note starts with its author's name, an existing one
   * with what it says. A threaded comment's cell gets the dialog, which says it is kept as it is.
   */
  const openNote = useCallback((sheet: number, row: number, col: number) => {
    const engine = hostRef.current.engine();
    if (!engine) return;
    void engine
      .query({ kind: 'noteAt', sheet, row, col })
      .then((note) => {
        if (note?.threaded) {
          setDialog({ kind: 'note', row, col });
          return;
        }
        const author = note?.author ?? hostRef.current.userName();
        setNoteEdit({
          sheet,
          row,
          col,
          author,
          text: note?.text ?? `${author}:\n`,
          was: note?.text ?? null,
          anchor: note?.anchor ?? defaultNoteAnchor(row, col),
        });
      })
      .catch((error) => hostRef.current.notice(errorText(error)));
  }, []);

  /** The box closes: what it says is the note, made or changed in one step. */
  const finishNote = useCallback(() => {
    const edit = noteEditRef.current;
    // Esc closes the box, and the box losing the keyboard then closes it again.
    noteEditRef.current = null;
    setNoteEdit(null);
    if (edit && edit.text !== edit.was) {
      send({
        kind: 'setNote',
        sheet: edit.sheet,
        row: edit.row,
        col: edit.col,
        text: edit.text,
        author: edit.author,
      });
    }
    if (edit) hostRef.current.grid()?.focus();
  }, [send]);

  const actions = useMemo<DataActions>(() => {
    const withRegion = (then: (found: NonNullable<Awaited<ReturnType<typeof region>>>) => void) =>
      void region()
        .then((found) => {
          if (found) then(found);
        })
        .catch((error) => hostRef.current.notice(errorText(error)));
    const tableName = () => table?.name ?? null;
    const dataArea = (info: TableInfo): CellArea => ({
      ...info.area,
      top: info.area.top + (info.headerRow ? 1 : 0),
      bottom: info.area.bottom - (info.totalsRow ? 1 : 0),
    });
    return {
      sort: (descending) => {
        const here = selected();
        if (!here) return;
        const key = { index: here.selection.col + 1, descending };
        if (table) {
          send({ kind: 'sort', sheet: here.sheet, area: dataArea(table), keys: [key] });
          return;
        }
        withRegion(({ sheet, area, headers }) =>
          send({ kind: 'sort', sheet, area, header: headers, keys: [key] }),
        );
      },
      customSort: () => {
        if (table) {
          setDialog({ kind: 'sort', area: dataArea(table), headers: false });
          return;
        }
        withRegion(({ area, headers }) => setDialog({ kind: 'sort', area, headers }));
      },
      toggleFilter: () => {
        const here = selected();
        if (!here) return;
        if (table) {
          send({ kind: 'tableOptions', table: table.name, filterButton: !table.filterButton });
          return;
        }
        send({ kind: 'toggleFilter', sheet: here.sheet, area: here.area });
      },
      clearFilter: () => {
        const name = tableName();
        send({
          kind: 'clearFilter',
          sheet: hostRef.current.sheet(),
          ...(name ? { table: name } : {}),
        });
      },
      reapplyFilter: () => {
        const name = tableName();
        send({
          kind: 'reapplyFilter',
          sheet: hostRef.current.sheet(),
          ...(name ? { table: name } : {}),
        });
      },
      removeDuplicates: () => {
        if (table) {
          setDialog({ kind: 'duplicates', area: table.area, headers: table.headerRow });
          return;
        }
        withRegion(({ area, headers }) => setDialog({ kind: 'duplicates', area, headers }));
      },
      validation: () => setDialog({ kind: 'validation' }),
      circleInvalid: (on) => {
        setCircled(on);
        send({ kind: 'circleInvalid', sheet: hostRef.current.sheet(), on });
      },
      cfQuick: (quick) => setDialog({ kind: 'cfQuick', quick }),
      cfRule: (rule) => {
        const here = selected();
        if (here) send({ kind: 'addCf', sheet: here.sheet, areas: [here.area], rule });
      },
      cfClear: (whole) => {
        const here = selected();
        if (!here) return;
        send({ kind: 'clearCf', sheet: here.sheet, ...(whole ? {} : { areas: [here.area] }) });
      },
      cfManage: () => setDialog({ kind: 'cfManage' }),
      formatAsTable: (style) => {
        if (table) {
          send({ kind: 'tableOptions', table: table.name, style });
          return;
        }
        withRegion(({ area, headers }) => setDialog({ kind: 'createTable', area, headers, style }));
      },
      cellStyle: (name) => {
        const here = selected();
        if (here) send({ kind: 'cellStyle', sheet: here.sheet, areas: [here.area], name });
      },
      newCellStyle: () => {
        const taken = new Set(
          cellStyles.flatMap(({ styles }) => styles.map((style) => style.name.toLowerCase())),
        );
        let n = 1;
        while (taken.has('style ' + n)) n++;
        setDialog({
          kind: 'cellStyle',
          mode: 'new',
          name: 'Style ' + n,
          includes: ALL_INCLUDED,
          format: hostRef.current.cellFormat ?? null,
        });
      },
      modifyCellStyle: async (name) => {
        const engine = hostRef.current.engine();
        if (!engine) return;
        let found = await engine.query({ kind: 'styleFormat', name });
        // A built-in style the book doesn't have yet goes into it first, as in Excel.
        if (!found) {
          await hostRef.current.run({ kind: 'modifyCellStyle', name, change: {} });
          found = await engine.query({ kind: 'styleFormat', name });
        }
        if (!found) return;
        setDialog({
          kind: 'cellStyle',
          mode: 'modify',
          name,
          includes: found.includes,
          format: found.format,
        });
      },
      insertTable: () =>
        withRegion(({ area, headers }) => setDialog({ kind: 'createTable', area, headers })),
      hyperlink: () => {
        const here = selected();
        if (here) setDialog({ kind: 'hyperlink', area: here.area });
      },
      removeLink: () => {
        const here = selected();
        if (here) send({ kind: 'setLink', sheet: here.sheet, area: here.area, plain: true });
      },
      clearLinks: () => {
        const here = selected();
        if (here) send({ kind: 'setLink', sheet: here.sheet, area: here.area });
      },
      toggleNote: () => {
        const here = selected();
        if (!here) return;
        send({
          kind: 'noteVisible',
          sheet: here.sheet,
          row: here.selection.row + 1,
          col: here.selection.col + 1,
          visible: !noteShown,
        });
      },
      showAllNotes: () => setAllNotesShown((on) => !on),
      nameManager: () => setDialog({ kind: 'names' }),
      defineName: () => setDialog({ kind: 'defineName' }),
      namesFromSelection: () => {
        const here = selected();
        if (here) setDialog({ kind: 'createNames', area: here.area });
      },
      editNote: () => {
        const here = selected();
        if (here) openNote(here.sheet, here.selection.row + 1, here.selection.col + 1);
      },
      deleteNote: () => {
        const here = selected();
        if (!here) return;
        send({
          kind: 'setNote',
          sheet: here.sheet,
          row: here.selection.row + 1,
          col: here.selection.col + 1,
        });
      },
      nextNote: (back) => {
        const here = selected();
        const engine = hostRef.current.engine();
        if (!here || !engine) return;
        void engine.query({ kind: 'notes', sheet: here.sheet }).then((notes) => {
          if (!notes.length) return;
          const at = (here.selection.row + 1) * 16384 + here.selection.col + 1;
          const key = (note: { row: number; col: number }) => note.row * 16384 + note.col;
          const next = back
            ? ([...notes].reverse().find((note) => key(note) < at) ?? notes[notes.length - 1])
            : (notes.find((note) => key(note) > at) ?? notes[0]);
          if (!next) return;
          const cell = { top: next.row, left: next.col, bottom: next.row, right: next.col };
          void hostRef.current.goTo(here.sheet, cell).then(() => {
            openNote(here.sheet, next.row, next.col);
          });
        });
      },
      tableOptions: (options) => {
        if (table) send({ kind: 'tableOptions', table: table.name, ...options });
      },
      renameTable: (name) => {
        if (table) send({ kind: 'renameTable', table: table.name, name });
      },
      resizeTable: () => {
        if (table) {
          setDialog({
            kind: 'createTable',
            area: table.area,
            headers: table.headerRow,
            resize: table.name,
          });
        }
      },
      convertToRange: () => {
        if (table) setDialog({ kind: 'convert', table: table.name });
      },
    };
  }, [noteShown, openNote, region, selected, send, table]);

  /** A filter's or a list's button in the grid. */
  const onCellButton = useCallback(
    (button: CellButton, box: ClientBox) => {
      const sheet = hostRef.current.sheet();
      if (button.kind === 'filter') {
        const col = button.col + 1;
        if (menuClosed.current.col === col && performance.now() - menuClosed.current.at < 300) {
          return;
        }
        const filter = filters.find(
          (entry) =>
            entry.area.top === button.row + 1 && col >= entry.area.left && col <= entry.area.right,
        );
        if (filter) setFilterMenu({ table: filter.table, area: filter.area, col, anchor: box });
        return;
      }
      const engine = hostRef.current.engine();
      if (!engine) return;
      void engine
        .query({ kind: 'validationAt', sheet, row: button.row + 1, col: button.col + 1 })
        .then((validation) => {
          if (validation?.items.length) {
            setListMenu({
              sheet,
              row: button.row + 1,
              col: button.col + 1,
              items: validation.items,
              labels: validation.labels,
              anchor: box,
            });
          }
        });
    },
    [filters],
  );

  /** A note or a link's ScreenTip under the pointer. */
  const onHover = useCallback((cell: { row: number; col: number } | null, x: number, y: number) => {
    const engine = hostRef.current.engine();
    if (!cell || !engine) {
      setHoverTip(null);
      return;
    }
    const sheet = hostRef.current.sheet();
    const at = { sheet, row: cell.row + 1, col: cell.col + 1 };
    // A note opens to the left of the pointer on a right-to-left sheet.
    const leftward = hostRef.current.grid()?.rightToLeft ?? false;
    const near = { x: leftward ? x - 14 : x + 14, y: y + 10, leftward };
    void Promise.all([
      engine.query({ kind: 'noteAt', ...at }),
      engine.query({ kind: 'linkAt', ...at }),
    ])
      .then(([note, link]) => {
        if (note?.text !== undefined) {
          // A note that shows all the time needs no tip.
          if (note.visible || allNotesRef.current) setHoverTip(null);
          else setHoverTip({ ...near, title: note.author, text: note.text });
        } else if (note?.threaded) {
          setHoverTip({ ...near, text: 'Threaded comment (from a newer Excel)' });
        } else if (link) {
          const target = link.address ?? link.location ?? '';
          setHoverTip({
            ...near,
            text: link.tooltip ?? `${target} - Click once to follow.`,
          });
        } else {
          const details = hostRef.current.hoverDetails;
          if (!details) {
            setHoverTip(null);
            return;
          }
          void details(sheet, at.row, at.col)
            .then((tip) => setHoverTip(tip ? { ...near, ...tip } : null))
            .catch(() => setHoverTip(null));
        }
      })
      .catch(() => setHoverTip(null));
  }, []);

  /** A click on a link follows it: a web or mail address opens, a place is selected. */
  const onCellClick = useCallback((row: number, col: number) => {
    const engine = hostRef.current.engine();
    if (!engine) return;
    const sheet = hostRef.current.sheet();
    void engine.query({ kind: 'linkAt', sheet, row: row + 1, col: col + 1 }).then(async (link) => {
      if (!link) return;
      if (link.address) {
        const address = linkToOpen(link.address);
        if (address) window.open(address, '_blank', 'noopener,noreferrer');
        else hostRef.current.notice('Only web and mail addresses open from a link here.');
        return;
      }
      if (!link.location) return;
      const place = await engine.resolveName(link.location, sheet);
      if (place) {
        const { sheet: target, ...area } = place;
        await hostRef.current.goTo(target, area);
      } else {
        hostRef.current.notice('Reference isn’t valid.');
      }
    });
  }, []);

  /** Shows Excel's message for a refusal. Returns whether it was one. */
  const notice = useCallback((message: string | null) => {
    const refusal = message ? readRefusal(message) : null;
    if (!refusal) {
      hostRef.current.notice(message);
      return;
    }
    setDialog({
      kind: 'refused',
      style: refusal.style,
      title: refusal.title,
      text: refusal.text || DEFAULT_REFUSAL,
    });
  }, []);

  const close = useCallback(() => setDialog(null), []);

  const answerRefusal = (choice: 'retry' | 'cancel' | 'accept') => {
    const editor = hostRef.current.editor();
    setDialog(null);
    // Retry leaves the cell open on what was typed, which the editor already does.
    if (choice === 'retry') return;
    const state = editor.snapshot();
    editor.cancel();
    if (choice === 'accept' && state.active) {
      send({
        kind: 'input',
        sheet: state.sheet,
        row: state.row + 1,
        col: state.col + 1,
        text: state.text,
        force: true,
      });
    }
  };

  const sheet = host.sheet();
  const here = selected();
  const state: DataState = {
    hasBook: host.hasBook,
    table,
    tableStyles,
    cellStyles,
    filterOn: filters.some((filter) => filter.table === null),
    circled,
    hasNote,
    noteShown,
    allNotesShown,
    hasLink,
  };
  const engine = host.engine;
  const selectionText = here
    ? `=${quoteSheet(host.sheetNames[sheet] ?? '')}!${absoluteRange(here.area)}`
    : '';

  const message = (title: string, text: string, buttons: ReactNode) => (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) answerRefusal('cancel');
      }}
      headerText={title}
      width={380}
      data-testid="data-message"
      renderBody={() => <p style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{text}</p>}
      renderFooter={() => <div className="cgs-dialog-actions">{buttons}</div>}
    />
  );

  let shown: ReactNode = null;
  if (dialog && here) {
    const { area, selection } = here;
    switch (dialog.kind) {
      case 'sort':
        shown = (
          <SortDialog
            open
            engine={engine}
            sheet={sheet}
            area={dialog.area}
            headers={dialog.headers}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'validation':
        shown = (
          <ValidationDialog
            open
            engine={engine}
            sheet={sheet}
            areas={[area]}
            row={selection.row + 1}
            col={selection.col + 1}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'names':
        shown = (
          <NameManager
            open
            engine={engine}
            sheetNames={host.sheetNames}
            activeSheet={sheet}
            selectionText={selectionText}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'defineName':
        shown = (
          <DefineNameDialog
            open
            sheetNames={host.sheetNames}
            activeSheet={sheet}
            selectionText={selectionText}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'createNames':
        shown = (
          <CreateNamesDialog open sheet={sheet} area={dialog.area} run={run} onClose={close} />
        );
        break;
      case 'cfQuick':
        shown = (
          <CfQuickRule
            open
            kind={dialog.quick}
            sheet={sheet}
            areas={[area]}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'cfManage':
        shown = <CfManager open engine={engine} sheet={sheet} run={run} onClose={close} />;
        break;
      case 'duplicates':
        shown = (
          <RemoveDuplicatesDialog
            open
            engine={engine}
            sheet={sheet}
            area={dialog.area}
            headers={dialog.headers}
            run={run}
            onClose={close}
            onDone={(text) => setDialog({ kind: 'message', title: 'Microsoft Excel', text })}
          />
        );
        break;
      case 'hyperlink':
        shown = (
          <HyperlinkDialog
            open
            engine={engine}
            sheet={sheet}
            area={dialog.area}
            sheetNames={host.sheetNames}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'createTable': {
        const { style, resize } = dialog;
        // The same dialog makes a table with a style from the gallery, or resizes one.
        const create = (command: Command) => {
          if (command.kind !== 'createTable') return run(command);
          if (resize) return run({ kind: 'resizeTable', table: resize, area: command.area });
          return run(style ? { ...command, style } : command);
        };
        shown = (
          <CreateTableDialog
            open
            sheet={sheet}
            area={dialog.area}
            headers={dialog.headers}
            engine={engine}
            run={create}
            onClose={close}
          />
        );
        break;
      }
      case 'cellStyle': {
        const style = dialog;
        shown = (
          <StyleDialog
            open
            mode={style.mode}
            name={style.name}
            includes={style.includes}
            format={style.format}
            onFormat={
              host.formatStyle && style.format
                ? () =>
                    host.formatStyle?.(style.format as CellFormat, (change) =>
                      setDialog({ ...style, change: { ...style.change, ...change } }),
                    )
                : undefined
            }
            onOk={(name, includes) => {
              close();
              const command: Command =
                style.mode === 'new'
                  ? {
                      kind: 'newCellStyle',
                      name,
                      sheet,
                      row: selection.row + 1,
                      col: selection.col + 1,
                      includes,
                    }
                  : { kind: 'modifyCellStyle', name, change: style.change ?? {}, includes };
              run(command)
                .then(loadCellStyles)
                .catch((error) => hostRef.current.notice(errorText(error)));
            }}
            onClose={close}
          />
        );
        break;
      }
      case 'note':
        shown = (
          <NoteDialog
            open
            engine={engine}
            sheet={sheet}
            row={dialog.row}
            col={dialog.col}
            run={run}
            onClose={close}
          />
        );
        break;
      case 'message':
        shown = message(
          dialog.title,
          dialog.text,
          <CgButton intent="primary" data-testid="data-message-ok" onClick={close}>
            OK
          </CgButton>,
        );
        break;
      case 'convert': {
        const name = dialog.table;
        shown = message(
          'Microsoft Excel',
          'Do you want to convert the table to a normal range?',
          <>
            <CgButton
              intent="primary"
              data-testid="convert-yes"
              onClick={() => {
                close();
                send({ kind: 'convertToRange', table: name });
              }}
            >
              Yes
            </CgButton>
            <CgButton data-testid="convert-no" onClick={close}>
              No
            </CgButton>
          </>,
        );
        break;
      }
      case 'refused':
        shown = message(
          dialog.title,
          dialog.style === 'warning' ? dialog.text + '\n\nContinue?' : dialog.text,
          dialog.style === 'warning' ? (
            <>
              <CgButton
                intent="primary"
                data-testid="refused-yes"
                onClick={() => answerRefusal('accept')}
              >
                Yes
              </CgButton>
              <CgButton data-testid="refused-no" onClick={() => answerRefusal('retry')}>
                No
              </CgButton>
              <CgButton data-testid="refused-cancel" onClick={() => answerRefusal('cancel')}>
                Cancel
              </CgButton>
            </>
          ) : dialog.style === 'information' ? (
            <>
              <CgButton
                intent="primary"
                data-testid="refused-ok"
                onClick={() => answerRefusal('accept')}
              >
                OK
              </CgButton>
              <CgButton data-testid="refused-cancel" onClick={() => answerRefusal('cancel')}>
                Cancel
              </CgButton>
            </>
          ) : (
            <>
              <CgButton
                intent="primary"
                data-testid="refused-retry"
                onClick={() => answerRefusal('retry')}
              >
                Retry
              </CgButton>
              <CgButton data-testid="refused-cancel" onClick={() => answerRefusal('cancel')}>
                Cancel
              </CgButton>
            </>
          ),
        );
        break;
    }
  }

  const overlays = (
    <>
      {shown}
      <NoteLayer
        grid={host.grid}
        sheet={sheet}
        notes={sheetNotes.filter((note) => allNotesShown || note.visible)}
        editing={noteEdit && noteEdit.sheet === sheet ? noteEdit : null}
        onEditText={(text) => setNoteEdit((edit) => (edit ? { ...edit, text } : edit))}
        onDone={finishNote}
        onOpen={(note) => openNote(sheet, note.row, note.col)}
      />
      {filterMenu && (
        <FilterMenu
          engine={engine}
          sheet={sheet}
          table={filterMenu.table}
          area={filterMenu.area}
          col={filterMenu.col}
          anchor={filterMenu.anchor}
          leftward={hostRef.current.grid()?.rightToLeft ?? false}
          run={run}
          onClose={() => {
            menuClosed.current = { col: filterMenu.col, at: performance.now() };
            setFilterMenu(null);
            hostRef.current.grid()?.focus();
          }}
        />
      )}
      {listMenu && (
        <div
          className="cgs-list-menu"
          data-testid="validation-list"
          role="listbox"
          style={{
            position: 'fixed',
            left: listMenu.anchor.left + listMenu.anchor.width - 160,
            top: listMenu.anchor.top + listMenu.anchor.height,
            minWidth: 160,
            maxHeight: 220,
            overflowY: 'auto',
            background: 'var(--cg-color-surface, #fff)',
            border: '1px solid var(--cg-color-border, #888)',
            zIndex: 50,
          }}
          onMouseLeave={() => undefined}
        >
          {listMenu.items.map((item, index) => (
            <div
              key={item}
              role="option"
              aria-selected={false}
              aria-label={listMenu.labels ? `${item} · ${listMenu.labels[index] ?? ''}` : item}
              tabIndex={-1}
              style={{ padding: '2px 6px', cursor: 'default', whiteSpace: 'nowrap' }}
              onMouseDown={(event) => {
                event.preventDefault();
                const { sheet: at, row, col } = listMenu;
                setListMenu(null);
                send({ kind: 'input', sheet: at, row, col, text: item });
                hostRef.current.grid()?.focus();
              }}
            >
              {item}
              {listMenu.labels?.[index] && listMenu.labels[index] !== item && (
                <span className="cgs-list-label">{` · ${listMenu.labels[index]}`}</span>
              )}
            </div>
          ))}
        </div>
      )}
      {[inputTip, hoverTip].map(
        (tip, index) =>
          tip && (
            <div
              key={index}
              className="cgs-cell-tip"
              data-testid={index === 0 ? 'input-message' : 'hover-tip'}
              style={{
                position: 'fixed',
                left: tip.x,
                top: tip.y,
                transform: tip.leftward ? 'translateX(-100%)' : undefined,
                maxWidth: 260,
                padding: '4px 6px',
                background: index === 0 ? '#ffffe1' : '#ffffe1',
                color: '#000',
                border: '1px solid #767676',
                boxShadow: '1px 1px 3px rgba(0,0,0,0.3)',
                pointerEvents: 'none',
                whiteSpace: 'pre-wrap',
                fontSize: 12,
                zIndex: 40,
              }}
            >
              {tip.title && <strong style={{ display: 'block' }}>{tip.title}</strong>}
              {tip.text}
            </div>
          ),
      )}
    </>
  );

  // A list's menu closes when the selection moves on.
  const onSelectionMoved = useCallback(() => {
    setListMenu(null);
    refresh();
  }, [refresh]);

  return {
    state,
    actions,
    overlays,
    refresh: onSelectionMoved,
    notice,
    onCellButton,
    onHover,
    onCellClick,
    placeButtons,
  };
}
