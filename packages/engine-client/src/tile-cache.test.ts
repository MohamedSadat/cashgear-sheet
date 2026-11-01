import { describe, expect, it, vi } from 'vitest';
import { TileCache } from './tile-cache';
import type { TileImage, TilePalette, TileRequest } from './types';

const palette = (background: string): TilePalette => ({
  background,
  gridline: '#000',
  text: '#000',
  fontFamily: 'sans-serif',
});

function bitmap() {
  return { close: vi.fn() } as unknown as ImageBitmap;
}

/** A cache whose draws finish when the test says so. */
function setup() {
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
  );
  return { cache, pending, onChange, onError };
}

const settle = () => new Promise((resolve) => setTimeout(resolve));

describe('TileCache', () => {
  it('asks for a tile once per style', async () => {
    const { cache, pending, onChange } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    expect(cache.get(0, 0)).toBeNull();
    expect(cache.get(0, 0)).toBeNull();
    expect(pending).toHaveLength(1);
    expect(pending[0].request).toMatchObject({ firstRow: 0, firstCol: 0, scale: 1 });

    const first = bitmap();
    pending[0].finish({ bitmap: first, hash: 7 });
    await settle();
    expect(onChange).toHaveBeenCalledOnce();
    expect(cache.get(0, 0)).toBe(first);
    expect(cache.hash(0, 0)).toBe(7);
    expect(cache.isCurrent(0, 0)).toBe(true);
    expect(pending).toHaveLength(1);
  });

  it('keeps the stale picture until the new style arrives', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.get(0, 0);
    const light = bitmap();
    pending[0].finish({ bitmap: light, hash: 7 });
    await settle();

    cache.setStyle({ scale: 1, palette: palette('#000') });
    expect(cache.isCurrent(0, 0)).toBe(false);
    expect(cache.get(0, 0)).toBe(light);
    expect(pending[1].request.palette.background).toBe('#000');

    const dark = bitmap();
    pending[1].finish({ bitmap: dark, hash: 7 });
    await settle();
    expect(cache.get(0, 0)).toBe(dark);
    expect(cache.isCurrent(0, 0)).toBe(true);
    expect(light.close).toHaveBeenCalledOnce();
  });

  it('drops an old draw that finishes after a newer one', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.get(0, 0);
    cache.setStyle({ scale: 1, palette: palette('#000') });
    cache.get(0, 0);
    const dark = bitmap();
    pending[1].finish({ bitmap: dark, hash: 7 });
    await settle();
    const late = bitmap();
    pending[0].finish({ bitmap: late, hash: 7 });
    await settle();
    expect(cache.get(0, 0)).toBe(dark);
    expect(late.close).toHaveBeenCalledOnce();
    expect(dark.close).not.toHaveBeenCalled();
  });

  it('closes everything when disposed, and later arrivals too', async () => {
    const { cache, pending } = setup();
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.get(0, 0);
    cache.get(0, 16);
    const kept = bitmap();
    pending[0].finish({ bitmap: kept, hash: 7 });
    await settle();
    cache.dispose();
    expect(kept.close).toHaveBeenCalledOnce();
    const late = bitmap();
    pending[1].finish({ bitmap: late, hash: 7 });
    await settle();
    expect(late.close).toHaveBeenCalledOnce();
  });

  it('reports a failed draw', async () => {
    const onError = vi.fn();
    const cache = new TileCache(() => Promise.reject(new Error('no')), vi.fn(), onError);
    cache.setStyle({ scale: 1, palette: palette('#fff') });
    cache.get(0, 0);
    await settle();
    expect(onError).toHaveBeenCalledOnce();
  });
});
