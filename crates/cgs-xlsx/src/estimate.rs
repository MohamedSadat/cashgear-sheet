//! How many cells and formulas a worksheet holds, counted from its XML before it is read, so that
//! a workbook too big for the engine can be turned away in seconds rather than after minutes of
//! reading.
//!
//! Counting looks for `<c` and `<f` tags only, without parsing. A shared formula's followers each
//! have an `<f>` of their own, so they count once each, as the engine holds them.

use std::io::{self, Read};

use crate::package::Package;

/// What a part holds, as far as it was read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellCount {
    /// Cells of every kind, formulas included.
    pub cells: u64,
    pub formulas: u64,
    /// Bytes of XML read, and in the whole part.
    pub read: u64,
    pub size: u64,
}

impl CellCount {
    /// The counts scaled up to the whole part, as if the rest were like what was read.
    pub fn extrapolated(&self) -> (u64, u64) {
        if self.read == 0 || self.read >= self.size {
            return (self.cells, self.formulas);
        }
        let scale = self.size as f64 / self.read as f64;
        (
            (self.cells as f64 * scale).round() as u64,
            (self.formulas as f64 * scale).round() as u64,
        )
    }
}

/// Counts a part's `<c>` and `<f>` tags in its first `limit` bytes of XML, or all of it.
pub fn count_cells(package: &Package, part: &str, limit: Option<u64>) -> io::Result<CellCount> {
    let info = package
        .part(part)
        .ok_or_else(|| io::Error::other("a part went missing"))?;
    let mut stream = package
        .stream(part)
        .ok_or_else(|| io::Error::other("a part went missing"))?;
    let mut count = CellCount {
        size: info.size,
        ..CellCount::default()
    };
    let limit = limit.unwrap_or(u64::MAX);
    let mut buffer = vec![0u8; 256 * 1024];
    // A tag can straddle two reads: keep its start.
    let mut carry = 0usize;
    while count.read < limit {
        let got = stream.read(&mut buffer[carry..])?;
        if got == 0 {
            break;
        }
        count.read += got as u64;
        let filled = carry + got;
        let (cells, formulas, used) = tags(&buffer[..filled]);
        count.cells += cells;
        count.formulas += formulas;
        buffer.copy_within(used..filled, 0);
        carry = filled - used;
    }
    Ok(count)
}

/// Counts whole `<c` and `<f` tags. Returns them, and how much was looked at: a `<` too close to
/// the end to tell is left for the next read.
fn tags(bytes: &[u8]) -> (u64, u64, usize) {
    let mut cells = 0;
    let mut formulas = 0;
    let mut at = 0;
    while let Some(found) = memchr::memchr(b'<', &bytes[at..]) {
        let open = at + found;
        if open + 2 >= bytes.len() {
            return (cells, formulas, open);
        }
        let after = bytes[open + 2];
        let ends_name = matches!(after, b' ' | b'>' | b'/' | b'\t' | b'\r' | b'\n');
        match bytes[open + 1] {
            b'c' if ends_name => cells += 1,
            b'f' if ends_name => formulas += 1,
            _ => {}
        }
        at = open + 1;
    }
    (cells, formulas, bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cells_and_formulas_but_not_lookalikes() {
        let xml = br#"<worksheet><cols><col min="1"/></cols><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B2" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="A2" s="1"/><c r="B2"><f t="shared" si="0"/><v>4</v></c></row></sheetData><conditionalFormatting><cfRule type="x"><formula>1</formula></cfRule></conditionalFormatting></worksheet>"#;
        let (cells, formulas, used) = tags(xml);
        assert_eq!((cells, formulas, used), (4, 2, xml.len()));
    }

    #[test]
    fn a_tag_split_between_reads_waits_for_the_next() {
        let (cells, _, used) = tags(b"<row><c");
        assert_eq!((cells, used), (0, 5));
        assert_eq!(tags(b"<c r=\"A1\">").0, 1);
    }

    #[test]
    fn a_sample_scales_up_to_the_part() {
        let count = CellCount {
            cells: 10,
            formulas: 4,
            read: 100,
            size: 1000,
        };
        assert_eq!(count.extrapolated(), (100, 40));
    }
}
