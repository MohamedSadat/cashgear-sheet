// The embed's side of `sheet-embed.1` (Track S3, S4): it reads the workbook it is given, in memory
// only, and draws one range or chart of it, read-only. It never saves, has no editing command and
// never holds a Workspace credential: the parent hands it a signed URL or the bytes.
import {
  createEngineClient,
  localeData,
  type EngineClient,
  type WorkbookChanges,
} from '@cgs/engine-client';
import { readPalette } from '../theme';
import {
  type Direction,
  type ErrorCode,
  type EmbedMessage,
  type InitMessage,
  type Locale,
  type LoadedMessage,
  type PortMessage,
  type Size,
  type Source,
  type Theme,
  type View,
  MAX_SIZE,
} from './contract';
import {
  catalogue,
  chartScreen,
  resolveView,
  type Catalogue,
  type RangeResolved,
  type Resolved,
} from './book';
import { ChartFigure } from './chart-view';
import { chartPng, rangePng } from './preview';
import { EmbedProtocol, allowedParents, type Answer, type EmbedHandler } from './protocol';
import { splitCopied } from './cells';
import { cssSize, RangeView } from './range-view';
import { EmbedFailure, readSource } from './source';
import { EMBED_TEXTS, browserLocale, type EmbedTexts } from './texts';

interface OpenBook {
  etag: string;
  catalogue: Catalogue;
}

const clampSide = (value: number) => Math.min(MAX_SIZE, Math.max(1, Math.round(value)));

export class EmbedController implements EmbedHandler {
  private engine: EngineClient | null = null;
  private starting: Promise<EngineClient> | null = null;
  private rangeView: RangeView | null = null;
  private readonly chart: ChartFigure;
  private readonly rangeHost: HTMLDivElement;
  private readonly cells: HTMLDivElement;
  /** Reads the light theme's tokens, for pictures in the file's own colours. */
  private readonly lightProbe: HTMLDivElement;
  private book: OpenBook | null = null;
  private shown: Resolved | null = null;
  /** The view last asked for, which a refresh finds again. */
  private view: View | null = null;
  private lastError: ErrorCode | null = null;
  private theme: Theme = 'light';
  private locale: Locale = 'en';
  private readonly fitted = new Set<number>();
  private announced: Size | null = null;
  /** A message is being answered, whose answer says the size itself. */
  private answering = false;
  /** Says a new size to the parent: `resize`. */
  onResize: (size: Size) => void = () => undefined;

  constructor(private readonly root: HTMLElement) {
    root.classList.add('cgs-embed');
    root.setAttribute('role', 'group');
    this.rangeHost = document.createElement('div');
    this.rangeHost.className = 'cgs-embed-range';
    root.appendChild(this.rangeHost);
    this.chart = new ChartFigure(root);
    this.cells = document.createElement('div');
    this.cells.className = 'cgs-embed-cells';
    root.appendChild(this.cells);
    this.lightProbe = document.createElement('div');
    this.lightProbe.dataset.cgTheme = 'light';
    this.lightProbe.hidden = true;
    root.appendChild(this.lightProbe);
    // The engine starts now, so a workbook can open as soon as `init` brings it.
    void this.ensureEngine().catch(() => undefined);
  }

  private get texts(): EmbedTexts {
    return EMBED_TEXTS[this.locale];
  }

  // -------------------------------------------------------------------------------------------
  // Messages

  async init(message: InitMessage): Promise<Answer> {
    this.setLook(message.theme, message.direction, message.locale);
    this.view = message.view;
    return this.load(message.source);
  }

  async handle(message: PortMessage): Promise<Answer | null> {
    switch (message.type) {
      case 'setView':
        this.view = message.view;
        if (!this.book) return this.failed(this.lastError ?? 'source.unavailable');
        return this.draw();
      case 'setTheme':
        this.setLook(message.theme, message.direction, message.locale);
        await this.restyle();
        return null;
      case 'refresh':
        // The version already drawn draws nothing new.
        if (this.book && this.shown && message.source.etag === this.book.etag) {
          return { message: this.loaded() };
        }
        if (this.book && message.source.etag === this.book.etag) return this.draw();
        return this.load(message.source);
      case 'requestPreview':
        return this.preview(message.id, message.maxWidth, message.maxHeight, message.scale);
    }
  }

  // -------------------------------------------------------------------------------------------
  // Reading and drawing

  private ensureEngine(): Promise<EngineClient> {
    if (this.starting) return this.starting;
    const engine = createEngineClient(() => this.lose(engine));
    this.engine = engine;
    this.rangeView = new RangeView(
      this.rangeHost,
      engine,
      readPalette(this.theme === 'dark'),
      () => undefined,
      () => this.checkSize(),
    );
    this.starting = engine
      .init({ inMemoryOnly: true })
      .then(() => engine.setLocale(localeData(navigator.language || 'en-US')))
      .then(() =>
        engine.watchChanges(
          (changes) => this.changed(changes),
          () => undefined,
        ),
      )
      .then(() => engine);
    return this.starting;
  }

  /** A worker stopped: the next workbook gets a new engine. */
  private lose(engine: EngineClient) {
    if (this.engine !== engine) return;
    this.engine = null;
    this.starting = null;
    this.rangeView?.dispose();
    this.rangeView = null;
    this.book = null;
    this.shown = null;
    engine.dispose();
  }

  private async load(source: Source): Promise<Answer> {
    try {
      const blob = await readSource(source);
      const engine = await this.ensureEngine();
      this.clear();
      this.book = null;
      this.fitted.clear();
      this.rangeView?.reset();
      const summary = await this.open(engine, blob);
      this.book = { etag: source.etag, catalogue: await catalogue(engine, summary) };
    } catch (error) {
      this.book = null;
      this.clear();
      return this.failure(error);
    }
    return this.draw();
  }

  /** Opens the bytes and waits until every sheet is read. */
  private open(engine: EngineClient, blob: Blob) {
    return new Promise<Awaited<ReturnType<EngineClient['open']>>>((resolve, reject) => {
      let summary: Awaited<ReturnType<EngineClient['open']>> | null = null;
      let read = false;
      const unreadable = () =>
        reject(new EmbedFailure('source.unavailable', 'This isn’t a workbook Sheet can read.'));
      engine
        .open(
          { kind: 'blob', blob },
          (event) => {
            if (event.type === 'loaded') {
              read = true;
              if (summary) resolve(summary);
            } else if (event.type === 'error') unreadable();
          },
          'workbook.xlsx',
        )
        .then((opened) => {
          summary = opened;
          if (read) resolve(opened);
        }, unreadable);
    });
  }

  /** Draws the view asked for in the open workbook. */
  private async draw(): Promise<Answer> {
    const engine = this.engine;
    const book = this.book;
    if (!engine || !book || !this.view) return this.failed('source.unavailable');
    this.answering = true;
    try {
      const resolved = await resolveView(engine, book.catalogue, this.view, (sheet) =>
        this.prepare(engine, sheet),
      );
      await this.show(engine, resolved);
      this.lastError = null;
      const message = this.loaded();
      this.announced = message.size;
      return { message };
    } catch (error) {
      this.clear();
      return this.failure(error);
    } finally {
      this.answering = false;
    }
  }

  private async prepare(engine: EngineClient, sheet: number) {
    if (this.fitted.has(sheet)) return;
    this.fitted.add(sheet);
    await engine.fitOpeningRows(sheet).catch(() => undefined);
  }

  private async show(engine: EngineClient, resolved: Resolved) {
    this.shown = resolved;
    if (resolved.kind === 'range') {
      this.chart.hide();
      this.rangeView?.setVisible(true);
      this.rangeView?.show({
        sheet: resolved.sheet.index,
        layout: resolved.layout,
        area: resolved.area,
      });
      this.root.setAttribute(
        'aria-label',
        this.texts.range(
          resolved.sheet.name,
          resolved.drawn.kind === 'range' ? resolved.drawn.range : '',
        ),
      );
      void this.describeCells(engine, resolved);
      return;
    }
    this.rangeView?.setVisible(false);
    this.cells.replaceChildren();
    const { chart } = resolved;
    const view = await engine
      .chart(chart.index, chart.drawing.index, chartScreen(readPalette(this.theme === 'dark')))
      .catch(() => null);
    const label = this.texts.chart(chart.title, chart.sheet);
    this.root.setAttribute('aria-label', label);
    this.chart.show(
      view,
      chart.drawing,
      navigator.language || 'en-US',
      label,
      this.texts.unreadable(chart.drawing.name || chart.id),
    );
  }

  /** The range's text as a table, for screen readers; the canvas is only a picture of it. */
  private async describeCells(engine: EngineClient, resolved: RangeResolved) {
    const { area } = resolved;
    const copied = await engine.copyText(resolved.sheet.index, area).catch(() => null);
    if (this.shown !== resolved || !copied) return;
    const table = document.createElement('table');
    const caption = document.createElement('caption');
    caption.textContent = this.texts.cells(
      resolved.sheet.name,
      resolved.drawn.kind === 'range' ? resolved.drawn.range : '',
    );
    table.appendChild(caption);
    const body = document.createElement('tbody');
    for (const row of splitCopied(copied.text)) {
      const line = document.createElement('tr');
      for (const text of row) {
        const cell = document.createElement('td');
        cell.textContent = text;
        line.appendChild(cell);
      }
      body.appendChild(line);
    }
    table.appendChild(body);
    table.dir = resolved.layout.rightToLeft ? 'rtl' : 'ltr';
    this.cells.replaceChildren(table);
  }

  /** Nothing drawn: after an error, before another view. */
  private clear() {
    this.shown = null;
    this.chart.hide();
    this.rangeView?.setVisible(false);
    this.cells.replaceChildren();
    this.root.removeAttribute('aria-label');
  }

  private changed(changes: WorkbookChanges) {
    this.rangeView?.invalidate(changes.sheets, changes.tiles);
    const shown = this.shown;
    if (shown?.kind !== 'range' || !changes.layouts.includes(shown.sheet.index)) return;
    const engine = this.engine;
    if (!engine) return;
    void engine.layout(shown.sheet.index).then((layout) => {
      if (this.shown !== shown) return;
      shown.layout = layout;
      this.rangeView?.show({ sheet: shown.sheet.index, layout, area: shown.area });
      this.checkSize();
    });
  }

  // -------------------------------------------------------------------------------------------
  // Answers

  private size(): Size {
    const shown = this.shown;
    if (!shown) return { width: 1, height: 1 };
    if (shown.kind === 'chart') {
      return {
        width: clampSide(shown.chart.drawing.width),
        height: clampSide(shown.chart.drawing.height),
      };
    }
    const size = cssSize(shown.layout, shown.area);
    // A range wider than the frame scrolls across, and its scrollbar takes room below it.
    const bar = this.rangeView?.overflowsWidth() ? (this.rangeView?.scrollbarHeight() ?? 0) : 0;
    return { width: clampSide(size.width), height: clampSide(size.height + bar) };
  }

  /** Says `resize` when the size changed since the last answer said it. */
  private checkSize() {
    if (this.answering || !this.shown || !this.announced) return;
    const size = this.size();
    if (size.width === this.announced.width && size.height === this.announced.height) return;
    this.announced = size;
    this.onResize(size);
  }

  private loaded(): LoadedMessage {
    const book = this.book!;
    const shown = this.shown!;
    return {
      type: 'loaded',
      etag: book.etag,
      view: shown.drawn,
      sheets: book.catalogue.sheets.map(({ name, direction }) => ({ name, direction })),
      charts: book.catalogue.charts.map(({ id, title, sheet }) => ({ id, title, sheet })),
      size: this.size(),
      capped: shown.kind === 'range' ? shown.capped : false,
    };
  }

  private failed(code: ErrorCode, message = 'Nothing is drawn.', id?: string): Answer {
    this.lastError = code;
    const answer: EmbedMessage = { type: 'error', code, message: message.slice(0, 500) };
    if (id) answer.id = id;
    return { message: answer };
  }

  private failure(error: unknown, id?: string): Answer {
    if (error instanceof EmbedFailure) return this.failed(error.code, error.message, id);
    return this.failed('source.unavailable', 'The workbook couldn’t be shown.', id);
  }

  private async preview(
    id: string,
    maxWidth: number,
    maxHeight: number,
    scale: number,
  ): Promise<Answer> {
    const engine = this.engine;
    const shown = this.shown;
    const book = this.book;
    if (!engine || !shown || !book) {
      const answer = this.failed(this.lastError ?? 'source.unavailable', 'Nothing is drawn.', id);
      return answer;
    }
    try {
      const png =
        shown.kind === 'range'
          ? await rangePng(
              engine,
              shown.sheet.index,
              shown.layout,
              shown.area,
              readPalette(false, this.lightProbe),
              maxWidth,
              maxHeight,
              scale,
            )
          : await chartPng(
              engine,
              shown.chart.index,
              shown.chart.drawing,
              navigator.language || 'en-US',
              maxWidth,
              maxHeight,
              scale,
            );
      return {
        message: {
          type: 'preview',
          id,
          bytes: png.bytes,
          width: clampPreview(png.width),
          height: clampPreview(png.height),
          etag: book.etag,
          view: shown.drawn,
        },
        transfer: [png.bytes],
      };
    } catch {
      return {
        message: {
          type: 'error',
          code: 'source.unavailable',
          message: 'The picture couldn’t be made.',
          id,
        },
      };
    }
  }

  // -------------------------------------------------------------------------------------------
  // Look

  private setLook(theme: Theme, direction: Direction, locale: Locale) {
    this.theme = theme;
    this.locale = locale;
    const html = document.documentElement;
    html.dataset.cgTheme = theme;
    html.dir = direction;
    html.lang = locale;
    this.rangeView?.setPalette(readPalette(theme === 'dark'));
  }

  /** Draws again in the new look: a chart takes the theme's colours from the engine. */
  private async restyle() {
    const engine = this.engine;
    const shown = this.shown;
    if (engine && shown) await this.show(engine, shown);
  }
}

const clampPreview = (value: number) => Math.min(4096, Math.max(1, Math.round(value)));

/** `/embed` opened in a tab of its own: it says what it is for and links to the app. */
function showAlone(root: HTMLElement) {
  const texts = EMBED_TEXTS[browserLocale()];
  document.documentElement.lang = browserLocale();
  document.documentElement.dir = browserLocale() === 'ar' ? 'rtl' : 'ltr';
  const box = document.createElement('div');
  box.className = 'cgs-embed-alone';
  const line = document.createElement('p');
  line.textContent = texts.alone;
  const link = document.createElement('a');
  link.href = '/';
  link.textContent = texts.openSheet;
  box.append(line, link);
  root.appendChild(box);
}

/**
 * Starts the embed in `root`: the handshake with the parent frame, then the view. On its own in a
 * tab, it says what it is for instead.
 */
export function startEmbed(root: HTMLElement): () => void {
  if (window.parent === window) {
    showAlone(root);
    return () => undefined;
  }
  const controller = new EmbedController(root);
  const protocol = new EmbedProtocol(window, allowedParents(location.origin), controller);
  controller.onResize = (size) => protocol.send({ type: 'resize', size });
  protocol.start();
  return () => protocol.dispose();
}
