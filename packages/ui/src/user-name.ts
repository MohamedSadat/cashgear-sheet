// Excel's "User name" (Options › General): the author new notes carry. Kept on this device, as the
// default direction is.

const storageKey = 'cgs-user-name';

/** Who notes say wrote them until a user name is set, as before there was a setting. */
export const DEFAULT_AUTHOR = 'Author';

/** Storage can be missing or refuse access, in a private window for example. */
export function readUserName(storage: () => Storage = () => localStorage): string {
  try {
    return storage().getItem(storageKey) ?? '';
  } catch {
    return '';
  }
}

export function writeUserName(name: string, storage: () => Storage = () => localStorage) {
  try {
    const trimmed = name.trim();
    if (trimmed) storage().setItem(storageKey, trimmed);
    else storage().removeItem(storageKey);
  } catch {
    // The name then lasts until the page closes.
  }
}

/** The author a new note gets. */
export const noteAuthor = (storage?: () => Storage) => readUserName(storage) || DEFAULT_AUTHOR;
