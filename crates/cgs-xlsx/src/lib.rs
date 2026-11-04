//! Reads and saves `.xlsx` packages.
//!
//! Opening reads what is small and needed at once (`workbook.xml`, styles, theme, tables).
//! `Loader` then reads shared strings and worksheets a little at a time, sending values
//! straight to the formula engine and everything else to the model. Nothing calculates.
//!
//! Saving is in `save`: it patches changed parts at source byte spans and copies every other
//! entry as the bytes it is, so what this app can't edit survives.

pub mod blank;
pub mod package;
pub mod patch_parts;
pub mod patch_sheet;
pub mod save;
pub mod scan;
pub mod sheet;
pub mod strings;
pub mod styles;
pub mod workbook;
pub mod xml;
pub mod zipdir;

use std::rc::Rc;

use cgs_calc::{BookInfo, CalcEngine, NameInfo, Rect, TableInfo};
use cgs_model::{Book, SheetKind};

pub use package::{ByteSource, Package};
use sheet::{LoadContext, SheetLoader, Step};
use strings::{SharedStrings, StringsLoader};

/// Why a file wouldn't open, in terms the app can explain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// A password-protected file.
    Encrypted,
    /// An old binary `.xls`, or another compound file.
    OldFormat,
    /// Not a zip file, so not `.xlsx`.
    NotAPackage(String),
    /// A zip file with no workbook in it.
    NotAWorkbook,
    /// More than this app can hold in memory.
    TooLarge,
    Damaged(String),
    Engine(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encrypted => f.write_str("This file is password protected."),
            Self::OldFormat => f.write_str("This is an old-format Excel file (.xls)."),
            Self::NotAPackage(_) => f.write_str("This isn't an Excel workbook, or it is damaged."),
            Self::NotAWorkbook => f.write_str("This file has no workbook in it."),
            Self::TooLarge => f.write_str("This workbook is too large to open here."),
            Self::Damaged(detail) => write!(f, "This workbook is damaged: {detail}"),
            Self::Engine(detail) => write!(f, "The workbook couldn't be loaded: {detail}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// What the formula engine needs before any cell arrives.
pub fn book_info(book: &Book) -> BookInfo {
    BookInfo {
        sheets: book.sheets.iter().map(|sheet| sheet.name.clone()).collect(),
        names: book
            .names
            .iter()
            // Built-in names such as print areas never feed a formula, and some aren't
            // valid formulas at all.
            .filter(|name| !name.name.starts_with("_xlnm.") && !name.formula.contains("#REF!"))
            .map(|name| NameInfo {
                name: name.name.clone(),
                formula: name.formula.clone(),
                local_sheet: name.local_sheet,
            })
            .collect(),
        tables: book
            .tables
            .iter()
            .map(|table| TableInfo {
                name: table.name.clone(),
                sheet: table.sheet,
                range: Rect {
                    top: table.range.top,
                    left: table.range.left,
                    bottom: table.range.bottom,
                    right: table.range.right,
                },
                header_row: table.header_rows > 0,
                totals_row: table.totals_rows > 0,
                columns: table.columns.clone(),
            })
            .collect(),
        hidden_rows: Vec::new(),
        date1904: book.date1904,
    }
}

/// How far a load has come, in compressed bytes of the file's cell parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadProgress {
    pub done: u64,
    pub total: u64,
}

/// One workbook's way into the engine and the model.
pub struct Loader {
    package: Rc<Package>,
    strings_loader: Option<StringsLoader>,
    strings: SharedStrings,
    /// Worksheets still to read, the next one last.
    queue: Vec<u32>,
    current: Option<SheetLoader>,
    /// Sheets set aside because another one was asked for first.
    paused: Vec<SheetLoader>,
    digit: f64,
    /// How tall a row must be for each cell format's font, in pixels at 100%.
    xf_heights: Vec<u32>,
    finished_bytes: u64,
    total_bytes: u64,
}

/// The height Excel fits a row to for a font of this size, in pixels at 100%.
fn fitted_row_px(points: f32) -> u32 {
    (f64::from(points) * 1.75).ceil() as u32
}

impl Loader {
    /// Opens the package, and reads what must come before any cell.
    pub fn open(source: Rc<dyn ByteSource>) -> Result<(Self, Book), ReadError> {
        let package = Rc::new(Package::open(source)?);
        let mut book = workbook::read_workbook(&package)?;
        let digit = cgs_model::style::max_digit_width(book.styles.normal_font());
        for sheet in &mut book.sheets {
            sheet.rows.default_px = 20;
            sheet.cols.default_px = cgs_model::style::base_col_width_to_px(8.0, digit);
        }

        let xf_heights = book
            .styles
            .xfs
            .iter()
            .map(|xf| fitted_row_px(book.styles.font(xf).size))
            .collect();

        let mut total_bytes = 0;
        let strings_part = workbook::shared_strings_part(&package);
        if let Some(part) = strings_part.as_deref().and_then(|name| package.part(name)) {
            total_bytes += part.compressed;
        }
        let strings_loader = strings_part
            .as_deref()
            .and_then(|name| package.stream(name))
            .map(StringsLoader::new);

        let mut queue = Vec::new();
        for (index, sheet) in book.sheets.iter().enumerate() {
            if let (SheetKind::Worksheet, Some(part)) = (sheet.kind, &sheet.part) {
                total_bytes += package.part(part).map_or(0, |part| part.compressed);
                queue.push(index as u32);
            }
        }
        // Last in the queue goes first: the sheet showing when the file was saved, and
        // after it the others from the left.
        queue.reverse();
        let active = book.active_sheet;
        if let Some(at) = queue.iter().position(|&sheet| sheet == active) {
            queue.remove(at);
            queue.push(active);
        }

        Ok((
            Self {
                package,
                strings_loader,
                strings: SharedStrings::default(),
                queue,
                current: None,
                paused: Vec::new(),
                digit,
                xf_heights,
                finished_bytes: 0,
                total_bytes,
            },
            book,
        ))
    }

    pub fn package(&self) -> Rc<Package> {
        Rc::clone(&self.package)
    }

    pub fn progress(&self) -> LoadProgress {
        let strings = self
            .strings_loader
            .as_ref()
            .map_or(0, StringsLoader::consumed);
        let current = self.current.as_ref().map_or(0, SheetLoader::consumed);
        let paused: u64 = self.paused.iter().map(SheetLoader::consumed).sum();
        LoadProgress {
            done: self.finished_bytes + strings + current + paused,
            total: self.total_bytes,
        }
    }

    /// Reads this sheet next, if it isn't read yet.
    pub fn prioritize(&mut self, sheet: u32) {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.sheet_index() == sheet)
        {
            return;
        }
        if let Some(at) = self.queue.iter().position(|&queued| queued == sheet) {
            self.queue.remove(at);
            self.queue.push(sheet);
        } else if let Some(at) = self.paused.iter().position(|p| p.sheet_index() == sheet) {
            let wanted = self.paused.remove(at);
            self.paused.extend(self.current.take());
            self.current = Some(wanted);
            return;
        } else {
            return;
        }
        self.paused.extend(self.current.take());
    }

    /// Works until `keep_going` says stop. Returns whether everything is read.
    pub fn step(
        &mut self,
        book: &mut Book,
        engine: &mut dyn CalcEngine,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<bool, ReadError> {
        if let Some(strings) = self.strings_loader.as_mut() {
            loop {
                if strings.step(4096) {
                    break;
                }
                if !keep_going() {
                    return Ok(false);
                }
            }
            if let Some(done) = self.strings_loader.take() {
                self.finished_bytes += done.consumed();
                self.strings = done.finish();
            }
        }
        loop {
            if self.current.is_none() {
                self.current = self.next_loader(book);
            }
            let Some(current) = self.current.as_mut() else {
                self.strings = SharedStrings::default();
                return Ok(true);
            };
            let sheet = current.sheet_index();
            let info = &mut book.sheets[sheet as usize];
            let mut context = LoadContext {
                engine: &mut *engine,
                strings: &self.strings,
                digit: self.digit,
                xf_heights: &self.xf_heights,
            };
            match current.step(&mut context, info, keep_going)? {
                Step::More => return Ok(false),
                Step::Done => {
                    if let Some(done) = self.current.take() {
                        self.finished_bytes += done.consumed();
                        if let Some(rel) = done.drawing_rel.as_deref() {
                            self.read_drawings(book, sheet, rel);
                        }
                    }
                    if !keep_going() {
                        return Ok(false);
                    }
                }
                Step::Restart(shape) => {
                    let part = info.part.clone().unwrap_or_default();
                    let size = self.package.part(&part).map_or(0, |part| part.size);
                    let stream = self
                        .package
                        .stream(&part)
                        .ok_or_else(|| ReadError::Damaged(format!("missing part {part}")))?;
                    info.reset_cells();
                    self.current = Some(SheetLoader::restarted(sheet, stream, size, shape));
                }
            }
        }
    }

    fn next_loader(&mut self, book: &mut Book) -> Option<SheetLoader> {
        while let Some(sheet) = self.queue.pop() {
            let info = &mut book.sheets[sheet as usize];
            let found = info.part.as_deref().and_then(|part| {
                let size = self.package.part(part)?.size;
                Some((self.package.stream(part)?, size))
            });
            match found {
                Some((stream, size)) => return Some(SheetLoader::new(sheet, stream, size)),
                // A sheet whose part is missing shows empty, as Excel's repair leaves it.
                None => info.loaded = true,
            }
        }
        self.paused.pop()
    }

    fn read_drawings(&self, book: &mut Book, sheet: u32, rel: &str) {
        let info = &mut book.sheets[sheet as usize];
        let Some(part) = info.part.clone() else {
            return;
        };
        let rels = workbook::read_relationships(&self.package, &part);
        if let Some((_, target)) = rels.get(rel) {
            if let Some(xml) = self.package.read(target) {
                info.drawings = workbook::read_drawing(&xml);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fitted_row_px;

    #[test]
    fn rows_fit_large_fonts_as_excel_fits_them() {
        assert_eq!(fitted_row_px(11.0), 20);
        assert_eq!(fitted_row_px(14.0), 25);
        assert_eq!(fitted_row_px(16.0), 28);
        assert_eq!(fitted_row_px(20.0), 35);
        assert_eq!(fitted_row_px(24.0), 42);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io::Write;

    /// A deflated zip holding these parts.
    pub fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, text) in parts {
                writer.start_file(*name, options).unwrap();
                writer.write_all(text.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        out.into_inner()
    }
}
