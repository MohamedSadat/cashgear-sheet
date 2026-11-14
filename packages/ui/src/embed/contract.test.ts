// sheet-embed.1's validators (Track S5): each field's rule, both directions, and the messages
// that must be dropped. The vectors Workspace publishes are run in vectors.test.ts.
import { describe, expect, it } from 'vitest';
import {
  CONTRACT,
  initContract,
  parseEmbedMessage,
  parseParentMessage,
  parsePortMessage,
  validEtag,
  validPng,
  validRange,
  validSheetName,
  validUrl,
} from './contract';

const ETAG = '"0x8DCF1A2B3C4D5E6"';
const init = (fields: Record<string, unknown> = {}) => ({
  type: 'init',
  contract: CONTRACT,
  source: { kind: 'url', url: 'https://cgw.blob.core.windows.net/wb/v1?sig=x', etag: ETAG },
  view: { kind: 'range', sheet: null, range: null },
  theme: 'light',
  direction: 'ltr',
  locale: 'en',
  ...fields,
});

function png(size = 64): ArrayBuffer {
  const bytes = new Uint8Array(size);
  bytes.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  return bytes.buffer;
}

describe('field rules', () => {
  it('takes https URLs, and http only on this machine, up to 4,096 characters', () => {
    expect(validUrl('https://example.blob.core.windows.net/a?b=c')).toBe(true);
    expect(validUrl('http://127.0.0.1:10000/devstoreaccount1/x')).toBe(true);
    expect(validUrl('http://localhost:4281/blobs/x')).toBe(true);
    expect(validUrl('http://example.com/x')).toBe(false);
    expect(validUrl('ftp://example.com/x')).toBe(false);
    expect(validUrl('javascript:alert(1)')).toBe(false);
    expect(validUrl('/relative')).toBe(false);
    expect(validUrl('https://a.example/' + 'x'.repeat(4096))).toBe(false);
    expect(validUrl('https://a.example/' + 'x'.repeat(4096 - 18))).toBe(true);
  });

  it('takes a quoted entity tag of 3 to 256 characters', () => {
    expect(validEtag('"a"')).toBe(true);
    expect(validEtag(ETAG)).toBe(true);
    expect(validEtag('""')).toBe(false);
    expect(validEtag('abc')).toBe(false);
    expect(validEtag('W/"abc"')).toBe(false);
    expect(validEtag('"a"b"')).toBe(false);
    expect(validEtag('"' + 'a'.repeat(254) + '"')).toBe(true);
    expect(validEtag('"' + 'a'.repeat(255) + '"')).toBe(false);
  });

  it('takes sheet names as Excel does', () => {
    expect(validSheetName('Budget 2027')).toBe(true);
    expect(validSheetName('الميزانية')).toBe(true);
    expect(validSheetName('x'.repeat(31))).toBe(true);
    expect(validSheetName('x'.repeat(32))).toBe(false);
    expect(validSheetName('')).toBe(false);
    for (const bad of ['a[b', 'a]b', 'a:b', 'a*b', 'a?b', 'a/b', 'a\\b']) {
      expect(validSheetName(bad)).toBe(false);
    }
  });

  it('takes A1 ranges without a sheet or $, inside the grid', () => {
    for (const good of ['A1', 'A1:D20', 'XFD1048576', 'A1:XFD1048576', 'D20:A1']) {
      expect(validRange(good)).toBe(true);
    }
    for (const bad of [
      '',
      'a1',
      '$A$1',
      'Sheet1!A1',
      'XFE1',
      'A0',
      'A1048577',
      'A1:',
      'A:A',
      '1:1',
      'A1:B2:C3',
    ]) {
      expect(validRange(bad)).toBe(false);
    }
  });

  it('takes a PNG of at most 2 MB as a preview', () => {
    expect(validPng(png())).toBe(true);
    expect(validPng(png(2 * 1024 * 1024))).toBe(true);
    expect(validPng(png(2 * 1024 * 1024 + 1))).toBe(false);
    expect(validPng(new Uint8Array(64).buffer)).toBe(false);
    expect(validPng(new Uint8Array(png()))).toBe(false);
  });
});

describe('the parent’s messages', () => {
  it('reads an init, and refuses one with a field the contract has not', () => {
    expect(parseParentMessage(init())).toEqual(init());
    expect(parseParentMessage(init({ extra: 1 }))).toBeNull();
    expect(
      parseParentMessage(JSON.parse(JSON.stringify(init()).replace('{', '{"__proto__":{},'))),
    ).toBeNull();
    expect(parsePortMessage(init())).toBeNull();
  });

  it('drops an init with a bad field', () => {
    expect(parseParentMessage(init({ theme: 'blue' }))).toBeNull();
    expect(parseParentMessage(init({ direction: 'up' }))).toBeNull();
    expect(parseParentMessage(init({ locale: 'fr' }))).toBeNull();
    expect(parseParentMessage(init({ view: { kind: 'range', sheet: null } }))).toBeNull();
    expect(
      parseParentMessage(init({ view: { kind: 'range', sheet: 'a/b', range: null } })),
    ).toBeNull();
    expect(
      parseParentMessage(init({ source: { kind: 'url', url: 'http://x.test/a', etag: ETAG } })),
    ).toBeNull();
    expect(
      parseParentMessage(init({ source: { kind: 'bytes', bytes: new Uint8Array(4), etag: ETAG } })),
    ).toBeNull();
    expect(
      parseParentMessage(init({ source: { kind: 'bytes', bytes: new ArrayBuffer(4), etag: 'x' } })),
    ).toBeNull();
  });

  it('knows an init of another contract, whatever its fields', () => {
    expect(initContract({ type: 'init', contract: 'sheet-embed.2', source: 1 })).toBe(
      'sheet-embed.2',
    );
    expect(parseParentMessage({ type: 'init', contract: 'sheet-embed.2' })).toBeNull();
    expect(initContract({ type: 'init' })).toBeNull();
    expect(initContract({ type: 'setView', contract: CONTRACT })).toBeNull();
  });

  it('reads each port message', () => {
    const range = { kind: 'range', sheet: 'Budget', range: 'A1:D20' };
    const chart = { kind: 'chart', sheet: null, chartId: 'Chart 1' };
    expect(parsePortMessage({ type: 'setView', view: range })).toEqual({
      type: 'setView',
      view: range,
    });
    expect(parsePortMessage({ type: 'setView', view: chart })).toEqual({
      type: 'setView',
      view: chart,
    });
    expect(
      parsePortMessage({ type: 'setTheme', theme: 'dark', direction: 'rtl', locale: 'ar' }),
    ).toEqual({
      type: 'setTheme',
      theme: 'dark',
      direction: 'rtl',
      locale: 'ar',
    });
    const bytes = new ArrayBuffer(8);
    expect(
      parsePortMessage({ type: 'refresh', source: { kind: 'bytes', bytes, etag: ETAG } }),
    ).toEqual({
      type: 'refresh',
      source: { kind: 'bytes', bytes, etag: ETAG },
    });
    expect(
      parsePortMessage({
        type: 'requestPreview',
        id: 'p-1',
        maxWidth: 800,
        maxHeight: 600,
        scale: 2,
      }),
    ).toEqual({
      type: 'requestPreview',
      id: 'p-1',
      maxWidth: 800,
      maxHeight: 600,
      scale: 2,
    });
  });

  it('drops what isn’t a valid message', () => {
    const bad = [
      null,
      'init',
      [init()],
      { type: 'save' },
      { type: 'setView', view: { kind: 'chart', sheet: null, chartId: '' } },
      { type: 'setView', view: { kind: 'chart', sheet: null, chartId: 'x'.repeat(129) } },
      { type: 'setTheme', theme: 'dark', direction: 'rtl' },
      { type: 'requestPreview', id: 'a b', maxWidth: 800, maxHeight: 600, scale: 2 },
      { type: 'requestPreview', id: 'x'.repeat(65), maxWidth: 800, maxHeight: 600, scale: 2 },
      { type: 'requestPreview', id: 'p', maxWidth: 15, maxHeight: 600, scale: 2 },
      { type: 'requestPreview', id: 'p', maxWidth: 800, maxHeight: 4097, scale: 2 },
      { type: 'requestPreview', id: 'p', maxWidth: 800.5, maxHeight: 600, scale: 2 },
      { type: 'requestPreview', id: 'p', maxWidth: 800, maxHeight: 600, scale: 2.5 },
      new (class Message {
        type = 'setTheme';
        theme = 'dark';
        direction = 'ltr';
        locale = 'en';
      })(),
    ];
    for (const data of bad) expect(parsePortMessage(data)).toBeNull();
  });
});

describe('the embed’s messages', () => {
  const view = { kind: 'range', sheet: 'Budget', range: 'A1:D20' };
  const loaded = {
    type: 'loaded',
    etag: ETAG,
    view,
    sheets: [{ name: 'Budget', direction: 'ltr' }],
    charts: [{ id: 'Chart 1', title: 'Sales', sheet: 'Budget' }],
    size: { width: 320, height: 400 },
    capped: false,
  };

  it('are what the parent accepts', () => {
    expect(parseEmbedMessage({ type: 'ready', contracts: [CONTRACT] })).toEqual({
      type: 'ready',
      contracts: [CONTRACT],
    });
    expect(parseEmbedMessage(loaded)).toEqual(loaded);
    expect(
      parseEmbedMessage({ type: 'resize', size: { width: 1, height: 100_000 } }),
    ).not.toBeNull();
    const preview = {
      type: 'preview',
      id: 'p-1',
      bytes: png(),
      width: 320,
      height: 200,
      etag: ETAG,
      view,
    };
    expect(parseEmbedMessage(preview)).toEqual(preview);
    expect(parseEmbedMessage({ type: 'openRequested' })).toEqual({ type: 'openRequested' });
    expect(parseEmbedMessage({ type: 'error', code: 'view.notFound', message: 'gone' })).toEqual({
      type: 'error',
      code: 'view.notFound',
      message: 'gone',
    });
    expect(
      parseEmbedMessage({ type: 'error', code: 'source.unavailable', message: '', id: 'p-1' }),
    ).toEqual({
      type: 'error',
      code: 'source.unavailable',
      message: '',
      id: 'p-1',
    });
  });

  it('are refused when a field breaks its rule', () => {
    expect(
      parseEmbedMessage({ ...loaded, view: { kind: 'range', sheet: null, range: 'A1' } }),
    ).toBeNull();
    expect(parseEmbedMessage({ ...loaded, size: { width: 0, height: 1 } })).toBeNull();
    expect(parseEmbedMessage({ ...loaded, size: { width: 1, height: 100_001 } })).toBeNull();
    expect(parseEmbedMessage({ ...loaded, capped: 'no' })).toBeNull();
    expect(parseEmbedMessage({ ...loaded, sheets: [{ name: 'a', direction: 'up' }] })).toBeNull();
    expect(parseEmbedMessage({ type: 'ready', contracts: [] })).toBeNull();
    expect(parseEmbedMessage({ type: 'error', code: 'nope', message: '' })).toBeNull();
    expect(
      parseEmbedMessage({ type: 'error', code: 'view.notFound', message: 'x'.repeat(501) }),
    ).toBeNull();
    expect(
      parseEmbedMessage({
        type: 'preview',
        id: 'p',
        bytes: new ArrayBuffer(64),
        width: 1,
        height: 1,
        etag: ETAG,
        view,
      }),
    ).toBeNull();
  });
});
