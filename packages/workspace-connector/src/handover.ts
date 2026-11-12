// Reconnect (decisions, 2026-11-12, Track S1): a Workspace workbook whose session ended waits for
// the user's next "Open in Cash Gear Sheet" of the same workbook. The new tab redeems its own
// ticket, then asks over `BroadcastChannel`, which reaches only tabs of this origin, whether a tab
// waits for that workbook, user and API. The first that answers is handed the session, in memory,
// checks the scope again and takes it or refuses it; the new tab then closes, or opens the
// workbook itself when no tab waits. The protocol is ERP data's (`@cgs/erp-connector`'s
// `reconnect.ts`), on a channel of its own.

import type { FileSession } from './contract.ts';

export const RECONNECT_CHANNEL = 'cgs-cloud-reconnect';

/** What a session is for: a waiting tab takes a session only for the same three. */
export interface CloudScope {
  readonly apiBase: string;
  readonly workbookId: string;
  readonly userId: string;
}

export const scopeOf = (session: FileSession, apiBase: string): CloudScope => ({
  apiBase,
  workbookId: session.workbook.id,
  userId: session.user.id,
});

/** Ids are UUIDs, compared without regard to case; the API's origin exactly. */
export function sameScope(a: CloudScope, b: CloudScope): boolean {
  return (
    a.apiBase === b.apiBase &&
    a.workbookId.toLowerCase() === b.workbookId.toLowerCase() &&
    a.userId.toLowerCase() === b.userId.toLowerCase()
  );
}

type Message =
  | { readonly type: 'waiting?'; readonly ask: string; readonly scope: CloudScope }
  | { readonly type: 'waiting'; readonly ask: string; readonly tab: string }
  | {
      readonly type: 'handover';
      readonly ask: string;
      readonly tab: string;
      readonly apiBase: string;
      readonly session: FileSession;
    }
  | { readonly type: 'taken' | 'refused'; readonly ask: string; readonly tab: string };

/** The parts of `BroadcastChannel` this uses, so tests can pass their own. */
export interface Channel {
  postMessage(message: unknown): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  removeEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  close(): void;
}

export type ChannelFactory = () => Channel;

const broadcast: ChannelFactory = () =>
  new BroadcastChannel(RECONNECT_CHANNEL) as unknown as Channel;

const id = () => globalThis.crypto.randomUUID();

function isMessage(data: unknown): data is Message {
  return (
    typeof data === 'object' &&
    data !== null &&
    typeof (data as { type?: unknown }).type === 'string' &&
    typeof (data as { ask?: unknown }).ask === 'string'
  );
}

function isScope(value: unknown): value is CloudScope {
  const scope = value as Partial<CloudScope> | null;
  return (
    typeof scope === 'object' &&
    scope !== null &&
    typeof scope.apiBase === 'string' &&
    typeof scope.workbookId === 'string' &&
    typeof scope.userId === 'string'
  );
}

/** Why a waiting tab refused a session handed over. */
export type Refusal = 'scope' | 'unreadable' | 'failed';

export interface WaitOptions {
  /** What this tab's workbook is. */
  readonly scope: CloudScope;
  /** Takes a session handed over, whose scope has been checked. True once this tab uses it. */
  readonly take: (session: FileSession) => Promise<boolean>;
  /** Hears a session refused, and why. */
  readonly onRefused?: (why: Refusal) => void;
  readonly channel?: ChannelFactory;
}

/**
 * Waits for a launch to hand its session over, until the returned function stops it or a session
 * is taken, when it stops by itself once it has answered.
 */
export function waitForHandover(options: WaitOptions): () => void {
  const channel = (options.channel ?? broadcast)();
  const tab = id();
  let busy = false;
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    channel.removeEventListener('message', listener);
    channel.close();
  };
  function listener(event: { data: unknown }) {
    const message = event.data;
    if (!isMessage(message)) return;
    if (message.type === 'waiting?') {
      if (!busy && isScope(message.scope) && sameScope(message.scope, options.scope)) {
        channel.postMessage({ type: 'waiting', ask: message.ask, tab } satisfies Message);
      }
      return;
    }
    if (message.type !== 'handover' || message.tab !== tab || busy) return;
    const reply = (type: 'taken' | 'refused') => {
      if (!closed) channel.postMessage({ type, ask: message.ask, tab } satisfies Message);
    };
    const session = message.session as Partial<FileSession> | undefined;
    if (
      typeof message.apiBase !== 'string' ||
      !session ||
      typeof session.workbook?.id !== 'string' ||
      typeof session.user?.id !== 'string'
    ) {
      options.onRefused?.('unreadable');
      reply('refused');
      return;
    }
    // Checked again here: a message can come from any tab of this origin.
    if (!sameScope(scopeOf(session as FileSession, message.apiBase), options.scope)) {
      options.onRefused?.('scope');
      reply('refused');
      return;
    }
    busy = true;
    options
      .take(message.session)
      .then((taken) => {
        reply(taken ? 'taken' : 'refused');
        if (taken) close();
        else options.onRefused?.('failed');
      })
      .catch(() => {
        reply('refused');
        options.onRefused?.('failed');
      })
      .finally(() => {
        busy = false;
      });
  }
  channel.addEventListener('message', listener);
  return close;
}

export interface OfferOptions {
  /** How long to listen for a waiting tab. */
  readonly askMs?: number;
  /** How long a waiting tab has to take the session. */
  readonly takeMs?: number;
  readonly channel?: ChannelFactory;
}

/**
 * Offers a just-redeemed session to a tab waiting for the same workbook, user and API. `taken`:
 * that tab uses it now, so this one has nothing to open. `refused`: a tab answered but refused
 * it. `none`: no tab waits, or none answered in time.
 */
export function offerHandover(
  session: FileSession,
  apiBase: string,
  options: OfferOptions = {},
): Promise<'taken' | 'refused' | 'none'> {
  const channel = (options.channel ?? broadcast)();
  const ask = id();
  return new Promise((resolve) => {
    let chosen: string | null = null;
    let timer: ReturnType<typeof setTimeout>;
    const finish = (outcome: 'taken' | 'refused' | 'none') => {
      clearTimeout(timer);
      channel.removeEventListener('message', listener);
      channel.close();
      resolve(outcome);
    };
    const listener = (event: { data: unknown }) => {
      const message = event.data;
      if (!isMessage(message) || message.ask !== ask) return;
      if (message.type === 'waiting' && chosen === null) {
        chosen = message.tab;
        clearTimeout(timer);
        timer = setTimeout(() => finish('none'), options.takeMs ?? 20_000);
        channel.postMessage({
          type: 'handover',
          ask,
          tab: chosen,
          apiBase,
          session,
        } satisfies Message);
        return;
      }
      if ((message.type === 'taken' || message.type === 'refused') && message.tab === chosen) {
        finish(message.type);
      }
    };
    channel.addEventListener('message', listener);
    timer = setTimeout(() => finish('none'), options.askMs ?? 600);
    channel.postMessage({
      type: 'waiting?',
      ask,
      scope: scopeOf(session, apiBase),
    } satisfies Message);
  });
}
