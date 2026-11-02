import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { NativeInfo, OpenedFile, RecentFile, SheetPlatform } from '@cgs/ui';

/** A file the native side can serve through the `cgs-file` protocol. */
interface NativeFile {
  id: string;
  name: string;
  path: string;
  size: number;
}

const opened = (file: NativeFile): OpenedFile => ({
  name: file.name,
  source: { kind: 'url', url: convertFileSrc(file.id, 'cgs-file'), size: file.size },
  recent: file.path,
});

export const desktopPlatform: SheetPlatform = {
  kind: 'desktop',
  nativeInfo: () => invoke<NativeInfo>('native_info'),
  setWindowTheme: (theme) => getCurrentWindow().setTheme(theme),
  async pickFile() {
    const file = await invoke<NativeFile | null>('pick_file');
    return file && opened(file);
  },
  // A dropped file arrives as a browser File, whatever the platform.
  async fromDrop(transfer) {
    const file = transfer.files[0];
    return file ? { name: file.name, source: { kind: 'blob', blob: file } } : null;
  },
  recentFiles: () => invoke<RecentFile[]>('recent_files'),
  async openRecent(id) {
    const file = await invoke<NativeFile | null>('open_recent', { id });
    return file && opened(file);
  },
  async rememberFile(file) {
    if (typeof file.recent === 'string') await invoke('remember_file', { path: file.recent });
  },
};

/** For the desktop smoke test, which can't drive a native dialog. */
export async function openPathForTest(path: string): Promise<OpenedFile> {
  return opened(await invoke<NativeFile>('open_path', { path }));
}
