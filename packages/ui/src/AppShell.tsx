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
  CgButtonGroup,
  CgMenu,
  CgMessageBox,
  CgSlider,
  CgStatusBadge,
  CgStatusBar,
  CgTabs,
  type CgMenuItem,
} from '@cashgear/ui';
import {
  createEngineClient,
  localeData,
  type Applied,
  type BookSummary,
  type CalcStatus,
  type Command,
  type EngineClient,
  type LoadEvent,
  type Recoverable,
  type SaveReport,
  type WorkbookChanges,
} from '@cgs/engine-client';
import { CellEditor, type CellEditorHandle, type CompletionControl } from './CellEditor';
import { FormulaBar } from './FormulaBar';
import { type GridController, type GridEvents, type GridSelection } from './grid/controller';
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
import { saveReportLines, saveSummary, workbookExtension } from './save-report';
import { fromArea, SheetEditing, toArea, type EditingHost } from './sheet-editing';
import { readGridColors, readPalette, type ThemeController, type ThemePreference } from './theme';

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
}

const noCell: CellDetail = { name: '', edit: '' };

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

export function AppShell({ platform, theme }: AppShellProps) {
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
  const [lastSave, setLastSave] = useState<SaveReport | null>(null);
  const [showReport, setShowReport] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [calc, setCalc] = useState<CalcStatus>({ calculating: false, manual: false });
  const [undoLabels, setUndoLabels] = useState<{ undo: string | null; redo: string | null }>({
    undo: null,
    redo: null,
  });
  const [names, setNames] = useState<string[]>([]);
  const [renaming, setRenaming] = useState<{ sheet: number; name: string } | null>(null);
  const [recoverable, setRecoverable] = useState<Recoverable[]>([]);
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
  const loadedRef = useRef<ReadonlySet<number>>(new Set());
  /** The source has been copied, or there's nowhere to copy it, so edits can go ahead. */
  const protectedRef = useRef(false);
  const workingRef = useRef<WorkingCopy | null>(null);
  const savingRef = useRef(false);
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
  const report = useCallback((error: unknown) => {
    setProblem(error instanceof Error ? error.message : String(error));
  }, []);

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
  const showSheet = useCallback(async (engine: EngineClient, index: number) => {
    const [layout, drawings] = await Promise.all([engine.layout(index), engine.drawings(index)]);
    if (sheetRef.current !== index) return;
    shownSheet.current = index;
    controller.current?.showSheet(index, layout, drawings);
    if (!editingRef.current?.session.snapshot().active) controller.current?.focus();
  }, []);

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
            }
          }
          break;
        case 'loaded':
          setLoad((state) => ({ ...state, phase: 'preparing' }));
          if (csvRef.current !== null) {
            const text = csvRef.current;
            csvRef.current = null;
            engine.loadCsv(text).catch(report);
          }
          break;
        case 'prepared':
          setLoad((state) => ({ ...state, phase: 'ready' }));
          break;
        case 'error':
          setProblem(event.message);
          setBook(null);
          setFileName(null);
          setLoad({ phase: 'idle', done: 0, total: 0 });
          controller.current?.reset();
          // A trap leaves the module unusable, so the next file gets a new one.
          if (/ran out of memory/.test(event.message)) setClientVersion((version) => version + 1);
          break;
      }
    },
    [report, showSheet],
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
        if (error instanceof Error && /ran out of memory/.test(error.message)) {
          setClientVersion((version) => version + 1);
        }
      }
    },
    [onLoadEvent, platform, refreshRecent, releaseWorking, report],
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
      const bytes = await engine.blankWorkbook();
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
      const bytes = await engine.blankWorkbook();
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
      try {
        // Whatever was typed before Save is part of what gets saved.
        await commands.current.catch(() => undefined);
        const { blob, report: saved } = await engine.save();
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
      } catch (error) {
        report(error);
      } finally {
        savingRef.current = false;
        setSaving(false);
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
      report(error);
    }
  }, [platform, report, saveAs, writeTo]);

  /** Asks the engine what the active cell holds, for the formula bar. */
  const refreshCell = useCallback(() => {
    const engine = clientRef.current;
    const selection = selectionRef.current;
    if (!engine || !selection) return;
    const token = ++cellToken.current;
    setCell((previous) => ({ ...previous, name: selection.name }));
    engine
      .cellText(sheetRef.current, selection.row + 1, selection.col + 1)
      .then((text) => {
        if (token === cellToken.current) setCell({ name: selection.name, edit: text.edit });
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
          // The tabs have to be right before anything goes to a sheet.
          let summary: BookSummary | null = null;
          if (
            command.kind === 'insertSheet' ||
            command.kind === 'deleteSheet' ||
            command.kind === 'renameSheet' ||
            command.kind === 'undo' ||
            command.kind === 'redo'
          ) {
            summary = await engine.bookSummary();
            applySummary(summary);
          }
          if (summary && command.kind === 'insertSheet') {
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
      }).catch((error) => setNotice(error instanceof Error ? error.message : String(error)));
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
    notice: setNotice,
    focusEditor: () => editorRef.current?.focus(),
    editorElement: () => editorRef.current?.element()?.parentElement ?? null,
    insertSheet: () => insertSheet(false),
    calculate,
    completionKey: (event) => completionRef.current?.key(event) ?? false,
  };
  if (!editingRef.current) {
    const through: EditingHost = {
      engine: () => host.current!.engine(),
      grid: () => host.current!.grid(),
      run: (command) => host.current!.run(command),
      settled: () => host.current!.settled(),
      canEdit: () => host.current!.canEdit(),
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
    };
    editingRef.current = new SheetEditing(through);
  }
  const editing = editingRef.current;
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
      controller.current?.invalidate(changes.sheets, changes.tiles);
      if (changes.layouts.includes(sheetRef.current) || changes.sheets.includes(sheetRef.current)) {
        void showSheet(client, sheetRef.current).catch(report);
      }
      if (changes.book) {
        client.bookSummary().then(applySummary).catch(report);
      }
    };
    client.watchChanges(onChanges, setCalc).catch(report);
    client
      .recoverable()
      .then(setRecoverable)
      .catch((error) => console.warn('Could not look for recovered edits:', error));
  }, [applySummary, client, engineReady, report, showSheet]);

  // On the web, closing a tab with unsaved edits asks first.
  useEffect(() => {
    if (platform.kind !== 'web') return;
    const onLeave = (event: BeforeUnloadEvent) => {
      if (editedRef.current) event.preventDefault();
    };
    window.addEventListener('beforeunload', onLeave);
    return () => window.removeEventListener('beforeunload', onLeave);
  }, [platform]);

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
        edit: (
          sheet: number,
          row: number,
          col: number,
          content: { number?: number; text?: string; formula?: string },
        ) => Promise<void>;
        command: (command: Command) => Promise<Applied>;
        settled: () => Promise<unknown>;
        hasChanges: () => Promise<boolean>;
        writeTo: (target: SaveTarget) => Promise<void>;
        lastSave: () => SaveReport | null;
        problem: () => string | null;
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
      edit: async (sheet, row, col, content) => {
        let text = '';
        if (content.formula !== undefined) text = '=' + content.formula;
        else if (content.number !== undefined) text = String(content.number);
        else if (content.text !== undefined) text = "'" + content.text;
        await runCommand({ kind: 'input', sheet, row, col, text });
      },
      command: runCommand,
      settled: () => commands.current.catch(() => undefined),
      hasChanges: () => clientRef.current?.hasChanges() ?? Promise.resolve(false),
      writeTo,
      lastSave: () => lastSaveRef.current,
      problem: () => problemRef.current,
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
        selectionRef.current = selection;
        refreshCell();
      },
      onZoom: setZoom,
      onError: report,
      onType: editing.type,
      onAction: editing.action,
      onJump: editing.jump,
      onEditClick: editing.editClick,
      onEditDrag: editing.editDrag,
      onFill: editing.fill,
      onViewport: (index, range) => clientRef.current?.setViewport(index, toArea(range)),
    }),
    [editing, refreshCell, report],
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
  const send = useCallback(
    (command: Command) => {
      setNotice(null);
      runCommand(command).catch((error) =>
        setNotice(error instanceof Error ? error.message : String(error)),
      );
    },
    [runCommand],
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
        ],
      },
      {
        key: 'edit',
        text: 'Edit',
        children: [
          {
            key: 'undo',
            text: undoLabels.undo ? `Undo ${undoLabels.undo}` : 'Undo',
            shortcut: 'Ctrl+Z',
            disabled: !undoLabels.undo,
            onActivate: () => send({ kind: 'undo' }),
          },
          {
            key: 'redo',
            text: undoLabels.redo ? `Redo ${undoLabels.redo}` : 'Redo',
            shortcut: 'Ctrl+Y',
            disabled: !undoLabels.redo,
            onActivate: () => send({ kind: 'redo' }),
          },
          {
            key: 'cut',
            text: 'Cut',
            shortcut: 'Ctrl+X',
            disabled: !book,
            onActivate: () => editing.copy(true),
          },
          {
            key: 'copy',
            text: 'Copy',
            shortcut: 'Ctrl+C',
            disabled: !book,
            onActivate: () => editing.copy(false),
          },
          {
            key: 'paste',
            text: 'Paste',
            shortcut: 'Ctrl+V',
            disabled: !book,
            onActivate: () => void editing.pasteFromMenu(),
          },
        ],
      },
      {
        key: 'insert',
        text: 'Insert',
        children: [
          {
            key: 'insert-rows',
            text: 'Rows',
            disabled: !book,
            onActivate: () => editing.axis('row', false),
          },
          {
            key: 'insert-cols',
            text: 'Columns',
            disabled: !book,
            onActivate: () => editing.axis('col', false),
          },
          {
            key: 'delete-rows',
            text: 'Delete rows',
            disabled: !book,
            onActivate: () => editing.axis('row', true),
          },
          {
            key: 'delete-cols',
            text: 'Delete columns',
            disabled: !book,
            onActivate: () => editing.axis('col', true),
          },
        ],
      },
      {
        key: 'sheet',
        text: 'Sheet',
        children: [
          {
            key: 'sheet-insert',
            text: 'Insert sheet',
            shortcut: 'Shift+F11',
            disabled: !book,
            onActivate: () => insertSheet(false),
          },
          {
            key: 'sheet-rename',
            text: 'Rename…',
            disabled: !book,
            onActivate: openRename,
          },
          {
            key: 'sheet-delete',
            text: 'Delete sheet',
            disabled: !book,
            onActivate: () => send({ kind: 'deleteSheet', sheet: sheetRef.current }),
          },
        ],
      },
      {
        key: 'formulas',
        text: 'Formulas',
        children: [
          {
            key: 'calculate-now',
            text: 'Calculate now',
            shortcut: 'F9',
            disabled: !book,
            onActivate: () => calculate('now'),
          },
          {
            key: 'calculate-sheet',
            text: 'Calculate sheet',
            shortcut: 'Shift+F9',
            disabled: !book,
            onActivate: () => calculate('sheet'),
          },
          {
            key: 'calc-manual',
            text: 'Manual calculation',
            checked: calc.manual,
            disabled: !book,
            onActivate: () => void clientRef.current?.setManualCalc(!calc.manual).catch(report),
          },
        ],
      },
      {
        key: 'view',
        text: 'View',
        children: [
          {
            key: 'theme',
            text: 'Theme',
            children: themeChoices.map((choice) => ({
              key: 'theme-' + choice.name,
              text: choice.text,
              radioGroup: 'theme',
              checked: preference === choice.name,
              onActivate: () => theme.setPreference(choice.name as ThemePreference),
            })),
          },
          {
            key: 'zoom-100',
            text: 'Zoom to 100%',
            onActivate: () => controller.current?.setZoom(100),
          },
        ],
      },
    ],
    [
      book,
      calc.manual,
      calculate,
      canSave,
      editing,
      exportCsv,
      insertSheet,
      newWorkbook,
      openFromRecent,
      openRename,
      pick,
      preference,
      recent,
      report,
      save,
      saveAs,
      saveReady,
      saving,
      send,
      theme,
      undoLabels,
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
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [newWorkbook, pick, save, saveAs]);

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
            ? 'Saving…'
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
  const offered = recoverable[0];

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
          <h1 className="cgs-title">{fileName ?? 'CashGear Sheet'}</h1>
          <CgMenu
            className="cgs-menu"
            items={menu}
            semanticMode="application-menu"
            selectionMode="none"
            submenuTrigger="click"
            ariaLabel="Main menu"
            size="small"
            displayMode="desktop"
            data-testid="main-menu"
          />
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
      <main className="cgs-grid">
        {client && (
          <SheetCanvas
            client={client}
            palette={palette}
            colors={colors}
            events={events}
            onController={handController}
          />
        )}
        <CellEditor
          ref={editorRef}
          session={editing.session}
          names={names}
          onKey={editing.key}
          completionRef={completionRef}
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
        {book && !sheetLoaded && shown?.kind === 'worksheet' && (
          <div className="cgs-sheet-note" data-testid="sheet-loading" role="status">
            Reading this sheet… {percent}%
          </div>
        )}
        {shown && shown.kind !== 'worksheet' && (
          <div className="cgs-sheet-note" data-testid="sheet-placeholder">
            {shown.kind === 'chart'
              ? 'This is a chart sheet. Charts show in a later version.'
              : 'This kind of sheet can’t be shown.'}
          </div>
        )}
      </main>
      <div
        className="cgs-tabs"
        onDoubleClick={(event) => {
          const tab = (event.target as HTMLElement).closest('[data-testid^="sheet-tab-"]');
          if (tab) openRename();
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
      {offered && (
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
