import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { isTauri } from '@tauri-apps/api/core';
import { AppShell, ThemeController } from '@cgs/ui';
import { desktopPlatform, openPathForTest } from './platform/desktop';
import { webPlatform } from './platform/web';

const desktop = isTauri();
const platform = desktop ? desktopPlatform : webPlatform;
// Set before the first render, so the page never flashes the wrong theme.
const theme = new ThemeController(platform);
if (desktop) {
  (window as Window & { cgsOpenPath?: typeof openPathForTest }).cgsOpenPath = openPathForTest;
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppShell platform={platform} theme={theme} />
  </StrictMode>,
);
