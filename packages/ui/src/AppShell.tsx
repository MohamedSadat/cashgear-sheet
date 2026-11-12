import '@cashgear/ui/styles.css';
import './styles.css';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type DragEvent,
} from 'react';
import {
  CgButton,
  CgButtonGroup,
  CgContextMenu,
  CgMenu,
  CgMessageBox,
  CgRibbon,
  CgSlider,
  CgStatusBadge,
  CgStatusBar,
  CgTabs,
  CgWindow,
  type CgBorderApplyDetails,
  type CgBorderLineStyle,
  type CgColorPickerValue,
  type CgContextMenuActions,
  type CgContextMenuItem,
  type CgMenuItem,
} from '@cashgear/ui';
import {
  createEngineClient,
  ENGINE_STOPPED,
  localeData,
  type Applied,
  type BookSummary,
  type CalcStatus,
  type CellFormat,
  type ChartView,
  type Command,
  type CellArea,
  type DrawingBox,
  type EngineClient,
  type ErpBinding,
  type ErpChangeset,
  type Question,
  type FormatChange,
  type FormatLists,
  type FoundCell,
  type LoadEvent,
  type Recoverable,
  type SaveReport,
  type WorkbookChanges,
} from '@cgs/engine-client';
import { CellEditor, type CellEditorHandle, type CompletionControl } from './CellEditor';
import { FindDialog } from './FindDialog';
import { FormatCellsDialog } from './FormatCellsDialog';
import { FormulaBar } from './FormulaBar';
import {
  type CellButton,
  type ClientBox,
  type GridController,
  type GridEvents,
  type GridRegion,
  type GridSelection,
  type ResizeHint,
} from './grid/controller';
import { sliderToZoom, zoomToSlider } from './grid/geometry';
import { SheetCanvas } from './grid/SheetCanvas';
import {
  OPENABLE_EXTENSIONS,
  type NativeInfo,
  type OpenedFile,
  type RecentFile,
  type SaveTarget,
  type SheetPlatform,
  type WorkingCopy,
} from './platform';
import { buildRibbonTabs, themeColors, type FormatTab, type RibbonActions } from './ribbon';
import { useDataTools } from './data/useDataTools';
import { useChartTools } from './charts/useChartTools';
import { chartInsertGroups, chartTabs } from './charts/ribbon-charts';
import { usePivotTools } from './pivots/usePivotTools';
import { usePrintTools } from './print/usePrintTools';
import { pageLayoutTab } from './print/ribbon-page';
import { BROWSER_ALT_KEYS, withKeyTips } from './key-tips';
import type { PrintChoice } from './print/printJob';
import { pivotInsertItem, pivotTabs } from './pivots/ribbon-pivots';
import { saveReportLines, saveSummary, workbookExtension } from './save-report';
import { fromArea, SheetEditing, toArea, type EditingHost } from './sheet-editing';
import { bordersFromPicker, SheetFormat } from './sheet-format';
import type { GridAction } from './shortcuts';
import { readGridColors, readPalette, type ThemeController, type ThemePreference } from './theme';
import { AboutDialog, UpdateDialog } from './release/ReleaseDialogs';
import { useUpdates } from './release/useUpdates';
import { readDefaultDirection, writeDefaultDirection, type Direction } from './direction';
import { UnhideDialog } from './sheets/UnhideDialog';
import { noteAuthor, readUserName, writeUserName } from './user-name';
import {
  readErrorSettings,
  ruleBits,
  useErrorTools,
  writeErrorSettings,
  RULE_BITS,
  type ErrorSettings,
} from './errors/useErrorTools';
import { RULE_LABELS } from './errors/error-text';
import { cellName } from './grid/geometry';
import { PROTECTED_MESSAGE, useProtectTools } from './sheets/useProtectTools';
import type { EntitySchema, SessionInfo } from '@cgs/erp-connector';
import { isErpRefusal, READ_ONLY_MESSAGE } from './erp/messages';
import { useErpTools } from './erp/useErpTools';
import { useCloudTools } from './cloud/useCloudTools';

const themeChoices = [
  { name: 'system', text: 'System' },
  { name: 'light', text: 'Light' },
  { name: 'dark', text: 'Dark' },
];

interface LoadState {
  phase: 'idle' | 'reading' | 'preparing' | 'ready';
  done: number;
  total: number;
}

interface CellDetail {
  name: string;
  edit: string;
}

interface AppShellProps {
  platform: SheetPlatform;
  theme: ThemeController;
  /** The app's version, for About. The desktop's own comes from its bundle. */
  version?: string;
}

const noCell: CellDetail = { name: '', edit: '' };

/**
 * The engine's memory budget (docs/decisions.md, Phase 9): WASM can have 4 GiB, and its memory
 * never shrinks. Past this much the status bar says so, while there is still room to save.
 */
const MEMORY_WARNING_BYTES = 3 * 1024 ** 3;
/** Commands after which Circular References is looked at again. */
const FORMULA_CHANGES = new Set<string>([
  'input',
  'arrayInput',
  'clear',
  'fill',
  'paste',
  'pasteExternal',
  'move',
  'axis',
  'shiftCells',
  'deleteSheet',
  'renameSheet',
  'replaceAll',
  'replaceOne',
  'includeCells',
  'undo',
  'redo',
]);
/** Excel 2016's words when a formula closes a loop. */
const CIRCULAR_WARNING =
  'There are one or more circular references where a formula refers to its own cell either ' +
  'directly or indirectly. This might cause them to calculate incorrectly.' +
  '\n\nTry removing or changing these references, or moving the formulas to different cells.';
const memoryNotice = (bytes: number) =>
  `This workbook uses ${(bytes / 1024 ** 3).toFixed(1)} of the 4 GB the engine can have: ` +
  'save your work before adding much more.';

/** A dialog the shell is showing. */
type ShellDialog =
  | { kind: 'format'; tab: FormatTab }
  /** Format Cells on a cell style's look, for the Style dialog's Format…. */
  | { kind: 'styleFormat'; format: CellFormat; done: (change: FormatChange) => void }
  | { kind: 'find'; tab: 'find' | 'replace' }
  | { kind: 'size'; axis: 'row' | 'col'; value: string }
  | { kind: 'shift'; remove: boolean; choice: 'side' | 'down' | 'row' | 'col' }
  | { kind: 'merge'; answer: (accepted: boolean) => void }
  | { kind: 'options'; direction: Direction; userName: string; errors: ErrorSettings }
  | { kind: 'unhide' }
  | { kind: 'close'; answer: (close: boolean) => void };

/** How the sheet showing is set up, for the View tab. */
interface SheetFlags {
  frozen: boolean;
  showGrid: boolean;
  showHeadings: boolean;
  rightToLeft: boolean;
}

/** A CSV's bytes as text: UTF-8 if they can be, or else Windows' usual code page. */
async function csvFileText(source: OpenedFile['source']) {
  const bytes =
    source.kind === 'blob'
      ? await source.blob.arrayBuffer()
      : await (await fetch(source.url)).arrayBuffer();
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return new TextDecoder('windows-1252').decode(bytes);
  }
}

export function AppShell({ platform, theme, version = 'development' }: AppShellProps) {
  const [client, setClient] = useState<EngineClient | null>(null);
  const [clientVersion, setClientVersion] = useState(0);
  const [problem, setProblem] = useState<string | null>(null);
  const [engineReady, setEngineReady] = useState(false);
  const [sharedMemory, setSharedMemory] = useState<boolean | null>(null);
  const [native, setNative] = useState<NativeInfo | null>(null);
  const [book, setBook] = useState<BookSummary | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [sheet, setSheet] = useState(0);
  const [load, setLoad] = useState<LoadState>({ phase: 'idle', done: 0, total: 0 });
  const [loadedSheets, setLoadedSheets] = useState<ReadonlySet<number>>(new Set());
  const [recent, setRecent] = useState<RecentFile[]>([]);
  const [zoom, setZoom] = useState(100);
  const [cell, setCell] = useState<CellDetail>(noCell);
  const [dragOver, setDragOver] = useState(false);
  const [canSave, setCanSave] = useState(false);
  const [saving, setSaving] = useState(false);
  /** How far a save has come, in whole percent, while one runs. */
  const [saveProgress, setSaveProgress] = useState<{ checking: boolean; percent: number } | null>(
    null,
  );
  const [lastSave, setLastSave] = useState<SaveReport | null>(null);
  const [showReport, setShowReport] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [calc, setCalc] = useState<CalcStatus>({ calculating: false, manual: false });
  const calcRef = useRef(calc);
  calcRef.current = calc;
  const [undoLabels, setUndoLabels] = useState<{ undo: string | null; redo: string | null }>({
    undo: null,
    redo: null,
  });
  const [names, setNames] = useState<string[]>([]);
  const [renaming, setRenaming] = useState<{ sheet: number; name: string } | null>(null);
  const [recoverable, setRecoverable] = useState<Recoverable[]>([]);
  const [release, setRelease] = useState<'about' | 'update' | null>(null);
  const updates = useUpdates(platform);
  const [format, setFormat] = useState<CellFormat | null>(null);
  const [lists, setLists] = useState<FormatLists | null>(null);
  const [dialog, setDialog] = useState<ShellDialog | null>(null);
  const [flags, setFlags] = useState<SheetFlags>({
    frozen: false,
    showGrid: true,
    showHeadings: true,
    rightToLeft: false,
  });
  const [showFormulaBar, setShowFormulaBar] = useState(true);
  const [fontColor, setFontColor] = useState<CgColorPickerValue>({ kind: 'rgb', color: '#ff0000' });
  const [fillColor, setFillColor] = useState<CgColorPickerValue>({ kind: 'rgb', color: '#ffff00' });
  const [borderStyle, setBorderStyle] = useState<CgBorderLineStyle>('thin');
  const [borderColor, setBorderColor] = useState<CgColorPickerValue>({ kind: 'automatic' });
  const [borderPreset, setBorderPreset] = useState<CgBorderApplyDetails | null>(null);
  const [resizeHint, setResizeHint] = useState<ResizeHint | null>(null);
  const formatRef = useRef<CellFormat | null>(null);
  formatRef.current = format;
  const protectMessageRef = useRef<(message: string) => void>(() => undefined);
  const erpReadOnlyAtRef = useRef<(sheet: number, row: number, col: number) => boolean>(
    () => false,
  );
  const erpHoverRef = useRef<
    (sheet: number, row: number, col: number) => Promise<{ title: string; text: string } | null>
  >(() => Promise.resolve(null));
  const erpBindRef = useRef<
    (session: SessionInfo, label: string, opened?: Promise<void>) => Promise<ErpBinding>
  >(() => Promise.reject(new Error('Not ready.')));
  const erpPrepareRef = useRef<(session: SessionInfo, schema: EntitySchema) => Promise<void>>(() =>
    Promise.reject(new Error('Not ready.')),
  );
  /** A binding is writing the new workbook: the grid draws it once it's done (Phase 19). */
  const viewHeldRef = useRef<{ held: boolean; changed: boolean }>({ held: false, changed: false });
  const refusedRef = useRef<(error: unknown) => void>(() => undefined);
  /**
   * A protected sheet that keeps locked cells, or every cell, from being selected moves the
   * selection on to one it allows, the way it was going, or back where it was.
   */
  const keepSelectableRef = useRef<(before: GridSelection | null, after: GridSelection) => void>(
    () => undefined,
  );
  keepSelectableRef.current = (before, after) => {
    const guarded = bookRef.current?.sheets[sheetRef.current]?.protection;
    const engine = clientRef.current;
    if (!guarded || (guarded.selectLocked && guarded.selectUnlocked) || !engine) return;
    const sheet = sheetRef.current;
    const step = (to: number, from: number | undefined) =>
      from === undefined || Math.abs(to - from) !== 1 ? 0 : to - from;
    const rows = step(after.row, before?.row);
    const cols = rows ? 0 : step(after.col, before?.col);
    void engine
      .query({ kind: 'nextSelectable', sheet, row: after.row + 1, col: after.col + 1, rows, cols })
      .then((target) => {
        const grid = controller.current;
        if (!grid || sheetRef.current !== sheet) return;
        if (target && target[0] === after.row + 1 && target[1] === after.col + 1) return;
        const to = target ? { row: target[0] - 1, col: target[1] - 1 } : before;
        if (!to) return;
        grid.selectArea({ top: to.row, left: to.col, bottom: to.row, right: to.col });
      })
      .catch(() => undefined);
  };
  const contextMenu = useRef<CgContextMenuActions<GridRegion | 'tab' | 'drawing'> | null>(null);
  const fileRef = useRef<OpenedFile | null>(null);
  /** Commands run in order, as the first has to make the source safe before any of them. */
  const commands = useRef<Promise<unknown>>(Promise.resolve());
  const editedRef = useRef(false);
  const csvRef = useRef<string | null>(null);
  const bookRef = useRef<BookSummary | null>(null);
  const cellRef = useRef<CellDetail>(noCell);
  const editorRef = useRef<CellEditorHandle | null>(null);
  const completionRef = useRef<CompletionControl | null>(null);
  const selectionRef = useRef<GridSelection | null>(null);
  // The data tools' refresh and message, which come later than what calls them.
  const dataRefreshRef = useRef<() => void>(() => undefined);
  /** Reads the ERP binding again after a command, which may have moved its table. */
  const erpSyncRef = useRef<() => void>(() => undefined);
  /** Settles when a workbook opened for ERP data can be written, or fails with its open. */
  const erpLoadRef = useRef<{ resolve: () => void; reject: (error: Error) => void } | null>(null);
  /** Opens a recovered draft, set once `restoreRecovered` is made. */
  const restoreRef = useRef<(entry: Recoverable) => Promise<void>>(() => Promise.resolve());
  /** The page was opened from Cash Gear ERP, whose launch offers its own drafts back. */
  const hadLaunchRef = useRef(false);
  const errorRefreshRef = useRef<() => void>(() => undefined);
  /** Circular References, as the status bar names its first cell, and the cells it knows. */
  const [circular, setCircular] = useState<[number, number, number][]>([]);
  const circularRef = useRef<[number, number, number][]>([]);
  const pivotRefreshRef = useRef<() => void>(() => undefined);
  const pivotButtonsRef = useRef<() => CellButton[]>(() => []);
  const dataNoticeRef = useRef<(message: string | null) => void>(() => undefined);
  // The chart tools come later than the code that tells them about sheets and changes.
  const chartDrawingsRef = useRef<((drawings: DrawingBox[]) => void) | null>(null);
  const chartRefreshRef = useRef<(() => void) | null>(null);
  const printRefreshRef = useRef<() => void>(() => undefined);
  const printSelectionRef = useRef<(selection: GridSelection) => void>(() => undefined);
  const printPdfRef = useRef<
    (choice: PrintChoice) => Promise<{ bytes: Uint8Array; pages: number }>
  >(() => Promise.reject(new Error('No workbook is open.')));
  const chartDeselectRef = useRef<(() => void) | null>(null);
  const loadedRef = useRef<ReadonlySet<number>>(new Set());
  /** The source has been copied, or there's nowhere to copy it, so edits can go ahead. */
  const protectedRef = useRef(false);
  const workingRef = useRef<WorkingCopy | null>(null);
  const savingRef = useRef(false);
  // Takes a save's failure when it is a Cash Gear Workspace one (`useCloudTools`), which shows
  // what to do about it; anything else is reported as it always was.
  const cloudErrorRef = useRef<(error: unknown) => boolean>(() => false);
  const untitled = useRef(0);
  const controller = useRef<GridController | null>(null);
  const openToken = useRef(0);
  const cellToken = useRef(0);
  const shownSheet = useRef(-1);
  const clientRef = useRef<EngineClient | null>(null);
  const sheetRef = useRef(0);
  const loadRef = useRef<LoadState['phase']>('idle');
  const lastSaveRef = useRef<SaveReport | null>(null);
  const problemRef = useRef<string | null>(null);
  sheetRef.current = sheet;
  clientRef.current = client;
  loadRef.current = load.phase;
  lastSaveRef.current = lastSave;
  problemRef.current = problem;
  bookRef.current = book;
  cellRef.current = cell;
  loadedRef.current = loadedSheets;

  const preference = useSyncExternalStore(theme.subscribe, () => theme.preference);
  const current = useSyncExternalStore(theme.subscribe, () => theme.theme);
  // The tokens change with `data-cg-theme`, which the controller has already set.
  const palette = useMemo(() => {
    void current;
    return readPalette(current === 'dark');
  }, [current]);
  const colors = useMemo(() => {
    void current;
    return readGridColors();
  }, [current]);
  /**
   * A module that ran out of memory can't be used again: the workbook goes, a new engine starts,
   * and it offers what the journal kept. Once per engine, however many calls fail with it.
   */
  const lostRef = useRef<EngineClient | null>(null);
  const loseEngine = useCallback(() => {
    const engine = clientRef.current;
    if (!engine || lostRef.current === engine) return;
    lostRef.current = engine;
    // Commands queued behind one the old engine never answered start afresh.
    commands.current = Promise.resolve();
    fileRef.current = null;
    setBook(null);
    setFileName(null);
    setLoad({ phase: 'idle', done: 0, total: 0 });
    controller.current?.reset();
    setClientVersion((version) => version + 1);
  }, []);
  const report = useCallback(
    (error: unknown) => {
      const text = error instanceof Error ? error.message : String(error);
      // A call the engine was answering when it was replaced: what replaced it has said why.
      if (text === ENGINE_STOPPED) return;
      setProblem(text);
      if (/ran out of memory/.test(text)) loseEngine();
    },
    [loseEngine],
  );

  useEffect(() => {
    const started = createEngineClient(report);
    let live = true;
    setClient(started);
    started
      .init()
      .then(() => started.setLocale(localeData(navigator.language || 'en-US')))
      .then(() => {
        if (live) setEngineReady(true);
        return started.checkSharedMemory();
      })
      .then((shared) => {
        if (live) setSharedMemory(shared);
      })
      .catch((error) => {
        if (live) report(error);
      });
    return () => {
      live = false;
      setClient(null);
      setEngineReady(false);
      started.dispose();
    };
  }, [report, clientVersion]);

  useEffect(() => {
    let live = true;
    platform
      .nativeInfo()
      .then((info) => {
        if (live) setNative(info);
      })
      .catch(report);
    return () => {
      live = false;
    };
  }, [platform, report]);

  const refreshRecent = useCallback(() => {
    platform
      .recentFiles()
      .then(setRecent)
      .catch((error) => console.warn('Could not read the recent files:', error));
  }, [platform]);
  useEffect(refreshRecent, [refreshRecent]);

  /** Shows a sheet whose cells are all readable. */
  /**
   * What the view says, which a save keeps but undo doesn't: the zoom and the sheet showing. Each
   * goes through the command queue, so it lands in order with edits and saves, and calls queued
   * while one waits are folded into it.
   */
  const viewQueued = useRef<Map<string, Command> | null>(null);
  const rulesSent = useRef<EngineClient | null>(null);
  const viewCommand = useCallback((command: Command) => {
    const key = command.kind === 'zoom' ? 'zoom:' + command.sheet : command.kind;
    if (viewQueued.current) {
      viewQueued.current.set(key, command);
      return;
    }
    const pending = new Map<string, Command>([[key, command]]);
    viewQueued.current = pending;
    commands.current = commands.current
      .catch(() => undefined)
      .then(async () => {
        viewQueued.current = null;
        const engine = clientRef.current;
        if (!engine) return;
        for (const next of pending.values()) await engine.command(next).catch(() => undefined);
      });
  }, []);

  const showSheet = useCallback(
    async (engine: EngineClient, index: number) => {
      const [layout, drawings] = await Promise.all([engine.layout(index), engine.drawings(index)]);
      if (sheetRef.current !== index) return;
      if (shownSheet.current !== index) viewCommand({ kind: 'activeSheet', sheet: index });
      // Each engine starts with Excel's rules; the device's settings go to it once.
      if (rulesSent.current !== engine) {
        rulesSent.current = engine;
        viewCommand({ kind: 'errorRules', rules: ruleBits(readErrorSettings()) });
      }
      shownSheet.current = index;
      controller.current?.showSheet(index, layout, drawings);
      chartDrawingsRef.current?.(drawings);
      printRefreshRef.current();
      setFlags({
        frozen: layout.frozenRows > 0 || layout.frozenCols > 0,
        showGrid: layout.showGrid,
        showHeadings: layout.showHeaders,
        rightToLeft: layout.rightToLeft,
      });
      // A layout that changed under a ribbon box being typed in leaves the keyboard there.
      const active = document.activeElement;
      const typing =
        active instanceof HTMLInputElement ||
        active instanceof HTMLTextAreaElement ||
        active instanceof HTMLSelectElement;
      if (!typing && !editingRef.current?.session.snapshot().active) controller.current?.focus();
    },
    [viewCommand],
  );

  const onLoadEvent = useCallback(
    (token: number, engine: EngineClient, event: LoadEvent) => {
      if (token !== openToken.current) return;
      switch (event.type) {
        case 'progress':
          setLoad((state) => ({
            phase: state.phase === 'idle' ? 'reading' : state.phase,
            done: event.done,
            total: event.total,
          }));
          if (event.ready.length) {
            setLoadedSheets((sheets) => new Set([...sheets, ...event.ready]));
            for (const index of event.ready) {
              controller.current?.sheetReady(index);
              if (index === sheetRef.current) void showSheet(engine, index).catch(report);
              void engine.fitOpeningRows(index).catch(report);
            }
          }
          break;
        case 'loaded':
          setLoad((state) => ({ ...state, phase: 'preparing' }));
          erpLoadRef.current?.resolve();
          erpLoadRef.current = null;
          if (csvRef.current !== null) {
            const text = csvRef.current;
            csvRef.current = null;
            engine.loadCsv(text).catch(report);
          }
          break;
        case 'prepared':
          setLoad((state) => ({ ...state, phase: 'ready' }));
          void engine.memoryBytes().then((bytes) => {
            if (bytes >= MEMORY_WARNING_BYTES && token === openToken.current) {
              setNotice(memoryNotice(bytes));
            }
          });
          break;
        case 'error':
          erpLoadRef.current?.reject(new Error(event.message));
          erpLoadRef.current = null;
          setProblem(event.message);
          setBook(null);
          setFileName(null);
          setLoad({ phase: 'idle', done: 0, total: 0 });
          controller.current?.reset();
          // A trap leaves the module unusable, so the next file gets a new one.
          if (/ran out of memory/.test(event.message)) loseEngine();
          break;
      }
    },
    [loseEngine, report, showSheet],
  );

  /** A working copy on the native side isn't needed once something else is the source. */
  const releaseWorking = useCallback(() => {
    const copy = workingRef.current;
    workingRef.current = null;
    if (copy) void copy.release().catch((error) => console.warn('Working copy:', error));
  }, []);

  const openBook = useCallback(
    async (file: OpenedFile, recovered?: string) => {
      const engine = clientRef.current;
      if (!engine) return;
      const token = ++openToken.current;
      editingRef.current?.cancel();
      fileRef.current = file;
      // A recovered workbook already reads from its own private copy.
      protectedRef.current = Boolean(recovered);
      editedRef.current = Boolean(recovered);
      releaseWorking();
      setCanSave(Boolean(file.isNew));
      setNotice(null);
      setUndoLabels({ undo: null, redo: null });
      setLastSave(null);
      setProblem(null);
      setCell(noCell);
      setLoadedSheets(new Set());
      shownSheet.current = -1;
      setLoad({ phase: 'reading', done: 0, total: 0 });
      controller.current?.reset();
      try {
        const onEvent = (event: LoadEvent) => onLoadEvent(token, engine, event);
        const summary = recovered
          ? await engine.openRecovered(recovered, onEvent)
          : await engine.open(file.source, onEvent, file.name);
        if (token !== openToken.current) return;
        const visible = summary.sheets.findIndex(
          (entry, index) => index === summary.active && entry.state === 'visible',
        );
        const first =
          visible >= 0 ? visible : summary.sheets.findIndex((s) => s.state === 'visible');
        const active = Math.max(first, 0);
        sheetRef.current = active;
        setSheet(active);
        setBook(summary);
        setFileName(file.name);
        setLoadedSheets(
          new Set(summary.sheets.flatMap((entry, index) => (entry.loaded ? [index] : []))),
        );
        void Promise.all([engine.functionNames(), engine.definedNames()])
          .then(([functions, defined]) => {
            if (token === openToken.current) setNames([...defined, ...functions]);
          })
          .catch(report);
        await platform.rememberFile(file);
        refreshRecent();
      } catch (error) {
        if (token !== openToken.current) return;
        fileRef.current = null;
        setBook(null);
        setFileName(null);
        setLoad({ phase: 'idle', done: 0, total: 0 });
        report(error);
        if (error instanceof Error && /ran out of memory/.test(error.message)) loseEngine();
      }
    },
    [loseEngine, onLoadEvent, platform, refreshRecent, releaseWorking, report],
  );

  const openFile = useCallback(
    async (file: OpenedFile) => {
      const engine = clientRef.current;
      if (!engine || !/\.csv$/i.test(file.name)) {
        await openBook(file);
        return;
      }
      // A CSV is cells for a new workbook, and saving it asks where.
      csvRef.current = await csvFileText(file.source);
      const bytes = await engine.blankWorkbook(readDefaultDirection() === 'rtl');
      await openBook({
        name: file.name.replace(/\.csv$/i, '.xlsx'),
        source: { kind: 'blob', blob: new Blob([bytes as BlobPart]) },
        isNew: true,
      });
    },
    [openBook],
  );

  const newWorkbook = useCallback(async () => {
    const engine = clientRef.current;
    if (!engine) return;
    try {
      const bytes = await engine.blankWorkbook(readDefaultDirection() === 'rtl');
      untitled.current += 1;
      await openFile({
        name: `Book${untitled.current}.xlsx`,
        source: { kind: 'blob', blob: new Blob([bytes as BlobPart]) },
        isNew: true,
      });
    } catch (error) {
      report(error);
    }
  }, [openFile, report]);

  /** Saves to a place that already has permission, and moves the workbook on to the new file. */
  const writeTo = useCallback(
    async (target: SaveTarget) => {
      const engine = clientRef.current;
      const file = fileRef.current;
      if (!engine || !file || savingRef.current) return;
      savingRef.current = true;
      setSaving(true);
      setProblem(null);
      setSaveProgress(null);
      // The save takes its turn in the command queue: whatever was typed before Save is part of
      // what gets saved, and edits made while it runs wait behind it, until the file has been
      // replaced. Tiles and reads go on meanwhile.
      const job = commands.current
        .catch(() => undefined)
        .then(async () => {
          const { blob, report: saved } = await engine.save((progress) => {
            const percent = Math.floor(progress.fraction * 100);
            const checking = progress.stage === 'checking';
            setSaveProgress((last) =>
              last?.percent === percent && last.checking === checking
                ? last
                : { checking, percent },
            );
          });
          const written = await target.write(blob);
          await engine.commitSaved(written.source);
          editedRef.current = false;
          // The old file's working copy has nothing left to do, and the next edit makes a new one.
          protectedRef.current = false;
          releaseWorking();
          fileRef.current = written;
          setFileName(written.name);
          setCanSave(false);
          setLastSave(saved);
          await platform.rememberFile(written);
          refreshRecent();
        });
      commands.current = job;
      try {
        await job;
      } catch (error) {
        if (!cloudErrorRef.current(error)) report(error);
      } finally {
        savingRef.current = false;
        setSaving(false);
        setSaveProgress(null);
      }
    },
    [platform, refreshRecent, releaseWorking, report],
  );

  const saveAs = useCallback(async () => {
    const file = fileRef.current;
    if (!file || loadRef.current === 'reading') return;
    try {
      const target = await platform.pickSaveTarget(file.name, workbookExtension(file.name));
      if (target) await writeTo(target);
    } catch (error) {
      report(error);
    }
  }, [platform, report, writeTo]);

  const save = useCallback(async () => {
    const file = fileRef.current;
    if (!file || loadRef.current === 'reading') return;
    try {
      editingRef.current?.commit();
      await commands.current.catch(() => undefined);
      // Nothing to write, so the file stays as it is.
      if (!file.isNew && !(await clientRef.current?.hasChanges())) return;
      const target = await platform.saveTarget(file);
      if (target) await writeTo(target);
      else await saveAs();
    } catch (error) {
      if (!cloudErrorRef.current(error)) report(error);
    }
  }, [platform, report, saveAs, writeTo]);

  // A workbook from Cash Gear Workspace: its badge, panels and save dialogs (Track S1).
  const cloudTools = useCloudTools({
    cloud: platform.cloud,
    fileName,
    openFile,
    writeTo,
    saveAs,
  });
  cloudErrorRef.current = cloudTools.onSaveError;

  /** Asks the engine what the active cell holds, for the formula bar. */
  const refreshCell = useCallback(() => {
    const engine = clientRef.current;
    const selection = selectionRef.current;
    if (!engine || !selection) return;
    const token = ++cellToken.current;
    dataRefreshRef.current();
    errorRefreshRef.current();
    pivotRefreshRef.current();
    setCell((previous) => ({ ...previous, name: selection.name }));
    engine
      .cellText(sheetRef.current, selection.row + 1, selection.col + 1)
      .then((text) => {
        if (token === cellToken.current) setCell({ name: selection.name, edit: text.edit });
      })
      .catch(report);
    engine
      .cellFormat(sheetRef.current, selection.row + 1, selection.col + 1)
      .then((next) => {
        // The same look keeps the same object, so that the ribbon only draws again when it differs.
        if (token === cellToken.current) {
          setFormat((old) => (JSON.stringify(old) === JSON.stringify(next) ? old : next));
        }
      })
      .catch(report);
  }, [report]);

  /** The list of sheets as the engine now has it. */
  const applySummary = useCallback((summary: BookSummary) => {
    const loaded = new Set(summary.sheets.flatMap((entry, index) => (entry.loaded ? [index] : [])));
    bookRef.current = summary;
    loadedRef.current = loaded;
    setBook(summary);
    setLoadedSheets(loaded);
  }, []);

  const goToSheet = useCallback(
    async (index: number) => {
      const engine = clientRef.current;
      if (!engine || sheetRef.current === index) return;
      sheetRef.current = index;
      setSheet(index);
      if (loadedRef.current.has(index)) await showSheet(engine, index);
      else await engine.prioritize(index);
    },
    [showSheet],
  );

  /**
   * The way in for anything that edits, so that the first edit always finds the source safe. A file
   * that has changed refuses the edit.
   */
  const runCommand = useCallback(
    (command: Command): Promise<Applied> => {
      const job = commands.current
        .catch(() => undefined)
        .then(async () => {
          const engine = clientRef.current;
          const file = fileRef.current;
          if (!engine || !file) throw new Error('Open a workbook first.');
          if (!protectedRef.current) {
            const copy = await platform.workingCopy(file);
            await engine.protectSource(copy?.source);
            workingRef.current = copy;
            protectedRef.current = true;
          }
          const applied = await engine.command(command);
          const changed = await engine.hasChanges();
          editedRef.current = changed;
          setCanSave(changed || Boolean(file.isNew));
          setUndoLabels({ undo: applied.undo, redo: applied.redo });
          if (applied.lost) setNotice('That change was too big to keep for undo.');
          const bytes = await engine.memoryBytes();
          if (bytes >= MEMORY_WARNING_BYTES) setNotice(memoryNotice(bytes));
          // The tabs have to be right before anything goes to a sheet.
          let summary: BookSummary | null = null;
          if (
            command.kind === 'insertSheet' ||
            command.kind === 'deleteSheet' ||
            command.kind === 'renameSheet' ||
            command.kind === 'sheetState' ||
            command.kind === 'protectSheet' ||
            command.kind === 'unprotectSheet' ||
            command.kind === 'protectWorkbook' ||
            command.kind === 'unprotectWorkbook' ||
            command.kind === 'undo' ||
            command.kind === 'redo'
          ) {
            summary = await engine.bookSummary();
            applySummary(summary);
          }
          const shows = (index: number) => {
            const entry = summary?.sheets[index];
            return Boolean(entry && !entry.deleted && entry.state === 'visible');
          };
          if (summary && command.kind === 'sheetState') {
            // The engine says which sheet shows now, as Excel picks it.
            await goToSheet(summary.active);
          } else if (
            summary &&
            (command.kind === 'undo' || command.kind === 'redo') &&
            summary.active !== sheetRef.current &&
            shows(summary.active)
          ) {
            // Undoing or redoing Hide or Unhide goes where the engine says, as the step did.
            await goToSheet(summary.active);
          } else if (summary && command.kind === 'insertSheet') {
            const at = Math.min(command.position, summary.order.length - 1);
            await goToSheet(summary.order[at] ?? 0);
          } else if (summary?.sheets[sheetRef.current]?.deleted) {
            const next = summary.order.find((index) => {
              const entry = summary?.sheets[index];
              return entry && !entry.deleted && entry.state === 'visible';
            });
            if (next !== undefined) await goToSheet(next);
          }
          // Typing has already moved on from the cell, and the next key may be on its way.
          if (applied.selection && command.kind !== 'input') {
            const { sheet: target, ...area } = applied.selection;
            await goToSheet(target);
            const grid = controller.current;
            if (grid?.currentSheet() === target && !editingRef.current?.session.snapshot().active) {
              grid.selectArea(fromArea(area));
            }
          }
          refreshCell();
          erpSyncRef.current();
          // Circular References, after anything that can change what formulas read.
          if (FORMULA_CHANGES.has(command.kind)) {
            const loops = (await engine.query({ kind: 'circular' })) ?? [];
            const known = circularRef.current;
            circularRef.current = loops;
            setCircular(loops);
            // A formula typed that closes a loop gets Excel's warning, once.
            const typed =
              command.kind === 'input' &&
              command.text.startsWith('=') &&
              loops.some(
                ([s, r, c]) => s === command.sheet && r === command.row && c === command.col,
              ) &&
              !known.some(
                ([s, r, c]) => s === command.sheet && r === command.row && c === command.col,
              );
            if (typed) protectMessageRef.current(CIRCULAR_WARNING);
          }
          return applied;
        });
      commands.current = job;
      return job;
    },
    [applySummary, goToSheet, platform, refreshCell],
  );

  const insertSheet = useCallback(
    (after: boolean) => {
      const order = bookRef.current?.order ?? [];
      const at = order.indexOf(sheetRef.current);
      setNotice(null);
      runCommand({
        kind: 'insertSheet',
        position: at < 0 ? order.length : at + (after ? 1 : 0),
        rightToLeft: readDefaultDirection() === 'rtl',
      }).catch((error) => refusedRef.current(error));
    },
    [runCommand],
  );

  const calculate = useCallback(
    (kind: 'now' | 'sheet' | 'all') => {
      const engine = clientRef.current;
      if (!engine) return;
      const done =
        kind === 'sheet'
          ? engine.calculateSheet(sheetRef.current)
          : engine.calculateNow(kind === 'all');
      done.catch(report);
    },
    [report],
  );

  const editingRef = useRef<SheetEditing | null>(null);
  const sheetFormatRef = useRef<SheetFormat | null>(null);
  const throughRef = useRef<EditingHost | null>(null);
  const formatActionRef = useRef<(action: GridAction) => void>(() => undefined);
  // Generate GetPivotData, which the pivot tools keep, and what a double-click on a value does.
  const pivotGenerateRef = useRef(true);
  const doubleCellRef = useRef<(row: number, col: number) => boolean>(() => false);
  const host = useRef<EditingHost | null>(null);
  host.current = {
    engine: () => clientRef.current,
    grid: () => controller.current,
    run: runCommand,
    settled: () => commands.current.catch(() => undefined),
    canEdit: () => {
      const shown = bookRef.current?.sheets[sheetRef.current];
      return (
        loadedRef.current.has(sheetRef.current) && shown?.kind === 'worksheet' && !savingRef.current
      );
    },
    mayEdit: (index) => {
      // ERP data's read-only cells refuse at the first key too, with the guard's own message.
      const active = selectionRef.current;
      if (active && erpReadOnlyAtRef.current(index, active.row + 1, active.col + 1)) {
        protectMessageRef.current(READ_ONLY_MESSAGE);
        return false;
      }
      // A locked cell on a protected sheet refuses at the first key, as in Excel. The active
      // cell's format is what the ribbon shows; the engine refuses anything else.
      const guarded = bookRef.current?.sheets[index]?.protection;
      if (!guarded || formatRef.current?.locked === false) return true;
      protectMessageRef.current(PROTECTED_MESSAGE);
      return false;
    },
    activeText: () => cellRef.current.edit,
    sheetName: (index) => bookRef.current?.sheets[index]?.name ?? '',
    sheetIndex: (name) => {
      const wanted = name.toLocaleLowerCase();
      const found = bookRef.current?.sheets.findIndex(
        (entry) => !entry.deleted && entry.name.toLocaleLowerCase() === wanted,
      );
      return found === undefined || found < 0 ? null : found;
    },
    showSheet: goToSheet,
    nextSheet: (step) => {
      const summary = bookRef.current;
      if (!summary) return null;
      const tabs = summary.order.filter((index) => {
        const entry = summary.sheets[index];
        return entry && !entry.deleted && entry.state === 'visible';
      });
      return tabs[tabs.indexOf(sheetRef.current) + step] ?? null;
    },
    notice: (message) => dataNoticeRef.current(message),
    focusEditor: () => editorRef.current?.focus(),
    editorElement: () => editorRef.current?.element()?.parentElement ?? null,
    insertSheet: () => insertSheet(false),
    calculate,
    completionKey: (event) => completionRef.current?.key(event) ?? false,
    generatesPivotData: () => pivotGenerateRef.current,
    fitColumns: (index, first, last) => {
      const job = commands.current
        .catch(() => undefined)
        .then(async () => {
          const engine = clientRef.current;
          const file = fileRef.current;
          if (!engine || !file) return;
          if (!protectedRef.current) {
            const copy = await platform.workingCopy(file);
            await engine.protectSource(copy?.source);
            workingRef.current = copy;
            protectedRef.current = true;
          }
          const applied = await engine.autoFitColumns(index, first, last);
          if (!applied) return;
          const changed = await engine.hasChanges();
          editedRef.current = changed;
          setCanSave(changed || Boolean(file.isNew));
          setUndoLabels({ undo: applied.undo, redo: applied.redo });
        });
      commands.current = job;
      job.catch((error) => setNotice(error instanceof Error ? error.message : String(error)));
    },
    formatAction: (action) => formatActionRef.current(action),
  };
  if (!editingRef.current) {
    const through: EditingHost = {
      engine: () => host.current!.engine(),
      grid: () => host.current!.grid(),
      run: (command) => host.current!.run(command),
      settled: () => host.current!.settled(),
      canEdit: () => host.current!.canEdit(),
      mayEdit: (sheet, row, col) => host.current!.mayEdit(sheet, row, col),
      activeText: () => host.current!.activeText(),
      sheetName: (index) => host.current!.sheetName(index),
      sheetIndex: (name) => host.current!.sheetIndex(name),
      showSheet: (index) => host.current!.showSheet(index),
      nextSheet: (step) => host.current!.nextSheet(step),
      notice: (message) => host.current!.notice(message),
      focusEditor: () => host.current!.focusEditor(),
      editorElement: () => host.current!.editorElement(),
      insertSheet: () => host.current!.insertSheet(),
      calculate: (kind) => host.current!.calculate(kind),
      completionKey: (event) => host.current!.completionKey(event),
      generatesPivotData: () => host.current!.generatesPivotData?.() ?? true,
      fitColumns: (index, first, last) => host.current!.fitColumns(index, first, last),
      formatAction: (action) => host.current!.formatAction(action),
    };
    editingRef.current = new SheetEditing(through);
    throughRef.current = through;
  }
  const editing = editingRef.current;
  const dataTools = useDataTools({
    engine: () => clientRef.current,
    grid: () => controller.current,
    sheet: () => sheetRef.current,
    selection: () => selectionRef.current,
    hasBook: Boolean(book),
    sheetNames: book?.sheets.map((entry) => entry.name) ?? [],
    run: runCommand,
    notice: (message) => {
      if (message && isErpRefusal(message)) protectMessageRef.current(message);
      else setNotice(message);
    },
    goTo: async (index, area) => {
      await goToSheet(index);
      controller.current?.selectArea(fromArea(area));
    },
    editor: () => ({
      snapshot: () => editing.session.snapshot(),
      cancel: () => editing.cancel(),
    }),
    extraButtons: () => pivotButtonsRef.current(),
    userName: () => noteAuthor(),
    cellFormat: format,
    formatStyle: (styleFormat, done) =>
      setDialog({ kind: 'styleFormat', format: styleFormat, done }),
    hoverDetails: (index, row, col) => erpHoverRef.current(index, row, col),
  });
  const pivotTools = usePivotTools({
    engine: () => clientRef.current,
    grid: () => controller.current,
    sheet: () => sheetRef.current,
    selection: () => selectionRef.current,
    hasBook: Boolean(book),
    sheetNames: book?.sheets.map((entry) => entry.name) ?? [],
    run: runCommand,
    notice: setNotice,
    goTo: async (index, row, col) => {
      let target = index;
      if (target < 0) {
        // A sheet the command just added: the last one.
        const summary = await clientRef.current?.bookSummary();
        if (summary) applySummary(summary);
        target = (summary?.sheets.length ?? 1) - 1;
      }
      await goToSheet(target);
      controller.current?.selectArea(fromArea({ top: row, left: col, bottom: row, right: col }));
    },
    placeButtons: () => dataTools.placeButtons(),
    fitColumns: (index, first, last) => throughRef.current?.fitColumns(index, first, last),
    showAdded: async (index, area) => {
      const summary = await clientRef.current?.bookSummary();
      if (summary) applySummary(summary);
      await goToSheet(index);
      if (area) controller.current?.selectArea(fromArea(area));
    },
  });
  pivotGenerateRef.current = pivotTools.state.generate;
  // ERP data, from a launch in Cash Gear ERP: staged, then bound into a new workbook.
  const erpTools = useErpTools({
    engine: () => clientRef.current,
    queue: (job) => {
      const run = commands.current
        .catch(() => undefined)
        .then(async () => {
          const engine = clientRef.current;
          if (!engine) throw new Error('The engine isn’t running.');
          return job(engine);
        });
      commands.current = run;
      return run;
    },
    openBlank: async (name, rightToLeft) => {
      const engine = clientRef.current;
      if (!engine) throw new Error('The engine isn’t running.');
      const loaded = new Promise<void>((resolve, reject) => {
        erpLoadRef.current = { resolve, reject };
      });
      const bytes = await engine.blankWorkbook(rightToLeft);
      await openBook({
        name,
        source: { kind: 'blob', blob: new Blob([bytes as BlobPart]) },
        isNew: true,
      });
      if (fileRef.current?.name !== name) {
        erpLoadRef.current = null;
        throw new Error('The new workbook didn’t open.');
      }
      await loaded;
    },
    closeBlank: () => {
      openToken.current += 1;
      fileRef.current = null;
      setBook(null);
      setFileName(null);
      setLoad({ phase: 'idle', done: 0, total: 0 });
      controller.current?.reset();
    },
    holdView: (held) => {
      const view = viewHeldRef.current;
      if (held) {
        viewHeldRef.current = { held: true, changed: false };
        return;
      }
      view.held = false;
      // What changed meanwhile is drawn at once, with the layout and summary afterBind reads.
      if (view.changed) controller.current?.invalidate([sheetRef.current], []);
      view.changed = false;
    },
    afterBind: async () => {
      const engine = clientRef.current;
      if (!engine) return;
      applySummary(await engine.bookSummary());
      await showSheet(engine, sheetRef.current);
      setUndoLabels({ undo: null, redo: null });
      refreshCell();
    },
    notice: setNotice,
    sheet,
    run: runCommand,
    selection: () => {
      const active = selectionRef.current;
      if (!active) return null;
      return { areas: [toArea(active.range)], row: active.row + 1, col: active.col + 1 };
    },
    select: (row, col) =>
      controller.current?.selectArea({
        top: row - 1,
        left: col - 1,
        bottom: row - 1,
        right: col - 1,
      }),
    recoverable: () => clientRef.current?.recoverable() ?? Promise.resolve([]),
    recover: async (entry) => {
      const loaded = new Promise<void>((resolve, reject) => {
        erpLoadRef.current = { resolve, reject };
      });
      await restoreRef.current(entry);
      await loaded;
    },
    apiBase: () => platform.erpApiBase?.() ?? '',
  });
  erpSyncRef.current = erpTools.sync;
  erpBindRef.current = erpTools.bindStaged;
  erpPrepareRef.current = erpTools.prepare;
  erpReadOnlyAtRef.current = erpTools.readOnlyAt;
  erpHoverRef.current = erpTools.hoverDetails;
  // A double-click on a pivot table's value shows its details, as in Excel.
  doubleCellRef.current = (row, col) => {
    // Inside a pivot table, a double-click isn't an edit: on a value it shows the details.
    const area = pivotTools.state.info?.area;
    if (!area || row + 1 < area.top || row + 1 > area.bottom) return false;
    if (col + 1 < area.left || col + 1 > area.right) return false;
    pivotTools.actions.showDetails(row, col);
    return true;
  };
  pivotRefreshRef.current = pivotTools.refresh;
  pivotButtonsRef.current = pivotTools.buttons;
  const chartScreen = useMemo(
    () => ({
      dark: palette.dark,
      background: parseInt(palette.background.slice(1), 16) || 0xffffff,
      text: parseInt(palette.text.slice(1), 16) || 0,
    }),
    [palette],
  );
  const chartTools = useChartTools({
    engine: () => clientRef.current,
    grid: () => controller.current,
    sheet: () => sheetRef.current,
    selection: () => selectionRef.current,
    hasBook: Boolean(book),
    worksheets:
      book?.sheets
        .map((entry, index) => ({ index, name: entry.name, entry }))
        .filter(({ entry }) => entry.kind === 'worksheet' && !entry.deleted)
        .map(({ index, name }) => ({ index, name })) ?? [],
    run: runCommand,
    notice: setNotice,
    screen: chartScreen,
    themeColors: themeColors(lists),
    menu: (x, y) => void contextMenu.current?.showAt(x, y, 'drawing'),
  });
  const printTools = usePrintTools({
    engine: () => clientRef.current,
    grid: () => controller.current,
    sheet: () => sheetRef.current,
    selection: () => selectionRef.current,
    book: () => bookRef.current,
    hasBook: Boolean(book),
    file: () => {
      const file = fileRef.current;
      const path = typeof file?.recent === 'string' ? file.recent : (file?.name ?? '');
      return { name: file?.name ?? 'Book1.xlsx', path };
    },
    platform,
    fontFamily: () => palette.fontFamily,
    locale: navigator.language || 'en-US',
    showGrid: flags.showGrid,
    showHeadings: flags.showHeadings,
    rightToLeft: flags.rightToLeft,
    setView: (next) => send({ kind: 'viewFlags', sheet: sheetRef.current, ...next }),
    run: runCommand,
    notice: setNotice,
  });
  printRefreshRef.current = () => void printTools.refresh();
  printSelectionRef.current = printTools.selectionChanged;
  printPdfRef.current = printTools.pdfFor;
  const openPrint = printTools.openPrint;
  chartDrawingsRef.current = chartTools.setDrawings;
  chartRefreshRef.current = chartTools.refresh;
  chartDeselectRef.current = chartTools.deselect;
  dataRefreshRef.current = dataTools.refresh;
  dataNoticeRef.current = dataTools.notice;
  if (!sheetFormatRef.current) {
    sheetFormatRef.current = new SheetFormat(throughRef.current!, editing, () => formatRef.current);
  }
  const sheetFormat = sheetFormatRef.current;
  const painterOn = useSyncExternalStore(sheetFormat.subscribe, sheetFormat.painterOn);
  // Only what the status bar says, so that a keystroke redraws the two text boxes and not this.
  const editMode = useSyncExternalStore(editing.session.subscribe, () => {
    const state = editing.session.snapshot();
    if (!state.active) return null;
    if (state.picked) return 'Point';
    return state.mode === 'enter' ? 'Enter' : 'Edit';
  });

  // What changed comes back from the engine as it calculates, for as long as this engine lasts.
  useEffect(() => {
    if (!client || !engineReady) return;
    const onChanges = (changes: WorkbookChanges) => {
      if (viewHeldRef.current.held) {
        viewHeldRef.current.changed = true;
        return;
      }
      controller.current?.invalidate(changes.sheets, changes.tiles);
      // A chart can read any sheet, so any change may be one it shows.
      chartRefreshRef.current?.();
      pivotRefreshRef.current();
      if (changes.layouts.includes(sheetRef.current) || changes.sheets.includes(sheetRef.current)) {
        void showSheet(client, sheetRef.current).catch(report);
      }
      if (changes.book) {
        client.bookSummary().then(applySummary).catch(report);
      }
    };
    client
      .watchChanges(onChanges, (status) => {
        setCalc(status);
        if (status.failure) report(new Error(status.failure));
      })
      .catch(report);
    client
      .recoverable()
      .then(setRecoverable)
      .catch((error) => console.warn('Could not look for recovered edits:', error));
  }, [applySummary, client, engineReady, report, showSheet]);

  // What Cash Gear ERP's button opened this tab with, once the engine can stage it.
  const erpLaunchRef = useRef(erpTools.launch);
  erpLaunchRef.current = erpTools.launch;
  useEffect(() => {
    if (!engineReady || !platform.erpLaunch) return;
    const launch = platform.erpLaunch();
    if (launch) {
      hadLaunchRef.current = true;
      erpLaunchRef.current(launch);
    }
  }, [engineReady, platform]);

  // The workbook Explorer started this window with: a double-click on the desktop, or Open with
  // for the installed web app.
  useEffect(() => {
    if (!engineReady || !platform.launchFile) return;
    platform
      .launchFile()
      .then((file) => file && openFile(file))
      .catch(report);
  }, [engineReady, openFile, platform, report]);

  // On the web, closing a tab with unsaved edits asks first.
  useEffect(() => {
    if (platform.kind !== 'web') return;
    const onLeave = (event: BeforeUnloadEvent) => {
      if (editedRef.current) event.preventDefault();
    };
    window.addEventListener('beforeunload', onLeave);
    return () => window.removeEventListener('beforeunload', onLeave);
  }, [platform]);

  // On the desktop, closing the window with unsaved edits asks the same.
  useEffect(
    () =>
      platform.onCloseRequested?.(async () => {
        await commands.current.catch(() => undefined);
        if (!editedRef.current) return true;
        return new Promise<boolean>((answer) => setDialog({ kind: 'close', answer }));
      }),
    [platform],
  );

  // Only a paste event brings the clipboard without a permission prompt.
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const grid = controller.current;
      const active = document.activeElement;
      if (
        !grid ||
        !event.clipboardData ||
        (active && active !== document.body && active.tagName !== 'CANVAS')
      ) {
        return;
      }
      event.preventDefault();
      editing.paste(
        event.clipboardData.getData('text/plain'),
        event.clipboardData.getData('text/html') || undefined,
      );
    };
    document.addEventListener('paste', onPaste);
    return () => document.removeEventListener('paste', onPaste);
  }, [editing]);

  const restoreRecovered = useCallback(
    async (entry: Recoverable) => {
      setRecoverable((list) => list.filter((other) => other.id !== entry.id));
      const dot = entry.name.lastIndexOf('.');
      const base = dot > 0 ? entry.name.slice(0, dot) : entry.name || 'Book';
      const extension = dot > 0 ? entry.name.slice(dot) : '.xlsx';
      await openBook(
        {
          name: `${base} (recovered)${extension}`,
          source: { kind: 'blob', blob: new Blob([]) },
          isNew: true,
        },
        entry.id,
      );
    },
    [openBook],
  );

  restoreRef.current = restoreRecovered;

  /**
   * A bound workbook's draft offered at start: it opens disconnected and waits for a launch from
   * Cash Gear ERP. One whose ERP connection can't be replayed says so, and a new workbook opens.
   */
  const erpRecoveredRef = useRef(erpTools.recovered);
  erpRecoveredRef.current = erpTools.recovered;
  const restoreErp = useCallback(
    async (entry: Recoverable) => {
      await restoreRecovered(entry);
      if (await erpRecoveredRef.current()) return;
      const texts = erpTools.textsFor(entry.erp?.language);
      await newWorkbook();
      protectMessageRef.current(
        texts.recoverFailedText(entry.erp?.labelEn.toLowerCase() ?? 'records'),
      );
    },
    // `erpTools.textsFor` is a module function.
    [newWorkbook, restoreRecovered],
  );

  const discardRecovered = useCallback(
    (entry: Recoverable) => {
      setRecoverable((list) => list.filter((other) => other.id !== entry.id));
      clientRef.current?.discardRecovered(entry.id).catch(report);
    },
    [report],
  );

  const exportCsv = useCallback(async () => {
    const engine = clientRef.current;
    const file = fileRef.current;
    if (!engine || !file) return;
    try {
      const name = file.name.replace(/\.[^.]*$/, '') + '.csv';
      const target = await platform.pickSaveTarget(name, 'csv');
      if (!target) return;
      await commands.current.catch(() => undefined);
      const text = await engine.csvText(sheetRef.current);
      await target.write(new Blob([text], { type: 'text/csv' }));
    } catch (error) {
      report(error);
    }
  }, [platform, report]);

  const pick = useCallback(async () => {
    try {
      const file = await platform.pickFile();
      if (file) await openFile(file);
    } catch (error) {
      report(error);
    }
  }, [openFile, platform, report]);

  const openFromRecent = useCallback(
    async (id: string) => {
      try {
        const file = await platform.openRecent(id);
        if (file) await openFile(file);
        else setProblem('That file is no longer available.');
        refreshRecent();
      } catch (error) {
        report(error);
      }
    },
    [openFile, platform, refreshRecent, report],
  );

  // Hooks for tests, which can't drive a native dialog. They add no ability a page lacks.
  useEffect(() => {
    const hooks = window as Window & {
      cgsTest?: {
        open: (file: OpenedFile) => Promise<void>;
        grid: () => GridController | null;
        cellText: (row: number, col: number) => Promise<string>;
        loadPhase: () => LoadState['phase'];
        memoryBytes: () => Promise<number>;
        /** What the ERP binding keeps in memory (plan B.6). */
        erpMemory: () => Promise<{
          editableValues: number;
          originalsBytes: number;
          bindingBytes: number;
        } | null>;
        calculateNow: (all: boolean) => Promise<void>;
        calculating: () => boolean;
        edit: (
          sheet: number,
          row: number,
          col: number,
          content: { number?: number; text?: string; formula?: string },
        ) => Promise<void>;
        command: (command: Command) => Promise<Applied>;
        drawings: () => Promise<DrawingBox[]>;
        chart: (index: number) => Promise<ChartView | null>;
        settled: () => Promise<unknown>;
        hasChanges: () => Promise<boolean>;
        writeTo: (target: SaveTarget) => Promise<void>;
        lastSave: () => SaveReport | null;
        problem: () => string | null;
        cellFormat: (row: number, col: number) => Promise<CellFormat | null>;
        saveBytes: () => Promise<Uint8Array>;
        /** Milliseconds for one cell read, tile read or drawn tile, for the save bench. */
        probe: (kind: 'cell' | 'block' | 'tile', row: number, col: number) => Promise<number>;
        pdf: (choice?: Partial<PrintChoice>) => Promise<{ bytes: Uint8Array; pages: number }>;
        /** The ERP binding, as the engine holds it. */
        binding: () => Promise<ErpBinding | null>;
        /** The ERP binding's changeset, as Commit sends it. */
        erpChanges: () => Promise<ErpChangeset | null>;
        /** Any question for the engine, as the shell asks it. */
        query: EngineClient['query'];
        /** An area's shown text, as Copy gives it: tabs between cells, lines between rows. */
        shownText: (area: CellArea) => Promise<string>;
        /**
         * The bind bench: stages Query pages' bytes and binds them as a launch does, without the
         * network. Times each step in milliseconds.
         */
        bindPages: (
          schema: string,
          pages: Uint8Array[],
          session: string,
        ) => Promise<{
          /** Opening the new workbook, which a launch does while the records download. */
          openMs: number;
          /** When the records were handed over: the response, as plan B.6 measures from it. */
          startedAt: number;
          stageMs: number;
          bindMs: number;
          records: number;
        }>;
      };
    };
    hooks.cgsTest = {
      open: openFile,
      grid: () => controller.current,
      cellText: async (row, col) => {
        const engine = clientRef.current;
        if (!engine) return '';
        return (await engine.cellText(sheetRef.current, row, col)).display;
      },
      loadPhase: () => loadRef.current,
      memoryBytes: () => clientRef.current?.memoryBytes() ?? Promise.resolve(0),
      erpMemory: () => clientRef.current?.erpMemory() ?? Promise.resolve(null),
      calculateNow: (all) => clientRef.current?.calculateNow(all) ?? Promise.resolve(),
      calculating: () => calcRef.current.calculating,
      edit: async (sheet, row, col, content) => {
        let text = '';
        if (content.formula !== undefined) text = '=' + content.formula;
        else if (content.number !== undefined) text = String(content.number);
        else if (content.text !== undefined) text = "'" + content.text;
        await runCommand({ kind: 'input', sheet, row, col, text });
      },
      command: runCommand,
      drawings: async () => (await clientRef.current?.drawings(sheetRef.current)) ?? [],
      chart: async (index) =>
        (await clientRef.current?.chart(sheetRef.current, index, {
          dark: false,
          background: 0xffffff,
          text: 0,
        })) ?? null,
      settled: () => commands.current.catch(() => undefined),
      hasChanges: () => clientRef.current?.hasChanges() ?? Promise.resolve(false),
      writeTo,
      lastSave: () => lastSaveRef.current,
      problem: () => problemRef.current,
      cellFormat: async (row, col) =>
        (await clientRef.current?.cellFormat(sheetRef.current, row, col)) ?? null,
      saveBytes: async () => {
        const saved = await clientRef.current?.save();
        return new Uint8Array((await saved?.blob.arrayBuffer()) ?? new ArrayBuffer(0));
      },
      probe: async (kind, row, col) => {
        const engine = clientRef.current;
        const grid = controller.current;
        if (!engine || !grid) return NaN;
        const started = performance.now();
        if (kind === 'cell') await engine.cellText(sheetRef.current, row, col);
        else if (kind === 'block') await engine.viewBlock(sheetRef.current, row - 1, col - 1);
        else (await engine.drawTile(grid.tileRequest(row - 1, col - 1))).bitmap.close();
        return performance.now() - started;
      },
      pdf: (choice) => printPdfRef.current({ what: 'active', ignorePrintArea: false, ...choice }),
      binding: async () => (await clientRef.current?.binding()) ?? null,
      erpChanges: async () => (await clientRef.current?.query({ kind: 'erpChanges' })) ?? null,
      query: (async (question: Question) => {
        const engine = clientRef.current;
        if (!engine) throw new Error('The engine isn’t running.');
        return engine.query(question);
      }) as EngineClient['query'],
      shownText: async (area) =>
        (await clientRef.current?.copyText(sheetRef.current, area))?.text ?? '',
      bindPages: async (schema, pages, session) => {
        const engine = clientRef.current;
        if (!engine) throw new Error('The engine isn’t running.');
        const info = JSON.parse(session) as SessionInfo;
        const parsed = JSON.parse(schema) as EntitySchema;
        const label = parsed.label;
        // As a launch does: the schema comes first, and the new workbook opens, and the tile
        // worker readies its fonts, while the records download, which here is before they're
        // handed over.
        const opening = performance.now();
        await engine.erpBegin(schema);
        await erpPrepareRef.current(info, parsed);
        // The records' download, kept short: a Query page of 1,000 records takes longer than this
        // on any network, and the tenant run measures it (Phase 19).
        await new Promise((done) => setTimeout(done, 150));
        const started = performance.now();
        for (const page of pages) await engine.erpPage(page);
        const staged = performance.now();
        const bound = await erpBindRef.current(info, label[info.language], Promise.resolve());
        return {
          openMs: started - opening,
          startedAt: started,
          stageMs: staged - started,
          bindMs: performance.now() - staged,
          records: bound.records,
        };
      },
    };
  }, [openFile, runCommand, writeTo]);

  const chooseSheet = useCallback(
    (key: string | null) => {
      if (key === null) return;
      const index = Number(key);
      editingRef.current?.beforeSheetChange();
      sheetRef.current = index;
      setSheet(index);
      const engine = clientRef.current;
      if (!engine) return;
      setLoadedSheets((sheets) => {
        if (sheets.has(index)) void showSheet(engine, index).catch(report);
        else void engine.prioritize(index).catch(report);
        return sheets;
      });
    },
    [report, showSheet],
  );

  const events = useMemo<GridEvents>(
    () => ({
      onSelection: (selection: GridSelection) => {
        const before = selectionRef.current;
        selectionRef.current = selection;
        keepSelectableRef.current(before, selection);
        // A sheet shown again says its selection again; only a real move leaves a chosen chart.
        const moved =
          !before ||
          before.row !== selection.row ||
          before.col !== selection.col ||
          JSON.stringify(before.range) !== JSON.stringify(selection.range);
        if (moved) chartDeselectRef.current?.();
        printSelectionRef.current(selection);
        refreshCell();
      },
      onZoom: (next: number) => {
        setZoom(next);
        const shown = shownSheet.current;
        if (shown >= 0) viewCommand({ kind: 'zoom', sheet: shown, zoom: next });
      },
      onError: report,
      onType: editing.type,
      onAction: editing.action,
      onJump: editing.jump,
      onEditClick: editing.editClick,
      onEditDrag: editing.editDrag,
      onDoubleCell: (row, col) => doubleCellRef.current(row, col),
      onFill: editing.fill,
      onViewport: (index, range) => clientRef.current?.setViewport(index, toArea(range)),
      onSelectEnd: sheetFormat.selectionDone,
      onResize: (axis, spans, px) => {
        for (const [first, last] of spans) {
          void runCommand({
            kind: 'lines',
            sheet: sheetRef.current,
            axis,
            first,
            last,
            size: px,
          }).catch((error) => setNotice(error instanceof Error ? error.message : String(error)));
        }
      },
      onAutoFit: (axis, spans) => {
        for (const span of spans) sheetFormat.autoFit(axis, span);
      },
      onResizeHint: setResizeHint,
      onContextMenu: (x, y, region) => void contextMenu.current?.showAt(x, y, region),
      onCellButton: (button: CellButton, box: ClientBox) =>
        button.kind === 'pivot'
          ? pivotTools.onCellButton(button, box)
          : dataTools.onCellButton(button, box),
      onHover: dataTools.onHover,
      onCellClick: (row: number, col: number) => {
        chartDeselectRef.current?.();
        dataTools.onCellClick(row, col);
      },
    }),
    [
      dataTools.onCellButton,
      pivotTools.onCellButton,
      dataTools.onCellClick,
      dataTools.onHover,
      editing,
      refreshCell,
      report,
      runCommand,
      sheetFormat,
    ],
  );

  const handController = useCallback((next: GridController | null) => {
    controller.current = next;
  }, []);

  const onDrop = useCallback(
    async (event: DragEvent) => {
      event.preventDefault();
      setDragOver(false);
      try {
        const file = await platform.fromDrop(event.dataTransfer);
        if (file) await openFile(file);
      } catch (error) {
        report(error);
      }
    },
    [openFile, platform, report],
  );

  // Sheets still being read hold on to the source, which a save is about to replace.
  const saveReady = Boolean(book) && !saving && load.phase !== 'reading';
  // What protection refuses, Excel says in a message box; anything else goes in the status bar.
  refusedRef.current = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    if (
      message === PROTECTED_MESSAGE ||
      message.startsWith('Workbook is protected') ||
      isErpRefusal(message)
    ) {
      protectMessageRef.current(message);
    } else {
      setNotice(message);
    }
  };
  const send = useCallback(
    (command: Command) => {
      setNotice(null);
      runCommand(command).catch((error) => refusedRef.current(error));
    },
    [runCommand],
  );
  const focusGrid = useCallback(() => controller.current?.focus(), []);
  const errorTools = useErrorTools({
    engine: () => clientRef.current,
    grid: () => controller.current,
    sheet: () => sheetRef.current,
    selection: () => selectionRef.current,
    run: runCommand,
    notice: setNotice,
    editInBar: () => void editingRef.current?.edit('bar'),
    goTo: (row, col) =>
      controller.current?.selectArea({
        top: row - 1,
        left: col - 1,
        bottom: row - 1,
        right: col - 1,
      }),
    openOptions: () =>
      setDialog({
        kind: 'options',
        direction: readDefaultDirection(),
        userName: readUserName(),
        errors: readErrorSettings(),
      }),
  });
  errorRefreshRef.current = errorTools.refresh;
  /** "Sheet2!A1", or "A1" on the sheet showing, as Excel's status bar names a loop's cell. */
  const circularLabel = ([index, row, col]: [number, number, number]) => {
    const name = cellName(row - 1, col - 1);
    return index === sheetRef.current ? name : `${bookRef.current?.sheets[index]?.name}!${name}`;
  };
  const protectTools = useProtectTools({
    book,
    sheet,
    hasBook: Boolean(book),
    run: runCommand,
    focusGrid,
  });
  // The engine's refusals on ERP data come in English, and show in the session's language.
  const erpLocalizeRef = useRef(erpTools.localize);
  erpLocalizeRef.current = erpTools.localize;
  protectMessageRef.current = (message: string) =>
    protectTools.showMessage(isErpRefusal(message) ? erpLocalizeRef.current(message) : message);
  const hideSheet = useCallback(
    () => send({ kind: 'sheetState', sheet: sheetRef.current, hidden: true }),
    [send],
  );
  /** Hidden sheets Unhide can bring back, in tab order. Very hidden ones aren't among them. */
  const hiddenSheets = useMemo(
    () =>
      (book?.order ?? []).flatMap((index) => {
        const entry = book?.sheets[index];
        return entry && !entry.deleted && entry.state === 'hidden'
          ? [{ index, name: entry.name }]
          : [];
      }),
    [book],
  );
  const openRename = useCallback(() => {
    const entry = bookRef.current?.sheets[sheetRef.current];
    if (entry) setRenaming({ sheet: sheetRef.current, name: entry.name });
  }, []);

  const menu = useMemo<CgMenuItem[]>(
    () => [
      {
        key: 'file',
        text: 'File',
        children: [
          {
            key: 'new',
            text: 'New',
            shortcut: 'Ctrl+Alt+N',
            disabled: saving,
            onActivate: () => void newWorkbook(),
          },
          { key: 'open', text: 'Open…', shortcut: 'Ctrl+O', onActivate: () => void pick() },
          {
            key: 'save',
            text: 'Save',
            shortcut: 'Ctrl+S',
            disabled: !saveReady || !canSave,
            onActivate: () => void save(),
          },
          {
            key: 'save-as',
            text: 'Save As…',
            shortcut: 'Ctrl+Shift+S',
            disabled: !saveReady,
            onActivate: () => void saveAs(),
          },
          {
            key: 'print',
            text: 'Print…',
            shortcut: 'Ctrl+P',
            disabled: !book,
            onActivate: () => openPrint('print'),
          },
          {
            key: 'export-pdf',
            text: 'Export as PDF…',
            disabled: !book,
            onActivate: () => openPrint('pdf'),
          },
          {
            key: 'export-csv',
            text: 'Export sheet as CSV…',
            disabled: !saveReady,
            onActivate: () => void exportCsv(),
          },
          {
            key: 'recent',
            text: 'Recent',
            disabled: recent.length === 0,
            children: recent.map((file) => ({
              key: 'recent-' + file.id,
              text: file.name,
              tooltip: file.detail,
              onActivate: () => void openFromRecent(file.id),
            })),
          },
          {
            key: 'options',
            text: 'Options…',
            onActivate: () =>
              setDialog({
                kind: 'options',
                direction: readDefaultDirection(),
                userName: readUserName(),
                errors: readErrorSettings(),
              }),
          },
          {
            key: 'about',
            text: 'About CashGear Sheet',
            onActivate: () => setRelease('about'),
          },
        ],
      },
    ],
    [
      book,
      canSave,
      exportCsv,
      newWorkbook,
      openPrint,
      openFromRecent,
      pick,
      recent,
      save,
      saveAs,
      saveReady,
      saving,
    ],
  );

  // A workbook's theme and its own number formats, for the pickers and the dialog.
  useEffect(() => {
    const engine = clientRef.current;
    if (!engine || !book) {
      setLists(null);
      return;
    }
    let live = true;
    engine
      .formatLists()
      .then((next) => {
        if (live) setLists(next);
      })
      .catch(report);
    return () => {
      live = false;
    };
  }, [book, load.phase, report]);

  const openShift = useCallback(
    (remove: boolean) => {
      const whole = sheetFormat.wholeLines();
      if (whole) {
        editing.axis(whole, remove);
        return;
      }
      setDialog({ kind: 'shift', remove, choice: remove ? 'side' : 'down' });
    },
    [editing, sheetFormat],
  );

  const openSize = useCallback((axis: 'row' | 'col') => {
    setDialog({ kind: 'size', axis, value: '' });
  }, []);

  const goToBox = useCallback(() => {
    const box = document.querySelector<HTMLInputElement>('[data-testid="name-box"]');
    box?.focus();
    box?.select();
  }, []);

  const mergeCells = useCallback(
    (kind: 'center' | 'across' | 'cells') => {
      void sheetFormat
        .merge(
          kind,
          () =>
            new Promise<boolean>((answer) => {
              setDialog({ kind: 'merge', answer });
            }),
        )
        .catch(report);
    },
    [report, sheetFormat],
  );

  const clearAll = useCallback(() => {
    const selection = controller.current?.selection();
    if (!selection) return;
    send({ kind: 'clear', sheet: selection.sheet, areas: selection.areas.map(toArea) });
    sheetFormat.format({ clear: true });
  }, [send, sheetFormat]);

  const clearContents = useCallback(() => {
    const selection = controller.current?.selection();
    if (!selection) return;
    send({ kind: 'clear', sheet: selection.sheet, areas: selection.areas.map(toArea) });
  }, [send]);

  const ribbonActions = useMemo<RibbonActions>(
    () => ({
      format: sheetFormat,
      editing,
      openFormat: (tab) => setDialog({ kind: 'format', tab }),
      openSize,
      openShift,
      openFind: (tab) => setDialog({ kind: 'find', tab }),
      focusGrid: () => controller.current?.focus(),
      goTo: goToBox,
      renameSheet: openRename,
      insertSheet: () => insertSheet(false),
      deleteSheet: () => send({ kind: 'deleteSheet', sheet: sheetRef.current }),
      hideSheet,
      unhideSheet: () => setDialog({ kind: 'unhide' }),
      canUnhide: hiddenSheets.length > 0,
      calculate,
      setManual: (manual) => void clientRef.current?.setManualCalc(manual).catch(report),
      setView: (next) => send({ kind: 'viewFlags', sheet: sheetRef.current, ...next }),
      setFormulaBar: setShowFormulaBar,
      zoom100: () => controller.current?.setZoom(100),
      zoomToSelection: () => controller.current?.zoomToSelection(),
      setFontColor,
      setFillColor,
      setBorderStyle,
      setBorderColor,
      applyBorders: (details) => {
        setBorderPreset(details);
        sheetFormat.format({ border: bordersFromPicker(details, borderStyle) });
      },
      mergeCells,
      clearAll,
      clearContents,
      protectSheet: protectTools.toggleSheet,
      lockCell: () => sheetFormat.format({ locked: !(formatRef.current?.locked ?? true) }),
      data: dataTools.actions,
    }),
    [
      dataTools.actions,
      borderStyle,
      calculate,
      clearAll,
      clearContents,
      editing,
      goToBox,
      hiddenSheets.length,
      hideSheet,
      insertSheet,
      mergeCells,
      openRename,
      openShift,
      openSize,
      report,
      send,
      sheetFormat,
    ],
  );

  const builtTabs = useMemo(
    () =>
      buildRibbonTabs(
        {
          hasBook: Boolean(book),
          format,
          lists,
          undo: undoLabels.undo,
          redo: undoLabels.redo,
          canSave,
          painterOn,
          manual: calc.manual,
          frozen: flags.frozen,
          showGrid: flags.showGrid,
          showHeadings: flags.showHeadings,
          showFormulaBar,
          fontColor,
          fillColor,
          borderStyle,
          borderColor,
          borderPreset,
          data: dataTools.state,
          protection: protectTools.protection,
        },
        ribbonActions,
      ),
    [
      dataTools.state,
      protectTools.protection,
      book,
      borderColor,
      borderPreset,
      borderStyle,
      calc.manual,
      canSave,
      fillColor,
      flags,
      fontColor,
      format,
      lists,
      painterOn,
      ribbonActions,
      showFormulaBar,
      undoLabels,
    ],
  );
  const ribbonTabs = useMemo(
    () =>
      [
        ...builtTabs.map((tab) =>
          tab.key === 'insert'
            ? {
                ...tab,
                groups: [
                  ...tab.groups.map((group) =>
                    group.key === 'tables'
                      ? {
                          ...group,
                          items: [
                            pivotInsertItem(pivotTools.state, pivotTools.actions),
                            ...group.items,
                          ],
                        }
                      : group,
                  ),
                  ...chartInsertGroups(chartTools.state, chartTools.actions),
                ],
              }
            : tab,
        ),
        ...pivotTabs(pivotTools.state, pivotTools.actions),
        ...chartTabs(chartTools.state, chartTools.actions),
      ]
        .map((tab) =>
          tab.key === 'review'
            ? { ...tab, groups: [...tab.groups, protectTools.group] }
            : tab.key === 'formulas'
              ? {
                  ...tab,
                  groups: [...tab.groups, errorTools.group],
                }
              : tab,
        )
        .flatMap((tab) =>
          // Page Layout comes after Insert, as in Excel, and ERP Data after Table Design.
          tab.key === 'insert'
            ? [tab, pageLayoutTab(printTools.state, printTools.actions)]
            : tab.key === 'table-design'
              ? [tab, ...erpTools.tabs]
              : [tab],
        ),
    [
      builtTabs,
      chartTools.state,
      chartTools.actions,
      pivotTools.state,
      pivotTools.actions,
      printTools.state,
      printTools.actions,
      protectTools.group,
      errorTools.group,
      erpTools.tabs,
    ],
  );
  // Excel's key tips on the tabs and the Home tab's commands.
  const tippedTabs = useMemo(() => withKeyTips(ribbonTabs), [ribbonTabs]);

  formatActionRef.current = (action) => {
    switch (action) {
      case 'cancel':
        sheetFormat.stopPainter();
        break;
      case 'bold':
        sheetFormat.toggleBold();
        break;
      case 'italic':
        sheetFormat.toggleItalic();
        break;
      case 'underline':
        sheetFormat.toggleUnderline('single');
        break;
      case 'strike':
        sheetFormat.toggleStrike();
        break;
      case 'formatCells':
        setDialog({ kind: 'format', tab: 'number' });
        break;
      case 'find':
        setDialog({ kind: 'find', tab: 'find' });
        break;
      case 'replace':
        setDialog({ kind: 'find', tab: 'replace' });
        break;
      case 'goTo':
        goToBox();
        break;
      case 'hyperlink':
        dataTools.actions.hyperlink();
        break;
      case 'note':
        dataTools.actions.editNote();
        break;
      case 'filter':
        dataTools.actions.toggleFilter();
        break;
      case 'hideRows':
        sheetFormat.setHidden('row', true);
        break;
      case 'hideCols':
        sheetFormat.setHidden('col', true);
        break;
      case 'unhideRows':
        sheetFormat.setHidden('row', false);
        break;
      case 'unhideCols':
        sheetFormat.setHidden('col', false);
        break;
      case 'formatGeneral':
        sheetFormat.format({ numberFormatId: 0 });
        break;
      case 'formatNumber':
        sheetFormat.format({ numberFormatId: 4 });
        break;
      case 'formatCurrency':
        sheetFormat.format({ numberFormatId: 8 });
        break;
      case 'formatPercent':
        sheetFormat.format({ numberFormatId: 9 });
        break;
      case 'formatScientific':
        sheetFormat.format({ numberFormatId: 11 });
        break;
      case 'formatDate':
        sheetFormat.format({ numberFormatId: 15 });
        break;
      case 'formatTime':
        sheetFormat.format({ numberFormatId: 18 });
        break;
      case 'borderOutline':
        sheetFormat.outline(true);
        break;
      case 'borderNone':
        sheetFormat.outline(false);
        break;
      case 'contextMenu':
        controller.current?.contextMenuAtActive();
        break;
      case 'insertCells':
        openShift(false);
        break;
      case 'deleteCells':
        openShift(true);
        break;
      default:
        break;
    }
  };

  const contextItems = useMemo<
    ReadonlyArray<CgContextMenuItem<GridRegion | 'tab' | 'drawing'>>
  >(() => {
    type Item = CgContextMenuItem<GridRegion | 'tab' | 'drawing'>;
    const only = (regions: (GridRegion | 'tab' | 'drawing')[]) => regions;
    void only;
    const items: Item[] = [
      {
        key: 'cut',
        text: 'Cut',
        icon: 'cut',
        shortcut: 'Ctrl+X',
        command: () => editing.copy(true),
      },
      {
        key: 'copy',
        text: 'Copy',
        icon: 'copy',
        shortcut: 'Ctrl+C',
        command: () => editing.copy(false),
      },
      {
        key: 'paste',
        text: 'Paste',
        icon: 'paste',
        shortcut: 'Ctrl+V',
        command: () => void editing.pasteFromMenu(),
      },
      {
        key: 'paste-special',
        text: 'Paste Special',
        children: [
          { key: 'paste-values', text: 'Values', command: () => editing.pasteMode('values') },
          { key: 'paste-formulas', text: 'Formulas', command: () => editing.pasteMode('formulas') },
          { key: 'paste-formats', text: 'Formatting', command: () => editing.pasteMode('formats') },
        ],
      },
      {
        key: 'insert',
        text: 'Insert…',
        beginGroup: true,
        command: () => openShift(false),
      },
      { key: 'delete', text: 'Delete…', command: () => openShift(true) },
      { key: 'clear', text: 'Clear Contents', command: clearContents },
      {
        key: 'note-edit',
        text: dataTools.state.hasNote ? 'Edit Note' : 'Insert Note',
        beginGroup: true,
        command: dataTools.actions.editNote,
      },
      ...(dataTools.state.hasNote
        ? [
            { key: 'note-delete', text: 'Delete Note', command: dataTools.actions.deleteNote },
            {
              key: 'note-show',
              text: dataTools.state.noteShown ? 'Hide Note' : 'Show/Hide Note',
              command: dataTools.actions.toggleNote,
            },
          ]
        : []),
      {
        key: 'link',
        text: dataTools.state.hasLink ? 'Edit Hyperlink…' : 'Link…',
        beginGroup: true,
        command: dataTools.actions.hyperlink,
      },
      ...(dataTools.state.hasLink
        ? [
            {
              key: 'link-remove',
              text: 'Remove Hyperlink',
              command: dataTools.actions.removeLink,
            },
          ]
        : []),
      {
        key: 'format-cells',
        text: 'Format Cells…',
        beginGroup: true,
        shortcut: 'Ctrl+1',
        command: () => setDialog({ kind: 'format', tab: 'number' }),
      },
      {
        key: 'merge',
        text: format?.merged ? 'Unmerge Cells' : 'Merge Cells',
        command: () => {
          if (format?.merged) sheetFormat.unmerge();
          else mergeCells('cells');
        },
      },
      {
        key: 'row-height',
        text: 'Row Height…',
        beginGroup: true,
        command: () => openSize('row'),
      },
      {
        key: 'hide-rows',
        text: 'Hide Rows',
        shortcut: 'Ctrl+9',
        command: () => sheetFormat.setHidden('row', true),
      },
      {
        key: 'unhide-rows',
        text: 'Unhide Rows',
        command: () => sheetFormat.setHidden('row', false),
      },
      {
        key: 'col-width',
        text: 'Column Width…',
        beginGroup: true,
        command: () => openSize('col'),
      },
      {
        key: 'hide-cols',
        text: 'Hide Columns',
        shortcut: 'Ctrl+0',
        command: () => sheetFormat.setHidden('col', true),
      },
      {
        key: 'unhide-cols',
        text: 'Unhide Columns',
        command: () => sheetFormat.setHidden('col', false),
      },
    ];
    return items;
  }, [
    clearContents,
    dataTools.actions,
    dataTools.state.hasLink,
    dataTools.state.hasNote,
    dataTools.state.noteShown,
    editing,
    format?.merged,
    mergeCells,
    openShift,
    openSize,
    sheetFormat,
  ]);

  const tabItems = useMemo<ReadonlyArray<CgContextMenuItem<GridRegion | 'tab' | 'drawing'>>>(
    () => [
      // A locked structure greys what would change the sheets, as in Excel.
      {
        key: 'tab-insert',
        text: 'Insert Sheet',
        disabled: protectTools.structureLocked,
        command: () => insertSheet(false),
      },
      {
        key: 'tab-delete',
        text: 'Delete Sheet',
        disabled: protectTools.structureLocked,
        command: () => send({ kind: 'deleteSheet', sheet: sheetRef.current }),
      },
      {
        key: 'tab-rename',
        text: 'Rename…',
        disabled: protectTools.structureLocked,
        command: openRename,
      },
      {
        key: 'tab-protect',
        text: protectTools.protection ? 'Unprotect Sheet…' : 'Protect Sheet…',
        beginGroup: true,
        command: protectTools.toggleSheet,
      },
      {
        key: 'tab-hide',
        text: 'Hide',
        beginGroup: true,
        disabled: protectTools.structureLocked,
        command: hideSheet,
      },
      {
        key: 'tab-unhide',
        text: 'Unhide…',
        disabled: hiddenSheets.length === 0 || protectTools.structureLocked,
        command: () => setDialog({ kind: 'unhide' }),
      },
    ],
    [
      hiddenSheets.length,
      hideSheet,
      insertSheet,
      openRename,
      protectTools.protection,
      protectTools.structureLocked,
      protectTools.toggleSheet,
      send,
    ],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      const key = event.key.toLowerCase();
      if (key === 'o') {
        event.preventDefault();
        void pick();
      } else if (key === 's') {
        // Always ours, so that the browser never offers to save the page.
        event.preventDefault();
        if (event.shiftKey) void saveAs();
        else void save();
      } else if (key === 'n' && event.altKey) {
        // Browsers keep Ctrl+N for a new window, and a page never sees it.
        event.preventDefault();
        void newWorkbook();
      } else if (key === 'p' && !event.altKey) {
        // Ours, so that the browser never prints the app's own window.
        event.preventDefault();
        if (!event.shiftKey) openPrint('print');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [newWorkbook, openPrint, pick, save, saveAs]);

  const hostText = native ? `Desktop ${native.version}` : 'Web';
  const sheets = book?.sheets ?? [];
  const shown = sheets[sheet];
  const sheetLoaded = loadedSheets.has(sheet);
  const percent = load.total ? Math.min(100, Math.round((load.done / load.total) * 100)) : 0;
  const reading = load.phase === 'reading';
  const statusText = problem
    ? problem
    : notice
      ? notice
      : !engineReady
        ? 'Starting the engine…'
        : reading
          ? `Reading ${percent}%`
          : saving
            ? saveProgress?.checking
              ? `Checking the saved file ${saveProgress.percent}%`
              : saveProgress
                ? `Saving ${saveProgress.percent}%`
                : 'Saving…'
            : (editMode ?? 'Ready');
  // The same list until a tab changes: CgTabs starts measuring over each time it's given a new one.
  const tabs = useMemo(
    () =>
      (book?.order ?? []).flatMap((index) => {
        const entry = book?.sheets[index];
        return entry && !entry.deleted && entry.state === 'visible'
          ? [
              {
                key: String(index),
                text: entry.name,
                headerAttributes: { 'data-testid': 'sheet-tab-' + index } as Record<string, string>,
              },
            ]
          : [];
      }),
    [book],
  );
  // A page opened from Cash Gear ERP leaves ERP drafts to the launch, which offers a matching one.
  const offered = recoverable.find((entry) => !(entry.erp && hadLaunchRef.current));
  const offeredTexts = offered?.erp ? erpTools.textsFor(offered.erp.language) : null;

  return (
    <div
      className={'cgs-shell' + (dragOver ? ' cgs-shell-drop' : '')}
      data-shared-memory={String(sharedMemory)}
      onDragOver={(event) => {
        event.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(event) => void onDrop(event)}
    >
      <header className="cgs-header">
        <div className="cgs-header-start">
          <div className="cgs-quick" role="toolbar" aria-label="Quick access">
            <CgButton
              icon="save"
              aria-label="Save"
              title="Save (Ctrl+S)"
              data-testid="quick-save"
              size="small"
              appearance="ghost"
              disabled={!saveReady || !canSave}
              onClick={() => void save()}
            >
              {null}
            </CgButton>
            <CgButton
              icon="undo"
              aria-label={undoLabels.undo ? `Undo ${undoLabels.undo}` : 'Undo'}
              title={undoLabels.undo ? `Undo ${undoLabels.undo} (Ctrl+Z)` : 'Undo (Ctrl+Z)'}
              data-testid="quick-undo"
              size="small"
              appearance="ghost"
              disabled={!undoLabels.undo}
              onClick={() => send({ kind: 'undo' })}
            >
              {null}
            </CgButton>
            <CgButton
              icon="redo"
              aria-label={undoLabels.redo ? `Redo ${undoLabels.redo}` : 'Redo'}
              title={undoLabels.redo ? `Redo ${undoLabels.redo} (Ctrl+Y)` : 'Redo (Ctrl+Y)'}
              data-testid="quick-redo"
              size="small"
              appearance="ghost"
              disabled={!undoLabels.redo}
              onClick={() => send({ kind: 'redo' })}
            >
              {null}
            </CgButton>
          </div>
          <h1 className="cgs-title">{fileName ?? 'CashGear Sheet'}</h1>
        </div>
        <CgButtonGroup
          ariaLabel="Theme"
          data-testid="theme-switch"
          size="small"
          items={themeChoices}
          selectionMode="single"
          selectedName={preference}
          onSelectedNameChange={(name) => theme.setPreference(name as ThemePreference)}
        />
      </header>
      <CgRibbon
        className="cgs-ribbon"
        tabs={tippedTabs}
        defaultActiveKey="home"
        ariaLabel="Ribbon"
        data-testid="ribbon"
        keyTips
        reservedAltKeys={platform.kind === 'web' ? BROWSER_ALT_KEYS : []}
        startContent={
          <CgMenu
            data-cg-keytip="F"
            className="cgs-menu"
            items={menu}
            semanticMode="application-menu"
            selectionMode="none"
            submenuTrigger="click"
            ariaLabel="File menu"
            size="small"
            displayMode="desktop"
            data-testid="main-menu"
          />
        }
      />
      {showFormulaBar && (
        <FormulaBar
          session={editing.session}
          name={cell.name}
          text={cell.edit}
          readOnly={!book}
          onBegin={() => void editing.edit('bar')}
          onEditorKey={editing.key}
          onGoTo={(text) => {
            const engine = clientRef.current;
            if (!engine) return;
            engine
              .resolveName(text, sheetRef.current)
              .then(async (found) => {
                if (!found) {
                  setNotice('That isn’t a cell or a name in this workbook.');
                  controller.current?.focus();
                  return;
                }
                const { sheet: target, ...area } = found;
                await goToSheet(target);
                controller.current?.selectArea(fromArea(area));
                controller.current?.focus();
              })
              .catch(report);
          }}
          onLeave={() => controller.current?.focus()}
        />
      )}
      <main className={pivotTools.pane ? 'cgs-grid cgs-grid-with-pane' : 'cgs-grid'}>
        {client && (
          <SheetCanvas
            client={client}
            palette={palette}
            colors={colors}
            events={events}
            onController={handController}
          />
        )}
        {chartTools.layer}
        {pivotTools.pane}
        <CellEditor
          ref={editorRef}
          session={editing.session}
          names={names}
          onKey={editing.key}
          completionRef={completionRef}
          zoom={zoom / 100}
        />
        {!book && load.phase === 'idle' && (
          <div className="cgs-welcome" data-testid="welcome">
            <p>Open a workbook, or drop one here.</p>
            <button type="button" className="cgs-open-button" onClick={() => void pick()}>
              Open a workbook…
            </button>
            <p className="cgs-welcome-note">{OPENABLE_EXTENSIONS.join(', ')}</p>
          </div>
        )}
        {resizeHint && (
          <div
            className="cgs-resize-hint"
            role="status"
            data-testid="resize-hint"
            style={{ left: resizeHint.clientX + 12, top: resizeHint.clientY + 12 }}
          >
            {resizeHint.axis === 'col' ? 'Width' : 'Height'}: {resizeHint.px} pixels
          </div>
        )}
        {book && !sheetLoaded && shown?.kind === 'worksheet' && (
          <div className="cgs-sheet-note" data-testid="sheet-loading" role="status">
            Reading this sheet… {percent}%
          </div>
        )}
        {shown && shown.kind === 'chart' && chartTools.chartSheet(sheet)}
        {shown && shown.kind !== 'worksheet' && shown.kind !== 'chart' && (
          <div className="cgs-sheet-note" data-testid="sheet-placeholder">
            This kind of sheet can’t be shown.
          </div>
        )}
      </main>
      <div
        className="cgs-tabs"
        onDoubleClick={(event) => {
          const tab = (event.target as HTMLElement).closest('[data-testid^="sheet-tab-"]');
          if (tab) openRename();
        }}
        onContextMenu={(event) => {
          const tab = (event.target as HTMLElement).closest<HTMLElement>(
            '[data-testid^="sheet-tab-"]',
          );
          if (!tab) return;
          event.preventDefault();
          const index = Number(tab.dataset.testid?.replace('sheet-tab-', ''));
          const { clientX, clientY } = event;
          const show = () => void contextMenu.current?.showAt(clientX, clientY, 'tab');
          if (index === sheetRef.current) return show();
          // As in Excel, the tab becomes active first. Showing it takes the keyboard, which
          // would close a menu already open.
          editingRef.current?.beforeSheetChange();
          void goToSheet(index).then(show, report);
        }}
      >
        {tabs.length > 0 && (
          <CgTabs
            tabs={tabs}
            activeKey={String(sheet)}
            onActiveKeyChange={chooseSheet}
            position="bottom"
            size="small"
            contentMode="active-only"
            ariaLabel="Sheets"
            data-testid="sheet-tabs"
          />
        )}
        {tabs.length > 0 && (
          <button
            type="button"
            className="cgs-add-sheet"
            aria-label="Insert a sheet"
            title="Insert a sheet"
            data-testid="add-sheet"
            disabled={protectTools.structureLocked}
            onClick={() => insertSheet(true)}
          >
            +
          </button>
        )}
      </div>
      <CgStatusBar
        data-testid="status-strip"
        ariaLabel="Status"
        items={[
          {
            key: 'status',
            content: (
              <CgStatusBadge
                data-testid="engine-status"
                type={
                  problem
                    ? 'error'
                    : notice
                      ? 'warning'
                      : engineReady && !reading && !saving
                        ? 'success'
                        : 'info'
                }
                text={statusText}
              />
            ),
          },
          ...(erpTools.badge ? [{ key: 'erp', content: erpTools.badge }] : []),
          ...(cloudTools.badge ? [{ key: 'cloud', content: cloudTools.badge }] : []),
          ...(circular.length
            ? [
                {
                  key: 'circular',
                  content: (
                    <span data-testid="circular-status">
                      {'Circular References: ' + circularLabel(circular[0]!)}
                    </span>
                  ),
                },
              ]
            : []),
          ...(calc.calculating
            ? [
                {
                  key: 'calc',
                  content: (
                    <span data-testid="calc-status">
                      {calc.manual ? 'Calculate' : 'Calculating…'}
                    </span>
                  ),
                },
              ]
            : []),
          ...(lastSave
            ? [
                {
                  key: 'last-save',
                  content: (
                    <button
                      type="button"
                      className="cgs-save-status"
                      data-testid="save-status"
                      title="Show the save report"
                      onClick={() => setShowReport(true)}
                    >
                      {saveSummary(lastSave)}
                    </button>
                  ),
                },
              ]
            : []),
          ...(updates.available
            ? [
                {
                  key: 'update',
                  content: (
                    <button
                      type="button"
                      className="cgs-update-status"
                      data-testid="update-status"
                      onClick={() => setRelease('update')}
                    >
                      Update to {updates.available.version}
                    </button>
                  ),
                },
              ]
            : []),
          ...(sharedMemory === false
            ? [
                {
                  key: 'shared-memory',
                  content: (
                    <CgStatusBadge
                      data-testid="shared-memory"
                      type="warning"
                      text="No shared memory"
                    />
                  ),
                },
              ]
            : []),
          {
            key: 'host',
            align: 'end' as const,
            content: <span data-testid="host-info">{hostText}</span>,
          },
          {
            key: 'zoom',
            align: 'end' as const,
            content: (
              <CgSlider
                data-testid="zoom-slider"
                className="cgs-zoom"
                min={0}
                max={100}
                step={1}
                value={zoomToSlider(zoom)}
                marks={[50]}
                showSteppers
                stepperStep={5}
                decreaseLabel="Zoom out"
                increaseLabel="Zoom in"
                ariaLabel="Zoom"
                formatValue={(value) => sliderToZoom(value) + '%'}
                onValueChange={(value) => controller.current?.setZoom(sliderToZoom(value))}
              />
            ),
          },
          {
            key: 'zoom-percent',
            align: 'end' as const,
            content: <span data-testid="zoom-percent">{zoom}%</span>,
          },
        ]}
      />
      <CgContextMenu<GridRegion | 'tab' | 'drawing'>
        actionsRef={contextMenu}
        items={contextItems}
        ariaLabel="Context menu"
        data-testid="context-menu"
        customizeMenu={({ invocation }) => {
          const region = invocation.context;
          if (region === 'tab') return tabItems;
          if (region === 'drawing')
            return chartTools.menuItems as ReadonlyArray<
              CgContextMenuItem<GridRegion | 'tab' | 'drawing'>
            >;
          // In a pivot table, its own menu, after Copy and Format Cells as Excel puts them.
          if (region === 'cells' && pivotTools.menuItems.length)
            return [
              ...contextItems.filter((item) => item.key === 'copy' || item.key === 'format-cells'),
              ...(pivotTools.menuItems as ReadonlyArray<
                CgContextMenuItem<GridRegion | 'tab' | 'drawing'>
              >),
            ];
          const hidden =
            region === 'rowHeader'
              ? ['col-width', 'hide-cols', 'unhide-cols']
              : region === 'colHeader'
                ? ['row-height', 'hide-rows', 'unhide-rows']
                : [
                    'col-width',
                    'hide-cols',
                    'unhide-cols',
                    'row-height',
                    'hide-rows',
                    'unhide-rows',
                  ];
          return contextItems.filter((item) => !hidden.includes(item.key));
        }}
      />
      {dialog?.kind === 'format' && format && (
        <FormatCellsDialog
          open
          tab={dialog.tab}
          format={format}
          theme={themeColors(lists)}
          codes={lists?.codes ?? []}
          sample={(code) => {
            const selection = selectionRef.current;
            const engine = clientRef.current;
            if (!selection || !engine) return Promise.resolve('');
            return engine.formatSample(
              sheetRef.current,
              selection.row + 1,
              selection.col + 1,
              code,
            );
          }}
          onApply={(change, merge) => {
            if (Object.keys(change).length) sheetFormat.format(change);
            if (merge === true) mergeCells('cells');
            if (merge === false) sheetFormat.unmerge();
          }}
          onClose={() => {
            setDialog(null);
            controller.current?.focus();
          }}
        />
      )}
      {dialog?.kind === 'styleFormat' && (
        <FormatCellsDialog
          open
          tab="number"
          format={dialog.format}
          theme={themeColors(lists)}
          codes={lists?.codes ?? []}
          sample={(code) => {
            const selection = selectionRef.current;
            const engine = clientRef.current;
            if (!selection || !engine) return Promise.resolve('');
            return engine.formatSample(
              sheetRef.current,
              selection.row + 1,
              selection.col + 1,
              code,
            );
          }}
          onApply={(change) => dialog.done(change)}
          onClose={() => setDialog(null)}
        />
      )}
      {dataTools.overlays}
      {erpTools.overlays}
      {cloudTools.overlays}
      {pivotTools.overlays}
      {chartTools.overlays}
      {printTools.overlays}
      {protectTools.overlays}
      {errorTools.overlays}
      {dialog?.kind === 'unhide' && (
        <UnhideDialog
          open
          sheets={hiddenSheets}
          onUnhide={(index) => send({ kind: 'sheetState', sheet: index, hidden: false })}
          onClose={() => {
            setDialog(null);
            controller.current?.focus();
          }}
        />
      )}
      {dialog?.kind === 'find' && (
        <FindDialog
          open
          tab={dialog.tab}
          engine={() => clientRef.current}
          selection={() => controller.current?.selection() ?? null}
          sheetName={(index) => bookRef.current?.sheets[index]?.name ?? ''}
          settled={() => commands.current.catch(() => undefined)}
          run={runCommand}
          goTo={async (found: FoundCell) => {
            await goToSheet(found.sheet);
            const at = { top: found.row - 1, left: found.col - 1 };
            controller.current?.selectArea({ ...at, bottom: at.top, right: at.left });
          }}
          onClose={() => {
            setDialog(null);
            controller.current?.focus();
          }}
        />
      )}
      {release === 'about' && (
        <AboutDialog version={version} native={native} onClose={() => setRelease(null)} />
      )}
      {release === 'update' && (
        <UpdateDialog
          updates={updates}
          dirty={canSave}
          save={() => save().then(() => !editedRef.current)}
          onClose={() => setRelease(null)}
        />
      )}
      {dialog?.kind === 'size' && (
        <CgMessageBox
          defaultOpen
          title={dialog.axis === 'row' ? 'Row Height' : 'Column Width'}
          actionLabel="OK"
          data-testid="size-dialog"
          onClosed={(accepted) => {
            const { axis, value } = dialog;
            setDialog(null);
            const size = Number(value);
            if (accepted && value.trim() && Number.isFinite(size) && size >= 0) {
              sheetFormat.setSize(axis, Math.round(size));
            }
            controller.current?.focus();
          }}
        >
          <label className="cgs-dialog-field">
            <span>{dialog.axis === 'row' ? 'Row height' : 'Column width'}, in pixels</span>
            <input
              data-testid="size-value"
              autoFocus
              inputMode="numeric"
              value={dialog.value}
              onChange={(event) => setDialog({ ...dialog, value: event.target.value })}
            />
          </label>
        </CgMessageBox>
      )}
      {dialog?.kind === 'options' && (
        <CgMessageBox
          defaultOpen
          title="Options"
          actionLabel="OK"
          data-testid="options-dialog"
          onClosed={(accepted) => {
            if (accepted) {
              writeDefaultDirection(dialog.direction);
              writeUserName(dialog.userName);
              writeErrorSettings(dialog.errors);
              viewCommand({ kind: 'errorRules', rules: ruleBits(dialog.errors) });
            }
            setDialog(null);
            controller.current?.focus();
          }}
        >
          <label className="cgs-dialog-field">
            <strong>User name</strong>
            <input
              data-testid="options-user-name"
              value={dialog.userName}
              maxLength={52}
              onChange={(event) => setDialog({ ...dialog, userName: event.target.value })}
            />
            <span>New notes say you wrote them.</span>
          </label>
          <div className="cgs-dialog-column" data-testid="options-error-checking">
            <strong>Error checking</strong>
            <label className="cgs-dialog-check">
              <input
                type="checkbox"
                data-testid="options-background-checking"
                checked={dialog.errors.background}
                onChange={(event) =>
                  setDialog({
                    ...dialog,
                    errors: { ...dialog.errors, background: event.target.checked },
                  })
                }
              />
              Enable background error checking
            </label>
            {RULE_LABELS.map(([rule, label]) => (
              <label key={rule} className="cgs-dialog-check" style={{ paddingInlineStart: 20 }}>
                <input
                  type="checkbox"
                  data-testid={'options-rule-' + rule}
                  disabled={!dialog.errors.background}
                  checked={(dialog.errors.rules & RULE_BITS[rule]) !== 0}
                  onChange={(event) =>
                    setDialog({
                      ...dialog,
                      errors: {
                        ...dialog.errors,
                        rules: event.target.checked
                          ? dialog.errors.rules | RULE_BITS[rule]
                          : dialog.errors.rules & ~RULE_BITS[rule],
                      },
                    })
                  }
                />
                {label}
              </label>
            ))}
          </div>
          <div className="cgs-dialog-column" role="radiogroup" aria-label="Default direction">
            <strong>Default direction</strong>
            {(
              [
                ['rtl', 'Right-to-left'],
                ['ltr', 'Left-to-right'],
              ] as const
            ).map(([value, text]) => (
              <label key={value} className="cgs-dialog-check">
                <input
                  type="radio"
                  name="cgs-direction"
                  data-testid={'direction-' + value}
                  checked={dialog.direction === value}
                  onChange={() => setDialog({ ...dialog, direction: value })}
                />
                {text}
              </label>
            ))}
            <span>
              New sheets and workbooks start this way. Sheet Right-to-Left changes one sheet.
            </span>
          </div>
        </CgMessageBox>
      )}
      {dialog?.kind === 'shift' && (
        <CgMessageBox
          defaultOpen
          title={dialog.remove ? 'Delete' : 'Insert'}
          actionLabel="OK"
          data-testid="shift-dialog"
          onClosed={(accepted) => {
            const { choice, remove } = dialog;
            setDialog(null);
            if (accepted) {
              if (choice === 'row' || choice === 'col') editing.axis(choice, remove);
              else sheetFormat.shift(choice === 'down', remove);
            }
            controller.current?.focus();
          }}
        >
          <div className="cgs-dialog-column" role="radiogroup" aria-label="Shift">
            {(
              [
                ['side', dialog.remove ? 'Shift cells left' : 'Shift cells right'],
                ['down', dialog.remove ? 'Shift cells up' : 'Shift cells down'],
                ['row', 'Entire row'],
                ['col', 'Entire column'],
              ] as const
            ).map(([value, text]) => (
              <label key={value} className="cgs-dialog-check">
                <input
                  type="radio"
                  name="cgs-shift"
                  data-testid={'shift-' + value}
                  checked={dialog.choice === value}
                  onChange={() => setDialog({ ...dialog, choice: value })}
                />
                {text}
              </label>
            ))}
          </div>
        </CgMessageBox>
      )}
      {dialog?.kind === 'close' && (
        <CgWindow
          open
          headerText="CashGear Sheet"
          width={420}
          data-testid="close-dialog"
          onOpenChange={(next) => {
            if (!next) {
              dialog.answer(false);
              setDialog(null);
            }
          }}
          renderBody={() => (
            <p>Do you want to save your changes to {fileName ?? 'this workbook'}?</p>
          )}
          renderFooter={() => (
            <div className="cgs-dialog-actions">
              <CgButton
                intent="primary"
                data-testid="close-save"
                onClick={() => {
                  const { answer } = dialog;
                  setDialog(null);
                  void save()
                    .then(() => answer(!editedRef.current))
                    .catch(() => answer(false));
                }}
              >
                Save
              </CgButton>
              <CgButton
                data-testid="close-discard"
                onClick={() => {
                  dialog.answer(true);
                  setDialog(null);
                }}
              >
                Don’t Save
              </CgButton>
              <CgButton
                data-testid="close-cancel"
                onClick={() => {
                  dialog.answer(false);
                  setDialog(null);
                }}
              >
                Cancel
              </CgButton>
            </div>
          )}
        />
      )}
      {dialog?.kind === 'merge' && (
        <CgMessageBox
          defaultOpen
          title="Merge cells"
          actionLabel="OK"
          data-testid="merge-dialog"
          onClosed={(accepted) => {
            const { answer } = dialog;
            setDialog(null);
            answer(accepted);
          }}
        >
          <p>Merging cells only keeps the upper-left value, and discards the other values.</p>
        </CgMessageBox>
      )}
      {renaming && (
        <CgMessageBox
          defaultOpen
          title="Rename sheet"
          actionLabel="Rename"
          data-testid="rename-sheet"
          onClosed={(accepted) => {
            const target = renaming;
            setRenaming(null);
            if (accepted) send({ kind: 'renameSheet', sheet: target.sheet, name: target.name });
            controller.current?.focus();
          }}
        >
          <input
            className="cgs-sheet-name"
            aria-label="Sheet name"
            data-testid="sheet-name"
            autoFocus
            maxLength={31}
            value={renaming.name}
            onChange={(event) => setRenaming({ ...renaming, name: event.target.value })}
          />
        </CgMessageBox>
      )}
      {offered && offeredTexts && offered.erp && (
        <CgMessageBox
          key={offered.id}
          defaultOpen
          title={offeredTexts.RECOVER_TITLE}
          actionLabel={offeredTexts.RECOVER}
          data-testid="recover"
          data-erp="true"
          onClosed={(accepted) => {
            if (accepted) void restoreErp(offered);
            else setRecoverable((list) => list.filter((other) => other.id !== offered.id));
          }}
        >
          <p dir={offeredTexts.dir} data-testid="recover-text">
            {offeredTexts.recoverText(offered.name, offered.erp.labelEn.toLowerCase())}
          </p>
          <button
            type="button"
            className="cgs-discard"
            data-testid="recover-discard"
            dir={offeredTexts.dir}
            onClick={() => discardRecovered(offered)}
          >
            {offeredTexts.RECOVER_DISCARD}
          </button>
        </CgMessageBox>
      )}
      {offered && !offered.erp && (
        <CgMessageBox
          key={offered.id}
          defaultOpen
          title="Recover your edits?"
          actionLabel="Recover"
          data-testid="recover"
          onClosed={(accepted) => {
            if (accepted) void restoreRecovered(offered);
            else setRecoverable((list) => list.filter((other) => other.id !== offered.id));
          }}
        >
          <p>
            {offered.name || 'A workbook'} has {offered.edits}{' '}
            {offered.edits === 1 ? 'edit' : 'edits'} from a session that ended before it was saved.
            Your file hasn’t changed.
          </p>
          <button
            type="button"
            className="cgs-discard"
            data-testid="recover-discard"
            onClick={() => discardRecovered(offered)}
          >
            Discard these edits
          </button>
        </CgMessageBox>
      )}
      {lastSave && (
        <CgMessageBox
          open={showReport}
          onOpenChange={setShowReport}
          title="Save report"
          data-testid="save-report"
        >
          <ul className="cgs-save-report">
            {saveReportLines(lastSave).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </CgMessageBox>
      )}
    </div>
  );
}
