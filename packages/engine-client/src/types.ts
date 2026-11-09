/**
 * The colours and font a tile is drawn with. Workers can't read CSS, so the main
 * thread builds this from the `--cg-*` tokens.
 */
export interface TilePalette {
  dark: boolean;
  /** `#rrggbb`. */
  background: string;
  gridline: string;
  /** `#rrggbb`. */
  text: string;
  /** A CSS font family list, for when a cell's own font is missing. */
  fontFamily: string;
  /** Draws every cell in this font instead, so that golden images don't depend on the machine. */
  fontOverride?: string;
  /** The sheet's own setting. */
  showGrid?: boolean;
  /** Sheet Right-to-Left: the tile's columns run from the right, as Excel mirrors them. */
  rightToLeft?: boolean;
}

/** One cell to measure for a row or column to fit. */
export interface FitCell {
  text: string;
  font: string;
  /** In points. */
  size: number;
  bold: boolean;
  italic: boolean;
  /** The column's width in pixels at 100%, for text that wraps. */
  width: number;
  rotation: number;
  indent: number;
  wrap: boolean;
}

/** What it takes to fit one row: see `Session::fit_rows_json`. */
export interface RowFit {
  sheet: number;
  row: number;
  /** What its fonts need anyway, in pixels at 100%. */
  base: number;
  cells: FitCell[];
}

/** A cell to measure for a column to fit: see `Session::fit_cols_json`. */
export type ColFit = FitCell & { col: number };

export interface TileRequest {
  sheet: number;
  /** The tile's top-left cell, counted from 0. */
  firstRow: number;
  firstCol: number;
  /** Device pixels per CSS pixel at 100%, zoom included. */
  scale: number;
  palette: TilePalette;
}

export interface TileImage {
  bitmap: ImageBitmap;
  /** FNV-1a of the tile's bytes from the engine, for tests. */
  hash: number;
  /** False if the sheet isn't read yet, so the picture is only its outline. */
  ready: boolean;
}

export interface EngineInfo {
  version: string;
}

/** A file to open, which the engine worker reads a slice at a time. */
export type FileSource =
  | { kind: 'blob'; blob: Blob }
  /** The desktop's read protocol, which serves byte ranges. */
  | { kind: 'url'; url: string; size: number };

/** A change to the workbook. See `Command` in `crates/cgs-engine/src/command.rs`. */
export interface CellArea {
  /** 1-based. */
  top: number;
  left: number;
  bottom: number;
  right: number;
}

/** Names a pivot table. */
export interface PivotRef {
  sheet: number;
  name: string;
}

export type PivotSummary =
  | 'sum'
  | 'count'
  | 'average'
  | 'max'
  | 'min'
  | 'product'
  | 'countNums'
  | 'stdDev'
  | 'stdDevp'
  | 'var'
  | 'varp';

export type PivotShowAs =
  | 'normal'
  | 'difference'
  | 'percent'
  | 'percentDiff'
  | 'runTotal'
  | 'percentOfRow'
  | 'percentOfCol'
  | 'percentOfTotal'
  | 'index'
  | 'percentOfParentRow'
  | 'percentOfParentCol'
  | 'percentOfParent'
  | 'percentOfRunningTotal'
  | 'rankAscending'
  | 'rankDescending';

/** A value field, as the field list sends it and `pivotInfo` reports it. */
export interface PivotValueSpec {
  field: number;
  summary?: PivotSummary;
  showAs?: PivotShowAs;
  baseField?: number;
  /** An item index, or 'previous' or 'next'. */
  baseItem?: number | 'previous' | 'next';
  name?: string;
  numberFormat?: string;
}

/** Where each field goes. -2 is the Σ Values field. */
export interface PivotLayoutSpec {
  rows: number[];
  cols: number[];
  pages: number[];
  values: PivotValueSpec[];
}

export interface PivotOptionSpec {
  form?: 'compact' | 'outline' | 'tabular';
  subtotals?: 'none' | 'top' | 'bottom';
  grandTotals?: 'off' | 'both' | 'rows' | 'cols';
  blankRows?: boolean;
  repeatLabels?: boolean;
  style?: string;
  rowHeaders?: boolean;
  colHeaders?: boolean;
  rowStripes?: boolean;
  colStripes?: boolean;
  name?: string;
  emptyText?: string;
  errorText?: string | null;
  headers?: boolean;
}

export interface PivotFieldChange {
  hidden?: number[];
  page?: number | null;
  multiple?: boolean;
  sort?: 'manual' | 'ascending' | 'descending';
  sortBy?: number | null;
  expand?: [number, boolean];
  expandAll?: boolean;
  subtotals?: string[];
  caption?: string;
  showAll?: boolean;
  blankRow?: boolean;
}

export interface PivotItemInfo {
  index: number;
  label: string;
  hidden: boolean;
  expanded: boolean;
}

export interface PivotFieldInfo {
  index: number;
  name: string;
  caption: string;
  axis: 'row' | 'col' | 'page' | 'values' | null;
  numeric: boolean;
  items: PivotItemInfo[];
  sort: 'manual' | 'ascending' | 'descending';
  sortBy: number | null;
  subtotals: string[];
  showAll: boolean;
  blankRow: boolean;
  multiple: boolean;
  grouped: boolean;
}

/** What a cell of a pivot table is. */
export type PivotCellInfo =
  | {
      role: 'item';
      axis: 'row' | 'col';
      field: number;
      item: number;
      subtotal: boolean;
      hasChildren: boolean;
    }
  | { role: 'value'; value: number }
  | { role: 'data'; value: number }
  | { role: 'page'; field: number }
  | { role: 'grand' | 'header' | 'other' };

/** The pivot table at a cell, for the field list, the ribbon and the context menu. */
export interface PivotInfo {
  name: string;
  sheet: number;
  area: CellArea;
  /** Why it can't be changed here, if it can't. */
  readOnly: string | null;
  /** Why it can't be refreshed here, if it can't. */
  noRefresh: string | null;
  fields: PivotFieldInfo[];
  rows: number[];
  cols: number[];
  pages: { field: number; item: number | null }[];
  values: (Required<
    Pick<PivotValueSpec, 'field' | 'name' | 'summary' | 'showAs' | 'baseField' | 'baseItem'>
  > & {
    numberFormat: string | null;
  })[];
  options: {
    form: 'compact' | 'outline' | 'tabular';
    subtotals: 'none' | 'top' | 'bottom';
    rowGrand: boolean;
    colGrand: boolean;
    blankRows: boolean;
    repeatLabels: boolean;
    emptyText: string | null;
    errorText: string | null;
    headers: boolean;
    style: {
      name: string | null;
      rowHeaders: boolean;
      colHeaders: boolean;
      rowStripes: boolean;
      colStripes: boolean;
    };
  };
  cell: PivotCellInfo;
  /** Filter buttons on its captions, 1-based, and the fields each one filters. */
  buttons: { row: number; col: number; fields: number[] }[];
}

/** A pivot table on a sheet, and the cells it covers. 1-based. */
export interface PivotBox {
  name: string;
  top: number;
  left: number;
  bottom: number;
  right: number;
}

export type Command =
  | {
      kind: 'input';
      sheet: number;
      row: number;
      col: number;
      text: string;
      areas?: CellArea[];
      /** Past a validation rule's Warning or Information message, when the answer was yes. */
      force?: boolean;
    }
  | { kind: 'arrayInput'; sheet: number; area: CellArea; text: string }
  | { kind: 'clear'; sheet: number; areas: CellArea[] }
  | { kind: 'fill'; sheet: number; from: CellArea; to: CellArea; copy?: boolean }
  | {
      kind: 'paste';
      fromSheet: number;
      from: CellArea;
      sheet: number;
      to: CellArea;
      mode?: 'all' | 'values' | 'formulas' | 'formats';
    }
  | {
      kind: 'move';
      fromSheet: number;
      from: CellArea;
      sheet: number;
      row: number;
      col: number;
    }
  | { kind: 'pasteExternal'; sheet: number; row: number; col: number; text: string; html?: string }
  | { kind: 'axis'; sheet: number; axis: 'row' | 'col'; at: number; count: number; delete: boolean }
  | { kind: 'insertSheet'; position: number; name?: string; rightToLeft?: boolean }
  | { kind: 'deleteSheet'; sheet: number }
  | { kind: 'renameSheet'; sheet: number; name: string }
  | { kind: 'sheetState'; sheet: number; hidden: boolean }
  | { kind: 'zoom'; sheet: number; zoom: number }
  | { kind: 'activeSheet'; sheet: number }
  | {
      kind: 'protectSheet';
      sheet: number;
      allow?: Partial<ProtectAllow>;
      password?: string;
      /** 16 random bytes, base64. */
      salt?: string;
    }
  | { kind: 'unprotectSheet'; sheet: number; password?: string }
  | { kind: 'protectWorkbook'; structure?: boolean; password?: string; salt?: string }
  | { kind: 'unprotectWorkbook'; password?: string }
  | {
      kind: 'format';
      sheet: number;
      areas: CellArea[];
      change: FormatChange;
      /** The cell a relative change, such as more decimals, goes by: [row, col], 1-based. */
      active?: [number, number];
    }
  | {
      kind: 'lines';
      sheet: number;
      axis: 'row' | 'col';
      first: number;
      last: number;
      size?: number;
      hidden?: boolean;
    }
  | { kind: 'autoFitRows'; sheet: number; first: number; last: number }
  | { kind: 'fitCols'; sheet: number; cols: [number, number][] }
  | { kind: 'fitRows'; rows: [number, number, number][]; quiet?: boolean }
  | { kind: 'merge'; sheet: number; areas: CellArea[]; center?: boolean; across?: boolean }
  | { kind: 'unmerge'; sheet: number; areas: CellArea[] }
  | { kind: 'freeze'; sheet: number; rows: number; cols: number; top?: number; left?: number }
  | { kind: 'shiftCells'; sheet: number; area: CellArea; down: boolean; delete?: boolean }
  | {
      kind: 'replaceOne';
      query: FindQuery;
      with: string;
      sheet: number;
      row: number;
      col: number;
    }
  | { kind: 'replaceAll'; query: FindQuery; with: string }
  | { kind: 'viewFlags'; sheet: number; grid?: boolean; headings?: boolean; rightToLeft?: boolean }
  | { kind: 'pageSetup'; sheets: number[]; spec: PageSpec }
  | {
      kind: 'pageBreak';
      sheet: number;
      /** The first row and column of the new page. */
      row?: number | null;
      col?: number | null;
      action: 'insert' | 'remove' | 'reset';
    }
  | { kind: 'printArea'; sheet: number; areas?: CellArea[]; action: 'set' | 'add' | 'clear' }
  | {
      kind: 'printTitles';
      sheet: number;
      rows?: [number, number] | null;
      cols?: [number, number] | null;
    }
  | {
      kind: 'createTable';
      sheet: number;
      area: CellArea;
      headers?: boolean;
      style?: string;
      name?: string;
    }
  | ({ kind: 'tableOptions'; table: string } & TableOptions)
  | { kind: 'totalsFunction'; table: string; column: string; function?: string | null }
  | { kind: 'renameTable'; table: string; name: string }
  | { kind: 'resizeTable'; table: string; area: CellArea }
  | { kind: 'convertToRange'; table: string }
  | {
      kind: 'sort';
      sheet: number;
      area: CellArea;
      keys: SortKey[];
      header?: boolean;
      caseSensitive?: boolean;
      byColumns?: boolean;
    }
  | { kind: 'cellStyle'; sheet: number; areas: CellArea[]; name: string }
  | {
      kind: 'removeDuplicates';
      sheet: number;
      area: CellArea;
      columns: number[];
      header?: boolean;
    }
  | { kind: 'setName'; old?: NameRef; name: string; formula: string; sheet?: number | null }
  | ({ kind: 'deleteName' } & NameRef)
  | {
      kind: 'namesFromSelection';
      sheet: number;
      area: CellArea;
      top?: boolean;
      left?: boolean;
      bottom?: boolean;
      right?: boolean;
    }
  | { kind: 'setNote'; sheet: number; row: number; col: number; text?: string; author?: string }
  /** A drawing moved or sized, in CSS pixels at 100%. */
  | {
      kind: 'setDrawingBox';
      sheet: number;
      index: number;
      x: number;
      y: number;
      width: number;
      height: number;
    }
  | { kind: 'deleteDrawings'; sheet: number; indexes: number[] }
  | { kind: 'insertChart'; sheet: number; area: CellArea; chart: ChartType; x: number; y: number }
  | {
      kind: 'insertPicture';
      sheet: number;
      /** From `addMedia`. */
      part: string;
      x: number;
      y: number;
      width: number;
      height: number;
      name?: string;
    }
  | { kind: 'editChart'; sheet: number; index: number; edit: ChartEdit }
  | { kind: 'moveDrawing'; sheet: number; index: number; to: number }
  | { kind: 'refreshPivot'; pivot?: PivotRef }
  | { kind: 'pivotLayout'; pivot: PivotRef; layout: PivotLayoutSpec }
  | { kind: 'pivotOptions'; pivot: PivotRef; options: PivotOptionSpec }
  | { kind: 'pivotField'; pivot: PivotRef; field: number; change: PivotFieldChange }
  | {
      kind: 'createPivot';
      sheet: number;
      area: CellArea;
      to: { kind: 'newSheet' } | { kind: 'existing'; sheet: number; row: number; col: number };
    }
  | { kind: 'deletePivot'; pivot: PivotRef }
  | {
      kind: 'setLink';
      sheet: number;
      area: CellArea;
      link?: LinkSpec;
      /** With no link: Remove Hyperlink, which takes the link's look off too. */
      plain?: boolean;
    }
  | { kind: 'noteVisible'; sheet: number; row: number; col: number; visible: boolean }
  /** Error checking's rules that are on, as bits; 0 turns background checking off. */
  | { kind: 'errorRules'; rules: number }
  /** Text with runs of their own fonts, from characters counted from 0. */
  | {
      kind: 'richInput';
      sheet: number;
      row: number;
      col: number;
      text: string;
      runs: { start: number; font: object }[];
    }
  | { kind: 'ignoreError'; sheet: number; row: number; col: number }
  | { kind: 'convertToNumber'; sheet: number; areas: CellArea[] }
  | { kind: 'includeCells'; sheet: number; row: number; col: number }
  | { kind: 'setValidation'; sheet: number; areas: CellArea[]; rule?: ValidationRule }
  | { kind: 'circleInvalid'; sheet: number; on: boolean }
  | { kind: 'addCf'; sheet: number; areas: CellArea[]; rule: CfRuleSpec }
  | { kind: 'clearCf'; sheet: number; areas?: CellArea[] }
  | { kind: 'setCf'; sheet: number; rules: ManagedCfRule[] }
  | { kind: 'toggleFilter'; sheet: number; area: CellArea }
  | { kind: 'setFilter'; sheet: number; table?: string; col: number; criteria?: FilterCriteria }
  | { kind: 'clearFilter'; sheet: number; table?: string }
  | { kind: 'reapplyFilter'; sheet: number; table?: string }
  | { kind: 'undo' }
  | { kind: 'redo' };

// Phase 6's data tools: what their commands take and what `query` answers. See
// crates/cgs-engine/src/ask.rs and the modules it names, and keep the two in step.

/** A table's options, as the Table Design tab sets them. */
export interface TableOptions {
  headerRow?: boolean;
  totalsRow?: boolean;
  firstColumn?: boolean;
  lastColumn?: boolean;
  rowStripes?: boolean;
  columnStripes?: boolean;
  filterButton?: boolean;
  style?: string | null;
}

export type SortOn =
  | { kind: 'value' }
  /** Cells with this fill first (`RRGGBB`), or with no fill. */
  | { kind: 'cellColor'; color?: string | null }
  | { kind: 'fontColor'; color?: string | null }
  /** Cells showing this conditional format icon first: its set and place from 0. */
  | { kind: 'icon'; set: string; id: number };

export interface SortKey {
  /** The sheet's column, 1-based, or its row for a sort left to right. */
  index: number;
  descending?: boolean;
  by?: SortOn;
  /** A custom order, such as months: these first, in this order. */
  list?: string[];
}

export interface NameRef {
  name: string;
  /** A sheet's own name, or none for the workbook's. */
  sheet?: number | null;
}

export interface LinkSpec {
  /** A web address, file or email address. */
  address?: string;
  /** A place in this workbook, such as `Sheet2!A1`. */
  location?: string;
  tooltip?: string;
  /** What the cell shows. */
  text?: string;
}

export type ValidationKind =
  'any' | 'whole' | 'decimal' | 'list' | 'date' | 'time' | 'textLength' | 'custom';

export type ValidationOp =
  | 'between'
  | 'notBetween'
  | 'equal'
  | 'notEqual'
  | 'greaterThan'
  | 'lessThan'
  | 'greaterThanOrEqual'
  | 'lessThanOrEqual';

export interface ValidationRule {
  kind: ValidationKind;
  op?: ValidationOp;
  /** As typed, with or without `=`. A list's items are comma-separated or a reference. */
  formula1?: string;
  formula2?: string;
  allowBlank?: boolean;
  /** A list's in-cell dropdown. */
  dropdown?: boolean;
  showInput?: boolean;
  inputTitle?: string;
  inputText?: string;
  showError?: boolean;
  errorTitle?: string;
  errorText?: string;
  style?: 'stop' | 'warning' | 'information';
}

/** A cell's rule, from `validationAt`. */
export interface ValidationInfo extends Required<Omit<ValidationRule, 'formula1' | 'formula2'>> {
  formula1: string | null;
  formula2: string | null;
  /** A list's items, as the dropdown shows them. */
  items: string[];
  /** A rule from newer Excel, kept but not edited. */
  extended: boolean;
  /** The cells the rule covers, as `A1:B5 D1`. */
  areas: string;
}

/** How a conditional format's cells look. Colours are `RRGGBB`. */
export interface CfFormat {
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: boolean;
  color?: string;
  fill?: string;
  border?: string;
  numberFormat?: string;
}

export interface CfPoint {
  /** `min`, `max`, `num`, `percent`, `percentile` or `formula`. */
  kind: string;
  value?: string;
  color?: string;
  gte?: boolean;
}

export type CfRuleSpec =
  | { type: 'cellIs'; op: string; formulas: string[]; format?: CfFormat }
  | { type: 'expression'; formula: string; format?: CfFormat }
  | {
      type: 'text';
      op: 'contains' | 'notContains' | 'beginsWith' | 'endsWith';
      text: string;
      format?: CfFormat;
    }
  | { type: 'blanks'; not?: boolean; format?: CfFormat }
  | { type: 'errors'; not?: boolean; format?: CfFormat }
  | { type: 'timePeriod'; period: string; format?: CfFormat }
  | { type: 'top10'; bottom?: boolean; percent?: boolean; rank: number; format?: CfFormat }
  | { type: 'average'; above?: boolean; equal?: boolean; stdDev?: number; format?: CfFormat }
  | { type: 'duplicates'; unique?: boolean; format?: CfFormat }
  | { type: 'colorScale'; points: CfPoint[] }
  | {
      type: 'dataBar';
      min: CfPoint;
      max: CfPoint;
      color: string;
      gradient?: boolean;
      showValue?: boolean;
    }
  | { type: 'iconSet'; set: string; points?: CfPoint[]; reverse?: boolean; showValue?: boolean };

/** A rule of the manager's list, in priority order: an existing one by its place, or a new one. */
export interface ManagedCfRule {
  block?: number;
  rule?: number;
  areas: CellArea[];
  spec?: CfRuleSpec;
  stop?: boolean;
}

/** A rule as the manager lists it, from `cfRules`. */
export interface CfRuleInfo {
  block: number;
  rule: number;
  priority: number;
  stop: boolean;
  /** From newer Excel: shown and kept, not edited. */
  extended: boolean;
  kind: string;
  description: string;
  /** As `A1:B5 D1`. */
  areas: string;
  fill: string | null;
  color: string | null;
  bold: boolean | null;
}

export type FilterCriteria =
  | {
      kind: 'values';
      values?: string[];
      blank?: boolean;
      /** `[year]`, `[year, month]` or `[year, month, day]`. */
      dates?: number[][];
    }
  | {
      kind: 'custom';
      and?: boolean;
      conditions: {
        op:
          | 'equal'
          | 'notEqual'
          | 'lessThan'
          | 'lessThanOrEqual'
          | 'greaterThan'
          | 'greaterThanOrEqual';
        value: string;
      }[];
    }
  | { kind: 'top10'; top?: boolean; percent?: boolean; value: number }
  /** `aboveAverage`, `belowAverage`, `today`, `thisMonth` and the rest of Excel's names. */
  | { kind: 'dynamic'; name: string }
  | { kind: 'color'; cell?: boolean; color?: string | null }
  /** Cells showing a conditional format's icon, or with no `id` cells showing none. */
  | { kind: 'icon'; set: string; id?: number };

/** A filter on a sheet, from `filters`: the sheet's own or a table's. */
export interface FilterInfo {
  table: string | null;
  area: CellArea;
  /** Columns with criteria. */
  filtered: number[];
  /** Columns whose button is hidden. */
  hiddenButtons: number[];
  sorted: { col: number; descending: boolean } | null;
}

/** What a filter's dropdown lists for one column, from `filterList`. */
export interface FilterList {
  values: { text: string; checked: boolean }[];
  dates: { date: [number, number, number]; checked: boolean }[];
  blanks: boolean;
  blanksChecked: boolean;
  filtered: 'none' | 'values' | 'custom' | 'top10' | 'dynamic' | 'color' | 'icon' | 'other';
  /** The fills the column shows, `RRGGBB` or null for none, when it shows more than one. */
  fills: (string | null)[];
  /** Its text colours, when more than one. */
  fonts: string[];
  /** Its conditional format icons, and an empty set for cells with none. */
  icons: { set: string; id: number | null }[];
}

/** A table, from `tableAt` or `tables`. */
export interface TableInfo {
  name: string;
  sheet: number;
  area: CellArea;
  headerRow: boolean;
  totalsRow: boolean;
  style: string | null;
  firstColumn: boolean;
  lastColumn: boolean;
  rowStripes: boolean;
  columnStripes: boolean;
  filterButton: boolean;
  columns: { name: string; totals: string | null; calculated: boolean }[];
}

export interface DefinedNameInfo {
  name: string;
  /** With its `=`. */
  formula: string;
  sheet: number | null;
  /** `Workbook` or the sheet's name. */
  scope: string;
}

export interface NoteInfo {
  author?: string;
  text?: string;
  /** Show/Hide Note: the box shows all the time. */
  visible?: boolean;
  /** Where its box is: see `Note::anchor` in cgs-model. */
  anchor?: number[];
  /** A threaded comment from newer Excel, shown and kept only. */
  threaded?: boolean;
}

export interface LinkInfo {
  address: string | null;
  location: string | null;
  tooltip: string | null;
}

export interface CellStyleInfo {
  name: string;
  font: string;
  size: number;
  bold: boolean;
  italic: boolean;
  /** `#RRGGBB`. */
  color: string | null;
  fill: string | null;
  /** Lines all round, as Note and Input have. */
  boxed: boolean;
  bottom: string | null;
  bottomColor: string | null;
}

/**
 * A table style with a preview of 5 columns by 7 rows. Each cell is [fill, text colour, bold,
 * top, bottom, left, right line colours], colours as `#RRGGBB` or null.
 */
export interface TableStyleInfo {
  name: string;
  group: 'Custom' | 'Light' | 'Medium' | 'Dark';
  cells: [
    string | null,
    string | null,
    boolean,
    string | null,
    string | null,
    string | null,
    string | null,
  ][][];
}

/** A question for `EngineClient.query`, and the type of its answer. */
export type Question =
  | { kind: 'tableAt'; sheet: number; row: number; col: number }
  | { kind: 'guessTable'; sheet: number; area: CellArea }
  | { kind: 'tables' }
  | { kind: 'filterList'; sheet: number; table?: string | null; col: number }
  | { kind: 'filters'; sheet: number }
  | { kind: 'cfRules'; sheet: number }
  | { kind: 'validationAt'; sheet: number; row: number; col: number }
  | { kind: 'invalidCells'; sheet: number }
  | { kind: 'names' }
  | {
      kind: 'nextSelectable';
      sheet: number;
      row: number;
      col: number;
      rows?: number;
      cols?: number;
    }
  | { kind: 'columnLooks'; sheet: number; col: number; top: number; bottom: number }
  | { kind: 'circular' }
  | { kind: 'richAt'; sheet: number; row: number; col: number }
  | { kind: 'cellErrors'; sheet: number; row: number; col: number }
  | { kind: 'nextError'; sheet: number; row: number; col: number; back?: boolean }
  | { kind: 'noteAt'; sheet: number; row: number; col: number }
  | { kind: 'linkAt'; sheet: number; row: number; col: number }
  | { kind: 'notes'; sheet: number }
  | { kind: 'cellStyles' }
  | { kind: 'tableStyles'; options?: Omit<TableOptions, 'style' | 'filterButton'> }
  | { kind: 'pageSetup'; sheet: number }
  | { kind: 'pages'; sheet: number; request?: PrintRequest };

export interface Answers {
  tableAt: TableInfo | null;
  guessTable: { area: CellArea; headers: boolean };
  tables: TableInfo[];
  filterList: FilterList;
  filters: FilterInfo[];
  cfRules: CfRuleInfo[];
  validationAt: ValidationInfo | null;
  invalidCells: [number, number][];
  names: DefinedNameInfo[];
  nextSelectable: [number, number] | null;
  columnLooks: Pick<FilterList, 'fills' | 'fonts' | 'icons'> | null;
  richAt: { start: number; font: Record<string, unknown> | null }[] | null;
  /** The cells on loops, as `[sheet, row, col]`, or null when the book is too big to check. */
  circular: [number, number, number][] | null;
  cellErrors: { rule: string; value?: string; from?: 'above' | 'left' }[];
  nextError: [number, number] | null;
  noteAt: NoteInfo | null;
  linkAt: LinkInfo | null;
  notes: {
    row: number;
    col: number;
    author: string;
    text: string;
    visible: boolean;
    anchor: number[];
  }[];
  cellStyles: { group: string; styles: CellStyleInfo[] }[];
  tableStyles: TableStyleInfo[];
  pageSetup: PageSetupInfo;
  pages: PagesInfo;
}

/** Margins in inches, as the file holds them. */
export interface PageMargins {
  left: number;
  right: number;
  top: number;
  bottom: number;
  header: number;
  footer: number;
}

export interface HeaderFooterTexts {
  oddHeader: string;
  oddFooter: string;
  evenHeader: string;
  evenFooter: string;
  firstHeader: string;
  firstFooter: string;
  differentOddEven: boolean;
  differentFirst: boolean;
  scaleWithDoc: boolean;
  alignWithMargins: boolean;
}

export type Orientation = 'default' | 'portrait' | 'landscape';

/** The parts of page setup to change. See `PageSpec` in crates/cgs-engine/src/page.rs. */
export interface PageSpec {
  paper?: number;
  orientation?: Orientation;
  scale?: number;
  fitToPage?: boolean;
  fitWidth?: number;
  fitHeight?: number;
  firstPage?: number;
  firstPageAuto?: boolean;
  pageOrder?: 'downThenOver' | 'overThenDown';
  blackAndWhite?: boolean;
  draft?: boolean;
  comments?: 'none' | 'atEnd' | 'asDisplayed';
  errors?: 'displayed' | 'blank' | 'dash' | 'NA';
  margins?: Partial<PageMargins>;
  centerHorizontally?: boolean;
  centerVertically?: boolean;
  headings?: boolean;
  gridlines?: boolean;
  headerFooter?: Partial<HeaderFooterTexts>;
}

/** A sheet's page setup, print area and titles. */
export interface PageSetupInfo {
  paper: number;
  paperWidth: number;
  paperHeight: number;
  orientation: Orientation;
  scale: number;
  fitToPage: boolean;
  fitWidth: number;
  fitHeight: number;
  firstPage: number | null;
  pageOrder: 'downThenOver' | 'overThenDown';
  blackAndWhite: boolean;
  draft: boolean;
  comments: 'none' | 'atEnd' | 'asDisplayed';
  errors: 'displayed' | 'blank' | 'dash' | 'NA';
  margins: PageMargins;
  centerHorizontally: boolean;
  centerVertically: boolean;
  headings: boolean;
  gridlines: boolean;
  headerFooter: HeaderFooterTexts;
  rowBreaks: number[];
  colBreaks: number[];
  /** As Excel shows it, without the sheet: `$A$1:$D$20`. */
  printArea: string;
  printAreas: CellArea[];
  titleRows: string | null;
  titleCols: string | null;
  papers: { code: number; name: string; width: number; height: number }[];
}

/** What to print instead of the print area. */
export interface PrintRequest {
  selection?: CellArea[];
  ignorePrintArea?: boolean;
}

/** A run of a header or footer. `field` names what `&P` and the rest show. */
export interface HfRun {
  field: 'page' | 'pages' | 'date' | 'time' | 'file' | 'path' | 'sheet' | 'picture' | null;
  /** The literal text, or `&P`'s offset. */
  text: string | number | null;
  font: string | null;
  bold: boolean;
  italic: boolean;
  size: number | null;
  underline: number;
  strike: boolean;
  superscript: boolean;
  subscript: boolean;
  color: string | null;
}

export interface HfSections {
  left: HfRun[];
  center: HfRun[];
  right: HfRun[];
}

/** One page: its cells, and its top left on the paper in points. */
export interface PageInfo {
  area: number;
  rows: [number, number];
  cols: [number, number];
  titleRows: [number, number] | null;
  titleCols: [number, number] | null;
  x: number;
  y: number;
  /** What it prints at 100%, in sheet points. */
  width: number;
  height: number;
}

/** A sheet's pages. Lengths are in points. See `pages_json` in crates/cgs-engine/src/print.rs. */
export interface PagesInfo {
  sheet: number;
  name: string;
  paper: [number, number];
  margins: {
    left: number;
    right: number;
    top: number;
    bottom: number;
    header: number;
    footer: number;
  };
  zoom: number;
  /** Every page, where `pages` stops at the engine's limit of 10,000. */
  pageCount: number;
  firstPage: number | null;
  gridlines: boolean;
  headings: boolean;
  blackAndWhite: boolean;
  draft: boolean;
  errors: 'displayed' | 'blank' | 'dash' | 'NA';
  /** In screen pixels at 100%. */
  headingSize: [number, number] | null;
  /** Excel's printer dots: per inch, and per pixel of a row and a column at 100%. */
  units?: { dpi: number; row: number; col: number };
  headerFooter: {
    odd: [HfSections | null, HfSections | null];
    even: [HfSections | null, HfSections | null];
    first: [HfSections | null, HfSections | null];
    differentOddEven: boolean;
    differentFirst: boolean;
    scaleWithDoc: boolean;
    alignWithMargins: boolean;
  };
  font: { name: string; size: number };
  pages: PageInfo[];
  rowBreaks: number[];
  colBreaks: number[];
  manualRows: number[];
  manualCols: number[];
  printAreas: CellArea[];
  /** The header and footer's pictures, by section: `CH` is the centre of the header. */
  pictures: { place: string; part: string; width: number; height: number }[];
}

/** A colour as a format gives it. Theme colours stay references to the theme. */
export type FormatColor =
  { kind: 'auto' } | { kind: 'rgb'; rgb: string } | { kind: 'theme'; theme: number; tint?: number };

export interface FormatLine {
  /** Excel's name for the style, or `none`. */
  style: string;
  color?: FormatColor;
}

/** Lines by where a cell's edge falls in the selection. */
export interface BorderChange {
  top?: FormatLine;
  bottom?: FormatLine;
  left?: FormatLine;
  right?: FormatLine;
  insideHorizontal?: FormatLine;
  insideVertical?: FormatLine;
  diagonal?: FormatLine;
  diagonalUp?: boolean;
  diagonalDown?: boolean;
}

/** What a format does. Anything left out stays as it is in each cell. */
export interface FormatChange {
  clear?: boolean;
  fontName?: string;
  fontSize?: number;
  fontStep?: number;
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: 'none' | 'single' | 'double' | 'singleAccounting' | 'doubleAccounting';
  fontColor?: FormatColor;
  /** Format Cells' Superscript and Subscript. */
  vertAlign?: 'baseline' | 'superscript' | 'subscript';
  fill?: { pattern: string; color?: FormatColor; patternColor?: FormatColor };
  numberFormat?: string;
  numberFormatId?: number;
  decimals?: number;
  horizontal?: string;
  vertical?: string;
  wrap?: boolean;
  shrink?: boolean;
  indent?: number;
  indentStep?: number;
  rotation?: number;
  readingOrder?: number;
  border?: BorderChange;
  /** Format Cells' Protection tab. */
  locked?: boolean;
  formulaHidden?: boolean;
}

/** A colour as a cell has it, with how it looks here. */
export type StoredColor =
  | { kind: 'auto' }
  | { kind: 'rgb'; color: string }
  | { kind: 'theme'; theme: number; tint: number; color: string };

export interface StoredLine {
  style: string;
  color: StoredColor;
}

/** How a cell is formatted, for the ribbon. */
export interface CellFormat {
  fontName: string;
  fontSize: number;
  bold: boolean;
  italic: boolean;
  strike: boolean;
  underline: NonNullable<FormatChange['underline']>;
  fontColor: StoredColor;
  vertAlign: 'baseline' | 'superscript' | 'subscript';
  fill: { pattern: string; color: StoredColor; patternColor: StoredColor };
  horizontal: string;
  vertical: string;
  wrap: boolean;
  shrink: boolean;
  indent: number;
  rotation: number;
  readingOrder: number;
  numberFormat: string;
  numberFormatId: number;
  merged: boolean;
  /** Format Cells' Protection tab. */
  locked: boolean;
  formulaHidden: boolean;
  border: {
    top: StoredLine;
    bottom: StoredLine;
    left: StoredLine;
    right: StoredLine;
    diagonal: StoredLine;
    diagonalUp: boolean;
    diagonalDown: boolean;
  };
}

/** The workbook's theme colours and its own number formats. */
export interface FormatLists {
  theme: string[];
  codes: string[];
}

/** A search, as the dialog sets it up. See `FindQuery` in `crates/cgs-engine/src/find.rs`. */
export interface FindQuery {
  text: string;
  sheet: number;
  workbook?: boolean;
  byColumns?: boolean;
  formulas?: boolean;
  matchCase?: boolean;
  wholeCell?: boolean;
  within?: CellArea;
}

export interface FoundCell {
  sheet: number;
  row: number;
  col: number;
  text: string;
}

/** What a command did. */
export interface Applied {
  /** How many cells a replace changed. */
  replaced?: number;
  selection: (CellArea & { sheet: number }) | null;
  undo: string | null;
  redo: string | null;
  /** The last change couldn't be kept for undo. */
  lost: boolean;
}

/** What changed for the UI, from `decodeChanges`. */
export interface WorkbookChanges {
  /** Sheets to draw again in full. */
  sheets: number[];
  /** Sheets whose sizes or merges changed. */
  layouts: number[];
  /** The list of sheets changed. */
  book: boolean;
  /** Tiles to draw again. */
  tiles: { sheet: number; firstRow: number; firstCol: number }[];
}

/** Whether calculation is running or due. */
export interface CalcStatus {
  calculating: boolean;
  manual: boolean;
  /** Why calculation stopped for good: the engine ran out of memory and can't be used again. */
  failure?: string;
}

/** Edits a crashed session left behind. */
export interface Recoverable {
  id: string;
  name: string;
  edits: number;
  /** When its last edit was made, in ms since 1970. */
  time: number;
}

/** What a save did. See `SaveReport` in `crates/cgs-xlsx/src/save.rs`. */
export interface SaveReport {
  keptParts: number;
  cellsWritten: number;
  stringsAdded: number;
  recalculatesOnOpen: boolean;
  pivotsRefreshOnOpen: boolean;
  resultsLeftToExcel: number;
  tableHeadersFixed: number;
  leftExtensions: string[];
  changedParts: string[];
  removedParts: string[];
  addedParts: string[];
  keptContent: { what: string; parts: number }[];
}

/** How far a save has come, for the status bar. */
export interface SaveProgress {
  /** Writing the file, then reading it back to check it. */
  stage: 'writing' | 'checking';
  /** From 0 to 1. */
  fraction: number;
}

/** A checked save, ready to replace the file. */
export interface SavedFile {
  blob: Blob;
  report: SaveReport;
}

/** What Protect Sheet lets people do. See `AllowSpec` in `crates/cgs-engine/src/protect.rs`. */
export interface ProtectAllow {
  selectLocked: boolean;
  selectUnlocked: boolean;
  formatCells: boolean;
  formatColumns: boolean;
  formatRows: boolean;
  insertColumns: boolean;
  insertRows: boolean;
  insertHyperlinks: boolean;
  deleteColumns: boolean;
  deleteRows: boolean;
  sort: boolean;
  autoFilter: boolean;
  pivotTables: boolean;
  editObjects: boolean;
  editScenarios: boolean;
}

export interface SheetSummary {
  name: string;
  state: 'visible' | 'hidden' | 'veryHidden';
  kind: 'worksheet' | 'chart' | 'other';
  loaded: boolean;
  deleted: boolean;
  /** What a protected sheet allows, and whether it has a password; none when unprotected. */
  protection: (ProtectAllow & { password: boolean }) | null;
}

export interface BookSummary {
  active: number;
  sheets: SheetSummary[];
  /** Sheets by index, in the order their tabs show. */
  order: number[];
  /** Protect Workbook's structure. */
  structureLocked: boolean;
  structurePassword: boolean;
}

export type LoadEvent =
  /** Bytes read so far, and sheets that became readable since the last event. */
  | { type: 'progress'; done: number; total: number; ready: number[] }
  /** All sheets are readable. */
  | { type: 'loaded' }
  /** The work the first edit would otherwise start with is done too. */
  | { type: 'prepared' }
  | { type: 'error'; message: string };

export interface DrawingBox {
  kind: 'chart' | 'picture' | 'shape' | 'group' | 'other';
  name: string;
  /** Its place in the sheet's drawings, which commands name it by. */
  index: number;
  /** In CSS pixels at 100%. */
  x: number;
  y: number;
  width: number;
  height: number;
  /** The chart part it shows, if it is a chart. */
  chart?: number;
  /** The picture's part, such as `xl/media/image1.png`. */
  image?: string;
  description?: string;
  /** False when it is hidden or set not to print. */
  prints?: boolean;
}

/** What Insert's chart buttons and Change Chart Type make. */
export type ChartType =
  | 'columnClustered'
  | 'columnStacked'
  | 'columnPercent'
  | 'barClustered'
  | 'barStacked'
  | 'barPercent'
  | 'line'
  | 'lineStacked'
  | 'linePercent'
  | 'lineMarkers'
  | 'lineMarkersStacked'
  | 'lineMarkersPercent'
  | 'pie'
  | 'doughnut'
  | 'area'
  | 'areaStacked'
  | 'areaPercent'
  | 'scatter'
  | 'scatterSmoothMarkers'
  | 'scatterSmooth'
  | 'scatterLinesMarkers'
  | 'scatterLines'
  | 'comboColumnLine'
  | 'comboColumnLineSecondary'
  | 'comboAreaColumn';

/** A part of a chart, as the Format tab names it to the engine. */
export type ChartElementRef =
  | { kind: 'chartArea' }
  | { kind: 'plotArea' }
  | { kind: 'title' }
  | { kind: 'legend' }
  | { kind: 'axis'; id: number }
  | { kind: 'axisTitle'; id: number }
  | { kind: 'gridlines'; id: number }
  | { kind: 'series'; index: number }
  | { kind: 'point'; index: number; point: number }
  | { kind: 'dataLabels'; index: number };

/** A colour for a chart: `#rrggbb`, or a theme colour by name with a luminance change. */
export interface ChartColor {
  rgb?: string;
  /** `bg1`, `tx1`, `bg2`, `tx2`, `accent1` to `accent6`. */
  theme?: string;
  lumMod?: number;
  lumOff?: number;
}

/** A change from the Chart Design or Format tab. See `ChartEdit` in `crates/cgs-engine`. */
export type ChartEdit =
  | { kind: 'type'; chart: ChartType }
  | { kind: 'seriesType'; index: number; chart: ChartType; secondary?: boolean }
  | { kind: 'title'; text?: string }
  | { kind: 'axisTitle'; id: number; text?: string }
  | { kind: 'legend'; position?: 'right' | 'left' | 'top' | 'bottom' | 'topRight' }
  | {
      kind: 'dataLabels';
      index?: number;
      show: boolean;
      position?: string;
      value?: boolean;
      category?: boolean;
      series?: boolean;
      percent?: boolean;
    }
  | { kind: 'gridlines'; id: number; major: boolean; minor: boolean }
  | { kind: 'axisVisible'; id: number; visible: boolean }
  | {
      kind: 'axisScale';
      id: number;
      min?: number;
      max?: number;
      major?: number;
      minor?: number;
      reversed?: boolean;
      format?: string;
    }
  | { kind: 'switchRowCol' }
  | { kind: 'range'; sheet: number; area: CellArea }
  | { kind: 'series'; list: { name?: string; categories?: string; values: string }[] }
  | { kind: 'style'; style: number }
  | { kind: 'colors'; palette: number }
  | { kind: 'quickLayout'; layout: number }
  | { kind: 'fill'; element: ChartElementRef; color?: ChartColor; none?: boolean }
  | {
      kind: 'outline';
      element: ChartElementRef;
      color?: ChartColor;
      none?: boolean;
      /** Points. */
      width?: number;
      dash?: string;
    }
  | {
      kind: 'font';
      element: ChartElementRef;
      size?: number;
      bold?: boolean;
      italic?: boolean;
      color?: ChartColor;
    }
  | { kind: 'gapWidth'; group: number; gap: number; overlap?: number };

/** The screen a chart is drawn for: dark mode swaps its text and background colours. */
export interface ChartScreen {
  dark: boolean;
  /** `0xRRGGBB`. */
  background: number;
  text: number;
}

export interface ChartLineView {
  color: string;
  /** CSS pixels. */
  width: number;
  dash: 'solid' | 'dot' | 'dash' | 'dashDot';
}

export interface ChartTextView {
  text: string;
  /** CSS pixels. */
  size: number;
  color: string;
  bold: boolean;
  italic: boolean;
  /** Degrees. */
  rotation: number;
}

export interface ChartAxisView {
  id: number;
  visible: boolean;
  position: 'left' | 'right' | 'top' | 'bottom';
  reversed: boolean;
  title: ChartTextView | null;
  labels: ChartTextView | null;
  line: ChartLineView | null;
  gridlines: ChartLineView | null;
  minorGridlines: ChartLineView | null;
  majorTick: 'none' | 'in' | 'out' | 'cross';
  minorTick: 'none' | 'in' | 'out' | 'cross';
  min: number | null;
  max: number | null;
  unit: number | null;
  ticks: { value: number; text: string }[];
  between: boolean;
  labelSkip: number;
  crossesAt: number | null;
  crosses: 'autoZero' | 'min' | 'max' | 'at';
}

export interface ChartSeriesView {
  name: string;
  group: number;
  values: (number | null)[];
  x: (number | null)[] | null;
  fill: string | null;
  line: ChartLineView | null;
  marker: { symbol: string; size: number; fill: string | null; line: ChartLineView | null } | null;
  smooth: boolean;
  pointFills: (string | null)[];
  pointLines: (ChartLineView | null)[];
  labels: { texts: string[]; position: string; look: ChartTextView } | null;
  index: number;
  explosion: number;
  /** What Select Data shows, each with `=`: the name's cell or quoted text, and the cells read. */
  nameRef: string | null;
  categoriesRef: string | null;
  valuesRef: string | null;
}

export interface ChartGroupView {
  kind: 'bar' | 'line' | 'area' | 'pie' | 'doughnut' | 'scatter';
  fileKind: string;
  horizontal: boolean;
  stacking: 'none' | 'stacked' | 'percent';
  gapWidth: number;
  overlap: number;
  hole: number;
  firstSlice: number;
  varyColors: boolean;
  axis: 0 | 1;
}

/** What a chart draws, worked out by the engine: see `crates/cgs-engine/src/charts.rs`. */
export interface ChartView {
  kind: 'classic' | 'extended' | 'unreadable';
  extended: string | null;
  editable: boolean;
  font: string;
  background: string | null;
  border: ChartLineView | null;
  rounded: boolean;
  plotBackground: string | null;
  plotBorder: ChartLineView | null;
  plotLayout: [number, number, number, number] | null;
  title: ChartTextView | null;
  legend: {
    position: 'right' | 'left' | 'top' | 'bottom' | 'topRight';
    overlay: boolean;
    look: ChartTextView;
    hidden: number[];
    fill: string | null;
    border: ChartLineView | null;
  } | null;
  categories: string[];
  categoryAxis: ChartAxisView | null;
  valueAxes: ChartAxisView[];
  groups: ChartGroupView[];
  series: ChartSeriesView[];
  approximate: boolean;
  blanks: 'gap' | 'zero' | 'span';
}

/** Regional conventions, from `Intl`. See `crates/cgs-numfmt/src/locale.rs`. */
export interface LocaleData {
  decimal: string;
  group: string;
  shortDate: string;
  longDate: string;
  shortTime: string;
  longTime: string;
  am: string;
  pm: string;
  months: string[];
  monthsShort: string[];
  days: string[];
  daysShort: string[];
  currency: string;
  dateSeparator: string;
  timeSeparator: string;
}

export interface CellText {
  /** What the cell shows. */
  display: string;
  /** What the formula bar shows. */
  edit: string;
}

/** The engine worker, as the main thread sees it. */
export interface EngineApi {
  init(): Promise<EngineInfo>;
  /** Connects the tile worker, which then reads tiles from this port. */
  attachTileWorker(port: MessagePort): void;
  /** Writes `value` into `buffer`, to prove the page and the worker share memory. */
  touchSharedMemory(buffer: SharedArrayBuffer, value: number): void;
  setLocale(locale: LocaleData): void;
  /** Opens a workbook and starts reading it. Rejects with a message people can read. */
  open(
    source: FileSource,
    onEvent: (event: LoadEvent) => void,
    name?: string,
  ): Promise<BookSummary>;
  close(): void;
  /** The bytes of a new blank workbook. */
  blankWorkbook(rightToLeft?: boolean): Promise<Uint8Array>;
  /** A PDF of recorded pages: `document` is JSON for `cgs_pdf::Document`, over `blob`. */
  writePdf(document: string, blob: Uint8Array): Promise<Uint8Array>;
  /** A tile's data, as the tile worker reads it, for drawing pages. */
  viewBlock(
    sheet: number,
    firstRow: number,
    firstCol: number,
    dark: boolean,
    background: number,
    text: number,
  ): Promise<ArrayBuffer>;
  /**
   * Makes saves independent of the file on disk before the first edit. With a source, the
   * workbook reads from that copy. With none, the worker copies a browser file to OPFS itself.
   * Rejects if the file has changed since it was opened. Resolves false if there is nowhere to
   * copy it, as saving then still works unless the file changes.
   */
  protectSource(source?: FileSource): Promise<boolean>;
  /** Runs a command. Rejects with a message people can read, changing nothing. */
  command(command: Command): Promise<Applied>;
  /** Rows waiting to be measured. */
  takeRowFits(): Promise<RowFit[]>;
  /** Rows of a sheet from another program's file that wrap and hold no height. */
  openingRowFits(sheet: number): Promise<RowFit[]>;
  /** What to measure to fit columns. */
  colFits(sheet: number, first: number, last: number): Promise<ColFit[]>;
  mergeDiscards(sheet: number, areas: CellArea[], across: boolean): Promise<boolean>;
  cellFormat(sheet: number, row: number, col: number): Promise<CellFormat | null>;
  formatLists(): Promise<FormatLists | null>;
  /** What a cell looks like in a format code, or null if the code can't be used. */
  formatSample(sheet: number, row: number, col: number, code: string): Promise<string | null>;
  findNext(query: FindQuery, row: number, col: number, back: boolean): Promise<FoundCell | null>;
  findAll(query: FindQuery): Promise<FoundCell[]>;
  /** Tells the main thread what changed and whether calculation is running, from now on. */
  watchChanges(
    onChanges: (numbers: Uint32Array) => void,
    onCalc: (status: CalcStatus) => void,
  ): void;
  /** Where the user is looking, which calculation gets to first. 1-based. */
  setViewport(sheet: number, area: CellArea): void;
  setManualCalc(manual: boolean): Promise<void>;
  /** F9, Ctrl+Alt+F9 with `all`. */
  calculateNow(all: boolean): Promise<void>;
  /** Shift+F9. */
  calculateSheet(sheet: number): Promise<void>;
  /** What the clipboard gets for cells of this workbook. */
  copyText(sheet: number, area: CellArea): Promise<{ text: string; html: string }>;
  /** Opens CSV text as a new workbook's cells. */
  loadCsv(text: string): Promise<void>;
  /** A sheet as CSV text. */
  csvText(sheet: number): Promise<string>;
  /** Where Ctrl+arrow goes. 1-based. */
  jumpTarget(
    sheet: number,
    row: number,
    col: number,
    rows: number,
    cols: number,
  ): Promise<[number, number]>;
  /** What the name box's text refers to, or null. */
  resolveName(text: string, sheet: number): Promise<(CellArea & { sheet: number }) | null>;
  functionNames(): Promise<string[]>;
  definedNames(): Promise<string[]>;
  /** One of the data tools' questions, answered as JSON. See `Question`. */
  query(question: Question): Promise<unknown>;
  /** The list of sheets as it now stands. */
  bookSummary(): Promise<BookSummary>;
  /** Edits a crashed session left, for the next launch to offer back. */
  recoverable(): Promise<Recoverable[]>;
  /** Opens what a crash left, with its edits applied again. */
  openRecovered(id: string, onEvent: (event: LoadEvent) => void): Promise<BookSummary>;
  discardRecovered(id: string): Promise<void>;
  hasChanges(): Promise<boolean>;
  /**
   * Writes the workbook to a checked blob, and leaves the file on disk alone. It runs in slices,
   * so tiles and reads are served meanwhile, and anything that would change the workbook waits
   * until it ends.
   */
  save(onProgress?: (progress: SaveProgress) => void): Promise<SavedFile>;
  /** The save now stands in `source`, which the workbook reads from from now on. */
  commitSaved(source: FileSource): Promise<void>;
  prioritize(sheet: number): void;
  /** The numbers `decodeLayout` reads. */
  layout(sheet: number): Promise<Uint32Array>;
  drawings(sheet: number): Promise<DrawingBox[]>;
  /** The pivot tables on a sheet. */
  pivots(sheet: number): Promise<PivotBox[]>;
  /** The pivot table at a cell, 1-based, or null. */
  pivotInfo(sheet: number, row: number, col: number): Promise<PivotInfo | null>;
  /** What a chart drawing shows, for this screen. Colours are `0xRRGGBB`. */
  chart(sheet: number, drawing: number, screen: ChartScreen): Promise<ChartView | null>;
  /** A part's bytes, such as a picture's. */
  media(part: string): Promise<Uint8Array>;
  /** Takes a picture's bytes for `insertPicture`, naming its part; null if it isn't one. */
  addMedia(bytes: Uint8Array): Promise<string | null>;
  /** `row` and `col` are 1-based. */
  cellText(sheet: number, row: number, col: number): Promise<CellText>;
  /** The size of WASM's memory, for the speed checks. */
  memoryBytes(): Promise<number>;
}

/** The engine worker, as the tile worker sees it over their `MessageChannel`. */
export interface EngineTilePort {
  viewBlock(
    sheet: number,
    firstRow: number,
    firstCol: number,
    dark: boolean,
    background: number,
    text: number,
  ): Promise<ArrayBuffer>;
}

/** The tile worker, as the main thread sees it. */
export interface TileWorkerApi {
  attachEngine(port: MessagePort): void;
  draw(request: TileRequest): Promise<TileImage>;
  /** Each row's height, as [sheet, row, pixels]. */
  fitRows(rows: RowFit[], palette: TilePalette): Promise<[number, number, number][]>;
  /** Each column's width, as [column, pixels]. */
  fitColumns(cells: ColFit[], palette: TilePalette): Promise<[number, number][]>;
}
