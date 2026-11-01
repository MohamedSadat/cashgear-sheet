import { describe, expect, it, vi } from 'vitest';
import type { SheetPlatform } from './platform';
import { ThemeController, type ThemeEnvironment } from './theme';

function setup({ dark = false, saved = null as string | null } = {}) {
  const changeListeners = new Set<() => void>();
  const values = new Map<string, string>(saved ? [['cgs-theme', saved]] : []);
  const systemDark = {
    matches: dark,
    addEventListener: (_: string, listener: () => void) => changeListeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => changeListeners.delete(listener),
  };
  const environment = {
    systemDark,
    storage: () => ({
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    }),
    root: { dataset: {} },
  } as unknown as ThemeEnvironment;
  const setWindowTheme = vi.fn(() => Promise.resolve());
  const platform = { kind: 'web', setWindowTheme } as unknown as SheetPlatform;
  const changeSystem = (next: boolean) => {
    systemDark.matches = next;
    for (const listener of changeListeners) listener();
  };
  return {
    theme: new ThemeController(platform, environment),
    environment,
    values,
    setWindowTheme,
    changeSystem,
    changeListeners,
  };
}

describe('ThemeController', () => {
  it('follows the system by default', () => {
    const { theme, environment, setWindowTheme, changeSystem } = setup({ dark: true });
    expect(theme.preference).toBe('system');
    expect(theme.theme).toBe('dark');
    expect(environment.root.dataset.cgTheme).toBe('dark');
    expect(setWindowTheme).toHaveBeenCalledWith(null);

    const listener = vi.fn();
    theme.subscribe(listener);
    changeSystem(false);
    expect(theme.theme).toBe('light');
    expect(environment.root.dataset.cgTheme).toBe('light');
    expect(listener).toHaveBeenCalledOnce();
  });

  it('remembers a manual choice, and it beats the system', () => {
    const { theme, environment, values, setWindowTheme, changeSystem } = setup();
    theme.setPreference('dark');
    expect(values.get('cgs-theme')).toBe('dark');
    expect(environment.root.dataset.cgTheme).toBe('dark');
    expect(setWindowTheme).toHaveBeenLastCalledWith('dark');
    changeSystem(false);
    expect(theme.theme).toBe('dark');
  });

  it('starts from the remembered choice', () => {
    const { theme, setWindowTheme } = setup({ saved: 'dark' });
    expect(theme.preference).toBe('dark');
    expect(theme.theme).toBe('dark');
    expect(setWindowTheme).toHaveBeenCalledWith('dark');
  });

  it('forgets the choice when going back to the system', () => {
    const { theme, values, setWindowTheme } = setup({ saved: 'dark' });
    theme.setPreference('system');
    expect(values.has('cgs-theme')).toBe(false);
    expect(theme.theme).toBe('light');
    expect(setWindowTheme).toHaveBeenLastCalledWith(null);
  });

  it('ignores an unknown saved value', () => {
    expect(setup({ saved: 'sepia' }).theme.preference).toBe('system');
  });

  it('works without storage', () => {
    const { theme, environment } = setup();
    environment.storage = () => {
      throw new Error('denied');
    };
    theme.setPreference('dark');
    expect(theme.theme).toBe('dark');
  });

  it('stops listening when disposed', () => {
    const { theme, changeListeners } = setup();
    expect(changeListeners.size).toBe(1);
    theme.dispose();
    expect(changeListeners.size).toBe(0);
  });
});
