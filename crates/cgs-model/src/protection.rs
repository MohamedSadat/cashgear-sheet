//! Sheet and workbook protection, as `sheetProtection` and `workbookProtection` hold it.

/// A password as a file keeps it: never the password itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordHash {
    /// The old 16-bit hash, as four hex digits in `password`.
    Legacy(u16),
    /// `algorithmName`, `hashValue` and `saltValue` (both base64) and `spinCount`, as Excel 2010
    /// and later write it.
    Hashed {
        algorithm: String,
        hash: String,
        salt: String,
        spin: u32,
    },
}

/// What a sheet's protection allows, and whether it is on.
///
/// The file says which things are protected (`formatCells="0"` allows formatting), and these say
/// which are allowed, as Excel's Protect Sheet dialog does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetProtection {
    /// The sheet's contents are protected: locked cells can't change. This is what makes a sheet
    /// protected, in Excel's `ProtectContents`.
    pub sheet: bool,
    /// Drawings, charts and notes can't change.
    pub objects: bool,
    pub scenarios: bool,
    pub format_cells: bool,
    pub format_columns: bool,
    pub format_rows: bool,
    pub insert_columns: bool,
    pub insert_rows: bool,
    pub insert_hyperlinks: bool,
    pub delete_columns: bool,
    pub delete_rows: bool,
    pub select_locked: bool,
    pub select_unlocked: bool,
    pub sort: bool,
    pub auto_filter: bool,
    pub pivot_tables: bool,
    pub password: Option<PasswordHash>,
}

impl Default for SheetProtection {
    /// Protect Sheet's own choices: contents, objects and scenarios protected, and only selecting
    /// cells allowed.
    fn default() -> Self {
        Self {
            sheet: true,
            objects: true,
            scenarios: true,
            format_cells: false,
            format_columns: false,
            format_rows: false,
            insert_columns: false,
            insert_rows: false,
            insert_hyperlinks: false,
            delete_columns: false,
            delete_rows: false,
            select_locked: true,
            select_unlocked: true,
            sort: false,
            auto_filter: false,
            pivot_tables: false,
            password: None,
        }
    }
}

impl SheetProtection {
    /// What the file says when `<sheetProtection>` is there but says nothing: each attribute's
    /// schema default, so nothing is protected and everything is allowed.
    pub fn unset() -> Self {
        Self {
            sheet: false,
            objects: false,
            scenarios: false,
            format_cells: false,
            format_columns: false,
            format_rows: false,
            insert_columns: false,
            insert_rows: false,
            insert_hyperlinks: false,
            delete_columns: false,
            delete_rows: false,
            select_locked: true,
            select_unlocked: true,
            sort: false,
            auto_filter: false,
            pivot_tables: false,
            password: None,
        }
    }
}

/// Protect Workbook's choices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkbookProtection {
    /// Sheets can't be added, deleted, renamed, moved, hidden or shown.
    pub structure: bool,
    /// The windows keep their size and place. Excel 2016 keeps it but can't set it.
    pub windows: bool,
    pub password: Option<PasswordHash>,
}
