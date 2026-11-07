// A 2D context that records what is drawn on it instead of drawing it, in page points: what the
// grid's own drawing code draws becomes vector PDF (`crates/cgs-pdf`), and the same recording
// plays back onto a canvas for the print preview. Text is measured on a real canvas, so it fits
// cells exactly as it does on screen.

/** A matrix as canvas gives one: x' = a x + c y + e, y' = b x + d y + f. */
export type Matrix = [number, number, number, number, number, number];

export type Paint =
  | string
  | { linear: [number, number, number, number]; stops: [number, string][] }
  | { image: number; x: number; y: number; w: number; h: number };

export type PdfOp =
  | { k: 'save' }
  | { k: 'restore' }
  | {
      k: 'path';
      d: number[];
      fill?: Paint;
      stroke?: string;
      lw?: number;
      dash?: number[];
      evenodd?: boolean;
      cap?: string;
      join?: string;
    }
  | { k: 'clip'; d: number[]; evenodd?: boolean }
  | {
      k: 'text';
      t: string;
      x: number;
      y: number;
      m: Matrix;
      face: number;
      size: number;
      fill: string;
      rtl?: boolean;
      align?: string;
      base?: string;
    }
  | { k: 'image'; image: number; m: Matrix };

/** A face as the drawing asks for it, before any file is found for it. */
export interface FaceRequest {
  /** The CSS families in order, quotes taken off. */
  families: string[];
  bold: boolean;
  italic: boolean;
}

/** A picture's pixels, kept until the document is written. */
export interface RecordedImage {
  width: number;
  height: number;
  /** RGBA, or a JPEG file as it is. */
  data: Uint8Array;
  kind: 'rgba' | 'jpeg';
}

/** What every page shares: the faces and pictures they use. */
export class Recording {
  readonly faces: FaceRequest[] = [];
  private readonly faceKeys = new Map<string, number>();
  readonly images: RecordedImage[] = [];
  private readonly imageKeys = new Map<unknown, number>();

  face(request: FaceRequest): number {
    const key = request.families.join(',') + '|' + request.bold + '|' + request.italic;
    let index = this.faceKeys.get(key);
    if (index === undefined) {
      index = this.faces.length;
      this.faces.push(request);
      this.faceKeys.set(key, index);
    }
    return index;
  }

  /** Adds a picture once, however often it is drawn. `key` says when two are the same. */
  image(key: unknown, make: () => RecordedImage | null): number | null {
    const known = this.imageKeys.get(key);
    if (known !== undefined) return known;
    const image = make();
    if (!image) return null;
    const index = this.images.length;
    this.images.push(image);
    this.imageKeys.set(key, index);
    return index;
  }
}

const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

export function multiply(m: Matrix, n: Matrix): Matrix {
  // n applied after m.
  return [
    m[0] * n[0] + m[1] * n[2],
    m[0] * n[1] + m[1] * n[3],
    m[2] * n[0] + m[3] * n[2],
    m[2] * n[1] + m[3] * n[3],
    m[4] * n[0] + m[5] * n[2] + n[4],
    m[4] * n[1] + m[5] * n[3] + n[5],
  ];
}

function apply(m: Matrix, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

/** Reads a CSS font as the grid writes it: `italic bold 14.67px "Calibri", Carlito, sans-serif`. */
export function parseFont(css: string): { px: number; face: FaceRequest } {
  const match =
    /^\s*((?:(?:italic|oblique|normal|bold|bolder|lighter|\d{3}|small-caps)\s+)*)([\d.]+)px\s*(?:\/[^\s]+\s*)?(.*)$/i.exec(
      css,
    );
  if (!match) return { px: 10, face: { families: ['sans-serif'], bold: false, italic: false } };
  const words = match[1]!.toLowerCase();
  const weight = /(\d{3})/.exec(words);
  const bold = /bold/.test(words) || (weight !== null && Number(weight[1]) >= 600);
  const italic = /italic|oblique/.test(words);
  const families = match[3]!
    .split(',')
    .map((family) => family.trim().replace(/^["']|["']$/g, ''))
    .filter(Boolean);
  return { px: Number(match[2]), face: { families, bold, italic } };
}

/** A CSS colour as `#rrggbbaa`, with `alpha` folded in. Canvas does the reading. */
function normalize(probe: OffscreenCanvasRenderingContext2D, css: string, alpha: number): string {
  let rgba: [number, number, number, number] | null = null;
  const hex = /^#([0-9a-f]{3,8})$/i.exec(css.trim());
  if (hex) {
    let h = hex[1]!;
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('');
    if (h.length === 6) h += 'ff';
    if (h.length === 8) {
      rgba = [0, 2, 4, 6].map((at) => parseInt(h.slice(at, at + 2), 16)) as [
        number,
        number,
        number,
        number,
      ];
    }
  }
  if (!rgba) {
    probe.fillStyle = '#000000';
    probe.fillStyle = css;
    const read = String(probe.fillStyle);
    const short = /^#([0-9a-f]{6})$/i.exec(read);
    if (short) {
      const h = short[1]!;
      rgba = [
        parseInt(h.slice(0, 2), 16),
        parseInt(h.slice(2, 4), 16),
        parseInt(h.slice(4, 6), 16),
        255,
      ];
    } else {
      const parts = /rgba?\(([^)]+)\)/
        .exec(read)?.[1]
        ?.split(',')
        .map((p) => Number(p.trim()));
      rgba = parts
        ? [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0, Math.round((parts[3] ?? 1) * 255)]
        : [0, 0, 0, 255];
    }
  }
  rgba[3] = Math.round(rgba[3] * Math.max(0, Math.min(1, alpha)));
  return (
    '#' + rgba.map((v) => Math.max(0, Math.min(255, v)).toString(16).padStart(2, '0')).join('')
  );
}

class RecordedGradient {
  stops: [number, string][] = [];
  constructor(readonly coords: [number, number, number, number]) {}
  addColorStop(offset: number, color: string) {
    this.stops.push([offset, color]);
  }
}

class RecordedPattern {
  constructor(readonly source: CanvasImageSource) {}
  setTransform() {}
}

interface State {
  matrix: Matrix;
  fillStyle: string | RecordedGradient | RecordedPattern;
  strokeStyle: string;
  lineWidth: number;
  dash: number[];
  font: string;
  textAlign: CanvasTextAlign;
  textBaseline: CanvasTextBaseline;
  direction: CanvasDirection;
  globalAlpha: number;
  lineCap: CanvasLineCap;
  lineJoin: CanvasLineJoin;
}

/** Pixels of anything canvas can draw, for a picture or a pattern. */
export function imagePixels(source: CanvasImageSource): RecordedImage | null {
  const size = source as { width?: number; height?: number };
  const width = Math.round(Number(size.width) || 0);
  const height = Math.round(Number(size.height) || 0);
  if (!width || !height || typeof OffscreenCanvas === 'undefined') return null;
  const canvas = new OffscreenCanvas(width, height);
  const context = canvas.getContext('2d');
  if (!context) return null;
  context.drawImage(source, 0, 0);
  const data = context.getImageData(0, 0, width, height).data;
  return { width, height, data: new Uint8Array(data.buffer.slice(0)), kind: 'rgba' };
}

/**
 * Records drawing in page points. Its starting matrix takes the drawing's own pixels to the page,
 * so the grid's code draws a page exactly as it draws the screen.
 */
export class RecordingContext {
  readonly ops: PdfOp[] = [];
  private state: State;
  private readonly stack: State[] = [];
  private path: number[] = [];
  private cursor: [number, number] | null = null;
  private start: [number, number] | null = null;

  constructor(
    private readonly recording: Recording,
    /** A real context, for measuring text and reading colours. */
    private readonly measure: OffscreenCanvasRenderingContext2D,
    matrix: Matrix = IDENTITY,
    /** Changes a font for measuring, so text fits as it will be drawn. */
    private readonly measuring: (css: string) => string = (css) => css,
  ) {
    this.state = {
      matrix,
      fillStyle: '#000000',
      strokeStyle: '#000000',
      lineWidth: 1,
      dash: [],
      font: '10px sans-serif',
      textAlign: 'start',
      textBaseline: 'alphabetic',
      direction: 'ltr',
      globalAlpha: 1,
      lineCap: 'butt',
      lineJoin: 'miter',
    };
  }

  // Canvas state, as properties.
  get fillStyle(): string | CanvasGradient | CanvasPattern {
    return this.state.fillStyle as unknown as string;
  }
  set fillStyle(value: string | CanvasGradient | CanvasPattern) {
    this.state.fillStyle =
      value instanceof RecordedGradient || value instanceof RecordedPattern ? value : String(value);
  }
  get strokeStyle(): string | CanvasGradient | CanvasPattern {
    return this.state.strokeStyle;
  }
  set strokeStyle(value: string | CanvasGradient | CanvasPattern) {
    this.state.strokeStyle = typeof value === 'string' ? value : '#000000';
  }
  get lineWidth() {
    return this.state.lineWidth;
  }
  set lineWidth(value: number) {
    this.state.lineWidth = value;
  }
  get lineCap() {
    return this.state.lineCap;
  }
  set lineCap(value: CanvasLineCap) {
    this.state.lineCap = value;
  }
  get lineJoin() {
    return this.state.lineJoin;
  }
  set lineJoin(value: CanvasLineJoin) {
    this.state.lineJoin = value;
  }
  get font() {
    return this.state.font;
  }
  set font(value: string) {
    this.state.font = value;
  }
  get textAlign() {
    return this.state.textAlign;
  }
  set textAlign(value: CanvasTextAlign) {
    this.state.textAlign = value;
  }
  get textBaseline() {
    return this.state.textBaseline;
  }
  set textBaseline(value: CanvasTextBaseline) {
    this.state.textBaseline = value;
  }
  get direction() {
    return this.state.direction;
  }
  set direction(value: CanvasDirection) {
    this.state.direction = value;
  }
  get globalAlpha() {
    return this.state.globalAlpha;
  }
  set globalAlpha(value: number) {
    this.state.globalAlpha = value;
  }

  save() {
    this.stack.push({ ...this.state, dash: [...this.state.dash] });
    this.ops.push({ k: 'save' });
  }

  restore() {
    const state = this.stack.pop();
    if (!state) return;
    this.state = state;
    this.ops.push({ k: 'restore' });
  }

  translate(x: number, y: number) {
    this.state.matrix = multiply([1, 0, 0, 1, x, y], this.state.matrix);
  }

  rotate(angle: number) {
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    this.state.matrix = multiply([cos, sin, -sin, cos, 0, 0], this.state.matrix);
  }

  scale(x: number, y: number) {
    this.state.matrix = multiply([x, 0, 0, y, 0, 0], this.state.matrix);
  }

  transform(a: number, b: number, c: number, d: number, e: number, f: number) {
    this.state.matrix = multiply([a, b, c, d, e, f], this.state.matrix);
  }

  setLineDash(segments: number[]) {
    this.state.dash = [...segments];
  }

  getLineDash() {
    return [...this.state.dash];
  }

  createLinearGradient(x0: number, y0: number, x1: number, y1: number): CanvasGradient {
    const [ax, ay] = apply(this.state.matrix, x0, y0);
    const [bx, by] = apply(this.state.matrix, x1, y1);
    return new RecordedGradient([ax, ay, bx, by]) as unknown as CanvasGradient;
  }

  /** Every pattern the grid makes repeats both ways, so the repetition isn't needed. */
  createPattern(image: CanvasImageSource): CanvasPattern | null {
    return new RecordedPattern(image) as unknown as CanvasPattern;
  }

  measureText(text: string): TextMetrics {
    this.measure.font = this.measuring(this.state.font);
    this.measure.direction = this.state.direction;
    return this.measure.measureText(text);
  }

  beginPath() {
    this.path = [];
    this.cursor = null;
    this.start = null;
  }

  private point(x: number, y: number): [number, number] {
    return apply(this.state.matrix, x, y);
  }

  moveTo(x: number, y: number) {
    const [px, py] = this.point(x, y);
    this.path.push(0, px, py);
    this.cursor = [px, py];
    this.start = [px, py];
  }

  lineTo(x: number, y: number) {
    const [px, py] = this.point(x, y);
    if (!this.cursor) {
      this.path.push(0, px, py);
      this.start = [px, py];
    } else this.path.push(1, px, py);
    this.cursor = [px, py];
  }

  bezierCurveTo(x1: number, y1: number, x2: number, y2: number, x: number, y: number) {
    if (!this.cursor) this.moveTo(x1, y1);
    const a = this.point(x1, y1);
    const b = this.point(x2, y2);
    const c = this.point(x, y);
    this.path.push(2, a[0], a[1], b[0], b[1], c[0], c[1]);
    this.cursor = c;
  }

  quadraticCurveTo(cx: number, cy: number, x: number, y: number) {
    if (!this.cursor) this.moveTo(cx, cy);
    // In page space, which an affine matrix keeps quadratic.
    const [p0x, p0y] = this.cursor!;
    const [qx, qy] = this.point(cx, cy);
    const [ex, ey] = this.point(x, y);
    this.path.push(
      2,
      p0x + (2 / 3) * (qx - p0x),
      p0y + (2 / 3) * (qy - p0y),
      ex + (2 / 3) * (qx - ex),
      ey + (2 / 3) * (qy - ey),
      ex,
      ey,
    );
    this.cursor = [ex, ey];
  }

  closePath() {
    this.path.push(3);
    this.cursor = this.start;
  }

  rect(x: number, y: number, w: number, h: number) {
    this.moveTo(x, y);
    this.lineTo(x + w, y);
    this.lineTo(x + w, y + h);
    this.lineTo(x, y + h);
    this.closePath();
  }

  /** An ellipse's arc as curves, a quarter turn at most each. */
  ellipse(
    x: number,
    y: number,
    rx: number,
    ry: number,
    rotation: number,
    start: number,
    end: number,
    counterclockwise = false,
  ) {
    let sweep = end - start;
    const full = Math.PI * 2;
    if (!counterclockwise && sweep < 0) sweep = (sweep % full) + full;
    if (counterclockwise && sweep > 0) sweep = (sweep % full) - full;
    if (Math.abs(end - start) >= full) sweep = counterclockwise ? -full : full;
    const steps = Math.max(1, Math.ceil(Math.abs(sweep) / (Math.PI / 2) - 1e-9));
    const step = sweep / steps;
    const cos = Math.cos(rotation);
    const sin = Math.sin(rotation);
    const at = (angle: number): [number, number] => {
      const px = rx * Math.cos(angle);
      const py = ry * Math.sin(angle);
      return [x + px * cos - py * sin, y + px * sin + py * cos];
    };
    const tangent = (angle: number): [number, number] => {
      const px = -rx * Math.sin(angle);
      const py = ry * Math.cos(angle);
      return [px * cos - py * sin, px * sin + py * cos];
    };
    const first = at(start);
    if (this.cursor) this.lineTo(first[0], first[1]);
    else this.moveTo(first[0], first[1]);
    const k = (4 / 3) * Math.tan(step / 4);
    for (let i = 0; i < steps; i++) {
      const a0 = start + step * i;
      const a1 = a0 + step;
      const p0 = at(a0);
      const p1 = at(a1);
      const t0 = tangent(a0);
      const t1 = tangent(a1);
      this.bezierCurveTo(
        p0[0] + k * t0[0],
        p0[1] + k * t0[1],
        p1[0] - k * t1[0],
        p1[1] - k * t1[1],
        p1[0],
        p1[1],
      );
    }
  }

  arc(x: number, y: number, radius: number, start: number, end: number, counterclockwise = false) {
    this.ellipse(x, y, radius, radius, 0, start, end, counterclockwise);
  }

  /** How much the matrix scales a line's width. */
  private widthScale() {
    const m = this.state.matrix;
    return Math.sqrt(Math.abs(m[0] * m[3] - m[1] * m[2]));
  }

  private paint(): Paint | null {
    const style = this.state.fillStyle;
    if (style instanceof RecordedGradient) {
      return {
        linear: style.coords,
        stops: style.stops.map(([o, c]) => [o, normalize(this.measure, c, this.state.globalAlpha)]),
      };
    }
    if (style instanceof RecordedPattern) {
      const index = this.recording.image(style.source, () => imagePixels(style.source));
      if (index === null) return null;
      const image = this.recording.images[index]!;
      const m = this.state.matrix;
      const scale = this.widthScale();
      return { image: index, x: m[4], y: m[5], w: image.width * scale, h: image.height * scale };
    }
    const color = normalize(this.measure, style, this.state.globalAlpha);
    return color.endsWith('00') ? null : color;
  }

  private strokeColor(): string | null {
    const color = normalize(this.measure, this.state.strokeStyle, this.state.globalAlpha);
    return color.endsWith('00') ? null : color;
  }

  fill(rule: CanvasFillRule = 'nonzero') {
    const fill = this.paint();
    if (!fill || !this.path.length) return;
    this.ops.push({ k: 'path', d: [...this.path], fill, evenodd: rule === 'evenodd' });
  }

  stroke() {
    const stroke = this.strokeColor();
    if (!stroke || !this.path.length) return;
    const scale = this.widthScale();
    this.ops.push({
      k: 'path',
      d: [...this.path],
      stroke,
      lw: this.state.lineWidth * scale,
      dash: this.state.dash.map((d) => d * scale),
      cap: this.state.lineCap,
      join: this.state.lineJoin,
    });
  }

  clip(rule: CanvasFillRule = 'nonzero') {
    this.ops.push({ k: 'clip', d: [...this.path], evenodd: rule === 'evenodd' });
  }

  private rectPath(x: number, y: number, w: number, h: number): number[] {
    const m = this.state.matrix;
    if (m[1] === 0 && m[2] === 0) {
      const [ax, ay] = apply(m, x, y);
      return [4, ax, ay, w * m[0], h * m[3]];
    }
    const a = apply(m, x, y);
    const b = apply(m, x + w, y);
    const c = apply(m, x + w, y + h);
    const d = apply(m, x, y + h);
    return [0, a[0], a[1], 1, b[0], b[1], 1, c[0], c[1], 1, d[0], d[1], 3];
  }

  fillRect(x: number, y: number, w: number, h: number) {
    if (!w || !h) return;
    const fill = this.paint();
    if (!fill) return;
    this.ops.push({ k: 'path', d: this.rectPath(x, y, w, h), fill });
  }

  strokeRect(x: number, y: number, w: number, h: number) {
    const stroke = this.strokeColor();
    if (!stroke) return;
    const scale = this.widthScale();
    this.ops.push({
      k: 'path',
      d: this.rectPath(x, y, w, h),
      stroke,
      lw: this.state.lineWidth * scale,
      dash: this.state.dash.map((d) => d * scale),
    });
  }

  clearRect(x: number, y: number, w: number, h: number) {
    // Pages start white, and nothing draws under what clears.
    const before = this.state.fillStyle;
    this.state.fillStyle = '#ffffff';
    this.fillRect(x, y, w, h);
    this.state.fillStyle = before;
  }

  fillText(text: string, x: number, y: number) {
    if (!text) return;
    const style = this.state.fillStyle;
    const color = normalize(
      this.measure,
      typeof style === 'string' ? style : '#000000',
      this.state.globalAlpha,
    );
    if (color.endsWith('00')) return;
    const { px, face } = parseFont(this.state.font);
    const rtl = this.state.direction === 'rtl';
    this.ops.push({
      k: 'text',
      t: text,
      x,
      y,
      m: [...this.state.matrix],
      face: this.recording.face(face),
      size: px,
      fill: color,
      rtl,
      align: this.state.textAlign,
      base: this.state.textBaseline,
    });
  }

  strokeText() {}

  /** Draws a picture that has already been added to the recording, into this rectangle. */
  drawRecorded(image: number, x: number, y: number, w: number, h: number) {
    const m = multiply([w, 0, 0, h, x, y], this.state.matrix);
    this.ops.push({ k: 'image', image, m });
  }

  drawImage(source: CanvasImageSource, ...args: number[]) {
    const index = this.recording.image(source, () => imagePixels(source));
    if (index === null) return;
    const image = this.recording.images[index]!;
    let [x, y, w, h] = [0, 0, image.width, image.height];
    if (args.length === 2) [x, y] = args as [number, number];
    else if (args.length >= 4) [x, y, w, h] = args.slice(-4) as [number, number, number, number];
    this.drawRecorded(index, x, y, w, h);
  }
}

/** Plays recorded ops back onto a canvas, for the print preview. Pictures come as bitmaps. */
export function replay(
  context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
  ops: PdfOp[],
  pictures: (CanvasImageSource | null)[],
  /** A face at a size, as a CSS font. */
  fonts: (face: number, size: number) => string,
) {
  const tracePath = (d: number[]) => {
    context.beginPath();
    let at = 0;
    while (at < d.length) {
      switch (d[at]) {
        case 0:
          context.moveTo(d[at + 1]!, d[at + 2]!);
          at += 3;
          break;
        case 1:
          context.lineTo(d[at + 1]!, d[at + 2]!);
          at += 3;
          break;
        case 2:
          context.bezierCurveTo(
            d[at + 1]!,
            d[at + 2]!,
            d[at + 3]!,
            d[at + 4]!,
            d[at + 5]!,
            d[at + 6]!,
          );
          at += 7;
          break;
        case 3:
          context.closePath();
          at += 1;
          break;
        case 4:
          context.rect(d[at + 1]!, d[at + 2]!, d[at + 3]!, d[at + 4]!);
          at += 5;
          break;
        default:
          return;
      }
    }
  };
  const base = context.getTransform();
  for (const op of ops) {
    switch (op.k) {
      case 'save':
        context.save();
        break;
      case 'restore':
        context.restore();
        break;
      case 'clip':
        tracePath(op.d);
        context.clip(op.evenodd ? 'evenodd' : 'nonzero');
        break;
      case 'path': {
        tracePath(op.d);
        if (op.fill) {
          const fill = op.fill;
          if (typeof fill === 'string') context.fillStyle = fill;
          else if ('linear' in fill) {
            const gradient = context.createLinearGradient(...fill.linear);
            for (const [offset, color] of fill.stops) gradient.addColorStop(offset, color);
            context.fillStyle = gradient;
          } else {
            const picture = pictures[fill.image];
            const pattern = picture && context.createPattern(picture, 'repeat');
            if (pattern) {
              const source = picture as { width: number; height: number };
              pattern.setTransform(
                new DOMMatrix([
                  fill.w / source.width,
                  0,
                  0,
                  fill.h / source.height,
                  fill.x,
                  fill.y,
                ]),
              );
              context.fillStyle = pattern;
            }
          }
          context.fill(op.evenodd ? 'evenodd' : 'nonzero');
        }
        if (op.stroke) {
          context.strokeStyle = op.stroke;
          context.lineWidth = op.lw ?? 1;
          context.setLineDash(op.dash ?? []);
          context.lineCap = (op.cap as CanvasLineCap) ?? 'butt';
          context.lineJoin = (op.join as CanvasLineJoin) ?? 'miter';
          context.stroke();
        }
        break;
      }
      case 'text': {
        context.save();
        const [a, b, c, d, e, f] = op.m;
        context.setTransform(base.multiply(new DOMMatrix([a, b, c, d, e, f])));
        context.font = fonts(op.face, op.size);
        context.fillStyle = op.fill;
        context.direction = op.rtl ? 'rtl' : 'ltr';
        context.textAlign = (op.align as CanvasTextAlign) ?? 'start';
        context.textBaseline = (op.base as CanvasTextBaseline) ?? 'alphabetic';
        context.fillText(op.t, op.x, op.y);
        context.restore();
        break;
      }
      case 'image': {
        const picture = pictures[op.image];
        if (!picture) break;
        context.save();
        const [a, b, c, d, e, f] = op.m;
        context.setTransform(base.multiply(new DOMMatrix([a, b, c, d, e, f])));
        context.drawImage(picture, 0, 0, 1, 1);
        context.restore();
        break;
      }
    }
  }
}
