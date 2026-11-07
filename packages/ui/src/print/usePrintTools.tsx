// Printing as the shell runs it: the Page Layout tab's state and commands, the Page Setup dialog,
// File › Print and Export › PDF, and the dashed lines where pages break in Normal view.
import { useCallback, useMemo, useRef, useState } from 'react';
import type {
  Applied,
  BookSummary,
  Command,
  EngineClient,
  FontSource,
  PageSetupInfo,
  PageSpec,
} from '@cgs/engine-client';
import type { GridController, GridSelection } from '../grid/controller';
import type { SheetPlatform } from '../platform';
import { toArea } from '../sheet-editing';
import { PageSetupDialog, type PageSetupChange } from './PageSetupDialog';
import { PrintView } from './PrintView';
import {
  arabicFamily,
  jobPdf,
  jobSvgs,
  makeJob,
  type PrintChoice,
  type PrintJob,
} from './printJob';
import type { PageLayoutActions, PageLayoutState, PageSetupTab } from './ribbon-page';

export interface PrintToolsHost {
  engine: () => EngineClient | null;
  grid: () => GridController | null;
  sheet: () => number;
  selection: () => GridSelection | null;
  book: () => BookSummary | null;
  hasBook: boolean;
  /** The workbook's file name and, where the platform knows it, its path. */
  file: () => { name: string; path: string };
  platform: SheetPlatform;
  /** The screen's fallback font families. */
  fontFamily: () => string;
  locale: string;
  showGrid: boolean;
  showHeadings: boolean;
  setView: (flags: { grid?: boolean; headings?: boolean }) => void;
  run: (command: Command) => Promise<Applied>;
  notice: (message: string | null) => void;
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function usePrintTools(host: PrintToolsHost) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [setup, setSetup] = useState<PageSetupInfo | null>(null);
  const [dialog, setDialog] = useState<PageSetupTab | null>(null);
  const [view, setView] = useState<'print' | 'pdf' | null>(null);
  const [canBreak, setCanBreak] = useState(false);
  // Sheets showing where pages break: those whose page setup changed or that were previewed.
  const breaksShown = useRef(new Set<number>());
  const lastSheet = useRef(-1);

  const showBreaks = useCallback(async (sheet: number) => {
    const { engine, grid } = { engine: hostRef.current.engine(), grid: hostRef.current.grid() };
    if (!engine || !grid) return;
    if (!breaksShown.current.has(sheet)) {
      grid.setPageBreaks(null);
      return;
    }
    try {
      const pages = await engine.query({ kind: 'pages', sheet });
      if (hostRef.current.sheet() !== sheet) return;
      grid.setPageBreaks({
        rows: pages.rowBreaks,
        cols: pages.colBreaks,
        manualRows: pages.manualRows,
        manualCols: pages.manualCols,
        areas: pages.printAreas,
      });
    } catch {
      grid.setPageBreaks(null);
    }
  }, []);

  /** Reads the active sheet's page setup again, after any change. */
  const refresh = useCallback(async () => {
    const engine = hostRef.current.engine();
    const sheet = hostRef.current.sheet();
    if (!engine || sheet < 0 || !hostRef.current.hasBook) {
      setSetup(null);
      return;
    }
    const kind = hostRef.current.book()?.sheets[sheet]?.kind;
    if (kind !== 'worksheet') {
      setSetup(null);
      return;
    }
    try {
      const next = await engine.query({ kind: 'pageSetup', sheet });
      if (hostRef.current.sheet() === sheet) setSetup(next);
    } catch {
      setSetup(null);
    }
    if (lastSheet.current !== sheet || breaksShown.current.has(sheet)) {
      lastSheet.current = sheet;
      await showBreaks(sheet);
    }
  }, [showBreaks]);

  /** Where Insert Page Break can go, as the selection moves. */
  const selectionChanged = useCallback((selection: GridSelection | null) => {
    setCanBreak(Boolean(selection && (selection.range.top > 0 || selection.range.left > 0)));
  }, []);

  const run = useCallback(
    async (command: Command) => {
      try {
        const applied = await hostRef.current.run(command);
        breaksShown.current.add(hostRef.current.sheet());
        await refresh();
        return applied;
      } catch (error) {
        hostRef.current.notice(errorText(error));
        throw error;
      }
    },
    [refresh],
  );

  const change = useCallback(
    async (spec: PageSpec) => {
      const sheet = hostRef.current.sheet();
      await run({ kind: 'pageSetup', sheets: [sheet], spec });
    },
    [run],
  );

  const actions = useMemo<PageLayoutActions>(
    () => ({
      change: (spec) => void change(spec).catch(() => undefined),
      openPageSetup: (tab = 'page') => setDialog(tab),
      printArea(action) {
        const selection = hostRef.current.selection();
        const sheet = hostRef.current.sheet();
        void run({
          kind: 'printArea',
          sheet,
          action,
          areas: selection && action !== 'clear' ? [toArea(selection.range)] : [],
        }).catch(() => undefined);
      },
      pageBreak(action) {
        const selection = hostRef.current.selection();
        const sheet = hostRef.current.sheet();
        const range = selection?.range;
        // A whole row selected breaks only above it, and a whole column only to its left.
        const wholeRow = range && range.left === 0 && range.right >= 16383;
        const wholeCol = range && range.top === 0 && range.bottom >= 1048575;
        void run({
          kind: 'pageBreak',
          sheet,
          action,
          row: action === 'reset' || wholeCol || !range ? null : range.top + 1,
          col: action === 'reset' || wholeRow || !range ? null : range.left + 1,
        }).catch(() => undefined);
      },
      setView: (flags) => hostRef.current.setView(flags),
    }),
    [change, run],
  );

  const state = useMemo<PageLayoutState>(
    () => ({
      hasBook: host.hasBook,
      setup,
      showGrid: host.showGrid,
      showHeadings: host.showHeadings,
      canBreak,
    }),
    [host.hasBook, setup, host.showGrid, host.showHeadings, canBreak],
  );

  /**
   * Whether PDFs can use installed fonts, as the screen does. When they can't, every face falls
   * back on a bundled font: the same widths for Latin, but not for Arabic, which pages then
   * measure in the bundled Arabic font.
   */
  const arabicMode = useCallback(async (): Promise<'system' | 'bundled'> => {
    const platform = hostRef.current.platform;
    if (!platform.systemFont) return 'bundled';
    const found = await platform.systemFont('Arial', false, false).catch(() => null);
    return found ? 'system' : 'bundled';
  }, []);

  const jobHost = useCallback(async () => {
    const h = hostRef.current;
    const engine = h.engine();
    const book = h.book();
    if (!engine || !book) throw new Error('Open a workbook first.');
    const selection = h.selection();
    return {
      engine,
      sheet: h.sheet(),
      selection: selection ? [toArea(selection.range)] : [],
      sheets: book.sheets,
      order: book.order,
      fileName: h.file().name,
      filePath: h.file().path,
      fontFamily: h.fontFamily(),
      locale: h.locale,
      arabic: await arabicMode(),
    };
  }, [arabicMode]);

  const fontSource = useCallback((): FontSource => {
    const platform = hostRef.current.platform;
    const family = arabicFamily(hostRef.current.fontFamily());
    return {
      installed: platform.systemFont
        ? async (name, bold, italic) => {
            const found = await platform.systemFont!(name, bold, italic);
            return found && { data: found.data, index: found.index, name: found.name };
          }
        : undefined,
      arabic:
        platform.systemFont && family
          ? async (bold) => {
              const found = await platform.systemFont!(family, bold, false);
              return found && { data: found.data, index: found.index, name: found.name };
            }
          : undefined,
    };
  }, []);

  const savePdf = useCallback(
    async (job: PrintJob) => {
      const h = hostRef.current;
      const engine = h.engine();
      if (!engine) throw new Error('Open a workbook first.');
      // Asking where to save comes first, while the click still counts.
      const target = await h.platform.pickSaveTarget(job.title + '.pdf', 'pdf');
      if (!target) return;
      const bytes = await jobPdf(engine, job, fontSource());
      await target.write(new Blob([bytes.slice()], { type: 'application/pdf' }));
      h.notice(`Saved ${job.pages.length} page${job.pages.length === 1 ? '' : 's'} as a PDF.`);
    },
    [fontSource],
  );

  const applySetup = useCallback(
    async (next: PageSetupChange, then: 'close' | 'print' | 'preview') => {
      const sheet = hostRef.current.sheet();
      if (Object.keys(next.spec).length) {
        await hostRef.current.run({ kind: 'pageSetup', sheets: [sheet], spec: next.spec });
      }
      if (next.printArea) {
        await hostRef.current.run({
          kind: 'printArea',
          sheet,
          areas: next.printArea,
          action: next.printArea.length ? 'set' : 'clear',
        });
      }
      if (next.titles) {
        await hostRef.current.run({
          kind: 'printTitles',
          sheet,
          rows: next.titles.rows,
          cols: next.titles.cols,
        });
      }
      breaksShown.current.add(sheet);
      setDialog(null);
      await refresh();
      if (then !== 'close') setView('print');
    },
    [refresh],
  );

  const openPrint = useCallback(
    (mode: 'print' | 'pdf') => {
      if (!hostRef.current.hasBook) return;
      breaksShown.current.add(hostRef.current.sheet());
      setView(mode);
      void refresh();
    },
    [refresh],
  );

  const platform = host.platform;
  const fontAccess =
    platform.askFontAccess && 'queryLocalFonts' in window ? () => platform.askFontAccess!() : null;
  const sheetName = host.book()?.sheets[host.sheet()]?.name ?? '';

  const overlays = (
    <>
      <PageSetupDialog
        open={dialog !== null}
        tab={dialog ?? 'page'}
        setup={setup}
        sheetName={sheetName}
        fileName={host.file().name}
        onClose={() => setDialog(null)}
        onApply={applySetup}
      />
      <PrintView
        open={view !== null}
        mode={view ?? 'print'}
        setup={setup}
        hasSelection={Boolean(host.selection())}
        makeJob={async (choice: PrintChoice) => makeJob(await jobHost(), choice)}
        svgs={jobSvgs}
        print={async () => {
          await hostRef.current.platform.print();
        }}
        savePdf={savePdf}
        change={change}
        openPageSetup={(tab) => setDialog(tab)}
        fontAccess={fontAccess}
        onClose={() => {
          setView(null);
          void refresh();
        }}
      />
    </>
  );

  /** For tests and the PDF review: a choice's PDF and its pages' count, with no dialog. */
  const pdfFor = useCallback(
    async (choice: PrintChoice) => {
      const job = await makeJob(await jobHost(), choice);
      const engine = hostRef.current.engine();
      if (!engine) throw new Error('Open a workbook first.');
      return { bytes: await jobPdf(engine, job, fontSource()), pages: job.pages.length };
    },
    [fontSource, jobHost],
  );

  return {
    state,
    actions,
    overlays,
    refresh,
    openPrint,
    selectionChanged,
    pdfFor,
    viewOpen: view !== null,
  };
}
