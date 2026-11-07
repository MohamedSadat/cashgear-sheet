// A print job: which sheets print, their pages as the engine breaks them, and those pages drawn
// into one recording. The same recording becomes the preview, the printout and the PDF, so they
// can't differ.
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { CgChart } from '@cashgear/ui';
import {
  imagePixels,
  imageUrls,
  loadBundledArabic,
  withBundledArabic,
  pageSvg,
  pdfParts,
  Recording,
  RecordingContext,
  recordSheetPages,
  svgToRecording,
  type CellArea,
  type DrawingBox,
  type EngineClient,
  type FontSource,
  type PagesInfo,
  type RecordedPage,
  type SheetSummary,
} from '@cgs/engine-client';
import { chartProps } from '../charts/chartProps';

export type PrintWhat = 'active' | 'workbook' | 'selection';

export interface PrintChoice {
  what: PrintWhat;
  ignorePrintArea: boolean;
}

export interface JobHost {
  engine: EngineClient;
  sheet: number;
  /** 1-based. */
  selection: CellArea[];
  sheets: SheetSummary[];
  /** Tab order, by index. */
  order: number[];
  fileName: string;
  filePath: string;
  fontFamily: string;
  locale: string;
  /** Whether Arabic draws in an installed font or the bundled one, which pages then measure in. */
  arabic: 'system' | 'bundled';
}

export interface PrintJob {
  title: string;
  pages: RecordedPage[];
  recording: Recording;
  /** Each sheet's pages, in print order. */
  sheets: PagesInfo[];
  /** Pages past the engine's limit, which are left out. */
  omitted: number;
  /** How a face's CSS changes to draw as the PDF will. */
  measuring: (css: string) => string;
}

/** The next frame, or a moment when frames don't come, as in a hidden window. */
export function nextFrame(): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, 50);
    requestAnimationFrame(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

/** Pages show the file's own colours, never dark mode's. */
const PAPER = { dark: false, background: 0xffffff, text: 0x000000 };

/** Draws a chart as the screen does, off screen, and records its SVG. */
function chartPainter(engine: EngineClient, sheet: number, locale: string) {
  return async (context: RecordingContext, drawing: DrawingBox) => {
    const view = await engine.chart(sheet, drawing.index, PAPER);
    if (!view) return;
    const host = document.createElement('div');
    host.style.cssText = `position:fixed;left:-100000px;top:0;width:${drawing.width}px;height:${drawing.height}px;pointer-events:none;`;
    host.setAttribute('aria-hidden', 'true');
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      const props = chartProps(view, drawing.width, drawing.height, locale);
      flushSync(() => root.render(<CgChart {...props} ariaLabel={drawing.name} />));
      // Its layout settles over a frame or two.
      await nextFrame();
      await nextFrame();
      const svg = host.querySelector('svg');
      if (svg) svgToRecording(svg, context);
    } finally {
      root.unmount();
      host.remove();
    }
  };
}

/** Which sheets a choice prints. Hidden sheets never print, as in Excel. */
function sheetsFor(host: JobHost, choice: PrintChoice): number[] {
  if (choice.what !== 'workbook') return [host.sheet];
  const order = host.order.length ? host.order : host.sheets.map((_, index) => index);
  return order.filter((index) => {
    const sheet = host.sheets[index];
    return (
      sheet &&
      !sheet.deleted &&
      sheet.state === 'visible' &&
      (sheet.kind === 'worksheet' || sheet.kind === 'chart')
    );
  });
}

function fields(host: JobHost) {
  const now = new Date();
  return {
    file: host.fileName,
    path: host.filePath,
    // Windows' short date, as Excel prints it: the year in full.
    date: new Intl.DateTimeFormat(host.locale, {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
    }).format(now),
    time: new Intl.DateTimeFormat(host.locale, { timeStyle: 'short' }).format(now),
  };
}

/** Lays out and records every page a choice prints. */
export async function makeJob(host: JobHost, choice: PrintChoice): Promise<PrintJob> {
  const { engine } = host;
  const recording = new Recording();
  const measure = new OffscreenCanvas(1, 1).getContext('2d');
  if (!measure) throw new Error('Pages can’t be measured here.');
  let measuring = (css: string) => css;
  if (host.arabic === 'bundled') {
    await loadBundledArabic();
    measuring = withBundledArabic;
  }
  const infos: PagesInfo[] = [];
  // A chart sheet prints its chart on one landscape Letter page, filling it inside Excel's normal
  // margins, as Excel prints one with its page setup untouched.
  const chartSheets = new Map<number, RecordedPage>();
  for (const sheet of sheetsFor(host, choice)) {
    if (host.sheets[sheet]?.kind === 'chart') {
      const drawing = (await engine.drawings(sheet))[0];
      if (!drawing) continue;
      const [width, height] = [792, 612];
      const [left, top] = [0.7 * 72, 0.75 * 72];
      const context = new RecordingContext(recording, measure, [0.75, 0, 0, 0.75, left, top]);
      await chartPainter(
        engine,
        sheet,
        host.locale,
      )(context, {
        ...drawing,
        width: (width - 2 * left) / 0.75,
        height: (height - 2 * top) / 0.75,
      });
      chartSheets.set(sheet, { width, height, ops: context.ops });
      infos.push({ sheet, chart: true } as unknown as PagesInfo);
      continue;
    }
    const info = await engine.query({
      kind: 'pages',
      sheet,
      request: {
        selection: choice.what === 'selection' ? host.selection : [],
        ignorePrintArea: choice.ignorePrintArea,
      },
    });
    if (info.pages.length) infos.push(info);
  }
  const total = infos.reduce(
    (sum, info) => sum + (chartSheets.has(info.sheet) ? 1 : info.pages.length),
    0,
  );
  const pages: RecordedPage[] = [];
  const pictures = new Map<string, number | null>();
  let number = 1;
  for (const info of infos) {
    const chart = chartSheets.get(info.sheet);
    if (chart) {
      pages.push(chart);
      number += 1;
      continue;
    }
    const [layout, drawings] = await Promise.all([
      engine.layout(info.sheet),
      engine.drawings(info.sheet),
    ]);
    const recorded = await recordSheetPages(
      { block: (sheet, row, col) => engine.viewBlock(sheet, row, col) },
      { info, layout, drawings },
      {
        recording,
        measure,
        fontFamily: host.fontFamily,
        measuring,
        fields: fields(host),
        firstNumber: number,
        total,
        drawChart: chartPainter(engine, info.sheet, host.locale),
        async picture(part) {
          if (pictures.has(part)) return pictures.get(part)!;
          let index: number | null;
          try {
            const bytes = await engine.media(part);
            const bitmap = await createImageBitmap(new Blob([bytes.slice()]));
            const pixels = imagePixels(bitmap);
            bitmap.close();
            index = pixels ? recording.image(part, () => pixels) : null;
          } catch {
            index = null;
          }
          pictures.set(part, index);
          return index;
        },
      },
    );
    pages.push(...recorded);
    number = (info.firstPage ?? number) + info.pages.length;
  }
  const title = host.fileName.replace(/\.[^.]*$/, '');
  return {
    title,
    pages,
    recording,
    sheets: infos.filter((info) => !chartSheets.has(info.sheet)),
    omitted: infos.reduce(
      (sum, info) =>
        sum + (chartSheets.has(info.sheet) ? 0 : Math.max(0, info.pageCount - info.pages.length)),
      0,
    ),
    measuring,
  };
}

/** The job as a PDF. */
export async function jobPdf(engine: EngineClient, job: PrintJob, fonts: FontSource) {
  const parts = await pdfParts(job.title, job.pages, job.recording, fonts);
  return engine.writePdf(parts.json, parts.blob);
}

/** The job's pages as SVG, with what to call once they are no longer shown. */
export async function jobSvgs(job: PrintJob): Promise<{ svgs: string[]; release: () => void }> {
  const urls = await imageUrls(job.recording.images);
  const svgs = job.pages.map((page, index) =>
    pageSvg(page, job.recording.faces, urls, 'p' + index, job.measuring),
  );
  return {
    svgs,
    release: () => urls.forEach((url) => url && URL.revokeObjectURL(url)),
  };
}

/** Which installed font the screen draws Arabic with, by measuring it against the candidates. */
export function arabicFamily(fontFamily: string): string | null {
  const context = new OffscreenCanvas(1, 1).getContext('2d');
  if (!context) return null;
  const sample = 'مرحبا بالعالم';
  context.font = '48px "Calibri", Carlito, ' + fontFamily;
  const used = context.measureText(sample).width;
  for (const candidate of [
    'Segoe UI',
    'Arial',
    'Tahoma',
    'Times New Roman',
    'Microsoft Sans Serif',
    'Traditional Arabic',
    'Simplified Arabic',
    'Sakkal Majalla',
  ]) {
    context.font = `48px "${candidate}", serif`;
    if (Math.abs(context.measureText(sample).width - used) < 0.05) return candidate;
  }
  return null;
}
