// `requestPreview`'s pictures (Track S4): the view as drawn, from its leading corner, cut to
// `maxWidth` by `maxHeight` CSS pixels and never scaled, in the file's own colours whatever the
// theme (decisions, 2026-11-14). The density starts at `scale` and steps down until the PNG is at
// most 2 MB; `width` and `height` stay the CSS pixels it shows.
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { CgChart } from '@cashgear/ui';
import {
  Recording,
  RecordingContext,
  replay,
  svgToRecording,
  type CellArea,
  type ChartView,
  type DrawingBox,
  type EngineClient,
  type SheetLayout,
  type TilePalette,
} from '@cgs/engine-client';
import { chartProps } from '../charts/chartProps';
import { mirroredContext } from '../grid/mirror';
import { PAPER } from './book';
import { MAX_PREVIEW_BYTES } from './contract';
import { RangeGeometry, cssSize } from './range-view';

/** The next frame, or a moment when frames don't come, as in a frame out of view. */
function nextFrame(): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, 50);
    requestAnimationFrame(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

export interface Png {
  bytes: ArrayBuffer;
  /** CSS pixels shown. */
  width: number;
  height: number;
}

type Context2D = OffscreenCanvasRenderingContext2D;

/**
 * Draws at `scale`, then at lower densities while the PNG is over the limit: each try scales by
 * how far over it was, and at least by 0.8.
 */
async function fitted(
  width: number,
  height: number,
  scale: number,
  paint: (
    context: Context2D,
    density: number,
    pixels: { width: number; height: number },
  ) => Promise<void>,
  limit = MAX_PREVIEW_BYTES,
): Promise<Png> {
  let density = scale;
  for (let attempt = 0; attempt < 8; attempt++) {
    const pixels = {
      width: Math.max(1, Math.round(width * density)),
      height: Math.max(1, Math.round(height * density)),
    };
    const canvas = new OffscreenCanvas(pixels.width, pixels.height);
    const context = canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('This browser has no 2D canvas.');
    await paint(context, density, pixels);
    const blob = await canvas.convertToBlob({ type: 'image/png' });
    if (blob.size <= limit) return { bytes: await blob.arrayBuffer(), width, height };
    density *= Math.min(0.8, Math.sqrt(limit / blob.size) * 0.95);
  }
  throw new Error('The picture can’t be made small enough.');
}

/** A range's picture: its tiles drawn again at the picture's density. */
export async function rangePng(
  engine: EngineClient,
  sheet: number,
  layout: SheetLayout,
  area: CellArea,
  palette: TilePalette,
  maxWidth: number,
  maxHeight: number,
  scale: number,
): Promise<Png> {
  const size = cssSize(layout, area);
  const width = Math.min(size.width, maxWidth);
  const height = Math.min(size.height, maxHeight);
  return fitted(width, height, scale, async (plain, density, pixels) => {
    const geometry = new RangeGeometry(layout, area, density);
    const context = layout.rightToLeft
      ? (mirroredContext(
          plain as unknown as CanvasRenderingContext2D,
          () => pixels.width,
        ) as unknown as Context2D)
      : plain;
    context.fillStyle = palette.background;
    context.fillRect(0, 0, pixels.width, pixels.height);
    context.save();
    context.beginPath();
    context.rect(0, 0, Math.min(pixels.width, geometry.width), pixels.height);
    context.clip();
    for (const { firstRow, firstCol } of geometry.tilesIn(0, 0, pixels.width, pixels.height)) {
      const image = await engine.drawTile({
        sheet,
        firstRow,
        firstCol,
        scale: density,
        palette: {
          ...palette,
          erp: undefined,
          showGrid: layout.showGrid,
          rightToLeft: layout.rightToLeft,
        },
      });
      const box = geometry.tileBox(firstRow, firstCol);
      context.drawImage(image.bitmap, box.x, box.y, box.width, box.height);
      image.bitmap.close();
    }
    context.restore();
  });
}

const GENERIC = new Set(['serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui']);

/** A recorded face at a size, as a CSS font. */
export function cssFont(recording: Recording, face: number, size: number): string {
  const request = recording.faces[face];
  if (!request) return `${size}px sans-serif`;
  const families = request.families
    .map((family) => (GENERIC.has(family) ? family : `"${family.replace(/"/g, '')}"`))
    .join(', ');
  return `${request.italic ? 'italic ' : ''}${request.bold ? 'bold ' : ''}${size}px ${families || 'sans-serif'}`;
}

async function recordedPictures(recording: Recording): Promise<(ImageBitmap | null)[]> {
  return Promise.all(
    recording.images.map(async (image) => {
      try {
        if (image.kind === 'jpeg') {
          return await createImageBitmap(
            new Blob([image.data as BlobPart], { type: 'image/jpeg' }),
          );
        }
        const data = new ImageData(new Uint8ClampedArray(image.data), image.width, image.height);
        return await createImageBitmap(data);
      } catch {
        return null;
      }
    }),
  );
}

/** Draws a chart off screen, as it shows, and records its SVG. */
async function recordChart(
  view: ChartView | null,
  drawing: DrawingBox,
  locale: string,
): Promise<{ recording: Recording; ops: RecordingContext['ops'] }> {
  const recording = new Recording();
  const measure = new OffscreenCanvas(1, 1).getContext('2d');
  if (!measure) throw new Error('This browser has no 2D canvas.');
  const context = new RecordingContext(recording, measure);
  if (!view || view.kind !== 'classic') {
    drawUnreadable(context as unknown as Context2D, drawing.width, drawing.height, drawing.name);
    return { recording, ops: context.ops };
  }
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
  return { recording, ops: context.ops };
}

/** What shows for a chart this app can't draw: its frame and its name, as the editor shows it. */
export function drawUnreadable(context: Context2D, width: number, height: number, name: string) {
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, width, height);
  context.strokeStyle = '#a0a7b4';
  context.lineWidth = 1;
  context.strokeRect(0.5, 0.5, width - 1, height - 1);
  context.fillStyle = '#5b6472';
  context.font = '12px sans-serif';
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.fillText(name, width / 2, height / 2);
}

/** A chart's picture, in the file's own colours. */
export async function chartPng(
  engine: EngineClient,
  sheet: number,
  drawing: DrawingBox,
  locale: string,
  maxWidth: number,
  maxHeight: number,
  scale: number,
): Promise<Png> {
  const view = await engine.chart(sheet, drawing.index, PAPER);
  const { recording, ops } = await recordChart(view, drawing, locale);
  const pictures = await recordedPictures(recording);
  const width = Math.min(Math.max(1, Math.round(drawing.width)), maxWidth);
  const height = Math.min(Math.max(1, Math.round(drawing.height)), maxHeight);
  try {
    return await fitted(width, height, scale, async (context, density) => {
      context.fillStyle = '#ffffff';
      context.fillRect(0, 0, context.canvas.width, context.canvas.height);
      context.scale(density, density);
      replay(context, ops, pictures, (face, size) => cssFont(recording, face, size));
    });
  } finally {
    for (const picture of pictures) picture?.close();
  }
}
