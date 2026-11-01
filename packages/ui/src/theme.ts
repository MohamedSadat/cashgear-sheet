import type { TilePalette } from '@cgs/engine-client';
import type { SheetPlatform } from './platform';

export type Theme = 'light' | 'dark';
export type ThemePreference = 'system' | Theme;

const storageKey = 'cgs-theme';

/** Storage can be missing or refuse access, in a private window for example. */
function readPreference(storage: () => Storage): ThemePreference {
  try {
    const saved = storage().getItem(storageKey);
    return saved === 'light' || saved === 'dark' ? saved : 'system';
  } catch {
    return 'system';
  }
}

function writePreference(storage: () => Storage, preference: ThemePreference) {
  try {
    if (preference === 'system') storage().removeItem(storageKey);
    else storage().setItem(storageKey, preference);
  } catch {
    // The choice then lasts until the page closes.
  }
}

/** What the page needs from its surroundings, so tests can supply their own. */
export interface ThemeEnvironment {
  systemDark: Pick<MediaQueryList, 'matches' | 'addEventListener' | 'removeEventListener'>;
  storage: () => Storage;
  root: { dataset: DOMStringMap };
}

function browserEnvironment(): ThemeEnvironment {
  return {
    systemDark: matchMedia('(prefers-color-scheme: dark)'),
    storage: () => localStorage,
    root: document.documentElement,
  };
}

/**
 * Follows the system unless the user picks a theme, and remembers that pick. It
 * sets `data-cg-theme` on the root element, where the CG library's overlays copy it
 * from, and sets the window's own theme on the desktop.
 */
export class ThemeController {
  private readonly listeners = new Set<() => void>();
  private readonly environment: ThemeEnvironment;
  private currentPreference: ThemePreference;

  constructor(
    private readonly platform: SheetPlatform,
    environment: ThemeEnvironment = browserEnvironment(),
  ) {
    this.environment = environment;
    this.currentPreference = readPreference(environment.storage);
    environment.systemDark.addEventListener('change', this.apply);
    this.apply();
    this.syncWindow();
  }

  get preference() {
    return this.currentPreference;
  }

  get theme(): Theme {
    if (this.currentPreference !== 'system') return this.currentPreference;
    return this.environment.systemDark.matches ? 'dark' : 'light';
  }

  setPreference(preference: ThemePreference) {
    if (preference === this.currentPreference) return;
    this.currentPreference = preference;
    writePreference(this.environment.storage, preference);
    this.apply();
    this.syncWindow();
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  dispose() {
    this.environment.systemDark.removeEventListener('change', this.apply);
    this.listeners.clear();
  }

  private readonly apply = () => {
    this.environment.root.dataset.cgTheme = this.theme;
    for (const listener of this.listeners) listener();
  };

  private syncWindow() {
    const { currentPreference: preference } = this;
    this.platform
      .setWindowTheme(preference === 'system' ? null : preference)
      .catch((error) => console.warn('Could not set the window theme:', error));
  }
}

/** The tile worker can't read CSS, so this turns the current theme's tokens into colours. */
export function readPalette(root: Element = document.documentElement): TilePalette {
  const style = getComputedStyle(root);
  const token = (name: string) => style.getPropertyValue(name).trim();
  return {
    background: token('--cg-surface'),
    gridline: token('--cg-border'),
    text: token('--cg-text'),
    fontFamily: token('--cg-font-family'),
  };
}
