// Reconnect (Phase 18, plan B.4; decisions, 2026-11-11): a bound sheet whose session ended waits
// for the user's next launch from Cash Gear ERP. The new tab redeems its own ticket, then asks
// over `BroadcastChannel`, which reaches only tabs of this origin, whether a tab waits for that
// company, user and entity. The first that answers is handed the session, in memory, checks the
// scope again and takes it or refuses it; the new tab then closes, or opens its own sheet when no
// tab waits. A launch for another company or user is never used up by a waiting tab.

import type { SheetSession } from './contract.ts';

export const RECONNECT_CHANNEL = 'cgs-erp-reconnect';

/** What a session is for: a binding takes a session only for the same three. */
export interface SessionScope {
  readonly company: string;
  readonly userId: string;
  readonly entity: string;
}

export const scopeOf = (session: SessionScope): SessionScope => ({
  company: session.company,
  userId: session.userId,
  entity: session.entity,
});

/** Companies and entities compare without regard to case, as the ERP's keys do; users exactly. */
export function sameScope(a: SessionScope, b: SessionScope): boolean {
  return (
    a.company.toLowerCase() === b.company.toLowerCase() &&
    a.userId === b.userId &&
    a.entity.toLowerCase() === b.entity.toLowerCase()
  );
}

type Message =
  | { readonly type: 'waiting?'; readonly ask: string; readonly scope: SessionScope }
  | { readonly type: 'waiting'; readonly ask: string; readonly tab: string }
  | {
      readonly type: 'handover';
      readonly ask: string;
      readonly tab: string;
      readonly session: SheetSession;
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

/** Why a waiting tab refused a session handed over. */
export type Refusal = 'scope' | 'unreadable' | 'failed';

export interface WaitOptions {
  /** What this tab's binding is for. */
  readonly scope: SessionScope;
  /**
   * Takes a session handed over, whose scope has been checked. Resolves true once this tab uses
   * it, false to refuse it.
   */
  readonly take: (session: SheetSession) => Promise<boolean>;
  /** Hears a session refused, and why. */
  readonly onRefused?: (why: Refusal, session: unknown) => void;
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
      if (!busy && sameScope(message.scope, options.scope)) {
        channel.postMessage({ type: 'waiting', ask: message.ask, tab } satisfies Message);
      }
      return;
    }
    if (message.type !== 'handover' || message.tab !== tab || busy) return;
    const reply = (type: 'taken' | 'refused') => {
      if (!closed) channel.postMessage({ type, ask: message.ask, tab } satisfies Message);
    };
    const session = message.session as Partial<SheetSession> | undefined;
    if (
      !session ||
      typeof session.company !== 'string' ||
      typeof session.userId !== 'string' ||
      typeof session.entity !== 'string'
    ) {
      options.onRefused?.('unreadable', session);
      reply('refused');
      return;
    }
    // Checked again here: a message can come from any tab of this origin.
    if (!sameScope(session as SessionScope, options.scope)) {
      options.onRefused?.('scope', session);
      reply('refused');
      return;
    }
    busy = true;
    options
      .take(message.session)
      .then((taken) => {
        reply(taken ? 'taken' : 'refused');
        if (taken) close();
        else options.onRefused?.('failed', session);
      })
      .catch(() => {
        reply('refused');
        options.onRefused?.('failed', session);
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
 * Offers a just-redeemed session to a tab waiting for the same company, user and entity.
 * `taken`: that tab uses it now, so this one has nothing to open. `refused`: a tab answered but
 * refused it. `none`: no tab waits, or none answered in time.
 */
export function offerHandover(
  session: SheetSession,
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
        channel.postMessage({ type: 'handover', ask, tab: chosen, session } satisfies Message);
        return;
      }
      if ((message.type === 'taken' || message.type === 'refused') && message.tab === chosen) {
        finish(message.type);
      }
    };
    channel.addEventListener('message', listener);
    timer = setTimeout(() => finish('none'), options.askMs ?? 600);
    channel.postMessage({ type: 'waiting?', ask, scope: scopeOf(session) } satisfies Message);
  });
}
