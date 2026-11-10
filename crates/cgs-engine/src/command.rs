//! Everything that changes a workbook, and undoing it. Each change is an `Op`, and applying an `Op`
//! returns the `Op` that puts things back, holding pre-images of whatever it replaced. Undo applies
//! that, which gives back the `Op` for redo. Because undoing is just more changes, the same path
//! keeps track of what differs from the file, however the workbook got to where it is.

use std::collections::{BTreeMap, HashSet};

use cgs_calc::{
    edits_bytes, CalcEngine, CalcValue, CellEdit, CellInput, CellRef, FormulaShape, NameInfo,
    StructuralOp, TableInfo,
};
use cgs_model::{DefinedName, Rect, SheetInfo, SheetKind, SheetShape, SheetState, Table};
use cgs_refs::{sheet_key, Axis, AxisEdit};
use cgs_xlsx::patch_sheet::{ArrayKind, CellChange, CellPatch};
use cgs_xlsx::structure::BookOp;
use serde::Deserialize;

use crate::edit::{EditCell, EditInput};
use crate::format::{FormatChange, StylesBefore};
use crate::input::{parse_input, Format, Typed};
use crate::{Engine, Session};

/// Excel's default, and how much pre-image memory undo may hold.
const UNDO_STEPS: usize = 100;
const UNDO_BYTES: usize = 256 * 1024 * 1024;
/// Past this many cells on a sheet, a repaint asks for the whole sheet's tiles again.
const DIRTY_TILE_LIMIT: usize = 4096;

/// What a sheet's tiles and shape need from the UI after a change.
#[derive(Debug, Default)]
pub struct Dirty {
    /// Tiles by sheet and top-left cell, counted from 0.
    pub tiles: HashSet<(u32, u32, u32)>,
    /// Sheets to repaint in full, because something moved.
    pub sheets: HashSet<u32>,
    /// Sheets whose sizes or merges changed.
    pub layouts: HashSet<u32>,
    /// The list of sheets itself changed.
    pub book: bool,
}

impl Dirty {
    pub(crate) fn cell(&mut self, sheet: u32, row: u32, col: u32) {
        if self.sheets.contains(&sheet) {
            return;
        }
        let tile_row = (row - 1) / cgs_model::TILE_ROWS * cgs_model::TILE_ROWS;
        let tile_col = (col - 1) / cgs_model::TILE_COLS * cgs_model::TILE_COLS;
        self.tiles.insert((sheet, tile_row, tile_col));
        // Text can overflow into the next tile either way.
        if tile_col > 0 {
            self.tiles
                .insert((sheet, tile_row, tile_col - cgs_model::TILE_COLS));
        }
        self.tiles
            .insert((sheet, tile_row, tile_col + cgs_model::TILE_COLS));
        if self.tiles.len() > DIRTY_TILE_LIMIT {
            let sheets: HashSet<u32> = self.tiles.iter().map(|&(sheet, _, _)| sheet).collect();
            self.tiles.clear();
            self.sheets.extend(sheets);
        }
    }
}

/// An inclusive area of a sheet, 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Area {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}

impl Area {
    pub fn cell(row: u32, col: u32) -> Self {
        Self {
            top: row,
            left: col,
            bottom: row,
            right: col,
        }
    }

    pub fn rows(&self) -> u32 {
        self.bottom - self.top + 1
    }

    pub fn cols(&self) -> u32 {
        self.right - self.left + 1
    }

    fn contains(&self, row: u32, col: u32) -> bool {
        row >= self.top && row <= self.bottom && col >= self.left && col <= self.right
    }

    pub(crate) fn valid(&self) -> bool {
        self.top >= 1
            && self.left >= 1
            && self.top <= self.bottom
            && self.left <= self.right
            && self.bottom <= cgs_model::MAX_ROWS
            && self.right <= cgs_model::MAX_COLS
    }
}

/// What to paste from cells in this workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PasteMode {
    #[default]
    All,
    Values,
    Formulas,
    Formats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AxisName {
    Row,
    Col,
}

/// A change as the UI asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    /// Text typed into a cell, read as Excel reads it. `areas` fills them all with it.
    Input {
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        #[serde(default)]
        areas: Vec<Area>,
        /// Typed anyway, past a validation warning the user said yes to.
        #[serde(default)]
        force: bool,
    },
    /// `{=...}`.
    ArrayInput {
        sheet: u32,
        area: Area,
        text: String,
    },
    /// Empties cells, keeping their formats.
    Clear {
        sheet: u32,
        areas: Vec<Area>,
    },
    /// Drags a selection out over `to`, which contains it.
    Fill {
        sheet: u32,
        from: Area,
        to: Area,
        #[serde(default)]
        copy: bool,
    },
    /// Pastes cells of this workbook.
    Paste {
        #[serde(rename = "fromSheet")]
        from_sheet: u32,
        from: Area,
        sheet: u32,
        to: Area,
        #[serde(default)]
        mode: PasteMode,
    },
    /// Moves cells, and what points at them follows.
    Move {
        #[serde(rename = "fromSheet")]
        from_sheet: u32,
        from: Area,
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Pastes from another program: Excel's HTML if there is any, or else tab-separated text.
    PasteExternal {
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        #[serde(default)]
        html: Option<String>,
    },
    Axis {
        sheet: u32,
        axis: AxisName,
        at: u32,
        count: u32,
        delete: bool,
    },
    InsertSheet {
        /// Before this tab, counted from 0.
        position: u32,
        #[serde(default)]
        name: Option<String>,
        /// The default direction setting, for a sheet that starts right to left.
        #[serde(default, rename = "rightToLeft")]
        right_to_left: bool,
    },
    DeleteSheet {
        sheet: u32,
    },
    RenameSheet {
        sheet: u32,
        name: String,
    },
    /// Hides a sheet, or shows a hidden one. The last visible sheet can't be hidden.
    SheetState {
        sheet: u32,
        hidden: bool,
    },
    /// Normal view's zoom, in percent, which is saved with the file but isn't a change to undo.
    Zoom {
        sheet: u32,
        zoom: u32,
    },
    /// The sheet showing, which is saved with the file as Excel saves it but isn't a change.
    ActiveSheet {
        sheet: u32,
    },
    /// Protect Sheet: what people may still do, and a password, hashed with `salt` (base64).
    ProtectSheet {
        sheet: u32,
        #[serde(default)]
        allow: crate::protect::AllowSpec,
        #[serde(default)]
        password: String,
        #[serde(default)]
        salt: String,
    },
    UnprotectSheet {
        sheet: u32,
        #[serde(default)]
        password: String,
    },
    /// Protect Structure and Windows.
    ProtectWorkbook {
        #[serde(default = "yes")]
        structure: bool,
        #[serde(default)]
        password: String,
        #[serde(default)]
        salt: String,
    },
    UnprotectWorkbook {
        #[serde(default)]
        password: String,
    },
    /// Formats cells. `active` is the cell a relative change, such as more decimals, goes by.
    Format {
        sheet: u32,
        areas: Vec<Area>,
        change: Box<FormatChange>,
        #[serde(default)]
        active: Option<(u32, u32)>,
    },
    /// Sets the size of rows or columns, or hides or shows them.
    Lines {
        sheet: u32,
        axis: AxisName,
        first: u32,
        last: u32,
        #[serde(default)]
        size: Option<u32>,
        #[serde(default)]
        hidden: Option<bool>,
    },
    /// Rows go back to fitting what they hold.
    AutoFitRows {
        sheet: u32,
        first: u32,
        last: u32,
    },
    /// Columns take the widths that were measured for them.
    FitCols {
        sheet: u32,
        cols: Vec<(u32, u32)>,
    },
    /// Rows take the heights that were measured for them, as part of the last step. `quiet` says
    /// it's only for how a file looks on opening, which isn't a change to the file.
    FitRows {
        rows: Vec<(u32, u32, u32)>,
        #[serde(default)]
        quiet: bool,
    },
    /// Merges each area, or each row of it with `across`.
    Merge {
        sheet: u32,
        areas: Vec<Area>,
        #[serde(default)]
        center: bool,
        #[serde(default)]
        across: bool,
    },
    Unmerge {
        sheet: u32,
        areas: Vec<Area>,
    },
    /// Freezes rows and columns from a first row and column. None of either unfreezes.
    Freeze {
        sheet: u32,
        rows: u32,
        cols: u32,
        #[serde(default = "first_line")]
        top: u32,
        #[serde(default = "first_line")]
        left: u32,
    },
    /// Inserts cells, or deletes them with `delete`, shifting what's there down or else across.
    ShiftCells {
        sheet: u32,
        area: Area,
        down: bool,
        #[serde(default)]
        delete: bool,
    },
    /// Replaces what a search finds in one cell.
    ReplaceOne {
        query: crate::find::FindQuery,
        with: String,
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Replaces everywhere a search reaches.
    ReplaceAll {
        query: crate::find::FindQuery,
        with: String,
    },
    /// Changes page setup on these sheets.
    PageSetup {
        sheets: Vec<u32>,
        spec: crate::page::PageSpec,
    },
    /// Inserts or removes a manual page break where `row` and `col` start a page, or takes
    /// every manual break off.
    PageBreak {
        sheet: u32,
        #[serde(default)]
        row: Option<u32>,
        #[serde(default)]
        col: Option<u32>,
        action: crate::page::BreakAction,
    },
    /// Sets, adds to or clears the print area.
    PrintArea {
        sheet: u32,
        #[serde(default)]
        areas: Vec<Area>,
        action: crate::page::PrintAreaAction,
    },
    /// The rows and columns that repeat on every page, or none.
    PrintTitles {
        sheet: u32,
        #[serde(default)]
        rows: Option<(u32, u32)>,
        #[serde(default)]
        cols: Option<(u32, u32)>,
    },
    /// Shows or hides gridlines and headings.
    ViewFlags {
        sheet: u32,
        #[serde(default)]
        grid: Option<bool>,
        #[serde(default)]
        headings: Option<bool>,
        /// Sheet Right-to-Left: column A on the right.
        #[serde(default, rename = "rightToLeft")]
        right_to_left: Option<bool>,
    },
    /// Makes a table. No `headers` guesses them from the first row.
    CreateTable {
        sheet: u32,
        area: Area,
        #[serde(default)]
        headers: Option<bool>,
        #[serde(default)]
        style: Option<String>,
        /// Its name, or else the next free TableN.
        #[serde(default)]
        name: Option<String>,
    },
    TableOptions {
        table: String,
        #[serde(flatten)]
        options: crate::tables::TableOptions,
    },
    TotalsFunction {
        table: String,
        column: String,
        #[serde(default)]
        function: Option<String>,
    },
    RenameTable {
        table: String,
        name: String,
    },
    ResizeTable {
        table: String,
        area: Area,
    },
    ConvertToRange {
        table: String,
    },
    /// Sorts an area by up to 64 keys, top to bottom or with `byColumns` left to right.
    Sort {
        sheet: u32,
        area: Area,
        keys: Vec<crate::sort::SortKeySpec>,
        #[serde(default)]
        header: bool,
        #[serde(default, rename = "caseSensitive")]
        case_sensitive: bool,
        #[serde(default, rename = "byColumns")]
        by_columns: bool,
    },
    /// A cell style, Excel's or the book's own, by name.
    CellStyle {
        sheet: u32,
        areas: Vec<Area>,
        name: String,
    },
    /// New Cell Style: a style with the look of a cell, by example.
    NewCellStyle {
        name: String,
        sheet: u32,
        row: u32,
        col: u32,
        includes: crate::cell_styles::IncludesSpec,
    },
    /// Modify Cell Style: a style takes a format change, and its cells follow.
    ModifyCellStyle {
        name: String,
        change: Box<crate::format::FormatChange>,
        #[serde(default)]
        includes: Option<crate::cell_styles::IncludesSpec>,
    },
    /// Takes out rows whose `columns` show what an earlier row does.
    RemoveDuplicates {
        sheet: u32,
        area: Area,
        columns: Vec<u32>,
        #[serde(default)]
        header: bool,
    },
    /// Defines a name, or changes `old`.
    SetName {
        #[serde(default)]
        old: Option<crate::names::NameRef>,
        name: String,
        formula: String,
        #[serde(default)]
        sheet: Option<u32>,
    },
    DeleteName(crate::names::NameRef),
    /// Names from a selection's labels.
    NamesFromSelection {
        sheet: u32,
        area: Area,
        #[serde(default)]
        top: bool,
        #[serde(default)]
        left: bool,
        #[serde(default)]
        bottom: bool,
        #[serde(default)]
        right: bool,
    },
    /// A note on a cell, or none to delete it.
    SetNote {
        sheet: u32,
        row: u32,
        col: u32,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        author: Option<String>,
    },
    /// A hyperlink on cells, or none to remove links from them.
    SetLink {
        sheet: u32,
        area: Area,
        #[serde(default)]
        link: Option<crate::notes::LinkSpec>,
        /// With no link: Remove Hyperlink, which takes the link's look off too.
        #[serde(default)]
        plain: bool,
    },
    /// Text with runs of its own fonts, as the cell editor makes it.
    RichInput {
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        runs: Vec<crate::rich::RunSpec>,
    },
    /// Error checking's rules that are on, as `errors`' bits; 0 turns background checking off.
    /// It is Excel's own setting, not the file's, so it isn't a change.
    ErrorRules {
        rules: u16,
    },
    /// Ignore Error: the rules this cell breaks stay quiet for it, saved in `ignoredErrors`.
    IgnoreError {
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Convert to Number: text that reads as a number becomes the number.
    ConvertToNumber {
        sheet: u32,
        areas: Vec<Area>,
    },
    /// Update Formula to Include Cells: the formula's range grows over the numbers beside it.
    IncludeCells {
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Show/Hide Note.
    NoteVisible {
        sheet: u32,
        row: u32,
        col: u32,
        visible: bool,
    },
    /// A validation rule on these cells, or none to take rules off them.
    SetValidation {
        sheet: u32,
        areas: Vec<Area>,
        #[serde(default)]
        rule: Option<crate::dv::DvSpec>,
    },
    /// Circle Invalid Data on a sheet, or off.
    CircleInvalid {
        sheet: u32,
        on: bool,
    },
    /// A conditional format over these cells, above the sheet's other rules.
    AddCf {
        sheet: u32,
        areas: Vec<Area>,
        rule: crate::cf_edit::RuleSpec,
    },
    /// Rules taken off these cells, or off the whole sheet with none.
    ClearCf {
        sheet: u32,
        #[serde(default)]
        areas: Option<Vec<Area>>,
    },
    /// The rules manager's list, in priority order.
    SetCf {
        sheet: u32,
        rules: Vec<crate::cf_edit::ManagedRule>,
    },
    /// Filter buttons on over the block around a selection, or off, on a sheet or a table.
    ToggleFilter {
        sheet: u32,
        area: Area,
    },
    /// What one column keeps, or nothing, and the filter applied again.
    SetFilter {
        sheet: u32,
        #[serde(default)]
        table: Option<String>,
        col: u32,
        #[serde(default)]
        criteria: Option<crate::filter::FilterSpec>,
    },
    ClearFilter {
        sheet: u32,
        #[serde(default)]
        table: Option<String>,
    },
    ReapplyFilter {
        sheet: u32,
        #[serde(default)]
        table: Option<String>,
    },
    /// A drawing moved or sized, in CSS pixels at 100%.
    SetDrawingBox {
        sheet: u32,
        index: u32,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
    },
    DeleteDrawings {
        sheet: u32,
        indexes: Vec<u32>,
    },
    /// A chart of these cells, at a place in CSS pixels at 100%.
    InsertChart {
        sheet: u32,
        area: Area,
        chart: crate::chart_edit::ChartType,
        x: i64,
        y: i64,
    },
    /// A picture whose bytes `add_media` took, at its place and size in CSS pixels at 100%.
    InsertPicture {
        sheet: u32,
        part: String,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
        #[serde(default)]
        name: String,
    },
    EditChart {
        sheet: u32,
        index: u32,
        edit: crate::chart_edit::ChartEdit,
    },
    /// Move Chart, to another worksheet.
    /// The Picture Format tab: turn, flip, alternative text, and order among the drawings.
    PictureFormat {
        sheet: u32,
        index: u32,
        change: crate::chart_edit::PictureChange,
    },
    MoveDrawing {
        sheet: u32,
        index: u32,
        to: u32,
    },
    /// Refresh, or Refresh All without a pivot table.
    RefreshPivot {
        #[serde(default)]
        pivot: Option<crate::pivots::PivotRef>,
    },
    /// A pivot table rearranged, as its field list does it.
    PivotLayout {
        pivot: crate::pivots::PivotRef,
        layout: crate::pivots::LayoutSpec,
    },
    /// Options from the PivotTable Analyze and Design tabs.
    PivotOptions {
        pivot: crate::pivots::PivotRef,
        options: crate::pivots::OptionSpec,
    },
    /// One field's filter, sort, subtotals or details.
    PivotField {
        pivot: crate::pivots::PivotRef,
        field: u32,
        change: crate::pivots::FieldChange,
    },
    /// Insert › PivotTable.
    CreatePivot {
        sheet: u32,
        area: Area,
        to: crate::pivots::Destination,
    },
    DeletePivot {
        pivot: crate::pivots::PivotRef,
    },
    /// Group Field or Group Selection: dates by period, numbers by steps, or items by hand.
    GroupPivot {
        pivot: crate::pivots::PivotRef,
        field: u32,
        group: crate::pivot_group::GroupSpec,
    },
    UngroupPivot {
        pivot: crate::pivots::PivotRef,
        field: u32,
    },
    /// Show Details on a value: its records on a new sheet.
    PivotDetails {
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Show Report Filter Pages for one of a pivot table's report filters.
    PivotFilterPages {
        pivot: crate::pivots::PivotRef,
        field: u32,
    },
    Undo,
    Redo,
}

fn first_line() -> u32 {
    1
}

fn yes() -> bool {
    true
}

/// Cells and the rich strings they show.
type RichCells = Vec<((u32, u32), Option<u32>)>;

/// One change to the workbook, and what is kept to undo it.
#[derive(Debug, Clone)]
pub(crate) enum Op {
    Cells(Vec<EditCell>),
    Axis {
        sheet: u32,
        edit: AxisEdit,
    },
    /// Rows or columns coming back or going again, with what the model looked like then.
    AxisBack {
        sheet: u32,
        edit: AxisEdit,
        shape: Box<SheetShape>,
        names: Vec<DefinedName>,
        tables: Vec<Table>,
    },
    Names(Vec<NameInfo>),
    /// The engine's tables as they were.
    Tables(Vec<TableInfo>),
    /// The model's tables, which hold how each looks.
    BookTables(Vec<Table>),
    /// The workbook's defined names, as the model holds them.
    BookNames(Vec<DefinedName>),
    /// A sheet's notes.
    SheetNotes {
        sheet: u32,
        notes: std::collections::BTreeMap<(u32, u32), cgs_model::Note>,
    },
    /// A sheet's hyperlinks.
    SheetLinks {
        sheet: u32,
        links: Vec<cgs_model::Hyperlink>,
    },
    /// A sheet's drawings: their places, and which are there.
    SheetDrawings {
        sheet: u32,
        drawings: Vec<cgs_model::Drawing>,
    },
    /// Every chart part's model, as a change to rows, columns or sheets left them.
    BookCharts(Vec<cgs_model::ChartPart>),
    /// A chart part's model. At the end of the list, `Some` adds one and `None` takes it off.
    ChartPart {
        index: u32,
        part: Option<Box<cgs_model::ChartPart>>,
    },
    /// A sheet's data validation.
    SheetDv {
        sheet: u32,
        list: Vec<cgs_model::Validation>,
    },
    /// A sheet's conditional formats.
    SheetCf {
        sheet: u32,
        blocks: Vec<cgs_model::CfBlock>,
    },
    /// A sheet's own autofilter.
    SheetFilter {
        sheet: u32,
        filter: Option<cgs_model::AutoFilter>,
    },
    Merges {
        sheet: u32,
        merges: Vec<Rect>,
    },
    AddSheet {
        name: String,
        position: u32,
        right_to_left: bool,
    },
    RemoveSheet {
        sheet: u32,
    },
    RestoreSheet {
        sheet: u32,
        position: u32,
        cells: Vec<CellEdit>,
        readers: Vec<CellEdit>,
        names: Vec<NameInfo>,
        tables: Vec<TableInfo>,
    },
    Rename {
        sheet: u32,
        name: String,
    },
    /// A sheet's protection, or none.
    SheetProtection {
        sheet: u32,
        protection: Option<Box<cgs_model::SheetProtection>>,
    },
    /// The workbook's protection, or none.
    BookProtection(Option<cgs_model::WorkbookProtection>),
    /// A sheet's ignored errors.
    IgnoredErrors {
        sheet: u32,
        list: Vec<(Vec<Rect>, u16)>,
    },
    /// Whether a sheet shows, and which sheet is active with it.
    SheetState {
        sheet: u32,
        state: SheetState,
        active: u32,
    },
    /// A format to apply, which comes back as what it changed.
    Format {
        sheet: u32,
        areas: Vec<Area>,
        change: Box<FormatChange>,
        active: Option<(u32, u32)>,
    },
    /// What a format changed.
    Styles(Box<StylesBefore>),
    /// Rows' and columns' records to put in place.
    Lines {
        sheet: u32,
        rows: Vec<(u32, cgs_model::LineInfo)>,
        cols: Vec<(u32, u32, cgs_model::LineInfo)>,
    },
    MergeArea {
        sheet: u32,
        area: Area,
        center: bool,
    },
    /// Cells moved within a band, for the rest of the file to follow when it's saved.
    ShiftLog {
        sheet: u32,
        edit: AxisEdit,
    },
    /// What defined names read, with what they read before.
    NamesTo {
        names: Vec<NameInfo>,
        before: Vec<NameInfo>,
    },
    /// Which rich string cells show. A string coming back means the cell is the file's again.
    Rich {
        sheet: u32,
        cells: RichCells,
    },
    /// Rich strings pasted or moved into cells, after their contents: each cell shows the file's
    /// shared string, and saves as it.
    RichCopy {
        sheet: u32,
        cells: Vec<((u32, u32), u32)>,
    },
    /// A sheet's view to put in place. `freeze` says the panes are part of the change.
    View {
        sheet: u32,
        view: cgs_model::SheetView,
        freeze: bool,
    },
    /// The model's pivot tables and caches, and the records each cache holds now.
    BookPivots {
        tables: Vec<cgs_model::pivot::PivotTable>,
        caches: Vec<cgs_model::pivot::PivotCache>,
        records: Vec<Option<std::rc::Rc<cgs_model::pivot::CacheRecords>>>,
    },
    /// A sheet's page setup.
    PageSetup {
        sheet: u32,
        page: Box<cgs_model::PageSetup>,
    },
    Many(Vec<Op>),
}

impl Op {
    fn bytes(&self) -> usize {
        match self {
            Self::Cells(edits) => edits.iter().map(EditCell::bytes).sum(),
            Self::Axis { .. }
            | Self::RemoveSheet { .. }
            | Self::Rename { .. }
            | Self::SheetState { .. }
            | Self::SheetProtection { .. }
            | Self::BookProtection(_) => 256,
            Self::RichCopy { cells, .. } => 64 + cells.len() * 16,
            Self::IgnoredErrors { list, .. } => 64 + list.len() * 64,
            Self::AxisBack {
                shape,
                names,
                tables,
                ..
            } => shape.bytes() + names.len() * 128 + tables.len() * 256,
            Self::Names(names) => names.len() * 128,
            Self::Tables(tables) => tables.len() * 256,
            Self::BookTables(tables) => tables.len() * 512,
            Self::BookPivots { tables, caches, .. } => {
                1024 + tables.len() * 4096
                    + caches
                        .iter()
                        .map(|c| {
                            c.fields
                                .iter()
                                .map(|f| 64 + f.shared.len() * 32)
                                .sum::<usize>()
                        })
                        .sum::<usize>()
            }
            Self::SheetFilter { .. } => 512,
            Self::SheetCf { blocks, .. } => 256 + blocks.len() * 512,
            Self::SheetDv { list, .. } => 256 + list.len() * 512,
            Self::SheetNotes { notes, .. } => {
                256 + notes.values().map(|n| n.text.len() + 64).sum::<usize>()
            }
            Self::SheetLinks { links, .. } => 256 + links.len() * 256,
            Self::BookNames(names) => names.len() * 128,
            Self::Merges { merges, .. } => merges.len() * 16,
            Self::PageSetup { page, .. } => {
                1024 + (page.row_breaks.len() + page.col_breaks.len()) * 16
            }
            Self::Format { .. } | Self::MergeArea { .. } | Self::View { .. } => 256,
            Self::Styles(before) => before.bytes(),
            Self::Lines { rows, cols, .. } => 64 + rows.len() * 24 + cols.len() * 32,
            Self::SheetDrawings { drawings, .. } => 64 + drawings.len() * 160,
            Self::BookCharts(charts) => 64 + charts.len() * 2048,
            Self::ChartPart { part, .. } => {
                512 + part.as_ref().map_or(0, |part| match &part.body {
                    cgs_model::ChartBody::Classic(chart) => chart
                        .series()
                        .map(|s| {
                            64 + s.values.as_ref().map_or(0, |v| v.cache.len() * 24)
                                + s.categories.as_ref().map_or(0, |v| v.cache.len() * 24)
                        })
                        .sum::<usize>(),
                    _ => 0,
                })
            }
            Self::Rich { cells, .. } => 32 + cells.len() * 16,
            Self::ShiftLog { .. } => 64,
            Self::NamesTo { names, before } => (names.len() + before.len()) * 128,
            Self::AddSheet { name, .. } => 64 + name.len(),
            Self::RestoreSheet {
                cells,
                readers,
                names,
                tables,
                ..
            } => edits_bytes(cells) + edits_bytes(readers) + (names.len() + tables.len()) * 128,
            Self::Many(list) => list.iter().map(Self::bytes).sum(),
        }
    }
}

struct Step {
    label: &'static str,
    /// What gets things back to `before`, or forward to `after` once undone.
    op: Op,
    bytes: usize,
    before: u64,
    after: u64,
    selection: Option<(u32, Area)>,
    /// How long the list of shape changes was before this step, and against which file. Undoing
    /// takes this step's changes back off the list, so that the file's own rows and sheets stand.
    log_before: usize,
    log_file: u64,
}

/// Undo and redo, and which state of the workbook is the file's.
#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    bytes: usize,
    pub(crate) state: u64,
    pub(crate) clean: u64,
    next_state: u64,
    /// The last change couldn't be kept, so there is no going back past it.
    lost: bool,
    /// Which file the list of shape changes is against, which changes with each save.
    file: u64,
}

impl History {
    pub(crate) fn touch(&mut self) {
        self.next_state += 1;
        self.state = self.next_state;
    }

    pub(crate) fn at_clean(&self) -> bool {
        self.state == self.clean
    }

    /// Nothing to undo or redo any more, as after protecting a sheet in Excel.
    pub(crate) fn forget(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }

    pub(crate) fn mark_clean(&mut self) {
        self.clean = self.state;
        self.file += 1;
    }

    fn push(
        &mut self,
        label: &'static str,
        op: Op,
        before: u64,
        selection: Option<(u32, Area)>,
        log_before: usize,
    ) {
        let bytes = op.bytes();
        for step in self.redo.drain(..) {
            self.bytes -= step.bytes;
        }
        self.bytes += bytes;
        self.undo.push(Step {
            label,
            op,
            bytes,
            before,
            after: self.state,
            selection,
            log_before,
            log_file: self.file,
        });
        self.lost = false;
        while self.undo.len() > UNDO_STEPS || self.bytes > UNDO_BYTES {
            let Some(oldest) = (!self.undo.is_empty()).then(|| self.undo.remove(0)) else {
                break;
            };
            self.bytes -= oldest.bytes;
            self.lost = self.undo.is_empty();
        }
    }
}

/// What a command did, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// The cells to select afterwards.
    pub selection: Option<(u32, Area)>,
    /// How many cells a replace changed.
    pub replaced: u32,
}

/// The area a paste of `from` into `to` writes: a destination that holds the source a whole
/// number of times gets it that many times, and any other gets it once from its top left, however
/// small the destination was. Protection and the ERP guard check this area, not `to`.
pub(crate) fn paste_area(from: Area, to: Area) -> Area {
    let fits = to.cols().is_multiple_of(from.cols()) && to.rows().is_multiple_of(from.rows());
    let (across, down) = if fits {
        (
            (to.cols() / from.cols()).max(1),
            (to.rows() / from.rows()).max(1),
        )
    } else {
        (1, 1)
    };
    Area {
        top: to.top,
        left: to.left,
        bottom: to.top + from.rows() * down - 1,
        right: to.left + from.cols() * across - 1,
    }
}

fn not_open() -> String {
    "No workbook is open.".to_string()
}

fn number_only(sheet: &SheetInfo) -> Result<(), String> {
    if sheet.deleted {
        return Err("That sheet has been deleted.".to_string());
    }
    if sheet.kind != SheetKind::Worksheet || !sheet.loaded {
        return Err(format!("Sheet {} has no cells to edit.", sheet.name));
    }
    Ok(())
}

/// Whether a name can be a sheet's, as Excel rules it.
pub(crate) fn sheet_name_problem(
    book: &cgs_model::Book,
    name: &str,
    except: Option<u32>,
) -> Option<String> {
    if name.is_empty() {
        return Some("A sheet needs a name.".to_string());
    }
    if name.chars().count() > 31 {
        return Some("A sheet's name can't be longer than 31 characters.".to_string());
    }
    if name.contains(['\\', '/', '?', '*', '[', ']', ':']) {
        return Some("A sheet's name can't contain \\ / ? * [ ] or :.".to_string());
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Some("A sheet's name can't start or end with an apostrophe.".to_string());
    }
    if name.eq_ignore_ascii_case("History") {
        return Some("\"History\" is a name Excel keeps for itself.".to_string());
    }
    let key = sheet_key(name);
    let taken = book.sheets.iter().enumerate().any(|(index, sheet)| {
        !sheet.deleted && Some(index as u32) != except && sheet_key(&sheet.name) == key
    });
    taken.then(|| "That name is already taken.".to_string())
}

/// A sheet's next free default name.
pub(crate) fn next_sheet_name(book: &cgs_model::Book) -> String {
    let mut number = book.sheets.len() + 1;
    loop {
        let name = format!("Sheet{number}");
        if sheet_name_problem(book, &name, None).is_none() {
            return name;
        }
        number += 1;
    }
}

impl Session {
    /// A formula's new text for a cell that already has one. An array keeps its area, or Excel's
    /// `{=...}` would become a single formula.
    pub(crate) fn rewritten(&self, at: CellRef, text: String) -> EditInput {
        match self.calc.cell_input(at) {
            CellInput::Formula {
                shape: shape @ (FormulaShape::Array(_) | FormulaShape::Dynamic(_)),
                ..
            } => EditInput::Array(text, shape),
            _ => EditInput::Formula(text),
        }
    }
}

/// The areas a fill writes or empties: `to` beyond `from`, or `from` beyond `to` when dragged back.
fn fill_written(from: Area, to: Area) -> impl Iterator<Item = Area> {
    let (outer, inner) = if to.top <= from.top
        && to.left <= from.left
        && to.bottom >= from.bottom
        && to.right >= from.right
    {
        (to, from)
    } else {
        (from, to)
    };
    let mut areas = Vec::new();
    if outer.top < inner.top {
        areas.push(Area {
            bottom: inner.top - 1,
            ..outer
        });
    }
    if outer.bottom > inner.bottom {
        areas.push(Area {
            top: inner.bottom + 1,
            ..outer
        });
    }
    if outer.left < inner.left {
        areas.push(Area {
            right: inner.left - 1,
            ..outer
        });
    }
    if outer.right > inner.right {
        areas.push(Area {
            left: inner.right + 1,
            ..outer
        });
    }
    areas.into_iter()
}

/// `input` as a command list wants it.
fn to_edit_input(input: &CellInput) -> Option<EditInput> {
    Some(match input {
        CellInput::Value(CalcValue::Empty) | CellInput::Clear => EditInput::Clear,
        CellInput::Value(value) => EditInput::Value(value.clone()),
        CellInput::Formula { text, shape, .. } => match shape {
            FormulaShape::Normal => EditInput::Formula(text.clone()),
            shape => EditInput::Array(text.clone(), *shape),
        },
        CellInput::Spill { .. } => return None,
    })
}

impl Session {
    pub(crate) fn absorb_results(&mut self) {
        for cell in self.calc.take_changed() {
            self.note_result(cell);
        }
    }

    pub(crate) fn note_result(&mut self, cell: CellRef) {
        let Some(info) = self.book.sheets.get(cell.sheet as usize) else {
            return;
        };
        if info.deleted {
            return;
        }
        self.dirty.cell(cell.sheet, cell.row, cell.col);
        let patches = self.edits.cells.entry(cell.sheet).or_default();
        let patch = patches.entry((cell.row, cell.col)).or_insert(CellPatch {
            change: CellChange::Keep,
            style: None,
        });
        if patch.change == CellChange::Keep {
            patch.change = CellChange::Result(CalcValue::Empty);
        }
    }

    /// Whether `area` holds part of a `{=...}` array, without all of it.
    pub(crate) fn cuts_an_array(&self, sheet: u32, area: Area) -> bool {
        self.calc.spill_areas().iter().any(|spill| {
            if spill.sheet != sheet {
                return false;
            }
            let anchor = CellRef {
                sheet,
                row: spill.rect.top,
                col: spill.rect.left,
            };
            let legacy = matches!(self.calc.formula_shape(anchor), FormulaShape::Array(_));
            let touches = spill.rect.top <= area.bottom
                && spill.rect.bottom >= area.top
                && spill.rect.left <= area.right
                && spill.rect.right >= area.left;
            let whole = area.top <= spill.rect.top
                && area.bottom >= spill.rect.bottom
                && area.left <= spill.rect.left
                && area.right >= spill.rect.right;
            legacy && touches && !whole
        })
    }

    pub(crate) fn apply_cells(&mut self, edits: Vec<EditCell>) -> Result<Op, String> {
        for edit in &edits {
            let sheet = self
                .book
                .sheets
                .get(edit.sheet as usize)
                .ok_or_else(|| format!("There is no sheet {}.", edit.sheet))?;
            if sheet.deleted {
                return Err("That sheet has been deleted.".to_string());
            }
            if sheet.kind != SheetKind::Worksheet {
                return Err(format!("Sheet {} has no cells to edit.", sheet.name));
            }
            if !(1..=cgs_model::MAX_ROWS).contains(&edit.row)
                || !(1..=cgs_model::MAX_COLS).contains(&edit.col)
            {
                return Err("That cell is off the sheet.".to_string());
            }
        }
        // ERP records keep their keys: every cell write passes the identity guard.
        self.guard_cells(&edits)?;
        let mut inputs = Vec::with_capacity(edits.len());
        let mut before_styles = Vec::with_capacity(edits.len());
        for edit in &edits {
            let style = edit
                .style
                .map(|_| self.book.sheets[edit.sheet as usize].style_at(edit.row, edit.col));
            before_styles.push(style);
            if let Some(input) = &edit.input {
                let input = match input {
                    EditInput::Value(CalcValue::Empty) | EditInput::Clear => CellInput::Clear,
                    EditInput::Value(value) => CellInput::Value(value.clone()),
                    EditInput::Formula(text) => CellInput::Formula {
                        text: text.clone(),
                        shape: FormulaShape::Normal,
                        cached: None,
                    },
                    EditInput::Array(text, shape) => CellInput::Formula {
                        text: text.clone(),
                        shape: *shape,
                        cached: None,
                    },
                };
                inputs.push(CellEdit {
                    at: CellRef {
                        sheet: edit.sheet,
                        row: edit.row,
                        col: edit.col,
                    },
                    input,
                });
            }
        }
        let mut previous = if inputs.is_empty() {
            Vec::new()
        } else {
            self.calc.set_cells(&inputs).map_err(|error| error.0)?
        }
        .into_iter();
        // New contents take a rich string's look away.
        let mut was_rich: BTreeMap<u32, RichCells> = BTreeMap::new();
        for edit in edits.iter().filter(|edit| edit.input.is_some()) {
            let info = &mut self.book.sheets[edit.sheet as usize];
            if !info.rich.is_empty() {
                if let Some(index) = info.rich.remove(&(edit.row, edit.col)) {
                    was_rich
                        .entry(edit.sheet)
                        .or_default()
                        .push(((edit.row, edit.col), Some(index)));
                }
            }
        }
        let mut undo = Vec::with_capacity(edits.len());
        for (edit, style) in edits.iter().zip(before_styles) {
            let input = if edit.input.is_some() {
                previous
                    .next()
                    .and_then(|before| to_edit_input(&before.input))
            } else {
                None
            };
            undo.push(EditCell {
                sheet: edit.sheet,
                row: edit.row,
                col: edit.col,
                input,
                style,
            });
            self.note_cell_patch(edit);
        }
        undo.reverse();
        if was_rich.is_empty() {
            return Ok(Op::Cells(undo));
        }
        let mut list = vec![Op::Cells(undo)];
        list.extend(
            was_rich
                .into_iter()
                .map(|(sheet, cells)| Op::Rich { sheet, cells }),
        );
        Ok(Op::Many(list))
    }

    fn note_cell_patch(&mut self, edit: &EditCell) {
        let patches = self.edits.cells.entry(edit.sheet).or_default();
        let patch = patches.entry((edit.row, edit.col)).or_insert(CellPatch {
            change: CellChange::Keep,
            style: None,
        });
        match &edit.input {
            None => {}
            Some(EditInput::Clear | EditInput::Value(CalcValue::Empty)) => {
                patch.change = CellChange::Clear;
            }
            Some(EditInput::Value(value)) => patch.change = CellChange::Value(value.clone()),
            // The text and the result come from the engine when saving.
            Some(EditInput::Formula(_)) => {
                patch.change = CellChange::Formula {
                    text: String::new(),
                    value: CalcValue::Empty,
                    array: ArrayKind::None,
                };
            }
            Some(EditInput::Array(_, shape)) => {
                let array = match shape {
                    FormulaShape::Array(rect) => ArrayKind::Legacy(Rect {
                        top: rect.top,
                        left: rect.left,
                        bottom: rect.bottom,
                        right: rect.right,
                    }),
                    _ => ArrayKind::None,
                };
                patch.change = CellChange::Formula {
                    text: String::new(),
                    value: CalcValue::Empty,
                    array,
                };
            }
        }
        if edit.style.is_some() {
            patch.style = edit.style;
        }
        if edit.input.is_some() || edit.style.is_some() {
            self.touch_row(edit.sheet, edit.row);
        }
        let info = &mut self.book.sheets[edit.sheet as usize];
        if let Some(style) = edit.style {
            info.set_style(edit.row, edit.col, style);
        }
        if !matches!(
            edit.input,
            None | Some(EditInput::Clear | EditInput::Value(CalcValue::Empty))
        ) || edit.style.is_some()
        {
            info.used.0 = info.used.0.max(edit.row);
            info.used.1 = info.used.1.max(edit.col);
        }
        self.dirty.cell(edit.sheet, edit.row, edit.col);
    }

    /// Moves a sheet's patches to their new places.
    fn move_patches(&mut self, sheet: u32, edit: &AxisEdit) {
        if let Some(areas) = self.edits.style_areas.get_mut(&sheet) {
            *areas = areas
                .iter()
                .filter_map(|area| {
                    let (top, left, bottom, right) =
                        cgs_refs::edit_rect((area.top, area.left, area.bottom, area.right), edit)?;
                    Some(Rect {
                        top,
                        left,
                        bottom,
                        right,
                    })
                })
                .collect();
        }
        let lines = match edit.axis {
            Axis::Row => self.edits.row_lines.get_mut(&sheet),
            Axis::Col => self.edits.col_lines.get_mut(&sheet),
        };
        if let Some(lines) = lines {
            *lines = lines.iter().filter_map(|&span| edit.span(span)).collect();
        }
        let Some(patches) = self.edits.cells.get_mut(&sheet) else {
            return;
        };
        let moved: BTreeMap<(u32, u32), CellPatch> = std::mem::take(patches)
            .into_iter()
            .filter_map(|((row, col), patch)| {
                let at = match edit.axis {
                    Axis::Row => (edit.point(row)?, col),
                    Axis::Col => (row, edit.point(col)?),
                };
                Some((at, patch))
            })
            .collect();
        *patches = moved;
    }

    /// The charts as they are, before an op that may rewrite what their series read.
    fn charts_snapshot(&self, op: &Op) -> Option<Vec<cgs_model::ChartPart>> {
        fn rewrites(op: &Op) -> bool {
            match op {
                Op::Axis { .. }
                | Op::RemoveSheet { .. }
                | Op::Rename { .. }
                | Op::ShiftLog { .. } => true,
                Op::Many(list) => list.iter().any(rewrites),
                _ => false,
            }
        }
        (!self.book.charts.is_empty() && rewrites(op)).then(|| self.book.charts.clone())
    }

    fn with_charts(&self, inverse: Op, before: Option<Vec<cgs_model::ChartPart>>) -> Op {
        match before {
            Some(before) if before != self.book.charts => {
                Op::Many(vec![inverse, Op::BookCharts(before)])
            }
            _ => inverse,
        }
    }

    /// A chart part changed: the sheets showing it redraw, and save writes it.
    pub(crate) fn mark_chart(&mut self, index: u32) {
        if let Some(part) = self.book.charts.get_mut(index as usize) {
            part.changed = true;
        }
        for (sheet, info) in self.book.sheets.iter().enumerate() {
            if info.drawings.iter().any(|d| d.chart == Some(index)) {
                self.dirty.layouts.insert(sheet as u32);
                self.edits.drawing_sheets.insert(sheet as u32);
            }
        }
    }

    /// Rewrites what every chart's series read, and marks the charts it changed.
    pub(crate) fn rewrite_chart_formulas(&mut self, change: &dyn Fn(&str) -> Option<String>) {
        let mut changed = Vec::new();
        for (index, part) in self.book.charts.iter_mut().enumerate() {
            let cgs_model::ChartBody::Classic(chart) = &mut part.body else {
                continue;
            };
            if crate::chart_edit::rewrite_formulas(chart, change) {
                changed.push(index as u32);
            }
        }
        for index in changed {
            self.mark_chart(index);
        }
    }

    /// Everything but the engine, which `calc.structural` has already moved.
    fn move_model(&mut self, sheet: u32, edit: &AxisEdit) {
        let name = self.book.sheets[sheet as usize].name.clone();
        {
            let name = name.clone();
            let edit = *edit;
            self.rewrite_chart_formulas(&move |text| match cgs_refs::edit_formula(
                text, "", &name, &edit,
            ) {
                std::borrow::Cow::Owned(next) => Some(next),
                std::borrow::Cow::Borrowed(_) => None,
            });
        }
        self.book.sheets[sheet as usize].apply_axis(edit);
        self.move_patches(sheet, edit);
        self.move_pivots(sheet, edit);
        for defined in &mut self.book.names {
            let home = defined
                .local_sheet
                .map_or("", |index| self.book.sheets[index as usize].name.as_str());
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::edit_formula(&defined.formula, home, &name, edit)
            {
                defined.formula = next;
            }
        }
        self.book.tables.retain_mut(|table| {
            if table.sheet != sheet {
                return true;
            }
            let area = (
                table.range.top,
                table.range.left,
                table.range.bottom,
                table.range.right,
            );
            match cgs_refs::edit_rect(area, edit) {
                Some((top, left, bottom, right)) => {
                    table.range = Rect {
                        top,
                        left,
                        bottom,
                        right,
                    };
                    true
                }
                None => false,
            }
        });
        self.edits.ops.push(BookOp::Axis {
            sheet,
            name,
            edit: *edit,
        });
        self.dirty.sheets.insert(sheet);
        self.dirty.layouts.insert(sheet);
        self.dirty.tiles.retain(|&(on, _, _)| on != sheet);
    }

    /// Cells new rows or columns inherit a format for, as blank styled cells in the file.
    fn inherit_styles(&mut self, sheet: u32, edit: &AxisEdit) {
        if edit.delete || edit.at == 1 {
            return;
        }
        let info = &self.book.sheets[sheet as usize];
        let (mut styled, axis) = match edit.axis {
            Axis::Row => (info.own_styles_in_row(edit.at), Axis::Row),
            Axis::Col => (info.own_styles_in_col(edit.at), Axis::Col),
        };
        // A column styled all the way down would take a patch per row, which isn't worth it.
        if styled.len() > 200_000 {
            styled.clear();
        }
        let patches = self.edits.cells.entry(sheet).or_default();
        for step in 0..edit.count {
            for &(other, style) in &styled {
                let at = match axis {
                    Axis::Row => (edit.at + step, other),
                    Axis::Col => (other, edit.at + step),
                };
                if style != 0 {
                    patches.entry(at).or_insert(CellPatch {
                        change: CellChange::Keep,
                        style: Some(style),
                    });
                }
            }
        }
    }

    fn apply_axis(
        &mut self,
        sheet: u32,
        edit: AxisEdit,
        back: Option<(SheetShape, Vec<DefinedName>, Vec<Table>)>,
    ) -> Result<Op, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .ok_or_else(|| format!("There is no sheet {sheet}."))?;
        number_only(info)?;
        if edit.count == 0 || edit.at == 0 || edit.at > edit.axis.max() {
            return Err("That's off the sheet.".to_string());
        }
        // Excel refuses to split a `{=...}` array this way.
        let area = match edit.axis {
            Axis::Row => Area {
                top: edit.at,
                left: 1,
                bottom: (edit.at + edit.count - 1).min(edit.axis.max()),
                right: cgs_model::MAX_COLS,
            },
            Axis::Col => Area {
                top: 1,
                left: edit.at,
                bottom: cgs_model::MAX_ROWS,
                right: (edit.at + edit.count - 1).min(edit.axis.max()),
            },
        };
        if back.is_none() {
            self.guard_axis(sheet, &edit)?;
        }
        if back.is_none() && self.array_blocks(sheet, area, &edit) {
            return Err("You can't change part of an array.".to_string());
        }
        // A table can't lose its headings and keep its data.
        if back.is_none() && edit.delete && edit.axis == Axis::Row {
            let band = (edit.at, edit.at + edit.count - 1);
            let splits = self.calc.tables().iter().any(|table| {
                table.sheet == sheet
                    && table.header_row
                    && table.range.top >= band.0
                    && table.range.top <= band.1
                    && table.range.bottom > band.1
            });
            if splits {
                return Err("This would delete a table's headings and not its data.".to_string());
            }
        }
        let shape = info.shape();
        let names_before = self.book.names.clone();
        let tables_before = self.book.tables.clone();
        // Styled blanks in what a delete takes come back as patches.
        let mut styled = Vec::new();
        if edit.delete {
            let info = &self.book.sheets[sheet as usize];
            match edit.axis {
                Axis::Row => {
                    for row in edit.at..edit.at + edit.count {
                        if row > info.used.0 {
                            break;
                        }
                        for (col, style) in info.own_styles_in_row(row) {
                            styled.push((row, col, style));
                        }
                    }
                }
                Axis::Col => {
                    for col in edit.at..edit.at + edit.count {
                        if col > info.used.1 {
                            break;
                        }
                        for (row, style) in info.own_styles_in_col(col) {
                            styled.push((row, col, style));
                        }
                    }
                }
            }
        }
        let undo = self
            .calc
            .structural(&StructuralOp::new(sheet, edit))
            .map_err(|error| error.0)?;
        self.move_model(sheet, &edit);
        if !undo.gone_tables.is_empty() {
            self.edits.ops.push(BookOp::DeleteTables {
                names: undo.gone_tables.clone(),
            });
        }
        for (table, columns) in &undo.gone_columns {
            self.edits.ops.push(BookOp::DeleteTableColumns {
                table: table.clone(),
                columns: columns.clone(),
            });
        }
        // A column an insert puts inside a table gets a heading.
        let heading_edits: Vec<EditCell> = undo
            .new_headers
            .iter()
            .map(|(at, name)| EditCell {
                sheet: at.sheet,
                row: at.row,
                col: at.col,
                input: Some(EditInput::Value(CalcValue::Text(name.clone()))),
                style: None,
            })
            .collect();
        if !heading_edits.is_empty() {
            self.apply_cells(heading_edits)?;
        }
        match back {
            Some((shape, names, tables)) => {
                self.book.sheets[sheet as usize].restore_shape(shape);
                self.book.names = names;
                self.book.tables = tables;
            }
            None => self.inherit_styles(sheet, &edit),
        }
        // What was in the band, and formulas changed by more than position, as they were.
        let mut held: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        for removed in &undo.removed {
            if let Some(input) = to_edit_input(&removed.input) {
                held.insert(
                    (removed.at.sheet, removed.at.row, removed.at.col),
                    EditCell {
                        sheet: removed.at.sheet,
                        row: removed.at.row,
                        col: removed.at.col,
                        input: Some(input),
                        style: None,
                    },
                );
            }
        }
        for (row, col, style) in styled {
            let entry = held.entry((sheet, row, col)).or_insert(EditCell {
                sheet,
                row,
                col,
                input: None,
                style: None,
            });
            entry.style = Some(style);
        }
        let mut list = vec![Op::AxisBack {
            sheet,
            edit: edit.inverse(),
            shape: Box::new(shape),
            names: names_before,
            tables: tables_before,
        }];
        if !held.is_empty() {
            list.push(Op::Cells(held.into_values().collect()));
        }
        if !undo.names.is_empty() {
            list.push(Op::Names(undo.names));
        }
        list.push(Op::Tables(undo.tables));
        Ok(Op::Many(list))
    }

    /// Whether a change to rows or columns would split a `{=...}` array. Inserting exactly at its
    /// edge doesn't.
    fn array_blocks(&self, sheet: u32, area: Area, edit: &AxisEdit) -> bool {
        self.calc.spill_areas().iter().any(|spill| {
            if spill.sheet != sheet {
                return false;
            }
            let anchor = CellRef {
                sheet,
                row: spill.rect.top,
                col: spill.rect.left,
            };
            if !matches!(self.calc.formula_shape(anchor), FormulaShape::Array(_)) {
                return false;
            }
            let (first, last, from, to) = match edit.axis {
                Axis::Row => (spill.rect.top, spill.rect.bottom, area.top, area.bottom),
                Axis::Col => (spill.rect.left, spill.rect.right, area.left, area.right),
            };
            if edit.delete {
                from <= last && to >= first && !(from <= first && to >= last)
            } else {
                edit.at > first && edit.at <= last
            }
        })
    }

    pub(crate) fn apply_op(&mut self, op: Op) -> Result<Op, String> {
        match op {
            Op::Cells(edits) => self.apply_cells(edits),
            Op::Axis { sheet, edit } => self.apply_axis(sheet, edit, None),
            Op::AxisBack {
                sheet,
                edit,
                shape,
                names,
                tables,
            } => self.apply_axis(sheet, edit, Some((*shape, names, tables))),
            Op::Names(names) => {
                self.calc.restore_names(&names).map_err(|error| error.0)?;
                // Their positions come back through the inverse of the edit that moved them.
                Ok(Op::Names(names))
            }
            Op::Tables(tables) => {
                self.guard_calc_tables(&tables)?;
                let before = self.calc.tables().to_vec();
                self.calc.restore_tables(&tables).map_err(|error| error.0)?;
                for table in &tables {
                    if let Some(entry) = self
                        .book
                        .tables
                        .iter_mut()
                        .find(|entry| entry.name == table.name)
                    {
                        entry.columns = table.columns.clone();
                    }
                }
                Ok(Op::Tables(before))
            }
            Op::BookNames(names) => self.apply_names(names),
            Op::SheetNotes { sheet, notes } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.notes, notes);
                self.edits.notes_sheets.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::SheetNotes {
                    sheet,
                    notes: before,
                })
            }
            Op::SheetDrawings { sheet, drawings } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.drawings, drawings);
                self.edits.drawing_sheets.insert(sheet);
                self.dirty.layouts.insert(sheet);
                Ok(Op::SheetDrawings {
                    sheet,
                    drawings: before,
                })
            }
            Op::BookCharts(charts) => {
                let before = std::mem::replace(&mut self.book.charts, charts);
                let changed: Vec<u32> = before
                    .iter()
                    .zip(&self.book.charts)
                    .enumerate()
                    .filter(|(_, (old, new))| old != new)
                    .map(|(index, _)| index as u32)
                    .collect();
                for index in changed {
                    self.mark_chart(index);
                }
                Ok(Op::BookCharts(before))
            }
            Op::ChartPart { index, part } => {
                let at = index as usize;
                let before = match part {
                    Some(part) if at == self.book.charts.len() => {
                        self.book.charts.push(*part);
                        None
                    }
                    Some(part) => {
                        let slot = self
                            .book
                            .charts
                            .get_mut(at)
                            .ok_or_else(|| "That chart can't be found.".to_string())?;
                        Some(Box::new(std::mem::replace(slot, *part)))
                    }
                    None => {
                        if at + 1 != self.book.charts.len() {
                            return Err("That chart can't be taken off.".to_string());
                        }
                        self.book.charts.pop().map(Box::new)
                    }
                };
                self.mark_chart(index);
                Ok(Op::ChartPart {
                    index,
                    part: before,
                })
            }
            Op::SheetLinks { sheet, links } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.links, links);
                self.edits.link_sheets.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::SheetLinks {
                    sheet,
                    links: before,
                })
            }
            Op::SheetDv { sheet, list } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.validations, list);
                self.edits.dv_sheets.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::SheetDv {
                    sheet,
                    list: before,
                })
            }
            Op::SheetCf { sheet, blocks } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.cf, blocks);
                self.edits.cf_sheets.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::SheetCf {
                    sheet,
                    blocks: before,
                })
            }
            Op::SheetFilter { sheet, filter } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.filter, filter);
                self.edits.filters.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::SheetFilter {
                    sheet,
                    filter: before,
                })
            }
            Op::BookPivots {
                tables,
                caches,
                records,
            } => Ok(self.apply_book_pivots(tables, caches, records)),
            Op::BookTables(tables) => {
                self.guard_book_tables(&tables)?;
                for table in tables.iter().chain(self.book.tables.iter()) {
                    self.dirty.sheets.insert(table.sheet);
                }
                self.edits.tables_changed = true;
                Ok(Op::BookTables(std::mem::replace(
                    &mut self.book.tables,
                    tables,
                )))
            }
            Op::Merges { sheet, merges } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.merges, merges);
                info.index_merges();
                self.edits.merges.insert(sheet);
                self.dirty.sheets.insert(sheet);
                self.dirty.layouts.insert(sheet);
                Ok(Op::Merges {
                    sheet,
                    merges: before,
                })
            }
            Op::AddSheet {
                name,
                position,
                right_to_left,
            } => {
                let sheet = self.calc.add_sheet(&name).map_err(|error| error.0)?;
                let first = self.book.sheets.first();
                let mut info =
                    SheetInfo::new(name, SheetState::Visible, SheetKind::Worksheet, None);
                info.rows.default_px = first.map_or(20, |sheet| sheet.rows.default_px);
                info.cols.default_px = first.map_or(64, |sheet| sheet.cols.default_px);
                info.loaded = true;
                if right_to_left {
                    info.view.right_to_left = true;
                    self.edits.view_flags.insert(sheet);
                }
                let mut order = self.book.tab_order();
                order.insert((position as usize).min(order.len()), sheet);
                self.book.sheets.push(info);
                self.book.order = order;
                self.edits.ops.push(BookOp::Insert { sheet });
                self.dirty.book = true;
                Ok(Op::RemoveSheet { sheet })
            }
            Op::RemoveSheet { sheet } => {
                self.guard_sheet(sheet)?;
                let position = self
                    .book
                    .tab_order()
                    .iter()
                    .position(|&other| other == sheet)
                    .unwrap_or(0) as u32;
                let undo = self.calc.delete_sheet(sheet).map_err(|error| error.0)?;
                let name = self.book.sheets[sheet as usize].name.clone();
                {
                    let name = name.clone();
                    self.rewrite_chart_formulas(
                        &move |text| match cgs_refs::delete_sheet_in_formula(text, &name) {
                            std::borrow::Cow::Owned(next) => Some(next),
                            std::borrow::Cow::Borrowed(_) => None,
                        },
                    );
                }
                let mut order = self.book.tab_order();
                order.retain(|&other| other != sheet);
                self.book.order = order;
                self.book.sheets[sheet as usize].deleted = true;
                self.edits.cells.remove(&sheet);
                self.edits.ops.push(BookOp::Delete { sheet, name });
                if !undo.gone_tables.is_empty() {
                    self.edits.ops.push(BookOp::DeleteTables {
                        names: undo.gone_tables.clone(),
                    });
                }
                for reader in &undo.readers {
                    let patches = self.edits.cells.entry(reader.at.sheet).or_default();
                    patches.insert(
                        (reader.at.row, reader.at.col),
                        CellPatch {
                            change: CellChange::Formula {
                                text: String::new(),
                                value: CalcValue::Empty,
                                array: ArrayKind::None,
                            },
                            style: None,
                        },
                    );
                    self.dirty
                        .cell(reader.at.sheet, reader.at.row, reader.at.col);
                }
                self.dirty.book = true;
                Ok(Op::RestoreSheet {
                    sheet,
                    position,
                    cells: undo.cells,
                    readers: undo.readers,
                    names: undo.names,
                    tables: undo.tables,
                })
            }
            Op::RestoreSheet {
                sheet,
                position,
                cells,
                readers,
                names,
                tables,
            } => {
                self.calc.restore_sheet(sheet).map_err(|error| error.0)?;
                self.book.sheets[sheet as usize].deleted = false;
                let mut order = self.book.tab_order();
                order.retain(|&other| other != sheet);
                order.insert((position as usize).min(order.len()), sheet);
                self.book.order = order;
                self.dirty.book = true;
                self.dirty.sheets.insert(sheet);
                let to_edits = |list: Vec<CellEdit>| -> Vec<EditCell> {
                    list.iter()
                        .filter_map(|edit| {
                            Some(EditCell {
                                sheet: edit.at.sheet,
                                row: edit.at.row,
                                col: edit.at.col,
                                input: Some(to_edit_input(&edit.input)?),
                                style: None,
                            })
                        })
                        .collect()
                };
                self.apply_cells(to_edits(cells))?;
                self.apply_cells(to_edits(readers))?;
                self.calc.restore_names(&names).map_err(|error| error.0)?;
                self.calc.restore_tables(&tables).map_err(|error| error.0)?;
                Ok(Op::RemoveSheet { sheet })
            }
            Op::Rename { sheet, name } => {
                let old = self
                    .calc
                    .rename_sheet(sheet, &name)
                    .map_err(|error| error.0)?;
                self.book.sheets[sheet as usize].name = name.clone();
                self.rename_pivot_sources(&old, &name);
                for defined in &mut self.book.names {
                    if let std::borrow::Cow::Owned(next) =
                        cgs_refs::rename_in_formula(&defined.formula, &old, &name)
                    {
                        defined.formula = next;
                    }
                }
                {
                    let (old, name) = (old.clone(), name.clone());
                    self.rewrite_chart_formulas(&move |text| match cgs_refs::rename_in_formula(
                        text, &old, &name,
                    ) {
                        std::borrow::Cow::Owned(next) => Some(next),
                        std::borrow::Cow::Borrowed(_) => None,
                    });
                }
                self.edits.ops.push(BookOp::Rename {
                    sheet,
                    old: old.clone(),
                    new: name,
                });
                self.dirty.book = true;
                self.dirty.sheets.extend(0..self.book.sheets.len() as u32);
                Ok(Op::Rename { sheet, name: old })
            }
            Op::SheetProtection { sheet, protection } => {
                Ok(self.apply_sheet_protection(sheet, protection))
            }
            Op::BookProtection(protection) => Ok(self.apply_book_protection(protection)),
            Op::IgnoredErrors { sheet, list } => {
                let before =
                    std::mem::replace(&mut self.book.sheets[sheet as usize].ignored_errors, list);
                self.edits.ignored_sheets.insert(sheet);
                self.dirty.sheets.insert(sheet);
                Ok(Op::IgnoredErrors {
                    sheet,
                    list: before,
                })
            }
            Op::SheetState {
                sheet,
                state,
                active,
            } => {
                let before = std::mem::replace(&mut self.book.sheets[sheet as usize].state, state);
                let was_active = self.book.active_sheet;
                self.set_active(active);
                self.edits.sheet_states.insert(sheet);
                self.dirty.book = true;
                Ok(Op::SheetState {
                    sheet,
                    state: before,
                    active: was_active,
                })
            }
            Op::Format {
                sheet,
                areas,
                change,
                active,
            } => self.apply_format(sheet, &areas, *change, active),
            Op::ShiftLog { sheet, edit } => {
                let name = self.book.sheets[sheet as usize].name.clone();
                self.edits.ops.push(BookOp::Axis { sheet, name, edit });
                Ok(Op::ShiftLog {
                    sheet,
                    edit: edit.inverse(),
                })
            }
            Op::NamesTo { names, before } => {
                self.calc.restore_names(&names).map_err(|error| error.0)?;
                for entry in &names {
                    if let Some(defined) = self.book.names.iter_mut().find(|defined| {
                        defined.name == entry.name && defined.local_sheet == entry.local_sheet
                    }) {
                        defined.formula = entry.formula.clone();
                    }
                }
                Ok(Op::NamesTo {
                    names: before,
                    before: names,
                })
            }
            Op::Rich { sheet, cells } => {
                let mut before = Vec::with_capacity(cells.len());
                for (at, index) in cells {
                    let info = &mut self.book.sheets[sheet as usize];
                    match index {
                        Some(index) => {
                            before.push((at, info.rich.insert(at, index)));
                            if let Some(patch) = self
                                .edits
                                .cells
                                .get_mut(&sheet)
                                .and_then(|patches| patches.get_mut(&at))
                            {
                                patch.change = CellChange::Keep;
                            }
                        }
                        None => before.push((at, info.rich.remove(&at))),
                    }
                    self.dirty.cell(sheet, at.0, at.1);
                }
                Ok(Op::Rich {
                    sheet,
                    cells: before,
                })
            }
            Op::RichCopy { sheet, cells } => {
                let mut before = Vec::with_capacity(cells.len());
                for (at, index) in cells {
                    let info = &mut self.book.sheets[sheet as usize];
                    before.push((at, info.rich.insert(at, index)));
                    // The file's own string by its index; one made here by its text and runs.
                    let change = if index >= crate::rich::NEW_RICH {
                        let text = match self.value_of(cgs_calc::CellRef {
                            sheet,
                            row: at.0,
                            col: at.1,
                        }) {
                            CalcValue::Text(text) => text,
                            _ => String::new(),
                        };
                        CellChange::Rich {
                            text,
                            runs: self
                                .book
                                .rich_strings
                                .get(&index)
                                .cloned()
                                .unwrap_or_default(),
                        }
                    } else {
                        CellChange::Shared(index)
                    };
                    self.edits
                        .cells
                        .entry(sheet)
                        .or_default()
                        .entry(at)
                        .or_insert(CellPatch {
                            change: CellChange::Keep,
                            style: None,
                        })
                        .change = change;
                    self.dirty.cell(sheet, at.0, at.1);
                }
                Ok(Op::Rich {
                    sheet,
                    cells: before,
                })
            }
            Op::Styles(before) => Ok(self.apply_styles_before(*before)),
            Op::Lines { sheet, rows, cols } => Ok(self.apply_lines(sheet, rows, cols)),
            Op::MergeArea {
                sheet,
                area,
                center,
            } => self.apply_merge(sheet, area, center),
            Op::View {
                sheet,
                view,
                freeze,
            } => Ok(self.apply_view(sheet, view, freeze)),
            Op::PageSetup { sheet, page } => self.apply_page(sheet, *page),
            Op::Many(list) => {
                let log = self.edits.ops.len();
                let mut undo = Vec::with_capacity(list.len());
                for op in list {
                    match self.apply_op(op) {
                        Ok(inverse) => undo.push(inverse),
                        Err(problem) => {
                            // What went in comes out again, so a refusal changes nothing.
                            self.unguarded(|session| {
                                while let Some(inverse) = undo.pop() {
                                    let _ = session.apply_op(inverse);
                                }
                            });
                            self.edits.ops.truncate(log);
                            return Err(problem);
                        }
                    }
                }
                undo.reverse();
                Ok(Op::Many(undo))
            }
        }
    }

    /// Applies cells that are part of what opened, with no undo step.
    pub(crate) fn apply_loaded(&mut self, edits: Vec<EditCell>) -> Result<Op, String> {
        self.apply_cells(edits)
    }

    /// Reads typed text as it lands in one cell.
    pub(crate) fn typed_edit_at(&mut self, sheet: u32, at: (u32, u32), typed: &Typed) -> EditCell {
        self.typed_edit(sheet, at.0, at.1, typed, at)
    }

    /// The table a cell is a heading of, with its column's place.
    fn heading_at(&self, sheet: u32, row: u32, col: u32) -> Option<(usize, usize)> {
        self.calc
            .tables()
            .iter()
            .enumerate()
            .find_map(|(index, table)| {
                (table.sheet == sheet
                    && table.header_row
                    && table.range.top == row
                    && col >= table.range.left
                    && col <= table.range.right)
                    .then(|| (index, (col - table.range.left) as usize))
            })
    }

    /// A table's headings are always text, and always different: this makes edits to them so, and
    /// carries a new name into the table and the formulas that use it.
    pub(crate) fn with_table_headings(&mut self, op: Op) -> Result<Op, String> {
        if self.calc.tables().is_empty() {
            return Ok(op);
        }
        let mut list = match op {
            Op::Many(list) => list,
            other => vec![other],
        };
        // Headings are judged by the tables as the change leaves them.
        fn last_tables(list: &[Op]) -> Option<&Vec<TableInfo>> {
            list.iter().rev().find_map(|op| match op {
                Op::Tables(tables) => Some(tables),
                Op::Many(inner) => last_tables(inner),
                _ => None,
            })
        }
        let given = last_tables(&list).cloned();
        let mut tables = given.clone().unwrap_or_else(|| self.calc.tables().to_vec());
        let heading_at = |tables: &[TableInfo], sheet: u32, row: u32, col: u32| {
            tables.iter().enumerate().find_map(|(index, table)| {
                (table.sheet == sheet
                    && table.header_row
                    && table.range.top == row
                    && col >= table.range.left
                    && col <= table.range.right)
                    .then(|| (index, (col - table.range.left) as usize))
            })
        };
        let mut renames: Vec<(String, String, String)> = Vec::new();
        for item in &mut list {
            let Op::Cells(edits) = item else {
                continue;
            };
            for edit in edits.iter_mut() {
                let Some((index, column)) = heading_at(&tables, edit.sheet, edit.row, edit.col)
                else {
                    continue;
                };
                let Some(input) = &edit.input else {
                    continue;
                };
                let wanted = match input {
                    EditInput::Clear | EditInput::Value(CalcValue::Empty) => String::new(),
                    EditInput::Value(CalcValue::Text(text)) => text.clone(),
                    EditInput::Value(CalcValue::Number(number)) => {
                        cgs_xlsx::patch_sheet::number_text(*number)
                    }
                    EditInput::Value(CalcValue::Bool(flag)) => {
                        if *flag { "TRUE" } else { "FALSE" }.to_string()
                    }
                    EditInput::Value(CalcValue::Error(text)) => text.clone(),
                    // A heading can't be a formula, and Excel leaves what it would be as 0.
                    EditInput::Formula(_) | EditInput::Array(..) => "0".to_string(),
                };
                let table = &mut tables[index];
                let clash = |name: &str, columns: &[String]| {
                    columns
                        .iter()
                        .enumerate()
                        .any(|(at, other)| at != column && other.eq_ignore_ascii_case(name))
                };
                let name = if wanted.is_empty() {
                    let mut number = 1;
                    loop {
                        let name = format!("Column{number}");
                        if !clash(&name, &table.columns) {
                            break name;
                        }
                        number += 1;
                    }
                } else if clash(&wanted, &table.columns) {
                    let mut number = 2;
                    loop {
                        let name = format!("{wanted}{number}");
                        if !clash(&name, &table.columns) {
                            break name;
                        }
                        number += 1;
                    }
                } else {
                    wanted
                };
                edit.input = Some(EditInput::Value(CalcValue::Text(name.clone())));
                let old = std::mem::replace(&mut table.columns[column], name.clone());
                if !old.eq_ignore_ascii_case(&name) || old != name {
                    renames.push((table.name.clone(), old, name));
                }
            }
        }
        if renames.is_empty() {
            return Ok(Op::Many(list));
        }
        // Formulas follow the new names.
        let mut rewrites = Vec::new();
        let formulas = self.calc.all_formulas().map_err(|error| error.0)?;
        for (at, text) in formulas {
            let mut next = std::borrow::Cow::Borrowed(text.as_str());
            for (table_name, old, new) in &renames {
                let inside = tables.iter().any(|table| {
                    table.name == *table_name
                        && table.sheet == at.sheet
                        && table.range.contains(at.row, at.col)
                });
                if let std::borrow::Cow::Owned(changed) =
                    cgs_refs::rename_table_column(&next, table_name, old, new, inside)
                {
                    next = std::borrow::Cow::Owned(changed);
                }
            }
            if let std::borrow::Cow::Owned(changed) = next {
                let input = self.rewritten(at, changed);
                rewrites.push(EditCell {
                    sheet: at.sheet,
                    row: at.row,
                    col: at.col,
                    input: Some(input),
                    style: None,
                });
            }
        }
        // The change's own tables take the new names too.
        fn set_last_tables(list: &mut [Op], tables: &[TableInfo]) -> bool {
            for op in list.iter_mut().rev() {
                match op {
                    Op::Tables(found) => {
                        *found = tables.to_vec();
                        return true;
                    }
                    Op::Many(inner) => {
                        if set_last_tables(inner, tables) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        if given.is_some() {
            set_last_tables(&mut list, &tables);
        }
        // The table first, so that the new text finds its column.
        let mut all = vec![Op::Tables(tables)];
        all.extend(list);
        if !rewrites.is_empty() {
            all.push(Op::Cells(rewrites));
        }
        Ok(Op::Many(all))
    }

    /// Applies a change and remembers how to undo it.
    pub(crate) fn perform(
        &mut self,
        label: &'static str,
        op: Op,
        selection: Option<(u32, Area)>,
    ) -> Result<Applied, String> {
        self.absorb_results();
        let op = self.with_table_headings(op)?;
        let before = self.history.state;
        let log_before = self.edits.ops.len();
        self.touched_rows.clear();
        let charts_before = self.charts_snapshot(&op);
        let inverse = self.apply_op(op)?;
        let inverse = self.with_charts(inverse, charts_before);
        self.sync_book_tables();
        // Rows that hold something new may want another height, which goes back with the step.
        let inverse = match self.refit_touched() {
            Some(lines) => Op::Many(vec![lines, inverse]),
            None => inverse,
        };
        self.history.touch();
        self.history
            .push(label, inverse, before, selection, log_before);
        Ok(Applied {
            selection,
            replaced: 0,
        })
    }

    /// As `perform`, for a change that says for itself which rows to fit afterwards.
    fn perform_with_rows(
        &mut self,
        label: &'static str,
        op: Op,
        rows: std::collections::BTreeSet<(u32, u32)>,
    ) -> Result<Applied, String> {
        self.absorb_results();
        let before = self.history.state;
        let log_before = self.edits.ops.len();
        let charts_before = self.charts_snapshot(&op);
        let inverse = self.apply_op(op)?;
        let inverse = self.with_charts(inverse, charts_before);
        self.sync_book_tables();
        self.touched_rows = rows;
        let inverse = match self.refit_touched() {
            Some(lines) => Op::Many(vec![lines, inverse]),
            None => inverse,
        };
        self.history.touch();
        self.history.push(label, inverse, before, None, log_before);
        Ok(Applied::default())
    }

    /// Makes a sheet the active one, keeping what the file said for the save.
    fn set_active(&mut self, sheet: u32) {
        if self.book.active_sheet != sheet && self.edits.file_active.is_none() {
            self.edits.file_active = Some(self.book.active_sheet);
        }
        self.book.active_sheet = sheet;
    }

    /// Hides or shows a sheet, as Excel does: hiding the active sheet makes the next visible one
    /// to its right active, or else the nearest to its left. Showing one makes it active.
    fn sheet_state_command(&mut self, sheet: u32, hidden: bool) -> Result<Applied, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .filter(|info| !info.deleted)
            .ok_or_else(|| "That sheet has been deleted.".to_string())?;
        let state = if hidden {
            SheetState::Hidden
        } else {
            SheetState::Visible
        };
        if info.state == state {
            return Ok(Applied::default());
        }
        let order = self.book.tab_order();
        let visible = |other: u32| {
            other != sheet && self.book.sheets[other as usize].state == SheetState::Visible
        };
        let mut active = self.book.active_sheet;
        if hidden {
            if !order.iter().any(|&other| visible(other)) {
                return Err("A workbook must contain at least one visible worksheet.".to_string());
            }
            let at = order.iter().position(|&other| other == sheet).unwrap_or(0);
            if !visible(active) {
                active = order[at + 1..]
                    .iter()
                    .copied()
                    .find(|&other| visible(other))
                    .or_else(|| {
                        order[..at]
                            .iter()
                            .rev()
                            .copied()
                            .find(|&other| visible(other))
                    })
                    .unwrap_or(active);
            }
        } else {
            active = sheet;
        }
        let label = if hidden {
            "hiding a sheet"
        } else {
            "showing a sheet"
        };
        self.perform(
            label,
            Op::SheetState {
                sheet,
                state,
                active,
            },
            None,
        )
    }

    /// Applies a change made of steps that each build on the last, as one step to undo. `next`
    /// gives each in turn, and nothing once done. If one fails, those before it are taken back.
    pub(crate) fn perform_steps(
        &mut self,
        label: &'static str,
        selection: Option<(u32, Area)>,
        mut next: impl FnMut(&mut Self, usize) -> Result<Option<Op>, String>,
    ) -> Result<Applied, String> {
        self.absorb_results();
        let before = self.history.state;
        let log_before = self.edits.ops.len();
        self.touched_rows.clear();
        let mut undo = Vec::new();
        let mut step = 0;
        loop {
            let result = next(self, step).and_then(|op| match op {
                Some(op) => {
                    let op = self.with_table_headings(op)?;
                    self.apply_op(op).map(Some)
                }
                None => Ok(None),
            });
            match result {
                Ok(Some(inverse)) => {
                    undo.push(inverse);
                    self.sync_book_tables();
                }
                Ok(None) => break,
                Err(problem) => {
                    self.unguarded(|session| {
                        while let Some(inverse) = undo.pop() {
                            let _ = session.apply_op(inverse);
                        }
                    });
                    self.edits.ops.truncate(log_before);
                    self.sync_book_tables();
                    return Err(problem);
                }
            }
            step += 1;
        }
        if undo.is_empty() {
            return Ok(Applied {
                selection,
                replaced: 0,
            });
        }
        undo.reverse();
        let inverse = Op::Many(undo);
        let inverse = match self.refit_touched() {
            Some(lines) => Op::Many(vec![lines, inverse]),
            None => inverse,
        };
        self.history.touch();
        self.history
            .push(label, inverse, before, selection, log_before);
        Ok(Applied {
            selection,
            replaced: 0,
        })
    }

    /// Measured heights for rows that wrap, which belong to the step that asked for them.
    fn fit_rows_command(
        &mut self,
        rows: Vec<(u32, u32, u32)>,
        quiet: bool,
    ) -> Result<Applied, String> {
        let mut by_sheet: BTreeMap<u32, Vec<(u32, cgs_model::LineInfo)>> = BTreeMap::new();
        for (sheet, row, px) in rows {
            let info = self.sheet_for_edit(sheet)?;
            let line = info.rows.info(row).copied().unwrap_or_default();
            if line.custom || line.hidden {
                continue;
            }
            let size = (px != info.rows.default_px).then_some(px);
            if size != line.size {
                by_sheet
                    .entry(sheet)
                    .or_default()
                    .push((row, cgs_model::LineInfo { size, ..line }));
            }
        }
        for (sheet, mut list) in by_sheet {
            list.sort_unstable_by_key(|&(row, _)| row);
            if quiet {
                self.book.sheets[sheet as usize].rows.set_many(&list);
                self.dirty.sheets.insert(sheet);
                self.dirty.layouts.insert(sheet);
                continue;
            }
            let lines = self.apply_lines(sheet, list, Vec::new());
            if let Some(step) = self.history.undo.last_mut() {
                let old = std::mem::replace(&mut step.op, Op::Many(Vec::new()));
                step.op = Op::Many(vec![lines, old]);
            }
        }
        Ok(Applied::default())
    }

    fn step_back(&mut self) -> Result<Applied, String> {
        self.absorb_results();
        let Some(mut step) = self.history.undo.pop() else {
            return Ok(Applied::default());
        };
        self.history.bytes -= step.bytes;
        let op = std::mem::replace(&mut step.op, Op::Many(Vec::new()));
        let inverse = self.unguarded(|session| session.apply_op(op))?;
        self.sync_book_tables();
        self.touched_rows.clear();
        step.bytes = inverse.bytes();
        step.op = inverse;
        self.history.bytes += step.bytes;
        self.history.state = step.before;
        let selection = step.selection;
        // Against the same file, this step's changes come off the list: what they did to the file
        // is undone by not doing it. Against a newer file, they stay as changes to that file.
        if step.log_file == self.history.file {
            self.edits.ops.truncate(step.log_before);
        }
        self.history.redo.push(step);
        self.arrive_at_state();
        Ok(Applied {
            selection,
            replaced: 0,
        })
    }

    fn step_forward(&mut self) -> Result<Applied, String> {
        self.absorb_results();
        let Some(mut step) = self.history.redo.pop() else {
            return Ok(Applied::default());
        };
        self.history.bytes -= step.bytes;
        let op = std::mem::replace(&mut step.op, Op::Many(Vec::new()));
        step.log_before = self.edits.ops.len();
        step.log_file = self.history.file;
        let inverse = self.unguarded(|session| session.apply_op(op))?;
        self.sync_book_tables();
        self.touched_rows.clear();
        step.bytes = inverse.bytes();
        step.op = inverse;
        self.history.bytes += step.bytes;
        self.history.state = step.after;
        let selection = step.selection;
        self.history.undo.push(step);
        self.arrive_at_state();
        Ok(Applied {
            selection,
            replaced: 0,
        })
    }

    /// Back at the file's own state, nothing differs from it.
    fn arrive_at_state(&mut self) {
        if self.history.at_clean() {
            let styles = self.edits.styles.take();
            // Zoom and the sheet showing aren't undone, so they stay to be saved.
            let view_zoom = std::mem::take(&mut self.edits.view_zoom);
            let file_active = self.edits.file_active;
            self.edits = Default::default();
            self.edits.styles = styles;
            self.edits.view_zoom = view_zoom;
            self.edits.file_active = file_active;
        }
    }

    /// A cell format like `base`, with this number format, made at most once.
    fn style_with_format(&mut self, base: u32, format: Format) -> u32 {
        let key = (base, format);
        if let Some(&style) = self.format_styles.get(&key) {
            return style;
        }
        let new = crate::edit::NewStyle {
            number_format: match format {
                Format::Builtin(_) => None,
            },
            number_format_id: match format {
                Format::Builtin(id) => Some(id),
            },
            ..Default::default()
        };
        let style = self.add_style(base, &new);
        self.format_styles.insert(key, style);
        style
    }

    /// One cell's edit from typed text. `home` is where the formula was written, for filling it
    /// into other cells. The text can be a bad formula, which only `check_formula` can tell.
    fn typed_edit(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        typed: &Typed,
        home: (u32, u32),
    ) -> EditCell {
        let (input, style) = match typed {
            Typed::Clear => (EditInput::Clear, None),
            Typed::Value(value) => (EditInput::Value(value.clone()), None),
            Typed::Formula(text) => {
                let text = if (row, col) == home {
                    text.clone()
                } else {
                    cgs_refs::moved_formula(
                        text,
                        i64::from(row) - i64::from(home.0),
                        i64::from(col) - i64::from(home.1),
                    )
                };
                (EditInput::Formula(text), None)
            }
            Typed::Formatted(value, format) => {
                // A cell with a format of its own keeps it, as in Excel.
                let base = self.book.sheets[sheet as usize].style_at(row, col);
                let general = self.book.styles.xf(base).num_fmt == 0;
                let style = general.then(|| self.style_with_format(base, *format));
                (EditInput::Value(CalcValue::Number(*value)), style)
            }
        };
        EditCell {
            sheet,
            row,
            col,
            input: Some(input),
            style,
        }
    }

    fn input_command(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: &str,
        areas: &[Area],
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let mut typed = parse_input(text, locale, self.book.date1904, self.today);
        // A table's heading keeps what was typed as it was typed.
        if areas.is_empty()
            && self.heading_at(sheet, row, col).is_some()
            && matches!(typed, Typed::Value(_) | Typed::Formatted(..))
        {
            typed = Typed::Value(CalcValue::Text(
                text.strip_prefix('\'').unwrap_or(text).to_string(),
            ));
        }
        if let Typed::Formula(formula) = &typed {
            let fixed = self.calc.check_formula(formula).map_err(|error| error.0)?;
            typed = Typed::Formula(fixed);
        }
        let mut edits = Vec::new();
        // A cell formatted as text takes what's typed as text, formulas too, as in Excel.
        let as_text = |session: &Self, r: u32, c: u32| {
            let xf = session
                .book
                .styles
                .xf(session.book.sheets[sheet as usize].style_at(r, c));
            let text_format = xf.num_fmt == 49
                || session
                    .book
                    .styles
                    .num_fmts
                    .get(&xf.num_fmt)
                    .map(String::as_str)
                    == Some("@");
            (text_format && !matches!(typed, Typed::Clear)).then(|| EditCell {
                sheet,
                row: r,
                col: c,
                input: Some(EditInput::Value(CalcValue::Text(text.to_string()))),
                style: None,
            })
        };
        if areas.is_empty() {
            if self.cuts_an_array(sheet, Area::cell(row, col)) {
                return Err("You can't change part of an array.".to_string());
            }
            match as_text(self, row, col) {
                Some(edit) => edits.push(edit),
                None => edits.push(self.typed_edit(sheet, row, col, &typed, (row, col))),
            }
        } else {
            for area in areas {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                if self.cuts_an_array(sheet, *area) {
                    return Err("You can't change part of an array.".to_string());
                }
                for r in area.top..=area.bottom {
                    for c in area.left..=area.right {
                        match as_text(self, r, c) {
                            Some(edit) => edits.push(edit),
                            None => edits.push(self.typed_edit(sheet, r, c, &typed, (row, col))),
                        }
                    }
                }
            }
        }
        let area = areas.first().copied().unwrap_or(Area::cell(row, col));
        // A table beside what was typed may grow to take it in.
        if let Some(growth) = self.table_growth(sheet, &edits, false) {
            let mut list = vec![Op::Cells(edits)];
            list.extend(growth);
            return self.perform("typing", Op::Many(list), Some((sheet, area)));
        }
        self.perform("typing", Op::Cells(edits), Some((sheet, area)))
    }

    /// Cells that hold something in an area, which can be whole rows or columns.
    fn filled_cells(&self, sheet: u32, area: Area) -> Vec<(u32, u32)> {
        let info = &self.book.sheets[sheet as usize];
        let bottom = area.bottom.min(info.used.0.max(1));
        let right = area.right.min(info.used.1.max(1));
        if area.top > bottom || area.left > right {
            return Vec::new();
        }
        let mut found = crate::edit::CollectCells::default();
        self.calc.read_block(
            cgs_calc::SheetRect {
                sheet,
                rect: cgs_calc::Rect {
                    top: area.top,
                    left: area.left,
                    bottom,
                    right,
                },
            },
            &mut found,
        );
        found.0
    }

    fn clear_command(&mut self, sheet: u32, areas: &[Area]) -> Result<Applied, String> {
        let spills = self.calc.spill_areas();
        let mut edits = Vec::new();
        for area in areas {
            if !area.valid() {
                return Err("That's off the sheet.".to_string());
            }
            if self.cuts_an_array(sheet, *area) {
                return Err("You can't change part of an array.".to_string());
            }
            for (row, col) in self.filled_cells(sheet, *area) {
                // Clearing what an array spills does nothing; its anchor owns it.
                let spilled = spills.iter().any(|spill| {
                    spill.sheet == sheet
                        && spill.rect.contains(row, col)
                        && (spill.rect.top, spill.rect.left) != (row, col)
                });
                if !spilled {
                    edits.push(EditCell {
                        sheet,
                        row,
                        col,
                        input: Some(EditInput::Clear),
                        style: None,
                    });
                }
            }
        }
        let selection = areas.first().map(|area| (sheet, *area));
        self.perform("clearing", Op::Cells(edits), selection)
    }

    fn array_command(&mut self, sheet: u32, area: Area, text: &str) -> Result<Applied, String> {
        if !area.valid() {
            return Err("That's off the sheet.".to_string());
        }
        let text = text.strip_prefix('=').unwrap_or(text);
        let text = self.calc.check_formula(text).map_err(|error| error.0)?;
        if self.cuts_an_array(sheet, area) {
            return Err("You can't change part of an array.".to_string());
        }
        let rect = cgs_calc::Rect {
            top: area.top,
            left: area.left,
            bottom: area.bottom,
            right: area.right,
        };
        let edit = EditCell {
            sheet,
            row: area.top,
            col: area.left,
            input: Some(EditInput::Array(text, FormulaShape::Array(rect))),
            style: None,
        };
        let edits = vec![edit];
        self.refuse_array_in_table(sheet, area, &edits)?;
        self.perform("array formula", Op::Cells(edits), Some((sheet, area)))
    }

    /// The `{=...}` array a cell belongs to: its anchor's formula and its area.
    fn legacy_array_at(&self, sheet: u32, row: u32, col: u32) -> Option<(String, cgs_calc::Rect)> {
        let area = self.calc.spill_areas().into_iter().find(|area| {
            area.sheet == sheet
                && area.rect.contains(row, col)
                && matches!(
                    self.calc.formula_shape(CellRef {
                        sheet,
                        row: area.rect.top,
                        col: area.rect.left,
                    }),
                    FormulaShape::Array(_)
                )
        })?;
        let text = self.calc.formula_text(CellRef {
            sheet,
            row: area.rect.top,
            col: area.rect.left,
        })?;
        Some((text, area.rect))
    }

    /// What copying one cell to another place would put there.
    pub(crate) fn copied_input(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        rows: i64,
        cols: i64,
        mode: PasteMode,
    ) -> Option<EditInput> {
        let at = CellRef { sheet, row, col };
        let input = self.calc.cell_input(at);
        Some(match input {
            CellInput::Formula { text, cached, .. } => match mode {
                PasteMode::Values => match cached {
                    None | Some(CalcValue::Empty) => EditInput::Clear,
                    Some(value) => EditInput::Value(value),
                },
                _ => EditInput::Formula(cgs_refs::moved_formula(&text, rows, cols)),
            },
            CellInput::Value(value) => EditInput::Value(value),
            CellInput::Clear => EditInput::Clear,
            CellInput::Spill { .. } => return None,
        })
    }

    /// What a copied cell of a `{=...}` array becomes, if it is one. As read off Excel, the part of
    /// the array that was copied becomes one array of its own, wherever its first cell lands, with
    /// the formula moved from where the original array starts. So `H3:H4` of `{=C2:C4*2}` in `H2:H4`,
    /// copied to `I3`, is `{=D3:D5*2}` in `I3:I4`. `within` limits how far it can reach.
    pub(crate) fn copied_array_cell(
        &self,
        sheet: u32,
        from: Area,
        (row, col): (u32, u32),
        (new_row, new_col): (u32, u32),
        within: Area,
    ) -> Option<Option<EditInput>> {
        let (text, rect) = self.legacy_array_at(sheet, row, col)?;
        let part = cgs_calc::Rect {
            top: from.top.max(rect.top),
            left: from.left.max(rect.left),
            bottom: from.bottom.min(rect.bottom),
            right: from.right.min(rect.right),
        };
        if (row, col) != (part.top, part.left) {
            return Some(None);
        }
        Some(Some(EditInput::Array(
            cgs_refs::moved_formula(
                &text,
                i64::from(new_row) - i64::from(rect.top),
                i64::from(new_col) - i64::from(rect.left),
            ),
            FormulaShape::Array(cgs_calc::Rect {
                top: new_row,
                left: new_col,
                bottom: (new_row + (part.bottom - part.top)).min(within.bottom),
                right: (new_col + (part.right - part.left)).min(within.right),
            }),
        )))
    }

    /// Excel refuses an array of many cells in a table, and goes by the whole area being changed:
    /// the array itself can land just beside the table and still be refused.
    pub(crate) fn refuse_array_in_table(
        &self,
        sheet: u32,
        area: Area,
        edits: &[EditCell],
    ) -> Result<(), String> {
        let many = edits.iter().any(|edit| {
            matches!(&edit.input, Some(EditInput::Array(_, FormulaShape::Array(rect)))
                if rect.top != rect.bottom || rect.left != rect.right)
        });
        let touches = self.calc.tables().iter().any(|table| {
            table.sheet == sheet
                && table.range.top <= area.bottom
                && table.range.bottom >= area.top
                && table.range.left <= area.right
                && table.range.right >= area.left
        });
        if many && touches {
            return Err("Arrays of many cells aren't allowed in tables.".to_string());
        }
        Ok(())
    }

    fn paste_command(
        &mut self,
        from_sheet: u32,
        from: Area,
        sheet: u32,
        to: Area,
        mode: PasteMode,
    ) -> Result<Applied, String> {
        if !from.valid() || !to.valid() {
            return Err("That's off the sheet.".to_string());
        }
        for index in [from_sheet, sheet] {
            let info = self
                .book
                .sheets
                .get(index as usize)
                .ok_or_else(|| format!("There is no sheet {index}."))?;
            number_only(info)?;
        }
        let target = paste_area(from, to);
        let across = target.cols() / from.cols();
        let down = target.rows() / from.rows();
        if !target.valid() {
            return Err("That doesn't fit on the sheet.".to_string());
        }
        if self.cuts_an_array(sheet, target) {
            return Err("You can't change part of an array.".to_string());
        }
        let mut edits = Vec::with_capacity((from.rows() * from.cols() * across * down) as usize);
        let source = &self.book.sheets[from_sheet as usize];
        let source_merges: Vec<Rect> = source
            .merges
            .iter()
            .filter(|merge| {
                from.contains(merge.top, merge.left) && from.contains(merge.bottom, merge.right)
            })
            .copied()
            .collect();
        for repeat_down in 0..down {
            for repeat_across in 0..across {
                let top = to.top + repeat_down * from.rows();
                let left = to.left + repeat_across * from.cols();
                for row in from.top..=from.bottom {
                    for col in from.left..=from.right {
                        let rows = i64::from(top + (row - from.top)) - i64::from(row);
                        let cols = i64::from(left + (col - from.left)) - i64::from(col);
                        let new_row = (i64::from(row) + rows) as u32;
                        let new_col = (i64::from(col) + cols) as u32;
                        let input = match mode {
                            PasteMode::Formats => None,
                            // A table's heading takes what a formula comes to.
                            _ if self.heading_at(sheet, new_row, new_col).is_some() => self
                                .copied_input(from_sheet, row, col, rows, cols, PasteMode::Values),
                            PasteMode::Values => {
                                self.copied_input(from_sheet, row, col, rows, cols, mode)
                            }
                            _ => match self.copied_array_cell(
                                from_sheet,
                                from,
                                (row, col),
                                (new_row, new_col),
                                Area::cell(cgs_model::MAX_ROWS, cgs_model::MAX_COLS),
                            ) {
                                Some(input) => input,
                                None => self.copied_input(from_sheet, row, col, rows, cols, mode),
                            },
                        };
                        let style = match mode {
                            PasteMode::All | PasteMode::Formats => {
                                Some(self.book.sheets[from_sheet as usize].style_at(row, col))
                            }
                            _ => None,
                        };
                        edits.push(EditCell {
                            sheet,
                            row: new_row,
                            col: new_col,
                            input,
                            style,
                        });
                    }
                }
            }
        }
        self.refuse_array_in_table(sheet, target, &edits)?;
        // A rich string keeps its runs, as in Excel, when everything is pasted.
        let mut rich = Vec::new();
        if mode == PasteMode::All {
            let source = &self.book.sheets[from_sheet as usize];
            for repeat_down in 0..down {
                for repeat_across in 0..across {
                    for (&(row, col), &index) in &source.rich {
                        if !from.contains(row, col) {
                            continue;
                        }
                        let at = (
                            to.top + repeat_down * from.rows() + (row - from.top),
                            to.left + repeat_across * from.cols() + (col - from.left),
                        );
                        rich.push((at, index));
                    }
                }
            }
        }
        let mut list = vec![Op::Cells(edits)];
        if !rich.is_empty() {
            list.push(Op::RichCopy { sheet, cells: rich });
        }
        if matches!(mode, PasteMode::All | PasteMode::Formats) {
            let info = &self.book.sheets[sheet as usize];
            let mut merges: Vec<Rect> = info
                .merges
                .iter()
                .filter(|merge| {
                    !(merge.top <= target.bottom
                        && merge.bottom >= target.top
                        && merge.left <= target.right
                        && merge.right >= target.left)
                })
                .copied()
                .collect();
            let before = merges.len();
            for repeat_down in 0..down {
                for repeat_across in 0..across {
                    for merge in &source_merges {
                        let shift_rows = to.top + repeat_down * from.rows() - from.top;
                        let shift_cols = to.left + repeat_across * from.cols() - from.left;
                        merges.push(Rect {
                            top: merge.top + shift_rows,
                            left: merge.left + shift_cols,
                            bottom: merge.bottom + shift_rows,
                            right: merge.right + shift_cols,
                        });
                    }
                }
            }
            if merges.len() != before || before != info.merges.len() {
                list.push(Op::Merges { sheet, merges });
            }
        }
        self.perform("pasting", Op::Many(list), Some((sheet, target)))
    }

    fn move_command(
        &mut self,
        from_sheet: u32,
        from: Area,
        sheet: u32,
        row: u32,
        col: u32,
    ) -> Result<Applied, String> {
        if !from.valid() {
            return Err("That's off the sheet.".to_string());
        }
        let target = Area {
            top: row,
            left: col,
            bottom: row + from.rows() - 1,
            right: col + from.cols() - 1,
        };
        if !target.valid() {
            return Err("That doesn't fit on the sheet.".to_string());
        }
        if (from_sheet, from) == (sheet, target) {
            return Ok(Applied {
                selection: Some((sheet, target)),
                replaced: 0,
            });
        }
        // A rich string moves with its runs, as in Excel.
        let rich: Vec<((u32, u32), u32)> = self.book.sheets[from_sheet as usize]
            .rich
            .iter()
            .filter(|(&(r, c), _)| from.contains(r, c))
            .map(|(&(r, c), &index)| ((row + (r - from.top), col + (c - from.left)), index))
            .collect();
        let edits = self.move_edits(from_sheet, from, sheet, row, col)?;
        let op = if rich.is_empty() {
            Op::Cells(edits)
        } else {
            Op::Many(vec![Op::Cells(edits), Op::RichCopy { sheet, cells: rich }])
        };
        self.perform("moving", op, Some((sheet, target)))
    }

    /// What moving cells changes, with what points at them following.
    pub(crate) fn move_edits(
        &mut self,
        from_sheet: u32,
        from: Area,
        sheet: u32,
        row: u32,
        col: u32,
    ) -> Result<Vec<EditCell>, String> {
        let target = Area {
            top: row,
            left: col,
            bottom: row + from.rows() - 1,
            right: col + from.cols() - 1,
        };
        if self.cuts_an_array(from_sheet, from) || self.cuts_an_array(sheet, target) {
            return Err("You can't change part of an array.".to_string());
        }
        let mut edits: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        // Formulas anywhere that point at what moves follow it.
        let from_name = self.book.sheets[from_sheet as usize].name.clone();
        let to_name = self.book.sheets[sheet as usize].name.clone();
        let change = cgs_refs::MovedCells {
            from_sheet: &from_name,
            area: (from.top, from.left, from.bottom, from.right),
            to_sheet: &to_name,
            rows: i64::from(row) - i64::from(from.top),
            cols: i64::from(col) - i64::from(from.left),
        };
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            let home = self.book.sheets[at.sheet as usize].name.as_str();
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::follow_moved_cells(&text, home, &change)
            {
                let input = self.rewritten(at, next);
                edits.insert(
                    (at.sheet, at.row, at.col),
                    EditCell {
                        sheet: at.sheet,
                        row: at.row,
                        col: at.col,
                        input: Some(input),
                        style: None,
                    },
                );
            }
        }
        // What moves, as read after those changes, emptied where it was and written where it goes.
        let mut moved = Vec::new();
        for r in from.top..=from.bottom {
            for c in from.left..=from.right {
                let source = CellRef {
                    sheet: from_sheet,
                    row: r,
                    col: c,
                };
                let input = edits
                    .get(&(from_sheet, r, c))
                    .and_then(|edit| edit.input.clone())
                    .or_else(|| to_edit_input(&self.calc.cell_input(source)))
                    .unwrap_or(EditInput::Clear);
                let style = self.book.sheets[from_sheet as usize].style_at(r, c);
                moved.push((r, c, input, style));
            }
        }
        for (r, c, _, _) in &moved {
            edits.insert(
                (from_sheet, *r, *c),
                EditCell {
                    sheet: from_sheet,
                    row: *r,
                    col: *c,
                    input: Some(EditInput::Clear),
                    style: Some(0),
                },
            );
        }
        for (r, c, input, style) in moved {
            let (new_row, new_col) = (row + (r - from.top), col + (c - from.left));
            edits.insert(
                (sheet, new_row, new_col),
                EditCell {
                    sheet,
                    row: new_row,
                    col: new_col,
                    input: Some(input),
                    style: Some(style),
                },
            );
        }
        Ok(edits.into_values().collect())
    }

    /// Runs a command the UI asked for.
    pub(crate) fn command(
        &mut self,
        command: Command,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        if let Some(reason) = &self.cannot_edit {
            return Err(format!("This workbook can't be edited here: {reason}"));
        }
        if let Some(refused) = self.protection_refusal(&command) {
            return Err(refused.to_string());
        }
        if let Some(refused) = self.binding_refusal(&command) {
            return Err(refused.to_string());
        }
        match command {
            Command::Input {
                sheet,
                row,
                col,
                text,
                areas,
                force,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                if !force && areas.is_empty() {
                    let typed = parse_input(&text, locale, self.book.date1904, self.today);
                    let value = match &typed {
                        Typed::Value(value) => Some(value.clone()),
                        Typed::Formatted(number, _) => Some(CalcValue::Number(*number)),
                        Typed::Clear => Some(CalcValue::Empty),
                        _ => None,
                    };
                    if let Some(refused) =
                        value.and_then(|value| self.refusal(sheet, row, col, &value))
                    {
                        return Err(refused);
                    }
                }
                let applied = self.input_command(sheet, row, col, &text, &areas, locale)?;
                // A custom rule reads the cell as typed, so it's asked afterwards.
                if !force && areas.is_empty() {
                    if let Some(refused) = self.custom_refusal(sheet, row, col) {
                        self.step_back()?;
                        self.history.redo.pop();
                        return Err(refused);
                    }
                }
                Ok(applied)
            }
            Command::ArrayInput { sheet, area, text } => self.array_command(sheet, area, &text),
            Command::Clear { sheet, areas } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                self.clear_command(sheet, &areas)
            }
            Command::Fill {
                sheet,
                from,
                to,
                copy,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                // What a fill writes can't be part of an array, as in Excel. Reading from part of one is fine.
                if fill_written(from, to).any(|area| self.cuts_an_array(sheet, area)) {
                    return Err("You can't change part of an array.".to_string());
                }
                let edits = self.fill_edits(sheet, from, to, copy)?;
                self.refuse_array_in_table(sheet, to, &edits)?;
                // A fill beside a table grows it, as typing there does.
                if let Some(growth) = self.table_growth(sheet, &edits, true) {
                    let mut list = vec![Op::Cells(edits)];
                    list.extend(growth);
                    return self.perform("filling", Op::Many(list), Some((sheet, to)));
                }
                self.perform("filling", Op::Cells(edits), Some((sheet, to)))
            }
            Command::Paste {
                from_sheet,
                from,
                sheet,
                to,
                mode,
            } => self.paste_command(from_sheet, from, sheet, to, mode),
            Command::Move {
                from_sheet,
                from,
                sheet,
                row,
                col,
            } => self.move_command(from_sheet, from, sheet, row, col),
            Command::PasteExternal {
                sheet,
                row,
                col,
                text,
                html,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let (edits, merges, area) =
                    self.external_edits(sheet, row, col, &text, html.as_deref(), locale)?;
                let mut list = vec![Op::Cells(edits)];
                if let Some(merges) = merges {
                    list.push(Op::Merges { sheet, merges });
                }
                self.perform("pasting", Op::Many(list), Some((sheet, area)))
            }
            Command::Axis {
                sheet,
                axis,
                at,
                count,
                delete,
            } => {
                let axis = match axis {
                    AxisName::Row => Axis::Row,
                    AxisName::Col => Axis::Col,
                };
                let edit = AxisEdit {
                    axis,
                    at,
                    count,
                    delete,
                    band: None,
                };
                self.perform("rows or columns", Op::Axis { sheet, edit }, None)
            }
            Command::InsertSheet {
                position,
                name,
                right_to_left,
            } => {
                let name = match name {
                    Some(name) => name,
                    None => next_sheet_name(&self.book),
                };
                if let Some(problem) = sheet_name_problem(&self.book, &name, None) {
                    return Err(problem);
                }
                self.perform(
                    "a new sheet",
                    Op::AddSheet {
                        name,
                        position,
                        right_to_left,
                    },
                    None,
                )
            }
            Command::DeleteSheet { sheet } => {
                let info = self.sheet_for_edit(sheet)?;
                if info.deleted {
                    return Err("That sheet has been deleted.".to_string());
                }
                let visible = self
                    .book
                    .tab_order()
                    .iter()
                    .filter(|&&other| {
                        other != sheet
                            && self.book.sheets[other as usize].state == SheetState::Visible
                    })
                    .count();
                if visible == 0 {
                    return Err("A workbook must keep at least one visible sheet.".to_string());
                }
                self.perform("deleting a sheet", Op::RemoveSheet { sheet }, None)
            }
            Command::RenameSheet { sheet, name } => {
                let info = self.sheet_for_edit(sheet)?;
                if info.deleted {
                    return Err("That sheet has been deleted.".to_string());
                }
                if let Some(problem) = sheet_name_problem(&self.book, &name, Some(sheet)) {
                    return Err(problem);
                }
                self.perform("renaming a sheet", Op::Rename { sheet, name }, None)
            }
            Command::SheetState { sheet, hidden } => self.sheet_state_command(sheet, hidden),
            Command::ProtectSheet {
                sheet,
                allow,
                password,
                salt,
            } => self.protect_sheet(sheet, allow, &password, &salt),
            Command::UnprotectSheet { sheet, password } => self.unprotect_sheet(sheet, &password),
            Command::ProtectWorkbook {
                structure,
                password,
                salt,
            } => self.protect_workbook(structure, &password, &salt),
            Command::UnprotectWorkbook { password } => self.unprotect_workbook(&password),
            Command::ActiveSheet { sheet } => {
                let visible = self
                    .book
                    .sheets
                    .get(sheet as usize)
                    .is_some_and(|info| !info.deleted && info.state == SheetState::Visible);
                if visible {
                    self.set_active(sheet);
                }
                Ok(Applied::default())
            }
            Command::Zoom { sheet, zoom } => {
                let info = self
                    .book
                    .sheets
                    .get_mut(sheet as usize)
                    .filter(|info| !info.deleted)
                    .ok_or_else(|| "That sheet has been deleted.".to_string())?;
                let zoom = zoom.clamp(10, 400) as u16;
                if info.view.zoom != zoom {
                    info.view.zoom = zoom;
                    self.edits.view_zoom.insert(sheet);
                }
                Ok(Applied::default())
            }
            Command::Format {
                sheet,
                areas,
                change,
                active,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                if areas.iter().any(|area| !area.valid()) {
                    return Err("That's off the sheet.".to_string());
                }
                let selection = areas.first().map(|area| (sheet, *area));
                self.perform(
                    "formatting",
                    Op::Format {
                        sheet,
                        areas,
                        change,
                        active,
                    },
                    selection,
                )
            }
            Command::Lines {
                sheet,
                axis,
                first,
                last,
                size,
                hidden,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let max = match axis {
                    AxisName::Row => cgs_model::MAX_ROWS,
                    AxisName::Col => cgs_model::MAX_COLS,
                };
                if first == 0 || first > last || last > max {
                    return Err("That's off the sheet.".to_string());
                }
                let op = self.lines_op(sheet, axis, first, last, size, hidden);
                let label = if hidden.is_some() {
                    "hiding"
                } else {
                    match axis {
                        AxisName::Row => "row height",
                        AxisName::Col => "column width",
                    }
                };
                self.perform(label, op, None)
            }
            Command::AutoFitRows { sheet, first, last } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let used = info.used.0;
                let rows: Vec<(u32, cgs_model::LineInfo)> = (first..=last.min(used.max(first)))
                    .map(|row| {
                        let mut line = info.rows.info(row).copied().unwrap_or_default();
                        line.custom = false;
                        (row, line)
                    })
                    .collect();
                for &(row, _) in &rows {
                    self.touch_row(sheet, row);
                }
                let touched = std::mem::take(&mut self.touched_rows);
                self.perform_with_rows(
                    "row height",
                    Op::Lines {
                        sheet,
                        rows,
                        cols: Vec::new(),
                    },
                    touched,
                )
            }
            Command::FitCols { sheet, cols } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let list = cols
                    .iter()
                    .map(|&(col, px)| {
                        let line = info.cols.info(col).copied().unwrap_or_default();
                        (
                            col,
                            col,
                            cgs_model::LineInfo {
                                size: Some(px),
                                custom: true,
                                hidden: false,
                                ..line
                            },
                        )
                    })
                    .collect();
                self.perform(
                    "column width",
                    Op::Lines {
                        sheet,
                        rows: Vec::new(),
                        cols: list,
                    },
                    None,
                )
            }
            Command::FitRows { rows, quiet } => self.fit_rows_command(rows, quiet),
            Command::Merge {
                sheet,
                areas,
                center,
                across,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let mut list = Vec::new();
                for area in &areas {
                    if !area.valid() {
                        return Err("That's off the sheet.".to_string());
                    }
                    if self.cuts_an_array(sheet, *area) {
                        return Err("You can't change part of an array.".to_string());
                    }
                    if across {
                        for row in area.top..=area.bottom {
                            let line = Area {
                                top: row,
                                left: area.left,
                                bottom: row,
                                right: area.right,
                            };
                            if line.cols() > 1 {
                                list.push(Op::MergeArea {
                                    sheet,
                                    area: line,
                                    center,
                                });
                            }
                        }
                    } else if area.rows() > 1 || area.cols() > 1 {
                        list.push(Op::MergeArea {
                            sheet,
                            area: *area,
                            center,
                        });
                    }
                }
                let selection = areas.first().map(|area| (sheet, *area));
                self.perform("merging", Op::Many(list), selection)
            }
            Command::Unmerge { sheet, areas } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let merges: Vec<Rect> = info
                    .merges
                    .iter()
                    .filter(|merge| {
                        !areas.iter().any(|area| {
                            merge.top <= area.bottom
                                && merge.bottom >= area.top
                                && merge.left <= area.right
                                && merge.right >= area.left
                        })
                    })
                    .copied()
                    .collect();
                let selection = areas.first().map(|area| (sheet, *area));
                self.perform("unmerging", Op::Merges { sheet, merges }, selection)
            }
            Command::Freeze {
                sheet,
                rows,
                cols,
                top,
                left,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                if rows >= cgs_model::MAX_ROWS || cols >= cgs_model::MAX_COLS {
                    return Err("That's off the sheet.".to_string());
                }
                let freeze = cgs_model::Freeze {
                    rows,
                    cols,
                    top,
                    left,
                };
                let op = self.freeze_op(sheet, Some(freeze));
                self.perform("freezing panes", op, None)
            }
            Command::CreateTable {
                sheet,
                area,
                headers,
                style,
                name,
            } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                self.create_table_command(sheet, area, headers, style, name)
            }
            Command::TableOptions { table, options } => self.table_options_command(&table, options),
            Command::TotalsFunction {
                table,
                column,
                function,
            } => self.totals_function_command(&table, &column, function),
            Command::RenameTable { table, name } => self.rename_table_command(&table, &name),
            Command::ResizeTable { table, area } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                self.resize_table_command(&table, area)
            }
            Command::ConvertToRange { table } => self.convert_to_range_command(&table),
            Command::RefreshPivot { pivot } => self.refresh_pivot_command(pivot),
            Command::PivotLayout { pivot, layout } => self.pivot_layout_command(&pivot, layout),
            Command::PivotOptions { pivot, options } => self.pivot_options_command(&pivot, options),
            Command::GroupPivot {
                pivot,
                field,
                group,
            } => self.pivot_group_command(&pivot, field, group),
            Command::UngroupPivot { pivot, field } => self.pivot_ungroup_command(&pivot, field),
            Command::PivotDetails { sheet, row, col } => {
                self.pivot_details_command(sheet, row, col)
            }
            Command::PivotFilterPages { pivot, field } => {
                self.pivot_filter_pages_command(&pivot, field)
            }
            Command::PivotField {
                pivot,
                field,
                change,
            } => self.pivot_field_command(&pivot, field, change),
            Command::CreatePivot { sheet, area, to } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                self.create_pivot_command(sheet, area, to)
            }
            Command::DeletePivot { pivot } => self.delete_pivot_command(&pivot),
            Command::AddCf { sheet, areas, rule } => self.add_cf_command(sheet, &areas, &rule),
            Command::SetName {
                old,
                name,
                formula,
                sheet,
            } => self.set_name_command(old, &name, &formula, sheet),
            Command::DeleteName(which) => self.delete_name_command(&which),
            Command::NamesFromSelection {
                sheet,
                area,
                top,
                left,
                bottom,
                right,
            } => self.names_from_selection(sheet, area, (top, left, bottom, right)),
            Command::SetDrawingBox {
                sheet,
                index,
                x,
                y,
                width,
                height,
            } => self.set_drawing_box(sheet, index, (x, y, width, height)),
            Command::DeleteDrawings { sheet, indexes } => self.delete_drawings(sheet, &indexes),
            Command::InsertChart {
                sheet,
                area,
                chart,
                x,
                y,
            } => self.insert_chart(sheet, area, chart, (x, y)),
            Command::InsertPicture {
                sheet,
                part,
                x,
                y,
                width,
                height,
                name,
            } => self.insert_picture(sheet, &part, (x, y, width, height), &name),
            Command::EditChart { sheet, index, edit } => self.edit_chart(sheet, index, &edit),
            Command::MoveDrawing { sheet, index, to } => self.move_drawing_to(sheet, index, to),
            Command::PictureFormat {
                sheet,
                index,
                change,
            } => self.picture_format(sheet, index, &change),
            Command::SetNote {
                sheet,
                row,
                col,
                text,
                author,
            } => self.set_note_command(sheet, row, col, text, author),
            Command::SetLink {
                sheet,
                area,
                link,
                plain,
            } => self.set_link_command(sheet, area, link.as_ref(), plain),
            Command::RichInput {
                sheet,
                row,
                col,
                text,
                runs,
            } => self.rich_input(sheet, row, col, text, &runs),
            Command::ErrorRules { rules } => {
                self.error_rules = rules;
                self.dirty.sheets.extend(0..self.book.sheets.len() as u32);
                Ok(Applied::default())
            }
            Command::IgnoreError { sheet, row, col } => self.ignore_error(sheet, row, col, locale),
            Command::ConvertToNumber { sheet, areas } => {
                self.convert_to_number(sheet, &areas, locale)
            }
            Command::IncludeCells { sheet, row, col } => {
                self.include_cells(sheet, row, col, locale)
            }
            Command::NoteVisible {
                sheet,
                row,
                col,
                visible,
            } => self.note_visible_command(sheet, row, col, visible),
            Command::SetValidation { sheet, areas, rule } => {
                self.set_validation_command(sheet, &areas, rule.as_ref())
            }
            Command::CircleInvalid { sheet, on } => {
                if on {
                    self.circled.insert(sheet);
                } else {
                    self.circled.remove(&sheet);
                }
                self.dirty.sheets.insert(sheet);
                Ok(Applied::default())
            }
            Command::ClearCf { sheet, areas } => self.clear_cf_command(sheet, areas.as_deref()),
            Command::SetCf { sheet, rules } => self.set_cf_command(sheet, &rules),
            Command::ToggleFilter { sheet, area } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                self.toggle_filter_command(sheet, area)
            }
            Command::SetFilter {
                sheet,
                table,
                col,
                criteria,
            } => self.set_filter_command(sheet, &table, col, criteria, locale),
            Command::CellStyle { sheet, areas, name } => self.apply_cell_style(sheet, areas, &name),
            Command::NewCellStyle {
                name,
                sheet,
                row,
                col,
                includes,
            } => self.new_cell_style(&name, sheet, row, col, includes.into()),
            Command::ModifyCellStyle {
                name,
                change,
                includes,
            } => self.modify_cell_style(&name, *change, includes.map(Into::into)),
            Command::RemoveDuplicates {
                sheet,
                area,
                columns,
                header,
            } => self.remove_duplicates_command(sheet, area, &columns, header, locale),
            Command::ClearFilter { sheet, table } => self.clear_filter_command(sheet, &table),
            Command::ReapplyFilter { sheet, table } => {
                self.reapply_filter_command(sheet, &table, locale)
            }
            Command::Sort {
                sheet,
                area,
                keys,
                header,
                case_sensitive,
                by_columns,
            } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                self.sort_command(sheet, area, &keys, header, case_sensitive, by_columns)
            }
            Command::ShiftCells {
                sheet,
                area,
                down,
                delete,
            } => {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                let op = self.shift_op(sheet, area, down, delete)?;
                let label = if delete {
                    "deleting cells"
                } else {
                    "inserting cells"
                };
                self.perform(label, op, Some((sheet, area)))
            }
            Command::ReplaceOne {
                query,
                with,
                sheet,
                row,
                col,
            } => {
                let options = cgs_numfmt::Options {
                    locale,
                    date1904: self.book.date1904,
                };
                match self.replace_one(&query, &with, (sheet, row, col), &options, locale) {
                    Some(op) => {
                        let mut applied =
                            self.perform("replacing", op, Some((sheet, Area::cell(row, col))))?;
                        applied.replaced = 1;
                        Ok(applied)
                    }
                    None => Ok(Applied::default()),
                }
            }
            Command::ReplaceAll { query, with } => {
                let options = cgs_numfmt::Options {
                    locale,
                    date1904: self.book.date1904,
                };
                let edits = self.replace_all(&query, &with, &options, locale);
                let count = edits.len() as u32;
                if count == 0 {
                    return Ok(Applied::default());
                }
                // Replacing in a locked cell of a protected sheet stops the lot, as in Excel.
                if edits.iter().any(|edit| {
                    self.sheet_protection(edit.sheet).is_some()
                        && !self.all_unlocked(edit.sheet, Area::cell(edit.row, edit.col))
                }) {
                    return Err(crate::protect::PROTECTED.to_string());
                }
                let mut applied = self.perform("replacing", Op::Cells(edits), None)?;
                applied.replaced = count;
                Ok(applied)
            }
            Command::ViewFlags {
                sheet,
                grid,
                headings,
                right_to_left,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let mut view = info.view;
                if let Some(on) = right_to_left {
                    view.right_to_left = on;
                }
                if let Some(show) = grid {
                    view.show_grid = show;
                }
                if let Some(show) = headings {
                    view.show_headers = show;
                }
                self.perform(
                    "sheet view",
                    Op::View {
                        sheet,
                        view,
                        freeze: false,
                    },
                    None,
                )
            }
            Command::PageSetup { sheets, spec } => self.page_setup_command(&sheets, &spec),
            Command::PageBreak {
                sheet,
                row,
                col,
                action,
            } => self.page_break_command(sheet, row, col, action),
            Command::PrintArea {
                sheet,
                areas,
                action,
            } => self.print_area_command(sheet, &areas, action),
            Command::PrintTitles { sheet, rows, cols } => {
                self.print_titles_command(sheet, rows, cols)
            }
            Command::Undo => self.step_back(),
            Command::Redo => self.step_forward(),
        }
    }

    pub(crate) fn sheet_for_edit(&self, sheet: u32) -> Result<&SheetInfo, String> {
        self.book
            .sheets
            .get(sheet as usize)
            .ok_or_else(|| format!("There is no sheet {sheet}."))
    }
}

/// What a copied cell is to an array that is copied whole.
/// Undo and redo as the menu shows them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoState {
    pub undo: Option<&'static str>,
    pub redo: Option<&'static str>,
    pub lost: bool,
}

impl Engine {
    fn command_session(&mut self) -> Result<&mut Session, String> {
        let session = self.session.as_mut().ok_or_else(not_open)?;
        if session.loader.is_some() {
            return Err("This workbook is still being read.".to_string());
        }
        if session.saving.is_some() {
            return Err(crate::edit::SAVING.to_string());
        }
        Ok(session)
    }

    /// Runs a command, which can be told by the UI as JSON. See `Command`.
    pub fn apply(&mut self, command: Command) -> Result<Applied, String> {
        let locale = self.locale.clone();
        self.command_session()?.command(command, &locale)
    }

    pub fn apply_json(&mut self, json: &str) -> Result<Applied, String> {
        let command: Command = serde_json::from_str(json).map_err(|error| error.to_string())?;
        self.apply(command)
    }

    /// What undo and redo would do next.
    pub fn undo_state(&self) -> UndoState {
        let Some(session) = self.session.as_ref() else {
            return UndoState::default();
        };
        UndoState {
            undo: session.history.undo.last().map(|step| step.label),
            redo: session.history.redo.last().map(|step| step.label),
            lost: session.history.lost,
        }
    }

    /// Sets today's date serial, for dates typed with no year.
    pub fn set_today(&mut self, serial: f64) {
        self.today = serial;
        if let Some(session) = self.session.as_mut() {
            session.today = serial;
        }
    }

    /// What has changed for the UI since this was last asked, after folding in what calculation has
    /// finished.
    pub fn take_dirty(&mut self) -> Dirty {
        let Some(session) = self.session.as_mut() else {
            return Dirty::default();
        };
        session.absorb_results();
        std::mem::take(&mut session.dirty)
    }

    /// Calculates for about `budget_ms`, with these areas first. Returns whether more remains.
    pub fn calc_step(
        &mut self,
        budget_ms: u32,
        first: &[cgs_calc::SheetRect],
    ) -> Result<bool, String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(false);
        };
        // Results that change while a save is written would differ from what it writes.
        if session.saving.is_some() {
            return Ok(false);
        }
        if session.manual && !session.force_calc {
            return Ok(false);
        }
        // GETPIVOTDATA reads the pivot tables as they are before anything recalculates.
        if session.pivots_stale {
            session.rebuild_pivot_index();
        }
        if !session.calc.is_calculating() {
            session.force_calc = false;
            return Ok(false);
        }
        let progress = session
            .calc
            .calc_slice(budget_ms, first)
            .map_err(|error| error.0)?;
        if progress == cgs_calc::CalcProgress::Done {
            session.force_calc = false;
            session.absorb_results();
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether anything is waiting that `calculate_now` would do, or is being done.
    pub fn is_calculating(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.calc.is_calculating())
    }

    pub fn is_manual(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.manual)
    }

    /// Turns manual calculation on or off. Going back to automatic catches up.
    pub fn set_manual(&mut self, manual: bool) {
        if let Some(session) = self.session.as_mut() {
            if session.manual != manual {
                session.manual = manual;
                session.edits.calc_mode = Some(manual);
                session.history.touch();
            }
        }
    }

    /// F9, or Ctrl+Alt+F9 with `all`: calculates what is due, even in manual mode.
    pub fn calculate_now(&mut self, all: bool) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        if all {
            session.calc.recalc_all().map_err(|error| error.0)?;
        }
        session.force_calc = true;
        Ok(())
    }

    /// Shift+F9: calculates what this sheet needs, and goes no further.
    pub fn calculate_sheet(&mut self, sheet: u32) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let Some(info) = session.book.sheets.get(sheet as usize) else {
            return Ok(());
        };
        if !session.calc.is_calculating() {
            return Ok(());
        }
        let area = cgs_calc::SheetRect {
            sheet,
            rect: cgs_calc::Rect {
                top: 1,
                left: 1,
                bottom: info.used.0.max(1),
                right: info.used.1.max(1),
            },
        };
        session
            .calc
            .calc_slice(0, &[area])
            .map_err(|error| error.0)?;
        session.absorb_results();
        Ok(())
    }
}

impl EditCell {
    fn bytes(&self) -> usize {
        64 + match &self.input {
            Some(EditInput::Value(CalcValue::Text(text) | CalcValue::Error(text))) => text.len(),
            Some(EditInput::Formula(text) | EditInput::Array(text, _)) => text.len(),
            _ => 0,
        }
    }
}
