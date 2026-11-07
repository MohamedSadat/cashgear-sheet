// Turns recorded pages into a PDF: finds a font file for each face the drawing used, packs the
// files and pictures into one blob, and has `crates/cgs-pdf` write it. A face uses the font the
// file names when it is installed, else a bundled font with the same widths, and Arabic falls
// back on the Arabic font the screen draws it with.
import type { RecordedPage } from './print-pages';
import type { FaceRequest, Recording } from './recording';

/** A font file's bytes, and which font in a collection. */
export interface FontBytes {
  data: Uint8Array;
  index?: number;
  name: string;
}

export interface FontSource {
  /** The installed font of a family, if this platform can read it. */
  installed?(family: string, bold: boolean, italic: boolean): Promise<FontBytes | null>;
  /** The installed font the screen draws Arabic with, if this platform can read it. */
  arabic?(bold: boolean): Promise<FontBytes | null>;
}

type Bundled =
  | 'Carlito'
  | 'Caladea'
  | 'LiberationSans'
  | 'LiberationSerif'
  | 'LiberationMono'
  | 'NotoSansArabic';

/** Fonts with the widths of the ones Office uses, which pages fall back on. */
const TWINS: Record<string, Bundled> = {
  calibri: 'Carlito',
  carlito: 'Carlito',
  aptos: 'Carlito',
  'aptos narrow': 'Carlito',
  cambria: 'Caladea',
  caladea: 'Caladea',
  arial: 'LiberationSans',
  helvetica: 'LiberationSans',
  'liberation sans': 'LiberationSans',
  'segoe ui': 'LiberationSans',
  tahoma: 'LiberationSans',
  verdana: 'LiberationSans',
  'sans-serif': 'LiberationSans',
  'times new roman': 'LiberationSerif',
  times: 'LiberationSerif',
  georgia: 'LiberationSerif',
  'liberation serif': 'LiberationSerif',
  serif: 'LiberationSerif',
  'courier new': 'LiberationMono',
  courier: 'LiberationMono',
  consolas: 'LiberationMono',
  'liberation mono': 'LiberationMono',
  monospace: 'LiberationMono',
};

const STYLE_NAMES = ['Regular', 'Bold', 'Italic', 'BoldItalic'] as const;

function bundledUrl(name: Bundled, bold: boolean, italic: boolean): URL {
  const style =
    name === 'NotoSansArabic'
      ? bold
        ? 'Bold'
        : 'Regular'
      : STYLE_NAMES[(bold ? 1 : 0) + (italic ? 2 : 0)]!;
  switch (`${name}-${style}`) {
    case 'Carlito-Regular':
      return new URL('./pdf-fonts/Carlito-Regular.ttf', import.meta.url);
    case 'Carlito-Bold':
      return new URL('./pdf-fonts/Carlito-Bold.ttf', import.meta.url);
    case 'Carlito-Italic':
      return new URL('./pdf-fonts/Carlito-Italic.ttf', import.meta.url);
    case 'Carlito-BoldItalic':
      return new URL('./pdf-fonts/Carlito-BoldItalic.ttf', import.meta.url);
    case 'Caladea-Regular':
      return new URL('./pdf-fonts/Caladea-Regular.ttf', import.meta.url);
    case 'Caladea-Bold':
      return new URL('./pdf-fonts/Caladea-Bold.ttf', import.meta.url);
    case 'Caladea-Italic':
      return new URL('./pdf-fonts/Caladea-Italic.ttf', import.meta.url);
    case 'Caladea-BoldItalic':
      return new URL('./pdf-fonts/Caladea-BoldItalic.ttf', import.meta.url);
    case 'LiberationSans-Regular':
      return new URL('./pdf-fonts/LiberationSans-Regular.ttf', import.meta.url);
    case 'LiberationSans-Bold':
      return new URL('./pdf-fonts/LiberationSans-Bold.ttf', import.meta.url);
    case 'LiberationSans-Italic':
      return new URL('./pdf-fonts/LiberationSans-Italic.ttf', import.meta.url);
    case 'LiberationSans-BoldItalic':
      return new URL('./pdf-fonts/LiberationSans-BoldItalic.ttf', import.meta.url);
    case 'LiberationSerif-Regular':
      return new URL('./pdf-fonts/LiberationSerif-Regular.ttf', import.meta.url);
    case 'LiberationSerif-Bold':
      return new URL('./pdf-fonts/LiberationSerif-Bold.ttf', import.meta.url);
    case 'LiberationSerif-Italic':
      return new URL('./pdf-fonts/LiberationSerif-Italic.ttf', import.meta.url);
    case 'LiberationSerif-BoldItalic':
      return new URL('./pdf-fonts/LiberationSerif-BoldItalic.ttf', import.meta.url);
    case 'LiberationMono-Regular':
      return new URL('./pdf-fonts/LiberationMono-Regular.ttf', import.meta.url);
    case 'LiberationMono-Bold':
      return new URL('./pdf-fonts/LiberationMono-Bold.ttf', import.meta.url);
    case 'LiberationMono-Italic':
      return new URL('./pdf-fonts/LiberationMono-Italic.ttf', import.meta.url);
    case 'LiberationMono-BoldItalic':
      return new URL('./pdf-fonts/LiberationMono-BoldItalic.ttf', import.meta.url);
    case 'NotoSansArabic-Bold':
      return new URL('./pdf-fonts/NotoSansArabic-Bold.ttf', import.meta.url);
    default:
      return new URL('./pdf-fonts/NotoSansArabic-Regular.ttf', import.meta.url);
  }
}

const fetched = new Map<string, Promise<FontBytes>>();

/** A bundled font, fetched the first time a page needs it. */
export function bundledFont(name: Bundled, bold: boolean, italic: boolean): Promise<FontBytes> {
  const url = bundledUrl(name, bold, italic);
  let font = fetched.get(url.href);
  if (!font) {
    font = fetch(url).then(async (response) => {
      if (!response.ok) throw new Error('A font for the PDF couldn’t be loaded.');
      return { data: new Uint8Array(await response.arrayBuffer()), name };
    });
    font.catch(() => fetched.delete(url.href));
    fetched.set(url.href, font);
  }
  return font;
}

/** The family pages measure Arabic in when the PDF embeds the bundled Arabic font. */
export const BUNDLED_ARABIC = 'CgsPdfArabic';
const ARABIC_RANGE = 'U+0600-06FF, U+0750-077F, U+08A0-08FF, U+FB50-FDFF, U+FE70-FEFF';
let arabicFaces: Promise<void> | null = null;

/** Loads the bundled Arabic font for measuring, so cells fit as the PDF draws them. */
export function loadBundledArabic(): Promise<void> {
  arabicFaces ??= Promise.all(
    [false, true].map(async (bold) => {
      const face = new FontFace(
        BUNDLED_ARABIC,
        `url(${bundledUrl('NotoSansArabic', bold, false).href})`,
        {
          unicodeRange: ARABIC_RANGE,
          weight: bold ? '700' : '400',
        },
      );
      await face.load();
      document.fonts.add(face);
    }),
  ).then(() => undefined);
  arabicFaces.catch(() => {
    arabicFaces = null;
  });
  return arabicFaces;
}

/**
 * A CSS font with the bundled Arabic font first. It covers Arabic only, so other text measures
 * in the families after it, whose widths the PDF's fonts share.
 */
export function withBundledArabic(css: string): string {
  const match = /^(.*?\d(?:\.\d+)?px\s+)(.*)$/.exec(css);
  if (!match || css.includes(BUNDLED_ARABIC)) return css;
  return match[1]! + '"' + BUNDLED_ARABIC + '", ' + match[2]!;
}

/** What the document gives `cgs_pdf::Document`, and the bytes it points into. */
export interface PdfParts {
  json: string;
  blob: Uint8Array;
}

/** Each face's files, in the order characters fall back through them. */
async function resolveFaces(faces: FaceRequest[], source: FontSource) {
  const files: FontBytes[] = [];
  const known = new Map<FontBytes, number>();
  const add = (font: FontBytes) => {
    let index = known.get(font);
    if (index === undefined) {
      index = files.length;
      files.push(font);
      known.set(font, index);
    }
    return index;
  };
  const installed = new Map<string, Promise<FontBytes | null>>();
  const installedFont = (family: string, bold: boolean, italic: boolean) => {
    const key = family.toLowerCase() + '|' + bold + '|' + italic;
    let font = installed.get(key);
    if (!font) {
      font = source.installed?.(family, bold, italic).catch(() => null) ?? Promise.resolve(null);
      installed.set(key, font);
    }
    return font;
  };
  const arabicFonts = new Map<boolean, Promise<FontBytes>>();
  const arabic = (bold: boolean) => {
    let font = arabicFonts.get(bold);
    if (!font) {
      font = (source.arabic?.(bold).catch(() => null) ?? Promise.resolve(null)).then(
        (found) => found ?? bundledFont('NotoSansArabic', bold, false),
      );
      arabicFonts.set(bold, font);
    }
    return font;
  };
  const chains: number[][] = [];
  for (const face of faces) {
    const chain: number[] = [];
    for (const family of face.families) {
      const lower = family.toLowerCase();
      const generic = lower === 'sans-serif' || lower === 'serif' || lower === 'monospace';
      const own = generic ? null : await installedFont(family, face.bold, face.italic);
      if (own) {
        chain.push(add(own));
        break;
      }
      const twin = TWINS[lower];
      if (twin) {
        chain.push(add(await bundledFont(twin, face.bold, face.italic)));
        break;
      }
    }
    chain.push(add(await arabic(face.bold)));
    // Anything else falls back on a sans font, as the screen does.
    chain.push(add(await bundledFont('LiberationSans', face.bold, face.italic)));
    chains.push([...new Set(chain)]);
  }
  return { files, chains };
}

/** Packs recorded pages into what `writePdf` takes. */
export async function pdfParts(
  title: string,
  pages: RecordedPage[],
  recording: Recording,
  source: FontSource,
): Promise<PdfParts> {
  const { files, chains } = await resolveFaces(recording.faces, source);
  const pieces: Uint8Array[] = [];
  let offset = 0;
  const place = (data: Uint8Array) => {
    const at = offset;
    pieces.push(data);
    offset += data.length;
    return at;
  };
  const fileRefs = files.map((file) => ({
    offset: place(file.data),
    length: file.data.length,
    index: file.index ?? 0,
    name: file.name,
  }));
  const imageRefs = recording.images.map((image) => ({
    offset: place(image.data),
    length: image.data.length,
    width: image.width,
    height: image.height,
    kind: image.kind,
  }));
  const blob = new Uint8Array(offset);
  let at = 0;
  for (const piece of pieces) {
    blob.set(piece, at);
    at += piece.length;
  }
  const json = JSON.stringify({
    title,
    pages,
    files: fileRefs,
    faces: chains.map((chain) => ({ chain })),
    images: imageRefs,
  });
  return { json, blob };
}

/** A face at a size as a CSS font, for playing pages back on a canvas. */
export function faceCss(face: FaceRequest | undefined, size: number): string {
  const families = face
    ? face.families.map((f) => (/^(sans-serif|serif|monospace)$/.test(f) ? f : `"${f}"`)).join(', ')
    : 'sans-serif';
  return (
    (face?.italic ? 'italic ' : '') +
    (face?.bold ? 'bold ' : '') +
    size.toFixed(2) +
    'px ' +
    families
  );
}
