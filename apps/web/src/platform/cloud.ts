import {
  WorkspaceError,
  WorkspaceHttp,
  WorkspaceSession,
  offerHandover,
  readCloudLaunch,
  waitForHandover,
  workspaceApiBase,
  type CloudLaunch,
} from '@cgs/workspace-connector';
import {
  WORKBOOK_EXTENSIONS,
  type CloudReason,
  type CloudSaveError,
  type CloudStatus,
  type CloudWorkbook,
  type OpenedFile,
  type SaveTarget,
  type SheetPlatform,
} from '@cgs/ui';
import { webPlatform } from './web';

/**
 * The launch from Cash Gear Workspace's "Open in Cash Gear Sheet", read and cut out of the address
 * bar and history when this module loads, which is before the app renders (Track S2). Workspace's
 * origin is configured, never taken from the link: `VITE_WORKSPACE_API_URL` at build time, else
 * the mock on this machine for a page served from it, else Cash Gear Workspace's own.
 */
export const cloudLaunch: CloudLaunch | null =
  typeof window === 'undefined'
    ? null
    : readCloudLaunch(
        window.location,
        window.history,
        workspaceApiBase(window.location.origin, import.meta.env.VITE_WORKSPACE_API_URL),
      );

/** A save to Workspace that didn't go, as the shell's `isCloudSaveError` knows it. */
class CloudFailure extends Error implements CloudSaveError {
  override readonly name = 'CloudSaveError' as const;
  readonly reason: CloudReason;
  readonly current?: { savedBy: string; savedAt: string };
  readonly copy: boolean;

  constructor(
    reason: CloudReason,
    message: string,
    copy = false,
    current?: CloudFailure['current'],
  ) {
    super(message);
    this.reason = reason;
    this.copy = copy;
    if (current) this.current = current;
  }
}

function failure(error: unknown, copy = false): CloudFailure {
  if (error instanceof CloudFailure) return error;
  if (error instanceof WorkspaceError) {
    const current = error.problem?.current;
    return new CloudFailure(
      error.kind,
      error.message,
      copy,
      current && { savedBy: current.savedBy.name, savedAt: current.savedAt },
    );
  }
  return new CloudFailure('server', error instanceof Error ? error.message : String(error), copy);
}

/** A workbook's name as the shell shows and saves it: with `.xlsx` unless it has its own. */
function fileName(name: string): string {
  const lower = name.toLowerCase();
  return WORKBOOK_EXTENSIONS.some((extension) => lower.endsWith(extension)) ? name : `${name}.xlsx`;
}

/** What marks an opened file as Workspace's, and the version it was read or saved as. */
class CloudRef {
  constructor(
    readonly connection: CloudConnection,
    readonly workbookId: string,
    readonly etag: string | null,
  ) {}
}

const browserLanguage = (): 'en' | 'ar' =>
  typeof navigator !== 'undefined' && /^ar\b/i.test(navigator.language) ? 'ar' : 'en';

/**
 * One tab's Workspace workbook: its file session, which only this object holds, what the tab
 * last read or saved, and the status the shell's badge follows.
 */
class CloudConnection implements CloudWorkbook {
  readonly #launch: CloudLaunch;
  #session: WorkspaceSession | null = null;
  #status: CloudStatus = { kind: 'opening', name: null, received: 0, total: null };
  readonly #listeners = new Set<(status: CloudStatus) => void>();
  #language: 'en' | 'ar' = browserLanguage();
  #stopWaiting: (() => void) | null = null;
  #stopWatching: (() => void) | null = null;
  #savedAt: number | null = null;
  #opened = false;

  constructor(launch: CloudLaunch) {
    this.#launch = launch;
    // The tab closing ends the session in Workspace, unless it was handed to another tab.
    window.addEventListener('pagehide', () => this.#session?.end());
  }

  language() {
    return this.#language;
  }

  status() {
    return this.#status;
  }

  onStatus(listener: (status: CloudStatus) => void) {
    this.#listeners.add(listener);
    return () => void this.#listeners.delete(listener);
  }

  #set(status: CloudStatus) {
    this.#status = status;
    for (const listener of this.#listeners) listener(status);
  }

  #http() {
    return new WorkspaceHttp({ baseUrl: this.#launch.apiBase });
  }

  /** Takes a session up: the badge follows it, and its end shows Reconnect. */
  #use(session: WorkspaceSession) {
    this.#stopWatching?.();
    this.#session = session;
    this.#language = session.language;
    this.#stopWatching = session.onState((state) => {
      if (state === 'expired' && this.#session === session && this.#status.kind === 'open') {
        this.#set({ kind: 'ended', name: session.workbook.name });
      }
    });
  }

  #showOpen() {
    const session = this.#session;
    if (!session) return;
    this.#set({
      kind: 'open',
      name: session.workbook.name,
      access: session.access,
      savedAt: this.#savedAt,
    });
  }

  /** Redeems the launch, offers it to a tab waiting to reconnect, and reads the workbook. */
  async open(): Promise<OpenedFile | null> {
    if (this.#opened) return null;
    this.#opened = true;
    let session: WorkspaceSession;
    try {
      session = await WorkspaceSession.redeem(this.#http(), this.#launch.ticket);
    } catch (error) {
      this.#set({ kind: 'failed', reason: failure(error).reason });
      return null;
    }
    this.#use(session);
    this.#set({ kind: 'opening', name: session.workbook.name, received: 0, total: null });
    const reading = new AbortController();
    const read = this.#read(reading.signal);
    read.catch(() => undefined);
    const offered = await offerHandover(session.handover(), this.#launch.apiBase);
    if (offered === 'taken') {
      reading.abort();
      session.giveAway();
      this.#session = null;
      this.#set({ kind: 'handedOver', name: session.workbook.name });
      window.close();
      return null;
    }
    try {
      const file = await read;
      this.#showOpen();
      return file;
    } catch (error) {
      this.#set({ kind: 'failed', reason: failure(error).reason });
      session.end();
      this.#session = null;
      return null;
    }
  }

  /** The workbook's current version as a file the shell opens. */
  async #read(signal?: AbortSignal): Promise<OpenedFile> {
    const session = this.#session;
    if (!session) throw new CloudFailure('session', 'There is no Workspace session.');
    const read = await session.read({
      signal,
      onProgress: (received, total) => {
        if (this.#status.kind === 'opening') {
          this.#set({ kind: 'opening', name: session.workbook.name, received, total });
        }
      },
    });
    const ref = new CloudRef(this, session.workbook.id, read.etag);
    if (read.blob === null) {
      // No version yet: the shell makes it blank, and its first save sends If-None-Match: *.
      return {
        name: fileName(read.name),
        source: { kind: 'blob', blob: new Blob([]) },
        recent: ref,
        blank: session.language === 'ar' ? 'rtl' : 'ltr',
      };
    }
    return { name: fileName(read.name), source: { kind: 'blob', blob: read.blob }, recent: ref };
  }

  touch() {
    this.#session?.touch();
  }

  owns(file: OpenedFile) {
    return file.recent instanceof CloudRef && file.recent.connection === this;
  }

  /** Where Save writes this connection's workbook, or why it can't. */
  saveTarget(file: OpenedFile): SaveTarget {
    const ref = file.recent as CloudRef;
    const session = this.#session;
    if (!session || session.state === 'expired' || this.#status.kind !== 'open') {
      throw new CloudFailure('session', 'Your Cash Gear Workspace session has ended.');
    }
    if (session.access !== 'edit') {
      throw new CloudFailure('denied', 'You can only view this workbook in Cash Gear Workspace.');
    }
    if (ref.workbookId !== session.workbook.id) {
      throw new CloudFailure('session', 'This tab’s session is for another workbook now.');
    }
    return {
      write: async (blob) => {
        try {
          const saved = await session.saveVersion(blob, ref.etag);
          this.#savedAt = Date.now();
          this.#showOpen();
          return {
            name: fileName(session.workbook.name),
            source: { kind: 'blob', blob },
            recent: new CloudRef(this, session.workbook.id, saved.etag),
          };
        } catch (error) {
          // A save refused for access shows the badge as view only from here on.
          if (this.#status.kind === 'open') this.#showOpen();
          throw failure(error);
        }
      },
    };
  }

  copyTarget(): SaveTarget {
    return {
      write: async (blob) => {
        const session = this.#session;
        if (!session || session.state === 'expired') {
          throw new CloudFailure('session', 'Your Cash Gear Workspace session has ended.', true);
        }
        try {
          const copy = await session.saveCopy(blob);
          this.#savedAt = Date.now();
          this.#showOpen();
          return {
            name: fileName(copy.workbook.name),
            source: { kind: 'blob', blob },
            recent: new CloudRef(this, copy.workbook.id, copy.etag),
          };
        } catch (error) {
          throw failure(error, true);
        }
      },
    };
  }

  async reload(): Promise<OpenedFile> {
    try {
      const file = await this.#read();
      this.#savedAt = null;
      this.#showOpen();
      return file;
    } catch (error) {
      throw failure(error);
    }
  }

  reconnect() {
    const session = this.#session;
    if (!session || this.#status.kind !== 'ended') return;
    const { name, id } = session.workbook;
    const scope = { apiBase: this.#launch.apiBase, workbookId: id, userId: session.user.id };
    this.#stopWaiting?.();
    this.#set({ kind: 'waiting', name });
    this.#stopWaiting = waitForHandover({
      scope,
      take: async (given) => {
        const adopted = WorkspaceSession.adopt(this.#http(), given);
        this.#stopWaiting = null;
        this.#use(adopted);
        this.#showOpen();
        return true;
      },
    });
  }

  stopWaiting() {
    this.#stopWaiting?.();
    this.#stopWaiting = null;
    const session = this.#session;
    if (this.#status.kind === 'waiting' && session) {
      this.#set({ kind: 'ended', name: session.workbook.name });
    }
  }

  /** Another workbook replaced Workspace's in this tab: its session has nothing left to do. */
  replaced() {
    if (this.#status.kind === 'closed') return;
    this.#stopWaiting?.();
    this.#stopWaiting = null;
    this.#stopWatching?.();
    this.#session?.end();
    this.#session = null;
    this.#set({ kind: 'closed' });
  }
}

/**
 * The third platform (Track S1): the web platform, opening and saving one workbook of Cash Gear
 * Workspace's in place of a file, for a tab its "Open in Cash Gear Sheet" started.
 */
export function cloudPlatform(
  launch: CloudLaunch,
  base: SheetPlatform = webPlatform,
): SheetPlatform {
  const cloud = new CloudConnection(launch);
  return {
    ...base,
    cloud,
    // The workbook comes from Workspace; the web's launch queue has nothing for this tab.
    launchFile: () => cloud.open(),
    async saveTarget(file) {
      if (cloud.owns(file)) return cloud.saveTarget(file);
      return base.saveTarget(file);
    },
    // A Workspace workbook needs no working copy: the engine worker copies its bytes itself.
    async rememberFile(file) {
      if (cloud.owns(file)) return;
      cloud.replaced();
      await base.rememberFile(file);
    },
  };
}
