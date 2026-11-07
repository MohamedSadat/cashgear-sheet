import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { isTauri } from '@tauri-apps/api/core';
import { AppShell, ThemeController } from '@cgs/ui';
import { desktopPlatform, openPathForTest, saveTargetForTest } from './platform/desktop';
import { webPlatform } from './platform/web';

const desktop = isTauri();
const platform = desktop ? desktopPlatform : webPlatform;
// Set before the first render, so the page never flashes the wrong theme.
const theme = new ThemeController(platform);
if (desktop) {
  const hooks = window as Window & {
    cgsOpenPath?: typeof openPathForTest;
    cgsSaveTarget?: typeof saveTargetForTest;
  };
  hooks.cgsOpenPath = openPathForTest;
  hooks.cgsSaveTarget = saveTargetForTest;
}

// The web app works offline once it has loaded (apps/web/sw.js). The desktop app's files are
// its own, and a development server changes them all the time.
if (!desktop && import.meta.env.PROD && 'serviceWorker' in navigator) {
  navigator.serviceWorker
    .register('/sw.js')
    .catch((error) => console.warn('The app could not be made to work offline:', error));
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppShell platform={platform} theme={theme} version={__CGS_VERSION__} />
  </StrictMode>,
);
