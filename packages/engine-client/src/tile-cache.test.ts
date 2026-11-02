import { describe, expect, it, vi } from 'vitest';
import { TileCache, type TileAddress } from './tile-cache';
import type { TileImage, TilePalette, TileRequest } from './types';

const palette = (background: string): TilePalette => ({
  dark: false,
  background,
  gridline: '#000',
  text: '#000000',
  fontFamily: 'sans-serif',
});

function bitmap(width = 10, height = 10) {
  return { close: vi.fn(), width, height } as unknown as ImageBitmap;
}

/** A cache whose draws finish when the test says so. */
function setup(limit?: number) {
  const pending: { request: TileRequest; finish: (image: TileImage) => void }[] = [];
  const onChange = vi.fn();
  const onError = vi.fn();
  const cache = new TileCache(
    (request) =>
      new Promise((resolve) => {
        pending.push({ request, finish: resolve });
      }),
    onChange,
    onError,
    limit,
  );
  return { cache, pending, onChange, onError };
}

const settle = () => new Promise((resolve) => setTimeout(resolve));
const tile = (firstRow: number, firstCol = 0, sheet = 0): TileAddress => ({
  sheet,
  firstRow,
  firstCol,
});

describe('TileCache', () => {
  it('asks for a tile once per style', async () => {
    const { cache, pending, onChange } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0)]);
    cache.want([tile(0)]);
    expect(pending).toHaveLength(1);
    const image = bitmap();
    pending[0]!.finish({ bitmap: image, hash: 7, ready: true });
    await settle();
    expect(cache.get(0, 0, 0)).toBe(image);
    expect(cache.isCurrent(0, 0, 0)).toBe(true);
    expect(cache.hash(0, 0, 0)).toBe(7);
    expect(onChange).toHaveBeenCalledOnce();
    cache.want([tile(0)]);
    expect(pending).toHaveLength(1);
  });

  it('keeps a stale picture showing until its replacement arrives', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0)]);
    const first = bitmap();
    pending[0]!.finish({ bitmap: first, hash: 1, ready: true });
    await settle();

    cache.setStyle({ scale: 2, palette: palette('#000') });
    cache.want([tile(0)]);
    expect(pending).toHaveLength(2);
    expect(pending[1]!.request.scale).toBe(2);
    expect(cache.get(0, 0, 0)).toBe(first);
    expect(cache.isCurrent(0, 0, 0)).toBe(false);

    const second = bitmap();
    pending[1]!.finish({ bitmap: second, hash: 2, ready: true });
    await settle();
    expect(cache.get(0, 0, 0)).toBe(second);
    expect(first.close).toHaveBeenCalled();
  });

  it('draws only a few at once, and forgets tiles nobody wants any more', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0), tile(64), tile(128), tile(192), tile(256), tile(320)]);
    expect(pending).toHaveLength(4);
    // The view moved on, so what's left of the old list is never drawn.
    cache.want([tile(6400)]);
    for (const { finish } of pending.slice(0, 4)) {
      finish({ bitmap: bitmap(), hash: 0, ready: true });
    }
    await settle();
    expect(pending.map(({ request }) => request.firstRow)).toEqual([0, 64, 128, 192, 6400]);
  });

  it('draws a tile again once its sheet is read', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0)]);
    pending[0]!.finish({ bitmap: bitmap(), hash: 0, ready: false });
    await settle();
    expect(cache.isCurrent(0, 0, 0)).toBe(false);
    cache.want([tile(0)]);
    expect(pending).toHaveLength(1);
    cache.invalidateSheet(0);
    expect(pending).toHaveLength(2);
    expect(cache.get(0, 0, 0)).not.toBeNull();
  });

  it('keeps tiles of different sheets apart', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0, 0, 0), tile(0, 0, 1)]);
    expect(pending.map(({ request }) => request.sheet)).toEqual([0, 1]);
  });

  it('frees the longest-unused pictures, never one in view', async () => {
    // Room for three 10 x 10 pictures.
    const { cache, pending } = setup(1200);
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    const images: ImageBitmap[] = [];
    for (let index = 0; index < 4; index++) {
      cache.want([tile(index * 64)]);
      const image = bitmap();
      images.push(image);
      pending[index]!.finish({ bitmap: image, hash: 0, ready: true });
      await settle();
    }
    expect(images[0]!.close).toHaveBeenCalled();
    expect(cache.get(0, 0, 0)).toBeNull();
    expect(images[3]!.close).not.toHaveBeenCalled();
    expect(cache.get(0, 192, 0)).toBe(images[3]);
  });

  it('ignores a picture that arrives after the workbook changed', async () => {
    const { cache, pending, onChange } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0)]);
    cache.clear();
    const late = bitmap();
    pending[0]!.finish({ bitmap: late, hash: 0, ready: true });
    await settle();
    expect(late.close).toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('reports a failed drawing', async () => {
    const onError = vi.fn();
    const cache = new TileCache(() => Promise.reject(new Error('no')), vi.fn(), onError);
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.want([tile(0)]);
    await settle();
    expect(onError).toHaveBeenCalledOnce();
  });
});
