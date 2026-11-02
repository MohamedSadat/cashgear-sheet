import {
  MAX_RECENT_FILES,
  WORKBOOK_EXTENSIONS,
  type OpenedFile,
  type RecentFile,
  type SheetPlatform,
} from '@cgs/ui';

const DATABASE = 'cgs-recent';
const STORE = 'files';

interface StoredRecent {
  id: string;
  name: string;
  at: number;
  handle: FileSystemFileHandle;
}

/** The parts of the File System Access API this uses, which only some browsers have. */
interface PickerWindow {
  showOpenFilePicker?: (options: {
    types: { description: string; accept: Record<string, string[]> }[];
    excludeAcceptAllOption?: boolean;
    multiple?: boolean;
  }) => Promise<FileSystemFileHandle[]>;
}

interface PermissionHandle extends FileSystemFileHandle {
  queryPermission?: (options: { mode: 'read' }) => Promise<PermissionState>;
  requestPermission?: (options: { mode: 'read' }) => Promise<PermissionState>;
}

interface HandleItem extends DataTransferItem {
  getAsFileSystemHandle?: () => Promise<FileSystemHandle | null>;
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore(STORE, { keyPath: 'id' });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Runs one request in its own transaction. Storage can be missing or refuse access. */
async function withStore<T>(
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const database = await openDatabase();
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = database.transaction(STORE, mode);
      const request = action(transaction.objectStore(STORE));
      transaction.oncomplete = () => resolve(request.result);
      transaction.onerror = () => reject(transaction.error);
      transaction.onabort = () => reject(transaction.error);
    });
  } finally {
    database.close();
  }
}

async function allRecent(): Promise<StoredRecent[]> {
  try {
    const stored = await withStore<StoredRecent[]>('readonly', (store) => store.getAll());
    return stored.sort((a, b) => b.at - a.at);
  } catch {
    return [];
  }
}

function pickWithInput(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = WORKBOOK_EXTENSIONS.join(',');
    input.dataset.testid = 'file-input';
    input.style.display = 'none';
    input.addEventListener('change', () => {
      input.remove();
      resolve(input.files?.[0] ?? null);
    });
    input.addEventListener('cancel', () => {
      input.remove();
      resolve(null);
    });
    document.body.append(input);
    input.click();
  });
}

const opened = (file: File, handle?: FileSystemFileHandle): OpenedFile => ({
  name: file.name,
  source: { kind: 'blob', blob: file },
  recent: handle,
});

export const webPlatform: SheetPlatform = {
  kind: 'web',
  nativeInfo: () => Promise.resolve(null),
  // A browser's window isn't ours to theme.
  setWindowTheme: () => Promise.resolve(),
  async pickFile() {
    const picker = (window as PickerWindow).showOpenFilePicker;
    if (!picker) {
      const file = await pickWithInput();
      return file && opened(file);
    }
    try {
      const [handle] = await picker({
        types: [
          {
            description: 'Excel workbooks',
            accept: {
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet': [
                ...WORKBOOK_EXTENSIONS,
              ],
            },
          },
        ],
        multiple: false,
      });
      if (!handle) return null;
      return opened(await handle.getFile(), handle);
    } catch (error) {
      // Cancelling the picker is an AbortError.
      if (error instanceof DOMException && error.name === 'AbortError') return null;
      throw error;
    }
  },
  async fromDrop(transfer) {
    const item = [...transfer.items].find((entry) => entry.kind === 'file') as
      HandleItem | undefined;
    const file = item?.getAsFile();
    if (!item || !file) return null;
    const handle = await item.getAsFileSystemHandle?.();
    return opened(file, handle?.kind === 'file' ? (handle as FileSystemFileHandle) : undefined);
  },
  async recentFiles(): Promise<RecentFile[]> {
    return (await allRecent()).map(({ id, name }) => ({ id, name }));
  },
  async openRecent(id) {
    const stored = (await allRecent()).find((entry) => entry.id === id);
    if (!stored) return null;
    const handle = stored.handle as PermissionHandle;
    try {
      const options = { mode: 'read' } as const;
      let state = await handle.queryPermission?.(options);
      if (state !== 'granted') state = await handle.requestPermission?.(options);
      if (state !== 'granted') return null;
      return opened(await handle.getFile(), handle);
    } catch {
      await withStore('readwrite', (store) => store.delete(id)).catch(() => undefined);
      return null;
    }
  },
  async rememberFile(file) {
    const handle = file.recent as FileSystemFileHandle | undefined;
    if (!handle) return;
    try {
      const existing = await allRecent();
      const same = await Promise.all(
        existing.map(async (entry) => ((await entry.handle.isSameEntry(handle)) ? entry : null)),
      );
      const id = same.find(Boolean)?.id ?? crypto.randomUUID();
      await withStore('readwrite', (store) =>
        store.put({ id, name: file.name, at: Date.now(), handle } satisfies StoredRecent),
      );
      const kept = await allRecent();
      for (const entry of kept.slice(MAX_RECENT_FILES)) {
        await withStore('readwrite', (store) => store.delete(entry.id));
      }
    } catch (error) {
      // The list is a convenience, and a private window can refuse to keep it.
      console.warn('Could not remember this file:', error);
    }
  },
};
