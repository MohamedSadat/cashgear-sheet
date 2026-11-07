// Draws an SVG that is on the page, as the browser laid it out, into a recording: shapes as paths
// and text as text, so a chart prints as vectors. Styles come from the computed style, places from
// each element's matrix, and text from where the browser put each run.
import type { RecordingContext } from './recording';

type Ctx = RecordingContext;

const SKIPPED = new Set([
  'defs',
  'clipPath',
  'mask',
  'title',
  'desc',
  'metadata',
  'style',
  'script',
]);

function numbers(text: string): number[] {
  return (text.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?/gi) ?? []).map(Number);
}

/** The endpoint form of an SVG arc as a canvas ellipse. */
function arcTo(
  c: Ctx,
  x1: number,
  y1: number,
  rxIn: number,
  ryIn: number,
  angle: number,
  large: boolean,
  sweep: boolean,
  x2: number,
  y2: number,
) {
  let rx = Math.abs(rxIn);
  let ry = Math.abs(ryIn);
  if (!rx || !ry || (x1 === x2 && y1 === y2)) {
    c.lineTo(x2, y2);
    return;
  }
  const phi = (angle * Math.PI) / 180;
  const cos = Math.cos(phi);
  const sin = Math.sin(phi);
  const dx = (x1 - x2) / 2;
  const dy = (y1 - y2) / 2;
  const xp = cos * dx + sin * dy;
  const yp = -sin * dx + cos * dy;
  const lambda = (xp * xp) / (rx * rx) + (yp * yp) / (ry * ry);
  if (lambda > 1) {
    rx *= Math.sqrt(lambda);
    ry *= Math.sqrt(lambda);
  }
  const sign = large === sweep ? -1 : 1;
  const numerator = rx * rx * ry * ry - rx * rx * yp * yp - ry * ry * xp * xp;
  const factor = sign * Math.sqrt(Math.max(0, numerator) / (rx * rx * yp * yp + ry * ry * xp * xp));
  const cxp = (factor * rx * yp) / ry;
  const cyp = (-factor * ry * xp) / rx;
  const cx = cos * cxp - sin * cyp + (x1 + x2) / 2;
  const cy = sin * cxp + cos * cyp + (y1 + y2) / 2;
  const angleOf = (ux: number, uy: number) => Math.atan2(uy, ux);
  const start = angleOf((xp - cxp) / rx, (yp - cyp) / ry);
  let delta = angleOf((-xp - cxp) / rx, (-yp - cyp) / ry) - start;
  if (sweep && delta < 0) delta += Math.PI * 2;
  if (!sweep && delta > 0) delta -= Math.PI * 2;
  c.ellipse(cx, cy, rx, ry, phi, start, start + delta, !sweep);
}

/** Traces SVG path data. */
export function tracePath(c: Ctx, d: string) {
  const commands = d.match(/[a-df-z][^a-df-z]*/gi) ?? [];
  let x = 0;
  let y = 0;
  let startX = 0;
  let startY = 0;
  let lastControl: [number, number] | null = null;
  let lastQuad: [number, number] | null = null;
  for (const command of commands) {
    const op = command[0]!;
    const args = numbers(command.slice(1));
    const rel = op === op.toLowerCase();
    const upper = op.toUpperCase();
    let at = 0;
    const take = (n: number) => {
      const out = args.slice(at, at + n);
      at += n;
      return out;
    };
    do {
      switch (upper) {
        case 'M': {
          const [a = 0, b = 0] = take(2);
          x = rel ? x + a : a;
          y = rel ? y + b : b;
          c.moveTo(x, y);
          startX = x;
          startY = y;
          lastControl = lastQuad = null;
          // Further pairs are lines.
          while (at + 1 < args.length) {
            const [p = 0, q = 0] = take(2);
            x = rel ? x + p : p;
            y = rel ? y + q : q;
            c.lineTo(x, y);
          }
          break;
        }
        case 'L': {
          const [a = 0, b = 0] = take(2);
          x = rel ? x + a : a;
          y = rel ? y + b : b;
          c.lineTo(x, y);
          lastControl = lastQuad = null;
          break;
        }
        case 'H': {
          const [a = 0] = take(1);
          x = rel ? x + a : a;
          c.lineTo(x, y);
          lastControl = lastQuad = null;
          break;
        }
        case 'V': {
          const [a = 0] = take(1);
          y = rel ? y + a : a;
          c.lineTo(x, y);
          lastControl = lastQuad = null;
          break;
        }
        case 'C': {
          const [a = 0, b = 0, p = 0, q = 0, e = 0, f = 0] = take(6);
          const x1 = rel ? x + a : a;
          const y1 = rel ? y + b : b;
          const x2 = rel ? x + p : p;
          const y2 = rel ? y + q : q;
          x = rel ? x + e : e;
          y = rel ? y + f : f;
          c.bezierCurveTo(x1, y1, x2, y2, x, y);
          lastControl = [x2, y2];
          lastQuad = null;
          break;
        }
        case 'S': {
          const [p = 0, q = 0, e = 0, f = 0] = take(4);
          const x1 = lastControl ? 2 * x - lastControl[0] : x;
          const y1 = lastControl ? 2 * y - lastControl[1] : y;
          const x2 = rel ? x + p : p;
          const y2 = rel ? y + q : q;
          x = rel ? x + e : e;
          y = rel ? y + f : f;
          c.bezierCurveTo(x1, y1, x2, y2, x, y);
          lastControl = [x2, y2];
          lastQuad = null;
          break;
        }
        case 'Q': {
          const [a = 0, b = 0, e = 0, f = 0] = take(4);
          const qx = rel ? x + a : a;
          const qy = rel ? y + b : b;
          x = rel ? x + e : e;
          y = rel ? y + f : f;
          c.quadraticCurveTo(qx, qy, x, y);
          lastQuad = [qx, qy];
          lastControl = null;
          break;
        }
        case 'T': {
          const [e = 0, f = 0] = take(2);
          const qx: number = lastQuad ? 2 * x - lastQuad[0] : x;
          const qy: number = lastQuad ? 2 * y - lastQuad[1] : y;
          x = rel ? x + e : e;
          y = rel ? y + f : f;
          c.quadraticCurveTo(qx, qy, x, y);
          lastQuad = [qx, qy];
          lastControl = null;
          break;
        }
        case 'A': {
          const [rx = 0, ry = 0, rot = 0, large = 0, sweep = 0, e = 0, f = 0] = take(7);
          const tx = rel ? x + e : e;
          const ty = rel ? y + f : f;
          arcTo(c, x, y, rx, ry, rot, large !== 0, sweep !== 0, tx, ty);
          x = tx;
          y = ty;
          lastControl = lastQuad = null;
          break;
        }
        case 'Z':
          c.closePath();
          x = startX;
          y = startY;
          lastControl = lastQuad = null;
          break;
        default:
          at = args.length;
      }
    } while (at < args.length && upper !== 'Z');
  }
}

function length(element: SVGElement, name: string): number {
  const value = (element as unknown as Record<string, { baseVal?: { value: number } }>)[name];
  return value?.baseVal?.value ?? Number(element.getAttribute(name) ?? 0);
}

/** Traces an element's own geometry, if it is a shape. Returns whether it was one. */
function traceShape(c: Ctx, element: SVGElement): boolean {
  switch (element.localName) {
    case 'rect': {
      const x = length(element, 'x');
      const y = length(element, 'y');
      const w = length(element, 'width');
      const h = length(element, 'height');
      const rx = Math.min(length(element, 'rx') || length(element, 'ry'), w / 2);
      const ry = Math.min(length(element, 'ry') || rx, h / 2);
      if (rx > 0 && ry > 0) {
        c.moveTo(x + rx, y);
        c.lineTo(x + w - rx, y);
        c.ellipse(x + w - rx, y + ry, rx, ry, 0, -Math.PI / 2, 0);
        c.lineTo(x + w, y + h - ry);
        c.ellipse(x + w - rx, y + h - ry, rx, ry, 0, 0, Math.PI / 2);
        c.lineTo(x + rx, y + h);
        c.ellipse(x + rx, y + h - ry, rx, ry, 0, Math.PI / 2, Math.PI);
        c.lineTo(x, y + ry);
        c.ellipse(x + rx, y + ry, rx, ry, 0, Math.PI, (Math.PI * 3) / 2);
        c.closePath();
      } else c.rect(x, y, w, h);
      return true;
    }
    case 'circle': {
      const r = length(element, 'r');
      c.moveTo(length(element, 'cx') + r, length(element, 'cy'));
      c.arc(length(element, 'cx'), length(element, 'cy'), r, 0, Math.PI * 2);
      c.closePath();
      return true;
    }
    case 'ellipse': {
      const rx = length(element, 'rx');
      const cx = length(element, 'cx');
      const cy = length(element, 'cy');
      c.moveTo(cx + rx, cy);
      c.ellipse(cx, cy, rx, length(element, 'ry'), 0, 0, Math.PI * 2);
      c.closePath();
      return true;
    }
    case 'line':
      c.moveTo(length(element, 'x1'), length(element, 'y1'));
      c.lineTo(length(element, 'x2'), length(element, 'y2'));
      return true;
    case 'polyline':
    case 'polygon': {
      const points = numbers(element.getAttribute('points') ?? '');
      for (let i = 0; i + 1 < points.length; i += 2) {
        if (i === 0) c.moveTo(points[0]!, points[1]!);
        else c.lineTo(points[i]!, points[i + 1]!);
      }
      if (element.localName === 'polygon') c.closePath();
      return true;
    }
    case 'path':
      tracePath(c, element.getAttribute('d') ?? '');
      return true;
    default:
      return false;
  }
}

function opacityOf(style: CSSStyleDeclaration, own: string): number {
  const value = Number(style.getPropertyValue(own) || 1);
  return Number.isFinite(value) ? value : 1;
}

/** An element's matrix relative to the SVG's own top left. */
function matrixOf(root: SVGSVGElement, element: SVGGraphicsElement): DOMMatrix | null {
  const own = element.getScreenCTM();
  if (!own) return null;
  const box = root.getBoundingClientRect();
  // The screen matrix of the root includes where it is on the page, which the drawing doesn't.
  return new DOMMatrix([1, 0, 0, 1, -box.left, -box.top]).multiply(own);
}

function setMatrix(c: Ctx, m: DOMMatrix) {
  c.transform(m.a, m.b, m.c, m.d, m.e, m.f);
}

/** A clip path's rectangles and shapes, applied to the context. */
function applyClip(c: Ctx, root: SVGSVGElement, clipRef: string) {
  const id = /url\(\s*["']?#([^"')]+)["']?\s*\)/.exec(clipRef)?.[1];
  if (!id) return;
  const clip = root.ownerDocument.getElementById(id);
  if (!clip || clip.localName !== 'clipPath') return;
  c.beginPath();
  for (const child of Array.from(clip.children)) {
    if (!(child instanceof SVGGraphicsElement)) continue;
    const m = matrixOf(root, child);
    c.save();
    if (m) setMatrix(c, m);
    traceShape(c, child);
    c.restore();
  }
  c.clip();
}

function drawText(c: Ctx, root: SVGSVGElement, text: SVGTextElement, style: CSSStyleDeclaration) {
  const m = matrixOf(root, text);
  const pieces: { node: SVGTextContentElement; content: string }[] = [];
  const walk = (node: Node, owner: SVGTextContentElement) => {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        if (child.textContent) pieces.push({ node: owner, content: child.textContent });
      } else if (child instanceof SVGTextContentElement) walk(child, child);
    }
  };
  walk(text, text);
  let index = 0;
  for (const piece of pieces) {
    const owner = piece.node;
    const own = owner === text ? style : getComputedStyle(owner);
    const count = piece.content.length;
    if (own.display === 'none' || own.visibility === 'hidden' || !piece.content.trim()) {
      index += count;
      continue;
    }
    let start: DOMPoint;
    try {
      start = text.getStartPositionOfChar(index);
    } catch {
      index += count;
      continue;
    }
    index += count;
    const fill = own.fill;
    if (!fill || fill === 'none') continue;
    c.save();
    if (m) setMatrix(c, m);
    c.globalAlpha = opacityOf(own, 'opacity') * opacityOf(own, 'fill-opacity');
    c.fillStyle = fill;
    c.font =
      (own.fontStyle === 'italic' ? 'italic ' : '') +
      (Number(own.fontWeight) >= 600 || own.fontWeight === 'bold' ? 'bold ' : '') +
      own.fontSize +
      ' ' +
      own.fontFamily;
    const rtl = own.direction === 'rtl';
    c.direction = rtl ? 'rtl' : 'ltr';
    c.textAlign = 'start';
    c.textBaseline = 'alphabetic';
    c.fillText(piece.content.replace(/\s+/g, ' '), start.x, start.y);
    c.restore();
  }
}

/** Records an SVG's drawing at the context's origin, in the SVG's pixels. */
export function svgToRecording(root: SVGSVGElement, c: Ctx) {
  const visit = (element: Element) => {
    if (!(element instanceof SVGElement) || SKIPPED.has(element.localName)) return;
    const style = getComputedStyle(element);
    if (style.display === 'none') return;
    const clip = style.clipPath && style.clipPath !== 'none' ? style.clipPath : null;
    if (clip) {
      c.save();
      applyClip(c, root, clip);
    }
    if (element.localName === 'text') {
      if (style.visibility !== 'hidden') drawText(c, root, element as SVGTextElement, style);
    } else if (
      element instanceof SVGGraphicsElement &&
      element.localName !== 'g' &&
      element.localName !== 'svg'
    ) {
      const m = matrixOf(root, element);
      if (style.visibility !== 'hidden') {
        c.save();
        if (m) setMatrix(c, m);
        c.beginPath();
        if (traceShape(c, element)) {
          const opacity = opacityOf(style, 'opacity');
          const fill = style.fill;
          if (fill && fill !== 'none' && !fill.startsWith('url(')) {
            c.globalAlpha = opacity * opacityOf(style, 'fill-opacity');
            c.fillStyle = fill;
            c.fill(style.fillRule === 'evenodd' ? 'evenodd' : 'nonzero');
          }
          const stroke = style.stroke;
          const width = parseFloat(style.strokeWidth || '1');
          if (stroke && stroke !== 'none' && !stroke.startsWith('url(') && width > 0) {
            c.globalAlpha = opacity * opacityOf(style, 'stroke-opacity');
            c.strokeStyle = stroke;
            c.lineWidth = width;
            const dash = style.strokeDasharray;
            c.setLineDash(dash && dash !== 'none' ? numbers(dash) : []);
            c.lineCap = (style.strokeLinecap as CanvasLineCap) || 'butt';
            c.lineJoin = (style.strokeLinejoin as CanvasLineJoin) || 'miter';
            c.stroke();
          }
        }
        c.restore();
      }
    }
    if (element.localName !== 'text') {
      for (const child of Array.from(element.children)) visit(child);
    }
    if (clip) c.restore();
  };
  for (const child of Array.from(root.children)) visit(child);
}
