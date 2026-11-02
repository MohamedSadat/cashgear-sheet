import type { FileSource } from '@cgs/engine-client';
import type { Theme } from './theme';

export interface NativeInfo {
  product: string;
  version: string;
  os: string;
  arch: string;
}

/** A workbook the user chose, ready to hand to the engine. */
export interface OpenedFile {
  name: string;
  source: FileSource;
  /** What the recent list needs to find it again, if it can. */
  recent?: RecentHandle;
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
  /** The native host, or null on the web. */
  nativeInfo(): Promise<NativeInfo | null>;
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
}

/** The file types the app opens. Macros stay in the file and never run. */
export const WORKBOOK_EXTENSIONS = ['.xlsx', '.xlsm', '.xltx', '.xltm'];

export const MAX_RECENT_FILES = 10;
