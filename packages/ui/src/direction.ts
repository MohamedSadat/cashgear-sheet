// Excel's "Default direction" (Options › Advanced › Display): whether new sheets and workbooks start
// right to left. Kept on this device, as the theme is.

const storageKey = 'cgs-default-direction';

export type Direction = 'ltr' | 'rtl';

/** Storage can be missing or refuse access, in a private window for example. */
export function readDefaultDirection(storage: () => Storage = () => localStorage): Direction {
  try {
    return storage().getItem(storageKey) === 'rtl' ? 'rtl' : 'ltr';
  } catch {
    return 'ltr';
  }
}

export function writeDefaultDirection(
  direction: Direction,
  storage: () => Storage = () => localStorage,
) {
  try {
    if (direction === 'ltr') storage().removeItem(storageKey);
    else storage().setItem(storageKey, direction);
  } catch {
    // The choice then lasts until the page closes.
  }
}
