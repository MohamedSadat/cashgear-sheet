//! The binary tile sent from the engine to the tile worker, transferred rather
//! than sent as JSON. Everything is little-endian. `packages/engine-client/src/view-block.ts`
//! reads it, so keep the two in step.
//!
//! Header (24 bytes): magic `CGVB`, u16 version, u16 rows, u16 columns, u16 reserved,
//! u32 first row, u32 first column, u32 heap length.
//!
//! Then `rows * columns` cells of 24 bytes, row by row: u32 text offset into the
//! heap, u32 text length in bytes (0 means none), u32 fill, u32 text colour, and
//! 8 bytes kept for Phase 2. Colours are `0xRRGGBBAA`. Alpha 0 means no fill or
//! the automatic text colour, which the theme's palette supplies when the tile
//! is drawn, so explicit colours stay as stored in either theme.
//!
//! Then the heap of UTF-8 text.

pub const MAGIC: [u8; 4] = *b"CGVB";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 24;
pub const CELL_BYTES: usize = 24;

/// Alpha 0: the theme supplies this colour.
pub const AUTOMATIC: u32 = 0;

/// A cell's colours. Its text goes in the `String` the encoder passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellColors {
    pub fill: u32,
    pub text: u32,
}

/// An encoded tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewBlock {
    bytes: Vec<u8>,
}

impl ViewBlock {
    /// Encodes `cell(row, col, text)` for each cell of the tile, in absolute coordinates.
    pub fn encode(
        first_row: u32,
        first_col: u32,
        rows: u16,
        cols: u16,
        mut cell: impl FnMut(u32, u32, &mut String) -> CellColors,
    ) -> Self {
        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols) * CELL_BYTES);
        let mut heap = Vec::new();
        let mut text = String::new();
        for row in 0..u32::from(rows) {
            for col in 0..u32::from(cols) {
                text.clear();
                let colors = cell(first_row + row, first_col + col, &mut text);
                cells.extend_from_slice(&(heap.len() as u32).to_le_bytes());
                cells.extend_from_slice(&(text.len() as u32).to_le_bytes());
                cells.extend_from_slice(&colors.fill.to_le_bytes());
                cells.extend_from_slice(&colors.text.to_le_bytes());
                cells.extend_from_slice(&[0; 8]);
                heap.extend_from_slice(text.as_bytes());
            }
        }
        let mut bytes = Vec::with_capacity(HEADER_BYTES + cells.len() + heap.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&rows.to_le_bytes());
        bytes.extend_from_slice(&cols.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&first_row.to_le_bytes());
        bytes.extend_from_slice(&first_col.to_le_bytes());
        bytes.extend_from_slice(&(heap.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&cells);
        bytes.extend_from_slice(&heap);
        Self { bytes }
    }

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
    fn lays_out_header_cells_and_heap() {
        let block = ViewBlock::encode(64, 16, 1, 2, |row, col, text| {
            if col == 16 {
                text.push_str(&format!("{row},{col}"));
            } else {
                text.push_str("مرحبا");
            }
            CellColors {
                fill: 0x1122_33ff,
                text: AUTOMATIC,
            }
        });
        let bytes = block.as_bytes();
        assert_eq!(&bytes[..4], b"CGVB");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 2);
        assert_eq!(read_u32(bytes, 12), 64);
        assert_eq!(read_u32(bytes, 16), 16);
        let heap = HEADER_BYTES + 2 * CELL_BYTES;
        assert_eq!(read_u32(bytes, 20) as usize, bytes.len() - heap);

        let second = HEADER_BYTES + CELL_BYTES;
        assert_eq!(read_u32(bytes, HEADER_BYTES), 0);
        assert_eq!(read_u32(bytes, HEADER_BYTES + 4), 5);
        assert_eq!(read_u32(bytes, HEADER_BYTES + 8), 0x1122_33ff);
        assert_eq!(read_u32(bytes, HEADER_BYTES + 12), AUTOMATIC);
        assert_eq!(read_u32(bytes, second), 5);
        assert_eq!(read_u32(bytes, second + 4), "مرحبا".len() as u32);
        assert_eq!(&bytes[heap..heap + 5], b"64,16");
        assert_eq!(&bytes[heap + 5..], "مرحبا".as_bytes());
    }
}
