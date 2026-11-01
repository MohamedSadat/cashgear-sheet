import type { SheetPlatform } from '@cgs/ui';

export const webPlatform: SheetPlatform = {
  kind: 'web',
  nativeInfo: () => Promise.resolve(null),
  // A browser's window isn't ours to theme.
  setWindowTheme: () => Promise.resolve(),
};
