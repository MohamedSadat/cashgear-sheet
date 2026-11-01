import type { Theme } from './theme';

export interface NativeInfo {
  product: string;
  version: string;
  os: string;
  arch: string;
}

/**
 * What differs between the browser and the desktop app. apps/web provides both
 * implementations; the UI only talks to this interface. Later phases add opening
 * and saving files here.
 */
export interface SheetPlatform {
  kind: 'web' | 'desktop';
  /** The native host, or null on the web. */
  nativeInfo(): Promise<NativeInfo | null>;
  /** Sets the window's own theme, such as its title bar; null follows the system. */
  setWindowTheme(theme: Theme | null): Promise<void>;
}
