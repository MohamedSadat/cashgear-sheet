import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { NativeInfo, OpenedFile, RecentFile, SaveTarget, SheetPlatform } from '@cgs/ui';

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
  nativeId: file.id,
});

/** Bytes go through the protocol, which carries about four times what IPC does. */
const SAVE_CHUNK = 8 * 1024 * 1024;

const pathTarget = (path: string): SaveTarget => ({
  async write(blob) {
    const token = await invoke<string>('begin_save', { path });
    try {
      const url = convertFileSrc(token, 'cgs-file');
      for (let at = 0; at < blob.size; at += SAVE_CHUNK) {
        const body = await blob.slice(at, at + SAVE_CHUNK).arrayBuffer();
        const response = await fetch(url, { method: 'POST', body });
        if (!response.ok) throw new Error(await response.text());
      }
    } catch (error) {
      await invoke('abort_save', { token }).catch(() => undefined);
      throw error;
    }
    return opened(await invoke<NativeFile>('finish_save', { token, size: blob.size }));
  },
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
  async saveTarget(file) {
    if (typeof file.recent !== 'string' || file.isNew) return null;
    await invoke('allow_saving', { path: file.recent });
    return pathTarget(file.recent);
  },
  async pickSaveTarget(suggested, extension) {
    const path = await invoke<string | null>('pick_save_path', { suggested, extension });
    return path === null ? null : pathTarget(path);
  },
  async workingCopy(file) {
    if (file.source.kind !== 'url' || !file.nativeId) return null;
    const copy = await invoke<NativeFile>('make_working_copy', { id: file.nativeId });
    return {
      source: opened(copy).source,
      release: () => invoke('release_working_copy', { id: copy.id }),
    };
  },
  async rememberFile(file) {
    if (typeof file.recent === 'string') await invoke('remember_file', { path: file.recent });
  },
};

/** For the desktop smoke test, which can't drive a native dialog. */
export async function openPathForTest(path: string): Promise<OpenedFile> {
  return opened(await invoke<NativeFile>('open_path', { path }));
}

/** For the desktop smoke test, in place of the save dialog. */
export async function saveTargetForTest(path: string): Promise<SaveTarget> {
  await invoke('allow_save_path', { path });
  return pathTarget(path);
}
