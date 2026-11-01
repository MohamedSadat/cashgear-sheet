import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { isTauri } from '@tauri-apps/api/core';
import { AppShell, ThemeController } from '@cgs/ui';
import { desktopPlatform } from './platform/desktop';
import { webPlatform } from './platform/web';

const platform = isTauri() ? desktopPlatform : webPlatform;
// Set before the first render, so the page never flashes the wrong theme.
const theme = new ThemeController(platform);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppShell platform={platform} theme={theme} />
  </StrictMode>,
);
