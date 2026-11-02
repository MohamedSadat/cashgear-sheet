//! The binary tile sent from the engine to the tile worker, transferred rather than sent as
//! JSON. Everything is little-endian. `packages/engine-client/src/view-block.ts` reads it, so
//! keep the two in step. A tile describes itself completely, colours resolved for one theme,
//! so that drawing needs nothing else.
//!
//! Header (32 bytes): magic `CGVB`, u16 version, u16 rows, u16 columns, u16 flags, u32 first row,
//! u32 first column, u32 style count, u32 extra count, u32 heap length.
//!
//! Then u16 row heights and u16 column widths, in CSS pixels at 100% with 0 for hidden, padded
//! to 4 bytes.
//!
//! Then `rows * columns` cells of 24 bytes, row by row: u32 text offset into the heap, u32 text
//! length in bytes (0 means none), u32 fill, u32 text colour, u16 style, u16 flags, u32 fill
//! position (see `Display::fill`: 0 means none, otherwise the byte position plus 1). Colours
//! are `0xRRGGBBAA`. Alpha 0 means no fill or the automatic text colour, which the theme's
//! palette supplies when the tile is drawn.
//!
//! Then styles of 32 bytes: u32 font offset, u32 font length, u16 size in hundredths of a
//! point, u8 font flags, u8 underline, u8 horizontal, u8 vertical, u8 indent, u8 rotation, u8
//! layout flags, u8 right line, u8 bottom line, u8 diagonal line, u32 right colour, u32 bottom
//! colour, u32 diagonal colour.
//!
//! Then extras of 40 bytes, text that belongs to a cell outside the tile or spanning it: u8
//! kind, 3 bytes of padding, i32 x, i32 y, i32 width, i32 height (the source cell or merged
//! area in CSS pixels at 100%, relative to the tile's top left), u32 text offset, u32 text
//! length, u32 text colour, u16 style, u16 flags, u32 fill position.
//!
//! Then the heap of UTF-8 text.

pub const MAGIC: [u8; 4] = *b"CGVB";
pub const VERSION: u16 = 2;
pub const HEADER_BYTES: usize = 32;
pub const CELL_BYTES: usize = 24;
pub const STYLE_BYTES: usize = 32;
pub const EXTRA_BYTES: usize = 40;

/// Alpha 0: the theme supplies this colour.
pub const AUTOMATIC: u32 = 0;

/// Header flag: the sheet isn't read yet, so this tile says only its size.
pub const TILE_NOT_READY: u16 = 1;

/// What kind of value a cell holds, in the low 2 bits of its flags.
pub const KIND_TEXT: u16 = 0;
pub const KIND_NUMBER: u16 = 1;
pub const KIND_GENERAL: u16 = 2;
pub const KIND_OTHER: u16 = 3;
pub const KIND_MASK: u16 = 3;
/// The text reads right to left.
pub const CELL_RTL: u16 = 1 << 2;
/// Part of a merged area, whose text is an extra. Nothing overflows into it.
pub const CELL_MERGED: u16 = 1 << 3;
pub const CELL_NO_RIGHT_GRID: u16 = 1 << 4;
pub const CELL_NO_BOTTOM_GRID: u16 = 1 << 5;
/// `####` whatever the width.
pub const CELL_HASHES: u16 = 1 << 6;
/// Holds a value that displays as nothing, which still stops text overflowing into it.
pub const CELL_OCCUPIED: u16 = 1 << 7;

pub const FONT_BOLD: u8 = 1;
pub const FONT_ITALIC: u8 = 2;
pub const FONT_STRIKE: u8 = 4;

pub const LAYOUT_WRAP: u8 = 1;
pub const LAYOUT_SHRINK: u8 = 2;
pub const LAYOUT_DIAGONAL_UP: u8 = 4;
pub const LAYOUT_DIAGONAL_DOWN: u8 = 8;

pub const EXTRA_OVERFLOW: u8 = 0;
pub const EXTRA_MERGE: u8 = 1;

/// A cell's fixed-size record, apart from its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellRecord {
    pub fill: u32,
    pub color: u32,
    pub style: u16,
    pub flags: u16,
    pub fill_at: u32,
}

/// A style as a tile holds it, with its font's name passed alongside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct StyleRecord {
    pub size: u16,
    pub font_flags: u8,
    pub underline: u8,
    pub horizontal: u8,
    pub vertical: u8,
    pub indent: u8,
    pub rotation: u8,
    pub layout: u8,
    pub right_line: u8,
    pub bottom_line: u8,
    pub diagonal_line: u8,
    pub right_color: u32,
    pub bottom_color: u32,
    pub diagonal_color: u32,
}

/// Text from a cell outside the tile, or spanning it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ExtraRecord {
    pub kind: u8,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub color: u32,
    pub style: u16,
    pub flags: u16,
    pub fill_at: u32,
}

/// Writes one tile. Cells must be added row by row.
#[derive(Debug, Default)]
pub struct TileWriter {
    flags: u16,
    first_row: u32,
    first_col: u32,
    row_px: Vec<u16>,
    col_px: Vec<u16>,
    cells: Vec<u8>,
    styles: Vec<u8>,
    extras: Vec<u8>,
    heap: Vec<u8>,
}

impl TileWriter {
    pub fn new(first_row: u32, first_col: u32, row_px: Vec<u16>, col_px: Vec<u16>) -> Self {
        Self {
            flags: 0,
            first_row,
            first_col,
            cells: Vec::with_capacity(row_px.len() * col_px.len() * CELL_BYTES),
            row_px,
            col_px,
            styles: Vec::new(),
            extras: Vec::new(),
            heap: Vec::new(),
        }
    }

    pub fn set_flags(&mut self, flags: u16) {
        self.flags = flags;
    }

    fn push_text(&mut self, text: &str) -> (u32, u32) {
        let offset = self.heap.len() as u32;
        self.heap.extend_from_slice(text.as_bytes());
        (offset, text.len() as u32)
    }

    pub fn cell(&mut self, text: &str, record: CellRecord) {
        let (offset, length) = self.push_text(text);
        self.cells.extend_from_slice(&offset.to_le_bytes());
        self.cells.extend_from_slice(&length.to_le_bytes());
        self.cells.extend_from_slice(&record.fill.to_le_bytes());
        self.cells.extend_from_slice(&record.color.to_le_bytes());
        self.cells.extend_from_slice(&record.style.to_le_bytes());
        self.cells.extend_from_slice(&record.flags.to_le_bytes());
        self.cells.extend_from_slice(&record.fill_at.to_le_bytes());
    }

    /// Adds a style, and returns its index.
    pub fn style(&mut self, font: &str, style: StyleRecord) -> u16 {
        let index = (self.styles.len() / STYLE_BYTES) as u16;
        let (offset, length) = self.push_text(font);
        self.styles.extend_from_slice(&offset.to_le_bytes());
        self.styles.extend_from_slice(&length.to_le_bytes());
        self.styles.extend_from_slice(&style.size.to_le_bytes());
        self.styles.extend_from_slice(&[
            style.font_flags,
            style.underline,
            style.horizontal,
            style.vertical,
            style.indent,
            style.rotation,
            style.layout,
            style.right_line,
            style.bottom_line,
            style.diagonal_line,
        ]);
        self.styles
            .extend_from_slice(&style.right_color.to_le_bytes());
        self.styles
            .extend_from_slice(&style.bottom_color.to_le_bytes());
        self.styles
            .extend_from_slice(&style.diagonal_color.to_le_bytes());
        index
    }

    pub fn extra(&mut self, text: &str, record: ExtraRecord) {
        let (offset, length) = self.push_text(text);
        self.extras.extend_from_slice(&[record.kind, 0, 0, 0]);
        self.extras.extend_from_slice(&record.x.to_le_bytes());
        self.extras.extend_from_slice(&record.y.to_le_bytes());
        self.extras.extend_from_slice(&record.width.to_le_bytes());
        self.extras.extend_from_slice(&record.height.to_le_bytes());
        self.extras.extend_from_slice(&offset.to_le_bytes());
        self.extras.extend_from_slice(&length.to_le_bytes());
        self.extras.extend_from_slice(&record.color.to_le_bytes());
        self.extras.extend_from_slice(&record.style.to_le_bytes());
        self.extras.extend_from_slice(&record.flags.to_le_bytes());
        self.extras.extend_from_slice(&record.fill_at.to_le_bytes());
    }

    pub fn finish(self) -> ViewBlock {
        let rows = self.row_px.len() as u16;
        let cols = self.col_px.len() as u16;
        let sizes = (self.row_px.len() + self.col_px.len()) * 2;
        let padding = (4 - (HEADER_BYTES + sizes) % 4) % 4;
        let mut bytes = Vec::with_capacity(
            HEADER_BYTES
                + sizes
                + padding
                + self.cells.len()
                + self.styles.len()
                + self.extras.len()
                + self.heap.len(),
        );
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&rows.to_le_bytes());
        bytes.extend_from_slice(&cols.to_le_bytes());
        bytes.extend_from_slice(&self.flags.to_le_bytes());
        bytes.extend_from_slice(&self.first_row.to_le_bytes());
        bytes.extend_from_slice(&self.first_col.to_le_bytes());
        bytes.extend_from_slice(&((self.styles.len() / STYLE_BYTES) as u32).to_le_bytes());
        bytes.extend_from_slice(&((self.extras.len() / EXTRA_BYTES) as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.heap.len() as u32).to_le_bytes());
        for px in self.row_px.iter().chain(&self.col_px) {
            bytes.extend_from_slice(&px.to_le_bytes());
        }
        bytes.resize(bytes.len() + padding, 0);
        bytes.extend_from_slice(&self.cells);
        bytes.extend_from_slice(&self.styles);
        bytes.extend_from_slice(&self.extras);
        bytes.extend_from_slice(&self.heap);
        ViewBlock { bytes }
    }
}

/// An encoded tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewBlock {
    bytes: Vec<u8>,
}

impl ViewBlock {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn lays_out_every_section() {
        let mut writer = TileWriter::new(64, 16, vec![20], vec![64, 0]);
        let style = writer.style(
            "Calibri",
            StyleRecord {
                size: 1100,
                right_color: 0x1122_33ff,
                ..StyleRecord::default()
            },
        );
        writer.cell(
            "مرحبا",
            CellRecord {
                fill: 0x1122_33ff,
                color: AUTOMATIC,
                style,
                flags: KIND_TEXT | CELL_RTL,
                fill_at: 0,
            },
        );
        writer.cell("", CellRecord::default());
        writer.extra(
            "x",
            ExtraRecord {
                kind: EXTRA_MERGE,
                x: -64,
                ..ExtraRecord::default()
            },
        );
        let block = writer.finish();
        let bytes = block.as_bytes();
        assert_eq!(&bytes[..4], b"CGVB");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 2);
        assert_eq!(read_u32(bytes, 12), 64);
        assert_eq!(read_u32(bytes, 16), 16);
        assert_eq!(read_u32(bytes, 20), 1, "one style");
        assert_eq!(read_u32(bytes, 24), 1, "one extra");

        let sizes = HEADER_BYTES + 6;
        assert_eq!(sizes % 4, 2);
        let cells = sizes + 2;
        let text_length = read_u32(bytes, cells + 4);
        assert_eq!(text_length as usize, "مرحبا".len());
        assert_eq!(read_u32(bytes, cells + 8), 0x1122_33ff);

        let styles = cells + 2 * CELL_BYTES;
        assert_eq!(read_u32(bytes, styles + 20), 0x1122_33ff);
        let extras = styles + STYLE_BYTES;
        assert_eq!(bytes[extras], EXTRA_MERGE);
        assert_eq!(read_u32(bytes, extras + 4) as i32, -64);
        let heap = extras + EXTRA_BYTES;
        assert_eq!(read_u32(bytes, 28) as usize, bytes.len() - heap);
        assert_eq!(&bytes[heap..], "Calibriمرحباx".as_bytes());
    }
}
