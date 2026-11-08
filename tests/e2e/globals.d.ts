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
  boxForCell(row: number, col: number): { x: number; y: number; width: number; height: number };
  selectArea(range: { top: number; left: number; bottom: number; right: number }): void;
  /** Sheet Right-to-Left: column A on the right. */
  readonly rightToLeft: boolean;
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
    drawings(): Promise<
      {
        index: number;
        name: string;
        kind: string;
        x: number;
        y: number;
        width: number;
        height: number;
      }[]
    >;
    chart(
      index: number,
    ): Promise<{ groups: { kind: string }[]; title: { text: string } | null } | null>;
    settled(): Promise<unknown>;
    hasChanges(): Promise<boolean>;
    writeTo(target: unknown): Promise<void>;
    lastSave(): unknown;
    problem(): string | null;
    cellFormat(row: number, col: number): Promise<import('@cgs/engine-client').CellFormat | null>;
    saveBytes(): Promise<Uint8Array>;
    pdf(choice?: {
      what?: 'active' | 'workbook' | 'selection';
      ignorePrintArea?: boolean;
    }): Promise<{ bytes: Uint8Array; pages: number }>;
  };
}
