// A right-to-left sheet is drawn by the same code as a left-to-right one, through a context that
// mirrors where things go but not what they look like: rectangles and lines land at the other side
// of the canvas, while text and pictures keep reading the right way round.

const FLIPPED_ALIGN: Record<string, CanvasTextAlign> = {
  left: 'right',
  right: 'left',
  start: 'end',
  end: 'start',
  center: 'center',
};

/**
 * A 2D context whose x positions run from the right edge of a canvas `width()` device pixels
 * wide. Everything else passes straight through.
 */
export function mirroredContext(
  context: CanvasRenderingContext2D,
  width: () => number,
): CanvasRenderingContext2D {
  const box = (x: number, size: number) => width() - x - size;
  const at = (x: number) => width() - x;
  const methods: Partial<Record<keyof CanvasRenderingContext2D, unknown>> = {
    fillRect: (x: number, y: number, w: number, h: number) => context.fillRect(box(x, w), y, w, h),
    strokeRect: (x: number, y: number, w: number, h: number) =>
      context.strokeRect(box(x, w), y, w, h),
    clearRect: (x: number, y: number, w: number, h: number) =>
      context.clearRect(box(x, w), y, w, h),
    rect: (x: number, y: number, w: number, h: number) => context.rect(box(x, w), y, w, h),
    moveTo: (x: number, y: number) => context.moveTo(at(x), y),
    lineTo: (x: number, y: number) => context.lineTo(at(x), y),
    fillText: (text: string, x: number, y: number, maxWidth?: number) => {
      const align = context.textAlign;
      context.textAlign = FLIPPED_ALIGN[align] ?? align;
      if (maxWidth === undefined) context.fillText(text, at(x), y);
      else context.fillText(text, at(x), y, maxWidth);
      context.textAlign = align;
    },
    drawImage: (image: CanvasImageSource & { width: number }, ...args: number[]) => {
      if (args.length === 2) {
        context.drawImage(image, box(args[0]!, image.width), args[1]!);
      } else if (args.length === 4) {
        const [x, y, w, h] = args as [number, number, number, number];
        context.drawImage(image, box(x, w), y, w, h);
      } else {
        const [sx, sy, sw, sh, x, y, w, h] = args as [
          number,
          number,
          number,
          number,
          number,
          number,
          number,
          number,
        ];
        context.drawImage(image, sx, sy, sw, sh, box(x, w), y, w, h);
      }
    },
  };
  return new Proxy(context, {
    get(target, key) {
      const own = methods[key as keyof CanvasRenderingContext2D];
      if (own) return own;
      const value: unknown = Reflect.get(target, key, target);
      return typeof value === 'function'
        ? (value as (...a: unknown[]) => unknown).bind(target)
        : value;
    },
    set(target, key, value) {
      return Reflect.set(target, key, value, target);
    },
  });
}
