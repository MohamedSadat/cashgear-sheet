import type { FileSource } from '@cgs/engine-client';
import type { ErpLaunch } from '@cgs/erp-connector';
import type { Theme } from './theme';

export interface NativeInfo {
  product: string;
  version: string;
  os: string;
  arch: string;
}

/** A newer version the desktop's release feed offers. */
export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** A workbook the user chose, ready to hand to the engine. */
export interface OpenedFile {
  name: string;
  source: FileSource;
  /** What the recent list needs to find it again, if it can. */
  recent?: RecentHandle;
  /** How the native side knows this file, on the desktop. */
  nativeId?: string;
  /** Never saved: a new workbook, which has nowhere to save to yet. */
  isNew?: boolean;
  /**
   * A workbook with no bytes yet, which the shell makes blank in this direction and opens as new,
   * ignoring `source`: Cash Gear Workspace's new workbook, whose first save makes its first
   * version.
   */
  blank?: 'ltr' | 'rtl';
}

/** Somewhere a save can go, with permission already given. */
export interface SaveTarget {
  /** Replaces the file in one step and returns it opened again, or fails with it unchanged. */
  write(blob: Blob): Promise<OpenedFile>;
}

/** A private copy of a file on the native side. */
export interface WorkingCopy {
  source: FileSource;
  release(): Promise<void>;
}

/** A way back to a file, which only the platform that made it understands. */
export type RecentHandle = unknown;

export interface RecentFile {
  /** Unique within the list. */
  id: string;
  name: string;
  /** Where it is, if the platform knows. */
  detail?: string;
}

/**
 * What differs between the browser and the desktop app. apps/web provides both
 * implementations; the UI only talks to this interface.
 */
export interface SheetPlatform {
  kind: 'web' | 'desktop';
  /** Asks `ask` before the app's window closes, on the desktop. Gives what stops asking. */
  onCloseRequested?(ask: () => Promise<boolean>): () => void;
  /** The native host, or null on the web. */
  nativeInfo(): Promise<NativeInfo | null>;
  /**
   * The workbook a double-click or Open with in Explorer started this window with, once: the
   * desktop app's command line, or the installed web app's launch queue. Null when it started
   * without one, or after the first ask. A web app started without a file may never answer.
   */
  launchFile?(): Promise<OpenedFile | null>;
  /**
   * What Cash Gear ERP's "Edit in Cash Gear Sheet" opened this tab with, once: the launch ticket,
   * read from the URL's fragment and removed from it before the app rendered. Null otherwise, and
   * always on the desktop until its deep link (plan B.4).
   */
  erpLaunch?(): ErpLaunch | null;
  /**
   * The ERP API's origin for this page, configured as the launch's is: what a recovered draft
   * reconnects to, since it has no launch of its own (Phase 18).
   */
  erpApiBase?(): string;
  /** Desktop: newer versions from the release feed. */
  updates?: {
    /** The newer version the feed offers, or null. */
    check(): Promise<UpdateInfo | null>;
    /**
     * Downloads the update, checks its signature and installs it: the installer closes the app
     * and starts the new version.
     */
    install(onProgress: (received: number, total: number | null) => void): Promise<void>;
  };
  /** Sets the window's own theme, such as its title bar; null follows the system. */
  setWindowTheme(theme: Theme | null): Promise<void>;
  /** Asks the user for a workbook. Null if they changed their mind. */
  pickFile(): Promise<OpenedFile | null>;
  /** A workbook dropped on the window. */
  fromDrop(event: DataTransfer): Promise<OpenedFile | null>;
  recentFiles(): Promise<RecentFile[]>;
  /** Null if the file has gone or permission was refused, which takes it off the list. */
  openRecent(id: string): Promise<OpenedFile | null>;
  /** Puts a file at the top of the recent list. */
  rememberFile(file: OpenedFile): Promise<void>;
  /**
   * Where Save puts this file, asking for permission if it must, so this has to come before
   * anything slow while the user's click still counts. Null means it needs a name: Save As.
   */
  saveTarget(file: OpenedFile): Promise<SaveTarget | null>;
  /** Asks for a new place to save, in this file type. Null if they changed their mind. */
  pickSaveTarget(suggestedName: string, extension: string): Promise<SaveTarget | null>;
  /**
   * Copies a file on the native side, for the first edit. Null means the platform has nothing to
   * do, and the engine worker copies a browser file itself. Rejects if the file has changed.
   */
  workingCopy(file: OpenedFile): Promise<WorkingCopy | null>;
  /** An installed font's file, for PDFs. Null when it isn't installed or can't be read. */
  systemFont?(family: string, bold: boolean, italic: boolean): Promise<InstalledFont | null>;
  /** Asks to read installed fonts, while the user's click still allows asking. */
  askFontAccess?(): void;
  /** Opens a print dialog on what the page shows for printing: the system's on the desktop. */
  print(): Promise<void>;
  /**
   * A workbook from Cash Gear Workspace, when this tab was opened by its "Open in Cash Gear
   * Sheet" (the web's `/cloud` route, Track S1). `launchFile` then opens it, and `saveTarget`
   * saves new versions of it there.
   */
  cloud?: CloudWorkbook;
}

/** What a Workspace workbook's connection is doing, for its badge and dialogs. */
export type CloudStatus =
  /** Redeeming the launch and reading the workbook. */
  | { kind: 'opening'; name: string | null; received: number; total: number | null }
  /** Open: saves go to Workspace when `access` is `edit`. `savedAt` is this tab's last save. */
  | { kind: 'open'; name: string; access: 'edit' | 'view'; savedAt: number | null }
  /** The launch opened nothing, for `reason`. */
  | { kind: 'failed'; reason: CloudReason }
  /** The session ended with the workbook open: its edits stay, and Save offers Reconnect. */
  | { kind: 'ended'; name: string }
  /** Waiting for the next launch of this workbook to hand its session over. */
  | { kind: 'waiting'; name: string }
  /** This tab gave its session to a tab that was waiting for it. */
  | { kind: 'handedOver'; name: string }
  /** Another workbook replaced Workspace's in this tab, and its session ended. */
  | { kind: 'closed' };

/** Why a Workspace request failed, as the shell words it. */
export type CloudReason =
  | 'ticket'
  | 'session'
  | 'origin'
  | 'denied'
  | 'gone'
  | 'conflict'
  | 'tooLarge'
  | 'invalid'
  | 'busy'
  | 'server'
  | 'readUrl'
  | 'network'
  | 'contract'
  | 'cancelled'
  /** A tab waiting for this workbook refused the session handed to it. */
  | 'handoverRefused';

/**
 * A save to Workspace that didn't go: thrown by `saveTarget` (a view-only or ended session) and by
 * a cloud target's `write`. `current` names the version that made a save stale.
 */
export interface CloudSaveError extends Error {
  readonly name: 'CloudSaveError';
  readonly reason: CloudReason;
  readonly current?: { savedBy: string; savedAt: string };
  /** Whether it came from Save a copy's target. */
  readonly copy: boolean;
}

export const isCloudSaveError = (error: unknown): error is CloudSaveError =>
  error instanceof Error && error.name === 'CloudSaveError';

/** A Cash Gear Workspace workbook's connection, which the web's cloud platform provides. */
export interface CloudWorkbook {
  /** The Workspace user's language for the cloud texts (the browser's until Redeem answers). */
  language(): 'en' | 'ar';
  status(): CloudStatus;
  onStatus(listener: (status: CloudStatus) => void): () => void;
  /** The user did something in the tab: renews the session at most every 5 minutes. */
  touch(): void;
  /** Whether a file is Workspace's workbook. */
  owns(file: OpenedFile): boolean;
  /** The workbook's newest version, read again, for Reload. Rejects with a `CloudSaveError`. */
  reload(): Promise<OpenedFile>;
  /** Where Save a copy writes: a new workbook beside this one in Workspace. */
  copyTarget(): SaveTarget;
  /** Waits for the next launch of this workbook to hand its session over. */
  reconnect(): void;
  stopWaiting(): void;
}

/** An installed font's bytes, and which face in a collection. */
export interface InstalledFont {
  data: Uint8Array;
  index: number;
  name: string;
}

/** The file types the app opens. Macros stay in the file and never run. */
export const WORKBOOK_EXTENSIONS = ['.xlsx', '.xlsm', '.xltx', '.xltm'];

/** What Open offers: a CSV becomes a new workbook's cells. */
export const OPENABLE_EXTENSIONS = [...WORKBOOK_EXTENSIONS, '.csv'];

export const MAX_RECENT_FILES = 10;
