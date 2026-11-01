import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { NativeInfo, SheetPlatform } from '@cgs/ui';

export const desktopPlatform: SheetPlatform = {
  kind: 'desktop',
  nativeInfo: () => invoke<NativeInfo>('native_info'),
  setWindowTheme: (theme) => getCurrentWindow().setTheme(theme),
};
