import '@cashgear/ui/styles.css';
import './styles.css';
import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { CgButtonGroup, CgStatusBadge } from '@cashgear/ui';
import { createEngineClient, type EngineClient } from '@cgs/engine-client';
import { SheetCanvas } from './grid/SheetCanvas';
import type { NativeInfo, SheetPlatform } from './platform';
import { readPalette, type ThemeController, type ThemePreference } from './theme';

const themeChoices = [
  { name: 'system', text: 'System' },
  { name: 'light', text: 'Light' },
  { name: 'dark', text: 'Dark' },
];

interface AppShellProps {
  platform: SheetPlatform;
  theme: ThemeController;
}

export function AppShell({ platform, theme }: AppShellProps) {
  const [client, setClient] = useState<EngineClient | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [engineReady, setEngineReady] = useState(false);
  const [sharedMemory, setSharedMemory] = useState<boolean | null>(null);
  const [native, setNative] = useState<NativeInfo | null>(null);
  const preference = useSyncExternalStore(theme.subscribe, () => theme.preference);
  const current = useSyncExternalStore(theme.subscribe, () => theme.theme);
  // The tokens change with `data-cg-theme`, which the controller has already set.
  const palette = useMemo(() => {
    void current;
    return readPalette();
  }, [current]);
  const report = useCallback((error: unknown) => {
    setProblem(error instanceof Error ? error.message : String(error));
  }, []);

  useEffect(() => {
    const started = createEngineClient(report);
    let live = true;
    setClient(started);
    started
      .init()
      .then(() => {
        if (live) setEngineReady(true);
        return started.checkSharedMemory();
      })
      .then((shared) => {
        if (live) setSharedMemory(shared);
      })
      .catch((error) => {
        if (live) report(error);
      });
    return () => {
      live = false;
      setClient(null);
      setEngineReady(false);
      started.dispose();
    };
  }, [report]);

  useEffect(() => {
    let live = true;
    platform
      .nativeInfo()
      .then((info) => {
        if (live) setNative(info);
      })
      .catch(report);
    return () => {
      live = false;
    };
  }, [platform, report]);

  const host = native ? `Desktop ${native.version} · ${native.os} ${native.arch}` : 'Web';
  return (
    <div className="cgs-shell">
      <header className="cgs-header">
        <h1 className="cgs-title">CashGear Sheet</h1>
        <CgButtonGroup
          ariaLabel="Theme"
          data-testid="theme-switch"
          size="small"
          items={themeChoices}
          selectionMode="single"
          selectedName={preference}
          onSelectedNameChange={(name) => theme.setPreference(name as ThemePreference)}
        />
      </header>
      <main className="cgs-grid">
        {client && <SheetCanvas client={client} palette={palette} onError={report} />}
      </main>
      <footer className="cgs-status" data-testid="status-strip">
        <CgStatusBadge
          data-testid="engine-status"
          type={problem ? 'error' : engineReady ? 'success' : 'info'}
          text={problem ?? (engineReady ? 'Engine ready' : 'Starting the engine…')}
        />
        <CgStatusBadge
          data-testid="shared-memory"
          type={sharedMemory === false ? 'warning' : 'neutral'}
          text={
            sharedMemory === null
              ? 'Checking shared memory…'
              : sharedMemory
                ? 'Shared memory OK'
                : 'No shared memory'
          }
        />
        <span data-testid="host-info">{host}</span>
      </footer>
    </div>
  );
}
