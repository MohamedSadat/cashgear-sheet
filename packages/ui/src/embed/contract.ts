// Cash Gear Workspace's `sheet-embed.1` (its docs/INTEGRATIONS.md, "Contract `sheet-embed.1`",
// at Workspace's 99aef32): what Workspace's page and Sheet's `/embed` frame say to each other.
// The validators check every message both ways; a message that fails is dropped and never logged
// (tests/contracts/sheet-embed-vectors.json, Workspace's vectors, is the suite both repos run).

export const CONTRACT = 'sheet-embed.1';
/** Every version this embed speaks, as `ready` lists them. */
export const CONTRACTS: readonly string[] = [CONTRACT];

/** Limits agreed on 2026-11-14 (Workspace's decisions, Phase 5). */
export const MAX_WORKBOOK_BYTES = 20 * 1024 * 1024;
export const MAX_CELLS = 5000;
export const MAX_PREVIEW_BYTES = 2 * 1024 * 1024;
/** A `size`'s sides, in CSS pixels. */
export const MAX_SIZE = 100_000;

export type Source =
  /** A signed URL to exactly one version's xlsx, good for about 5 minutes. */
  | { kind: 'url'; url: string; etag: string }
  /** Transferred; for a desktop that has the bytes. */
  | { kind: 'bytes'; bytes: ArrayBuffer; etag: string };

export type View =
  /** A null sheet is the first visible; a null range the used range, cut to the cell limit. */
  | { kind: 'range'; sheet: string | null; range: string | null }
  | { kind: 'chart'; sheet: string | null; chartId: string };

export type Theme = 'light' | 'dark';
/** The page's; each sheet keeps its own in `sheets`. */
export type Direction = 'ltr' | 'rtl';
export type Locale = 'en' | 'ar';

/** CSS pixels, positive integers, at most 100,000. */
export interface Size {
  width: number;
  height: number;
}

export type ErrorCode =
  | 'source.unavailable'
  | 'source.forbidden'
  | 'view.notFound'
  | 'workbook.tooLarge'
  | 'range.tooLarge'
  | 'contract.unsupported';

export const ERROR_CODES: readonly ErrorCode[] = [
  'source.unavailable',
  'source.forbidden',
  'view.notFound',
  'workbook.tooLarge',
  'range.tooLarge',
  'contract.unsupported',
];

export interface InitMessage {
  type: 'init';
  contract: string;
  source: Source;
  view: View;
  theme: Theme;
  direction: Direction;
  locale: Locale;
}

export type ParentMessage =
  | InitMessage
  | { type: 'setView'; view: View }
  | { type: 'setTheme'; theme: Theme; direction: Direction; locale: Locale }
  | { type: 'refresh'; source: Source }
  | { type: 'requestPreview'; id: string; maxWidth: number; maxHeight: number; scale: number };

/** A port message from the parent: every message but `init`, which comes on the window. */
export type PortMessage = Exclude<ParentMessage, InitMessage>;

export interface SheetEntry {
  name: string;
  direction: Direction;
}

export interface ChartEntry {
  id: string;
  title: string;
  sheet: string;
}

/** A view as drawn: its sheet, and its range or chart, resolved. */
export type DrawnView =
  | { kind: 'range'; sheet: string; range: string }
  | { kind: 'chart'; sheet: string; chartId: string };

export interface LoadedMessage {
  type: 'loaded';
  etag: string;
  view: DrawnView;
  sheets: SheetEntry[];
  charts: ChartEntry[];
  size: Size;
  /** A null range was cut to the cell limit. */
  capped: boolean;
}

export interface PreviewMessage {
  type: 'preview';
  id: string;
  /** A PNG, transferred. */
  bytes: ArrayBuffer;
  width: number;
  height: number;
  etag: string;
  view: DrawnView;
}

export interface ErrorMessage {
  type: 'error';
  code: ErrorCode;
  /** For the log only; Workspace shows its own text by `code`. */
  message: string;
  /** Only in answer to `requestPreview`. */
  id?: string;
}

export type EmbedMessage =
  | { type: 'ready'; contracts: string[] }
  | LoadedMessage
  | { type: 'resize'; size: Size }
  | PreviewMessage
  | { type: 'openRequested' }
  | ErrorMessage;

// ---------------------------------------------------------------------------------------------
// Field rules, as Workspace's validators have them (its packages/editor/src/sheet/contract.ts)

type Fields = Record<string, unknown>;

/** `ready` lists at most 16 contracts, each named in 1 to 64 characters. */
const MAX_CONTRACTS = 16;
const MAX_CONTRACT_NAME = 64;
const MAX_CHART_TITLE = 255;
const MAX_MESSAGE = 500;

/** A plain object, as a structured clone of one arrives: not an array, a class or a function. */
export function isPlainObject(value: unknown): value is Fields {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const proto: unknown = Object.getPrototypeOf(value);
  if (proto !== null && Object.getPrototypeOf(proto) !== null) return false;
  return Object.prototype.toString.call(value) === '[object Object]';
}

/**
 * A plain object whose own keys are exactly `required` and any of `optional`, each a data
 * property, read into a map of its own; null otherwise. An extra key, `__proto__` among them,
 * fails the message.
 */
function fields(
  data: unknown,
  required: readonly string[],
  optional: readonly string[] = [],
): Fields | null {
  if (!isPlainObject(data)) return null;
  const out = Object.create(null) as Fields;
  let seen = 0;
  for (const key of Reflect.ownKeys(data)) {
    if (typeof key !== 'string') return null;
    const needed = required.includes(key);
    if (!needed && !optional.includes(key)) return null;
    const descriptor = Object.getOwnPropertyDescriptor(data, key);
    if (!descriptor || !('value' in descriptor)) return null;
    out[key] = descriptor.value;
    if (needed) seen++;
  }
  return seen === required.length ? out : null;
}

/** An object's own string field, read without touching its prototype. */
function own(data: unknown, key: string): string | null {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) return null;
  const descriptor = Object.getOwnPropertyDescriptor(data, key);
  return descriptor && typeof descriptor.value === 'string' ? descriptor.value : null;
}

const isText = (value: unknown, min: number, max: number): value is string =>
  typeof value === 'string' && value.length >= min && value.length <= max;

const isInteger = (value: unknown, min: number, max: number): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;

/**
 * `https://` to any host, or `http://` to 127.0.0.1 or localhost; visible ASCII only, at most
 * 4,096 characters, with no user name or password.
 */
export function validUrl(value: unknown): value is string {
  if (!isText(value, 1, 4096) || !/^[\x21-\x7e]+$/.test(value)) return false;
  const https = value.startsWith('https://');
  if (!https && !value.startsWith('http://')) return false;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }
  if (url.username !== '' || url.password !== '') return false;
  if (https) return url.protocol === 'https:' && url.hostname !== '';
  return url.protocol === 'http:' && (url.hostname === '127.0.0.1' || url.hostname === 'localhost');
}

/** A strong entity tag of visible ASCII, 3 to 256 characters with its quotes. */
export function validEtag(value: unknown): value is string {
  return isText(value, 3, 256) && /^"[\x21\x23-\x7e]+"$/.test(value);
}

/** A sheet's name as Excel allows it: 1 to 31 UTF-16 code units, none of `[ ] : * ? / \`. */
export function validSheetName(value: unknown): value is string {
  return isText(value, 1, 31) && !/[[\]:*?/\\]/.test(value);
}

const CELL = /^([A-Z]{1,3})([1-9][0-9]{0,6})$/;
export const MAX_ROW = 1_048_576;
export const MAX_COL = 16_384;

/** A column's letters as a number from 1, or 0 past XFD. */
export function columnNumber(letters: string): number {
  let number = 0;
  for (const letter of letters) number = number * 26 + (letter.charCodeAt(0) - 64);
  return number <= MAX_COL ? number : 0;
}

/** One cell's address, `A1` to `XFD1048576`, without a sheet or `$`. 1-based. */
export function parseCell(text: string): { row: number; col: number } | null {
  const match = CELL.exec(text);
  if (!match) return null;
  const col = columnNumber(match[1]!);
  const row = Number(match[2]);
  if (!col || row > MAX_ROW) return null;
  return { row, col };
}

/** A1 notation without a sheet or `$`: `A1` or `A1:D20`, corners in either order. */
export function validRange(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 21) return false;
  const parts = value.split(':');
  if (parts.length > 2) return false;
  return parts.every((part) => parseCell(part) !== null);
}

/** A range as a drawn view gives it: top-left first, and one cell as `A1`. */
function canonical(value: string): boolean {
  const [a, b] = value.split(':').map(parseCell);
  if (!a) return false;
  if (!b) return true;
  return a.row <= b.row && a.col <= b.col && (a.row !== b.row || a.col !== b.col);
}

const validChartId = (value: unknown): value is string => isText(value, 1, 128);

export const validId = (value: unknown): value is string =>
  typeof value === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value);

const validSide = (value: unknown): value is number => isInteger(value, 16, 4096);

const validScale = (value: unknown): value is number =>
  value === 1 || value === 1.5 || value === 2 || value === 3;

const validTheme = (value: unknown): value is Theme => value === 'light' || value === 'dark';
const validDirection = (value: unknown): value is Direction => value === 'ltr' || value === 'rtl';
const validLocale = (value: unknown): value is Locale => value === 'en' || value === 'ar';

/** An `ArrayBuffer` from any realm, never a shared one. */
export function isArrayBuffer(value: unknown): value is ArrayBuffer {
  return Object.prototype.toString.call(value) === '[object ArrayBuffer]';
}

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** A preview's bytes: a PNG, at most 2 MB. */
export function validPng(value: unknown): value is ArrayBuffer {
  if (!isArrayBuffer(value) || value.byteLength < PNG.length) return false;
  if (value.byteLength > MAX_PREVIEW_BYTES) return false;
  const head = new Uint8Array(value, 0, PNG.length);
  return PNG.every((byte, index) => head[index] === byte);
}

function parseSize(value: unknown): Size | null {
  const f = fields(value, ['width', 'height']);
  if (!f || !isInteger(f.width, 1, MAX_SIZE) || !isInteger(f.height, 1, MAX_SIZE)) return null;
  return { width: f.width, height: f.height };
}

function parseSource(value: unknown): Source | null {
  const kind = own(value, 'kind');
  if (kind === 'url') {
    const f = fields(value, ['kind', 'url', 'etag']);
    if (!f || !validUrl(f.url) || !validEtag(f.etag)) return null;
    return { kind, url: f.url, etag: f.etag };
  }
  if (kind === 'bytes') {
    const f = fields(value, ['kind', 'bytes', 'etag']);
    if (!f || !isArrayBuffer(f.bytes) || !validEtag(f.etag)) return null;
    return { kind, bytes: f.bytes, etag: f.etag };
  }
  return null;
}

function parseView(value: unknown): View | null {
  const kind = own(value, 'kind');
  const sheetOk = (sheet: unknown) => sheet === null || validSheetName(sheet);
  if (kind === 'range') {
    const f = fields(value, ['kind', 'sheet', 'range']);
    if (!f || !sheetOk(f.sheet) || !(f.range === null || validRange(f.range))) return null;
    return { kind, sheet: f.sheet as string | null, range: f.range as string | null };
  }
  if (kind === 'chart') {
    const f = fields(value, ['kind', 'sheet', 'chartId']);
    if (!f || !sheetOk(f.sheet) || !validChartId(f.chartId)) return null;
    return { kind, sheet: f.sheet as string | null, chartId: f.chartId };
  }
  return null;
}

/** A view the embed drew: its sheet named, its range top-left first. */
function parseDrawnView(value: unknown): DrawnView | null {
  const kind = own(value, 'kind');
  if (kind === 'range') {
    const f = fields(value, ['kind', 'sheet', 'range']);
    if (!f || !validSheetName(f.sheet) || !validRange(f.range) || !canonical(f.range)) return null;
    return { kind, sheet: f.sheet, range: f.range };
  }
  if (kind === 'chart') {
    const f = fields(value, ['kind', 'sheet', 'chartId']);
    if (!f || !validSheetName(f.sheet) || !validChartId(f.chartId)) return null;
    return { kind, sheet: f.sheet, chartId: f.chartId };
  }
  return null;
}

function parseList<T>(value: unknown, max: number, item: (entry: unknown) => T | null): T[] | null {
  if (!Array.isArray(value) || value.length > max) return null;
  const out: T[] = [];
  for (const entry of value) {
    const parsed = item(entry);
    if (parsed === null) return null;
    out.push(parsed);
  }
  return out;
}

const looks = (f: Fields) =>
  validTheme(f.theme) && validDirection(f.direction) && validLocale(f.locale);

// ---------------------------------------------------------------------------------------------
// Messages

/**
 * The `contract` an `init` asks for, if the data is shaped as an `init` at all, so an `init` of a
 * version this embed doesn't speak can be answered `contract.unsupported`, whatever its other
 * fields hold.
 */
export function initContract(data: unknown): string | null {
  if (own(data, 'type') !== 'init') return null;
  const contract = own(data, 'contract');
  return contract !== null && isText(contract, 1, MAX_CONTRACT_NAME) ? contract : null;
}

/** A parent's message, `init` included, or null when it fails validation. */
export function parseParentMessage(data: unknown): ParentMessage | null {
  switch (own(data, 'type')) {
    case 'init': {
      const f = fields(data, [
        'type',
        'contract',
        'source',
        'view',
        'theme',
        'direction',
        'locale',
      ]);
      if (!f || f.contract !== CONTRACT || !looks(f)) return null;
      const source = parseSource(f.source);
      const view = parseView(f.view);
      if (!source || !view) return null;
      return {
        type: 'init',
        contract: CONTRACT,
        source,
        view,
        theme: f.theme as Theme,
        direction: f.direction as Direction,
        locale: f.locale as Locale,
      };
    }
    case 'setView': {
      const f = fields(data, ['type', 'view']);
      const view = f && parseView(f.view);
      return view ? { type: 'setView', view } : null;
    }
    case 'setTheme': {
      const f = fields(data, ['type', 'theme', 'direction', 'locale']);
      if (!f || !looks(f)) return null;
      return {
        type: 'setTheme',
        theme: f.theme as Theme,
        direction: f.direction as Direction,
        locale: f.locale as Locale,
      };
    }
    case 'refresh': {
      const f = fields(data, ['type', 'source']);
      const source = f && parseSource(f.source);
      return source ? { type: 'refresh', source } : null;
    }
    case 'requestPreview': {
      const f = fields(data, ['type', 'id', 'maxWidth', 'maxHeight', 'scale']);
      if (!f || !validId(f.id) || !validSide(f.maxWidth) || !validSide(f.maxHeight)) return null;
      if (!validScale(f.scale)) return null;
      return {
        type: 'requestPreview',
        id: f.id,
        maxWidth: f.maxWidth,
        maxHeight: f.maxHeight,
        scale: f.scale,
      };
    }
    default:
      return null;
  }
}

/** A message on the port from the parent: anything but `init`. */
export function parsePortMessage(data: unknown): PortMessage | null {
  const message = parseParentMessage(data);
  return message && message.type !== 'init' ? message : null;
}

/** An embed's message, as the parent checks it, or null when it fails validation. */
export function parseEmbedMessage(data: unknown): EmbedMessage | null {
  switch (own(data, 'type')) {
    case 'ready': {
      const f = fields(data, ['type', 'contracts']);
      const contracts =
        f &&
        parseList(f.contracts, MAX_CONTRACTS, (entry) =>
          isText(entry, 1, MAX_CONTRACT_NAME) ? entry : null,
        );
      return contracts && contracts.length > 0 ? { type: 'ready', contracts } : null;
    }
    case 'loaded': {
      const f = fields(data, ['type', 'etag', 'view', 'sheets', 'charts', 'size', 'capped']);
      if (!f || !validEtag(f.etag) || typeof f.capped !== 'boolean') return null;
      const view = parseDrawnView(f.view);
      const size = parseSize(f.size);
      const sheets = parseList(f.sheets, Infinity, (entry): SheetEntry | null => {
        const g = fields(entry, ['name', 'direction']);
        return g && validSheetName(g.name) && validDirection(g.direction)
          ? { name: g.name, direction: g.direction }
          : null;
      });
      const charts = parseList(f.charts, Infinity, (entry): ChartEntry | null => {
        const g = fields(entry, ['id', 'title', 'sheet']);
        if (!g || !validChartId(g.id) || !isText(g.title, 1, MAX_CHART_TITLE)) return null;
        return validSheetName(g.sheet) ? { id: g.id, title: g.title, sheet: g.sheet } : null;
      });
      if (!view || !size || !sheets || !charts) return null;
      return { type: 'loaded', etag: f.etag, view, sheets, charts, size, capped: f.capped };
    }
    case 'resize': {
      const f = fields(data, ['type', 'size']);
      const size = f && parseSize(f.size);
      return size ? { type: 'resize', size } : null;
    }
    case 'preview': {
      const f = fields(data, ['type', 'id', 'bytes', 'width', 'height', 'etag', 'view']);
      if (!f || !validId(f.id) || !validPng(f.bytes) || !validEtag(f.etag)) return null;
      if (!isInteger(f.width, 1, 4096) || !isInteger(f.height, 1, 4096)) return null;
      const view = parseDrawnView(f.view);
      if (!view) return null;
      return {
        type: 'preview',
        id: f.id,
        bytes: f.bytes,
        width: f.width,
        height: f.height,
        etag: f.etag,
        view,
      };
    }
    case 'openRequested':
      return fields(data, ['type']) ? { type: 'openRequested' } : null;
    case 'error': {
      const f = fields(data, ['type', 'code', 'message'], ['id']);
      if (!f || !ERROR_CODES.includes(f.code as ErrorCode)) return null;
      if (!isText(f.message, 0, MAX_MESSAGE)) return null;
      if ('id' in f && !validId(f.id)) return null;
      const message: ErrorMessage = {
        type: 'error',
        code: f.code as ErrorCode,
        message: f.message,
      };
      if ('id' in f) message.id = f.id as string;
      return message;
    }
    default:
      return null;
  }
}

/** A chart's title as `loaded` carries it: 1 to 255 characters. */
export const chartTitleText = (title: string) => title.slice(0, MAX_CHART_TITLE);
