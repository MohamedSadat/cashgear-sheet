// What the specs reach for through the page. See `hooks` in packages/ui/src/AppShell.tsx.
interface GridForTests {
  state(): {
    sheet: number;
    scrollX: number;
    scrollY: number;
    zoom: number;
    row: number;
    col: number;
    topRow: number;
    contentHeight: number;
    drawings: { name: string }[];
  };
  scrollTo(x: number, y: number): void;
  pointForCell(row: number, col: number): { x: number; y: number };
}

interface Window {
  cgsTest: {
    open(file: {
      name: string;
      source: { kind: 'blob'; blob: Blob } | { kind: 'url'; url: string; size: number };
    }): Promise<void>;
    grid(): GridForTests | null;
    cellText(row: number, col: number): Promise<string>;
    loadPhase(): string;
    memoryBytes(): Promise<number>;
    edit(
      sheet: number,
      row: number,
      col: number,
      content: { number?: number; text?: string; formula?: string },
    ): Promise<void>;
    command(command: unknown): Promise<unknown>;
    hasChanges(): Promise<boolean>;
    writeTo(target: unknown): Promise<void>;
    lastSave(): unknown;
    problem(): string | null;
  };
}
