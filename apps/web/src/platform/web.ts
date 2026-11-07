import {
  MAX_RECENT_FILES,
  OPENABLE_EXTENSIONS,
  type OpenedFile,
  type RecentFile,
  type SaveTarget,
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
  showSaveFilePicker?: (options: {
    suggestedName?: string;
    types: { description: string; accept: Record<string, string[]> }[];
    excludeAcceptAllOption?: boolean;
  }) => Promise<FileSystemFileHandle>;
}

type HandleMode = { mode: 'read' | 'readwrite' };

interface PermissionHandle extends FileSystemFileHandle {
  queryPermission?: (options: HandleMode) => Promise<PermissionState>;
  requestPermission?: (options: HandleMode) => Promise<PermissionState>;
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
    input.accept = OPENABLE_EXTENSIONS.join(',');
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

const MIME_TYPES: Record<string, string> = {
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  csv: 'text/csv',
  xlsm: 'application/vnd.ms-excel.sheet.macroEnabled.12',
  xltx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.template',
  xltm: 'application/vnd.ms-excel.template.macroEnabled.12',
  pdf: 'application/pdf',
};

const DESCRIPTIONS: Record<string, string> = { csv: 'CSV', pdf: 'PDF' };

/** What Chrome's Local Font Access gives for each installed face. */
interface LocalFont {
  family: string;
  style: string;
  fullName: string;
  blob(): Promise<Blob>;
}

let localFonts: Promise<LocalFont[]> | null = null;

/** Installed fonts, once Chrome has been allowed to list them. */
function listLocalFonts(): Promise<LocalFont[]> {
  const query = (window as unknown as { queryLocalFonts?: () => Promise<LocalFont[]> })
    .queryLocalFonts;
  if (!query) return Promise.resolve([]);
  localFonts ??= query.call(window).catch(() => {
    localFonts = null;
    return [];
  });
  return localFonts;
}

/**
 * Writes through the browser's own swap file, which only replaces the real file on `close`. A
 * failure at any point before that leaves the original as it was.
 */
const handleTarget = (handle: FileSystemFileHandle): SaveTarget => ({
  async write(blob) {
    const writable = await handle.createWritable();
    try {
      await writable.write(blob);
      await writable.close();
    } catch (error) {
      await writable.abort().catch(() => undefined);
      throw error;
    }
    return opened(await handle.getFile(), handle);
  },
});

/** For a browser with no file handles: a download, which the workbook then reads from. */
const downloadTarget = (name: string): SaveTarget => ({
  async write(blob) {
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    link.style.display = 'none';
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
    return { name, source: { kind: 'blob', blob } };
  },
});

export const webPlatform: SheetPlatform = {
  kind: 'web',
  askFontAccess() {
    void listLocalFonts();
  },
  async systemFont(family, bold, italic) {
    const fonts = await listLocalFonts();
    const wanted = family.toLowerCase();
    const styled = (font: LocalFont) => {
      const style = font.style.toLowerCase();
      return /bold|black|heavy/.test(style) === bold && /italic|oblique/.test(style) === italic;
    };
    const font =
      fonts.find((f) => f.family.toLowerCase() === wanted && styled(f)) ??
      fonts.find((f) => f.family.toLowerCase() === wanted && /regular|normal/i.test(f.style));
    if (!font) return null;
    const data = new Uint8Array(await (await font.blob()).arrayBuffer());
    return { data, index: 0, name: font.fullName };
  },
  async print() {
    window.print();
  },
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
            description: 'Workbooks and CSV',
            accept: {
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet': [
                ...OPENABLE_EXTENSIONS,
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
  async saveTarget(file) {
    const handle = file.recent as PermissionHandle | undefined;
    if (!handle || file.isNew) return null;
    try {
      const options = { mode: 'readwrite' } as const;
      let state = await handle.queryPermission?.(options);
      if (state !== 'granted') state = await handle.requestPermission?.(options);
      // Without permission to write here, it can still go somewhere else.
      return state === 'granted' ? handleTarget(handle) : null;
    } catch {
      return null;
    }
  },
  async pickSaveTarget(suggestedName, extension) {
    const picker = (window as PickerWindow).showSaveFilePicker;
    if (!picker) return downloadTarget(suggestedName);
    try {
      const handle = await picker({
        suggestedName,
        types: [
          {
            description: DESCRIPTIONS[extension] ?? 'Excel workbook',
            accept: { [MIME_TYPES[extension] ?? MIME_TYPES.xlsx!]: ['.' + extension] },
          },
        ],
        excludeAcceptAllOption: true,
      });
      return handleTarget(handle);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') return null;
      throw error;
    }
  },
  // A browser file goes to OPFS, which the engine worker reaches itself.
  workingCopy: () => Promise.resolve(null),
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
