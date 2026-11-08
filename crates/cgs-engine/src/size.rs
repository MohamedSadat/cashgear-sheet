//! Turning away a workbook too big for the engine before reading it, in seconds rather than after
//! minutes of reading that end in running out of memory.
//!
//! Phase 9 measured what the engine holds in WASM: about 430 bytes per formula and 14 per value,
//! against 4 GiB. A sample of each worksheet's XML gives an estimate; only an estimate clearly past
//! the ceiling is checked by counting every cell, and only that count can refuse the workbook. A
//! workbook near the ceiling opens, and the status bar's memory warning takes over from there.

use cgs_model::{Book, SheetKind};
use cgs_xlsx::estimate::count_cells;
use cgs_xlsx::{Package, ReadError};

pub const BYTES_PER_FORMULA: u64 = 430;
pub const BYTES_PER_VALUE: u64 = 14;
/// What WASM can address.
pub const CEILING: u64 = 4 << 30;
/// An estimate this far past the ceiling is no near miss.
pub const REFUSE_ABOVE: u64 = CEILING + CEILING / 4;
/// How much of each worksheet's XML the first estimate reads.
const SAMPLE: u64 = 8 << 20;

/// Formulas and other cells in a workbook, and what the engine would need for them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeEstimate {
    pub formulas: u64,
    pub values: u64,
    /// Every cell was counted, rather than a sample scaled up.
    pub counted: bool,
}

impl SizeEstimate {
    pub fn bytes(&self) -> u64 {
        self.formulas * BYTES_PER_FORMULA + self.values * BYTES_PER_VALUE
    }
}

fn estimate(
    package: &Package,
    book: &Book,
    sample: Option<u64>,
) -> Result<SizeEstimate, ReadError> {
    let mut total = SizeEstimate {
        counted: sample.is_none(),
        ..SizeEstimate::default()
    };
    for sheet in &book.sheets {
        if sheet.kind != SheetKind::Worksheet {
            continue;
        }
        let Some(part) = sheet.part.as_deref() else {
            continue;
        };
        if package.part(part).is_none() {
            continue;
        }
        let count = count_cells(package, part, sample)
            .map_err(|error| ReadError::Damaged(error.to_string()))?;
        let (cells, formulas) = count.extrapolated();
        total.formulas += formulas;
        total.values += cells.saturating_sub(formulas);
    }
    Ok(total)
}

/// Refuses a workbook whose cells would clearly need more memory than the engine can have.
/// Returns what it found: a sample's estimate, or a full count when the sample was past the line.
pub fn check(package: &Package, book: &Book) -> Result<SizeEstimate, ReadError> {
    check_against(package, book, SAMPLE, REFUSE_ABOVE)
}

/// `check`, with the sample and the line given, for tests.
pub fn check_against(
    package: &Package,
    book: &Book,
    sample: u64,
    refuse_above: u64,
) -> Result<SizeEstimate, ReadError> {
    let sampled = estimate(package, book, Some(sample))?;
    if sampled.bytes() <= refuse_above {
        return Ok(sampled);
    }
    let counted = estimate(package, book, None)?;
    if counted.bytes() > refuse_above {
        return Err(ReadError::TooLargeFor {
            needs: counted.bytes(),
            limit: CEILING,
        });
    }
    Ok(counted)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::rc::Rc;

    use cgs_xlsx::Loader;

    use super::*;

    /// `rows` rows of a value and a formula, then `rows` more of two values: a sample of the top
    /// sees only formulas.
    fn book(rows: u32) -> Vec<u8> {
        let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let rels = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let mut sheet = format!(r#"<worksheet xmlns="{main}"><sheetData>"#);
        for row in 1..=rows {
            sheet.push_str(&format!(
                r#"<row r="{row}"><c r="A{row}"><v>1</v></c><c r="B{row}"><f>A{row}+1</f><v>2</v></c></row>"#
            ));
        }
        for row in rows + 1..=2 * rows {
            sheet.push_str(&format!(
                r#"<row r="{row}"><c r="A{row}"><v>1</v></c><c r="B{row}"><v>2</v></c></row>"#
            ));
        }
        sheet.push_str("</sheetData></worksheet>");
        let parts = [
            (
                "_rels/.rels".to_string(),
                format!(
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rels}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
                ),
            ),
            (
                "xl/workbook.xml".to_string(),
                format!(
                    r#"<workbook xmlns="{main}" xmlns:r="{rels}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/></sheets></workbook>"#
                ),
            ),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                format!(
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rels}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
                ),
            ),
            ("xl/worksheets/sheet1.xml".to_string(), sheet),
        ];
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in &parts {
            writer.start_file(name.as_str(), options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn a_sample_past_the_line_is_counted_before_anything_is_refused() {
        let (loader, book) = Loader::open(Rc::new(book(2000))).unwrap();
        let package = loader.package();
        // The sample sees formulas only, so it overestimates: the count finds half as many.
        let found = check_against(
            &package,
            &book,
            4096,
            2000 * BYTES_PER_FORMULA + 6000 * BYTES_PER_VALUE + 100,
        )
        .unwrap();
        assert!(found.counted);
        assert_eq!((found.formulas, found.values), (2000, 6000));
        // Past the line even when counted.
        match check_against(&package, &book, 4096, 1000) {
            Err(ReadError::TooLargeFor { needs, .. }) => {
                assert_eq!(needs, 2000 * BYTES_PER_FORMULA + 6000 * BYTES_PER_VALUE)
            }
            other => panic!("{other:?}"),
        }
        // Well inside it, the sample is enough.
        let sampled = check_against(&package, &book, 4096, u64::MAX / 2).unwrap();
        assert!(!sampled.counted);
    }
}
