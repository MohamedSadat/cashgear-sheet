import type { FileSource } from '@cgs/engine-client';
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
   * Desktop: the workbook a double-click in Explorer started this window with, once. Null when
   * it started without one, or after the first ask.
   */
  launchFile?(): Promise<OpenedFile | null>;
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
