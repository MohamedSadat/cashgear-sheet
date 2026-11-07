// Recorded pages as SVG, for the print preview and for printing through the browser or the
// system's dialog: the same paths and text the PDF gets, drawn by the browser with the fonts the
// screen uses.
import { faceCss } from './pdf-document';
import type { RecordedPage } from './print-pages';
import type { FaceRequest, PdfOp, RecordedImage } from './recording';

const escape = (text: string) =>
  text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

const n = (value: number) => (Math.round(value * 1000) / 1000).toString();

function pathData(d: number[]): string {
  const out: string[] = [];
  let at = 0;
  while (at < d.length) {
    switch (d[at]) {
      case 0:
        out.push('M' + n(d[at + 1]!) + ' ' + n(d[at + 2]!));
        at += 3;
        break;
      case 1:
        out.push('L' + n(d[at + 1]!) + ' ' + n(d[at + 2]!));
        at += 3;
        break;
      case 2:
        out.push(
          'C' +
            d
              .slice(at + 1, at + 7)
              .map(n)
              .join(' '),
        );
        at += 7;
        break;
      case 3:
        out.push('Z');
        at += 1;
        break;
      case 4: {
        const [x, y, w, h] = d.slice(at + 1, at + 5) as [number, number, number, number];
        out.push(`M${n(x)} ${n(y)}h${n(w)}v${n(h)}h${n(-w)}Z`);
        at += 5;
        break;
      }
      default:
        at = d.length;
    }
  }
  return out.join('');
}

/** `#rrggbbaa` as a colour and an opacity. */
function color(value: string): [string, number] {
  const hex = value.replace('#', '');
  const alpha = hex.length >= 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1;
  return ['#' + hex.slice(0, 6), alpha];
}

/** Pictures as URLs an SVG can show. */
export async function imageUrls(images: RecordedImage[]): Promise<string[]> {
  const urls: string[] = [];
  for (const image of images) {
    if (image.kind === 'jpeg') {
      urls.push(URL.createObjectURL(new Blob([image.data.slice()], { type: 'image/jpeg' })));
      continue;
    }
    const canvas = new OffscreenCanvas(image.width, image.height);
    const context = canvas.getContext('2d');
    if (!context) {
      urls.push('');
      continue;
    }
    const pixels = new ImageData(new Uint8ClampedArray(image.data), image.width, image.height);
    context.putImageData(pixels, 0, 0);
    urls.push(URL.createObjectURL(await canvas.convertToBlob({ type: 'image/png' })));
  }
  return urls;
}

/** One page as SVG, sized in points. `id` keeps its clip paths apart from other pages'. */
export function pageSvg(
  page: RecordedPage,
  faces: FaceRequest[],
  images: string[],
  id: string,
  /** Changes a face's CSS, as when Arabic is drawn in the bundled font. */
  measuring: (css: string) => string = (css) => css,
): string {
  const defs: string[] = [];
  const body: string[] = [];
  // How many groups each save level has opened, so a restore closes them.
  const opened: number[] = [0];
  let next = 0;
  const fresh = () => `${id}-${next++}`;
  const paint = (fill: Extract<PdfOp, { k: 'path' }>['fill']): string => {
    if (!fill) return ' fill="none"';
    if (typeof fill === 'string') {
      const [rgb, alpha] = color(fill);
      return ` fill="${rgb}"` + (alpha < 1 ? ` fill-opacity="${n(alpha)}"` : '');
    }
    const ref = fresh();
    if ('linear' in fill) {
      const [x1, y1, x2, y2] = fill.linear;
      const stops = fill.stops
        .map(([offset, stop]) => {
          const [rgb, alpha] = color(stop);
          return `<stop offset="${n(offset)}" stop-color="${rgb}" stop-opacity="${n(alpha)}"/>`;
        })
        .join('');
      defs.push(
        `<linearGradient id="${ref}" gradientUnits="userSpaceOnUse" x1="${n(x1)}" y1="${n(y1)}" x2="${n(x2)}" y2="${n(y2)}">${stops}</linearGradient>`,
      );
    } else {
      defs.push(
        `<pattern id="${ref}" patternUnits="userSpaceOnUse" x="${n(fill.x)}" y="${n(fill.y)}" width="${n(fill.w)}" height="${n(fill.h)}"><image href="${images[fill.image] ?? ''}" width="${n(fill.w)}" height="${n(fill.h)}" preserveAspectRatio="none" style="image-rendering:pixelated"/></pattern>`,
      );
    }
    return ` fill="url(#${ref})"`;
  };
  for (const op of page.ops) {
    switch (op.k) {
      case 'save':
        opened.push(0);
        break;
      case 'restore': {
        const count = opened.pop() ?? 0;
        for (let i = 0; i < count; i++) body.push('</g>');
        if (!opened.length) opened.push(0);
        break;
      }
      case 'clip': {
        const ref = fresh();
        defs.push(
          `<clipPath id="${ref}"><path d="${pathData(op.d)}"${op.evenodd ? ' clip-rule="evenodd"' : ''}/></clipPath>`,
        );
        body.push(`<g clip-path="url(#${ref})">`);
        opened[opened.length - 1]!++;
        break;
      }
      case 'path': {
        let attrs = ` d="${pathData(op.d)}"` + paint(op.fill);
        if (op.evenodd) attrs += ' fill-rule="evenodd"';
        if (op.stroke) {
          const [rgb, alpha] = color(op.stroke);
          attrs += ` stroke="${rgb}" stroke-width="${n(op.lw ?? 1)}"`;
          if (alpha < 1) attrs += ` stroke-opacity="${n(alpha)}"`;
          if (op.dash?.length) attrs += ` stroke-dasharray="${op.dash.map(n).join(' ')}"`;
          if (op.cap && op.cap !== 'butt') attrs += ` stroke-linecap="${op.cap}"`;
          if (op.join && op.join !== 'miter') attrs += ` stroke-linejoin="${op.join}"`;
        }
        body.push(`<path${attrs}/>`);
        break;
      }
      case 'text': {
        const face = faces[op.face];
        const [rgb, alpha] = color(op.fill);
        const rtl = Boolean(op.rtl);
        const anchor =
          op.align === 'center'
            ? 'middle'
            : op.align === 'right'
              ? rtl
                ? 'start'
                : 'end'
              : op.align === 'left'
                ? rtl
                  ? 'end'
                  : 'start'
                : op.align === 'end'
                  ? 'end'
                  : 'start';
        const baseline =
          op.base === 'middle'
            ? 'middle'
            : op.base === 'top' || op.base === 'hanging'
              ? 'text-before-edge'
              : op.base === 'bottom' || op.base === 'ideographic'
                ? 'text-after-edge'
                : null;
        const font = measuring(faceCss(face, op.size));
        body.push(
          `<text transform="matrix(${op.m.map(n).join(' ')})" x="${n(op.x)}" y="${n(op.y)}" style="font:${escape(font)};white-space:pre" fill="${rgb}"` +
            (alpha < 1 ? ` fill-opacity="${n(alpha)}"` : '') +
            (rtl ? ' direction="rtl"' : '') +
            (anchor !== 'start' ? ` text-anchor="${anchor}"` : '') +
            (baseline ? ` dominant-baseline="${baseline}"` : '') +
            `>${escape(op.t)}</text>`,
        );
        break;
      }
      case 'image':
        body.push(
          `<image transform="matrix(${op.m.map(n).join(' ')})" width="1" height="1" preserveAspectRatio="none" href="${images[op.image] ?? ''}"/>`,
        );
        break;
    }
  }
  for (const count of opened) for (let i = 0; i < count; i++) body.push('</g>');
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${n(page.width)} ${n(page.height)}" width="${n(page.width)}pt" height="${n(page.height)}pt">` +
    `<defs>${defs.join('')}</defs><rect width="100%" height="100%" fill="#ffffff"/>${body.join('')}</svg>`
  );
}
