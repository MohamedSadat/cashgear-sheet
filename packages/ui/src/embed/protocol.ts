// The `sheet-embed.1` handshake, on the frame's side (Track S3):
//
// 1. Once its script runs, the embed posts `ready` to `window.parent` with target "*": it holds
//    nothing private, and can't know the parent's origin before `init`.
// 2. It takes `init` only from `window.parent`, from an allowed Workspace origin, with one port,
//    and only once. Every later window message is ignored. An `init` of another contract is
//    answered on its port with `contract.unsupported`, and nothing else happens.
// 3. Everything after is on the port, one message at a time and in order. A message that fails
//    validation is dropped, and its content is never logged.
import {
  CONTRACTS,
  initContract,
  parseParentMessage,
  parsePortMessage,
  type EmbedMessage,
  type InitMessage,
  type PortMessage,
} from './contract';

/** Cash Gear Workspace's live origins, which may frame `/embed` (decisions, 2026-11-14). */
export const LIVE_PARENTS = [
  'https://workspace.cashgearerp.com',
  'tauri://localhost',
  'http://tauri.localhost',
];

/** Workspace's development and test origins, allowed when the embed itself is served locally. */
export const LOCAL_PARENTS = [
  'http://127.0.0.1:5273',
  'http://127.0.0.1:4273',
  'http://localhost:5273',
  'http://localhost:4273',
];

/** The origins whose `init` this page takes: Workspace's, and its local ones for a local page. */
export function allowedParents(pageOrigin: string): string[] {
  const local = /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i.test(pageOrigin);
  return local ? [...LIVE_PARENTS, ...LOCAL_PARENTS] : [...LIVE_PARENTS];
}

/** An answer to post on the port, with what it transfers. */
export interface Answer {
  message: EmbedMessage;
  transfer?: Transferable[];
}

/** What the embed does with each message; it answers with `loaded`, `preview` or `error`. */
export interface EmbedHandler {
  init(message: InitMessage): Promise<Answer>;
  /** Null for a message that has no answer (`setTheme`). */
  handle(message: PortMessage): Promise<Answer | null>;
}

/** The parts of `window` the handshake uses. */
export interface EmbedWindow {
  readonly parent: unknown;
  addEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
  removeEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
}

interface Postable {
  postMessage(message: unknown, targetOrigin: string): void;
}

export class EmbedProtocol {
  private readonly origins: ReadonlySet<string>;
  private port: MessagePort | null = null;
  /** An `init` was taken, so the window says nothing more. */
  private taken = false;
  /** The `init` was another contract's: nothing else happens. */
  private refused = false;
  private queue: Promise<void> = Promise.resolve();
  private disposed = false;

  constructor(
    private readonly win: EmbedWindow,
    origins: readonly string[],
    private readonly handler: EmbedHandler,
  ) {
    this.origins = new Set(origins);
  }

  /** Listens for `init`, then says `ready` to the parent. */
  start() {
    this.win.addEventListener('message', this.onWindowMessage);
    const parent = this.win.parent as Postable | null;
    parent?.postMessage({ type: 'ready', contracts: [...CONTRACTS] }, '*');
  }

  /** Posts on the port, once there is one: `resize`, say. */
  send(message: EmbedMessage, transfer: Transferable[] = []) {
    if (this.disposed || this.refused || !this.port) return;
    this.port.postMessage(message, transfer);
  }

  dispose() {
    this.disposed = true;
    this.win.removeEventListener('message', this.onWindowMessage);
    if (this.port) {
      this.port.onmessage = null;
      this.port.close();
    }
  }

  private readonly onWindowMessage = (event: MessageEvent) => {
    if (this.taken || this.disposed) return;
    if (event.source !== this.win.parent || !this.origins.has(event.origin)) return;
    const ports = event.ports;
    if (!ports || ports.length !== 1) return;
    const contract = initContract(event.data);
    if (contract === null) return;
    const port = ports[0]!;
    if (!CONTRACTS.includes(contract)) {
      this.take(port);
      this.refused = true;
      port.postMessage({
        type: 'error',
        code: 'contract.unsupported',
        message: `This embed speaks ${CONTRACTS.join(', ')}.`,
      } satisfies EmbedMessage);
      return;
    }
    const init = parseParentMessage(event.data);
    // An init that fails validation doesn't count as the one init.
    if (!init || init.type !== 'init') return;
    this.take(port);
    port.onmessage = this.onPortMessage;
    this.enqueue(() => this.handler.init(init), null);
  };

  private take(port: MessagePort) {
    this.taken = true;
    this.port = port;
    this.win.removeEventListener('message', this.onWindowMessage);
  }

  private readonly onPortMessage = (event: MessageEvent) => {
    if (this.disposed) return;
    const message = parsePortMessage(event.data);
    if (!message) return;
    const id = message.type === 'requestPreview' ? message.id : null;
    this.enqueue(() => this.handler.handle(message), id);
  };

  private enqueue(run: () => Promise<Answer | null>, id: string | null) {
    this.queue = this.queue.then(async () => {
      if (this.disposed) return;
      let answer: Answer | null;
      try {
        answer = await run();
      } catch {
        answer = {
          message: {
            type: 'error',
            code: 'source.unavailable',
            message: 'The embed couldn’t do that.',
            ...(id ? { id } : {}),
          },
        };
      }
      if (answer) this.send(answer.message, answer.transfer);
    });
  }
}
