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
  CgSlider,
  CgStatusBadge,
  CgStatusBar,
  CgTabs,
  type CgMenuItem,
} from '@cashgear/ui';
import {
  createEngineClient,
  localeData,
  type BookSummary,
  type EngineClient,
  type LoadEvent,
} from '@cgs/engine-client';
import { FormulaBar } from './FormulaBar';
import { type GridController, type GridEvents, type GridSelection } from './grid/controller';
import { sliderToZoom, zoomToSlider } from './grid/geometry';
import { SheetCanvas } from './grid/SheetCanvas';
import {
  WORKBOOK_EXTENSIONS,
  type NativeInfo,
  type OpenedFile,
  type RecentFile,
  type SheetPlatform,
} from './platform';
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
  const controller = useRef<GridController | null>(null);
  const openToken = useRef(0);
  const cellToken = useRef(0);
  const shownSheet = useRef(-1);
  const clientRef = useRef<EngineClient | null>(null);
  const sheetRef = useRef(0);
  const loadRef = useRef<LoadState['phase']>('idle');
  sheetRef.current = sheet;
  clientRef.current = client;
  loadRef.current = load.phase;

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
    controller.current?.focus();
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

  const openFile = useCallback(
    async (file: OpenedFile) => {
      const engine = clientRef.current;
      if (!engine) return;
      const token = ++openToken.current;
      setProblem(null);
      setCell(noCell);
      setLoadedSheets(new Set());
      shownSheet.current = -1;
      setLoad({ phase: 'reading', done: 0, total: 0 });
      controller.current?.reset();
      try {
        const summary = await engine.open(file.source, (event) =>
          onLoadEvent(token, engine, event),
        );
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
        await platform.rememberFile(file);
        refreshRecent();
      } catch (error) {
        if (token !== openToken.current) return;
        setBook(null);
        setFileName(null);
        setLoad({ phase: 'idle', done: 0, total: 0 });
        report(error);
        if (error instanceof Error && /ran out of memory/.test(error.message)) {
          setClientVersion((version) => version + 1);
        }
      }
    },
    [onLoadEvent, platform, refreshRecent, report],
  );

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
    };
  }, [openFile]);

  const chooseSheet = useCallback(
    (key: string | null) => {
      if (key === null) return;
      const index = Number(key);
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
        const engine = clientRef.current;
        if (!engine) return;
        const token = ++cellToken.current;
        setCell((previous) => ({ ...previous, name: selection.name }));
        engine
          .cellText(sheetRef.current, selection.row + 1, selection.col + 1)
          .then((text) => {
            if (token === cellToken.current) setCell({ name: selection.name, edit: text.edit });
          })
          .catch(report);
      },
      onZoom: setZoom,
      onError: report,
    }),
    [report],
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

  const menu = useMemo<CgMenuItem[]>(
    () => [
      {
        key: 'file',
        text: 'File',
        children: [
          { key: 'open', text: 'Open…', shortcut: 'Ctrl+O', onActivate: () => void pick() },
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
    [openFromRecent, pick, preference, recent, theme],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'o') {
        event.preventDefault();
        void pick();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [pick]);

  const host = native ? `Desktop ${native.version}` : 'Web';
  const sheets = book?.sheets ?? [];
  const shown = sheets[sheet];
  const sheetLoaded = loadedSheets.has(sheet);
  const percent = load.total ? Math.min(100, Math.round((load.done / load.total) * 100)) : 0;
  const reading = load.phase === 'reading';
  const statusText = problem
    ? problem
    : !engineReady
      ? 'Starting the engine…'
      : reading
        ? `Reading ${percent}%`
        : 'Ready';
  const tabs = sheets.flatMap((entry, index) =>
    entry.state === 'visible'
      ? [
          {
            key: String(index),
            text: entry.name,
            headerAttributes: { 'data-testid': 'sheet-tab-' + index } as Record<string, string>,
          },
        ]
      : [],
  );

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
      <FormulaBar name={cell.name} text={cell.edit} />
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
        {!book && load.phase === 'idle' && (
          <div className="cgs-welcome" data-testid="welcome">
            <p>Open a workbook, or drop one here.</p>
            <button type="button" className="cgs-open-button" onClick={() => void pick()}>
              Open a workbook…
            </button>
            <p className="cgs-welcome-note">{WORKBOOK_EXTENSIONS.join(', ')}</p>
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
      <div className="cgs-tabs">
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
                type={problem ? 'error' : engineReady && !reading ? 'success' : 'info'}
                text={statusText}
              />
            ),
          },
          ...(book
            ? [
                {
                  key: 'read-only',
                  content: <span data-testid="read-only">Read-only</span>,
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
            content: <span data-testid="host-info">{host}</span>,
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
    </div>
  );
}
