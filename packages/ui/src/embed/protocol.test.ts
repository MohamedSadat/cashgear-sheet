// The handshake as sheet-embed.1 writes it (Track S3, S5): `ready` to the parent with "*", `init`
// only from the parent, from an allowed origin, with a port, once; everything after on the port,
// in order; malformed messages dropped and never logged.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LIVE_FRAME_PARENTS, LOCAL_FRAME_PARENTS } from '../../../../apps/web/src/csp';
import { CONTRACT, type EmbedMessage, type InitMessage, type PortMessage } from './contract';
import {
  EmbedProtocol,
  LIVE_PARENTS,
  LOCAL_PARENTS,
  allowedParents,
  type Answer,
  type EmbedHandler,
  type EmbedWindow,
} from './protocol';

const WORKSPACE = 'http://127.0.0.1:4273';
const ETAG = '"v1"';

class FakeWindow implements EmbedWindow {
  readonly posted: { message: unknown; target: string }[] = [];
  readonly parent = {
    postMessage: (message: unknown, target: string) => this.posted.push({ message, target }),
  };
  private listener: ((event: MessageEvent) => void) | null = null;
  addEventListener(_: 'message', listener: (event: MessageEvent) => void) {
    this.listener = listener;
  }
  removeEventListener() {
    this.listener = null;
  }
  get listening() {
    return this.listener !== null;
  }
  /** A window message, as the browser delivers it. */
  deliver(
    data: unknown,
    options: { origin?: string; source?: unknown; ports?: MessagePort[] } = {},
  ) {
    this.listener?.({
      data,
      origin: options.origin ?? WORKSPACE,
      source: 'source' in options ? options.source : this.parent,
      ports: options.ports ?? [],
    } as unknown as MessageEvent);
  }
}

class FakeHandler implements EmbedHandler {
  readonly seen: (InitMessage | PortMessage)[] = [];
  delays = new Map<string, number>();
  async init(message: InitMessage): Promise<Answer> {
    this.seen.push(message);
    return { message: loaded('A1') };
  }
  async handle(message: PortMessage): Promise<Answer | null> {
    this.seen.push(message);
    const delay = this.delays.get(message.type) ?? 0;
    if (delay) await new Promise((done) => setTimeout(done, delay));
    if (message.type === 'requestPreview' && message.id === 'boom') throw new Error('secret');
    if (message.type === 'setTheme') return null;
    if (message.type === 'setView' && message.view.kind === 'range') {
      return { message: loaded(message.view.range ?? 'A1') };
    }
    return { message: loaded('A1') };
  }
}

function loaded(range: string): EmbedMessage {
  return {
    type: 'loaded',
    etag: ETAG,
    view: { kind: 'range', sheet: 'Budget', range },
    sheets: [{ name: 'Budget', direction: 'ltr' }],
    charts: [],
    size: { width: 100, height: 40 },
    capped: false,
  };
}

const init = (fields: Record<string, unknown> = {}) => ({
  type: 'init',
  contract: CONTRACT,
  source: { kind: 'url', url: 'https://blob.example/wb?sig=1', etag: ETAG },
  view: { kind: 'range', sheet: null, range: null },
  theme: 'light',
  direction: 'ltr',
  locale: 'en',
  ...fields,
});

const channels: MessageChannel[] = [];
function channel() {
  const made = new MessageChannel();
  channels.push(made);
  const received: unknown[] = [];
  made.port1.onmessage = (event: MessageEvent) => received.push(event.data);
  return { parentPort: made.port1, framePort: made.port2, received };
}
afterEach(() => {
  for (const made of channels.splice(0)) {
    made.port1.close();
    made.port2.close();
  }
  vi.restoreAllMocks();
});

const settle = () => new Promise((done) => setTimeout(done, 30));

function started() {
  const win = new FakeWindow();
  const handler = new FakeHandler();
  const protocol = new EmbedProtocol(win, [WORKSPACE], handler);
  protocol.start();
  return { win, handler, protocol };
}

describe('the handshake', () => {
  it('says ready to the parent, to any origin, listing the contracts it speaks', () => {
    const { win } = started();
    expect(win.posted).toEqual([
      { message: { type: 'ready', contracts: [CONTRACT] }, target: '*' },
    ]);
  });

  it('takes init only from the parent, from an allowed origin, with one port', async () => {
    const { win, handler } = started();
    const wrongOrigin = channel();
    win.deliver(init(), { origin: 'http://127.0.0.1:4274', ports: [wrongOrigin.framePort] });
    const notParent = channel();
    win.deliver(init(), { source: {}, ports: [notParent.framePort] });
    win.deliver(init(), { source: null, ports: [channel().framePort] });
    win.deliver(init());
    const two = channel();
    win.deliver(init(), { ports: [two.framePort, channel().framePort] });
    await settle();
    expect(handler.seen).toEqual([]);
    expect([wrongOrigin.received, notParent.received, two.received]).toEqual([[], [], []]);
    expect(win.listening).toBe(true);

    const good = channel();
    win.deliver(init(), { ports: [good.framePort] });
    await settle();
    expect(handler.seen).toHaveLength(1);
    expect(good.received).toEqual([loaded('A1')]);
    // Every later window message is ignored.
    expect(win.listening).toBe(false);
  });

  it('takes one init only', async () => {
    const { win, handler } = started();
    const first = channel();
    win.deliver(init(), { ports: [first.framePort] });
    const second = channel();
    win.deliver(init(), { ports: [second.framePort] });
    await settle();
    expect(handler.seen).toHaveLength(1);
    expect(second.received).toEqual([]);
  });

  it('answers an init of another contract with contract.unsupported, and nothing else', async () => {
    const { win, handler } = started();
    const port = channel();
    win.deliver(
      { type: 'init', contract: 'sheet-embed.9', anything: true },
      { ports: [port.framePort] },
    );
    await settle();
    expect(port.received).toEqual([
      { type: 'error', code: 'contract.unsupported', message: expect.any(String) },
    ]);
    port.parentPort.postMessage({
      type: 'setView',
      view: { kind: 'range', sheet: null, range: 'A1' },
    });
    const later = channel();
    win.deliver(init(), { ports: [later.framePort] });
    await settle();
    expect(handler.seen).toEqual([]);
    expect(later.received).toEqual([]);
    expect(port.received).toHaveLength(1);
  });

  it('doesn’t count an init that fails validation', async () => {
    const { win, handler } = started();
    const bad = channel();
    win.deliver(init({ theme: 'sepia' }), { ports: [bad.framePort] });
    const good = channel();
    win.deliver(init(), { ports: [good.framePort] });
    await settle();
    expect(bad.received).toEqual([]);
    expect(handler.seen).toHaveLength(1);
    expect(good.received).toHaveLength(1);
  });
});

describe('the port', () => {
  async function opened() {
    const { win, handler, protocol } = started();
    const port = channel();
    win.deliver(init(), { ports: [port.framePort] });
    await settle();
    port.received.length = 0;
    handler.seen.length = 0;
    return { port, handler, protocol };
  }

  it('drops malformed messages without logging them', async () => {
    const logs = (['log', 'info', 'warn', 'error', 'debug'] as const).map((name) =>
      vi.spyOn(console, name).mockImplementation(() => undefined),
    );
    const { port, handler } = await opened();
    const marker = 'SECRET-MARKER-1234';
    for (const data of [
      marker,
      { type: 'setView', view: { kind: 'range', sheet: marker + '/', range: null } },
      { type: 'requestPreview', id: marker + ' x', maxWidth: 10, maxHeight: 10, scale: 7 },
      { type: 'save', marker },
      init(),
      [1, 2, 3],
    ]) {
      port.parentPort.postMessage(data);
    }
    await settle();
    expect(handler.seen).toEqual([]);
    expect(port.received).toEqual([]);
    for (const log of logs) expect(log).not.toHaveBeenCalled();
  });

  it('handles messages one at a time, answering in order', async () => {
    const { port, handler } = await opened();
    handler.delays.set('setView', 40);
    port.parentPort.postMessage({
      type: 'setView',
      view: { kind: 'range', sheet: null, range: 'B2' },
    });
    port.parentPort.postMessage({
      type: 'setTheme',
      theme: 'dark',
      direction: 'rtl',
      locale: 'ar',
    });
    port.parentPort.postMessage({
      type: 'refresh',
      source: { kind: 'url', url: 'https://b.example/x', etag: ETAG },
    });
    await new Promise((done) => setTimeout(done, 120));
    expect(handler.seen.map((m) => m.type)).toEqual(['setView', 'setTheme', 'refresh']);
    expect(port.received).toEqual([loaded('B2'), loaded('A1')]);
  });

  it('answers a preview that fails with an error carrying its id, and no detail', async () => {
    const { port } = await opened();
    port.parentPort.postMessage({
      type: 'requestPreview',
      id: 'boom',
      maxWidth: 100,
      maxHeight: 100,
      scale: 1,
    });
    await settle();
    expect(port.received).toHaveLength(1);
    const answer = port.received[0] as { type: string; id: string; message: string };
    expect(answer).toMatchObject({ type: 'error', code: 'source.unavailable', id: 'boom' });
    expect(answer.message).not.toContain('secret');
  });

  it('sends resize once there is a port', async () => {
    const { port, protocol } = await opened();
    protocol.send({ type: 'resize', size: { width: 10, height: 20 } });
    await settle();
    expect(port.received).toEqual([{ type: 'resize', size: { width: 10, height: 20 } }]);
  });
});

describe('who may frame the embed', () => {
  it('is Workspace’s live origins, and its local ones for a page served locally', () => {
    expect(allowedParents('https://sheet.cashgearerp.com')).toEqual(LIVE_PARENTS);
    expect(allowedParents('http://127.0.0.1:4173')).toEqual([...LIVE_PARENTS, ...LOCAL_PARENTS]);
    expect(allowedParents('http://localhost:5173')).toContain('http://localhost:4273');
  });

  it('is the same list the pages’ frame-ancestors give', () => {
    expect(LIVE_PARENTS).toEqual(LIVE_FRAME_PARENTS);
    expect(LOCAL_PARENTS).toEqual(LOCAL_FRAME_PARENTS);
  });
});
