//! Phase 0's test tile. Each tile starts with four rows of eight colour bars with
//! explicit fills, which stay the same in either theme. The other cells show their
//! address in the automatic text colour with no fill, so the theme's palette draws
//! them. One cell holds mixed Arabic and Latin text, to carry UTF-8 through the heap.
//! `tests/e2e/pattern.mjs` reads the pattern back, so keep the two in step.
use std::fmt::Write;

use cgs_model::{TILE_COLS, TILE_ROWS};

use crate::view_block::{CellColors, ViewBlock, AUTOMATIC};

/// Rows of colour bars at the top of each tile.
pub const BAR_ROWS: u32 = 4;
/// Left to right: white, yellow, cyan, green, magenta, red, blue, black.
pub const BAR_FILLS: [u32; 8] = [
    0xffff_ffff,
    0xffff_00ff,
    0x00ff_ffff,
    0x00ff_00ff,
    0xff00_ffff,
    0xff00_00ff,
    0x0000_ffff,
    0x0000_00ff,
];
/// The first tile's first cell below the bars.
pub const MIXED_TEXT: &str = "مرحبا Sheet";

pub fn block(first_row: u32, first_col: u32) -> ViewBlock {
    ViewBlock::encode(
        first_row,
        first_col,
        TILE_ROWS as u16,
        TILE_COLS as u16,
        |row, col, text| {
            if row % TILE_ROWS < BAR_ROWS {
                let bar = (col % TILE_COLS) / (TILE_COLS / BAR_FILLS.len() as u32);
                return CellColors {
                    fill: BAR_FILLS[bar as usize],
                    text: AUTOMATIC,
                };
            }
            if (row, col) == (BAR_ROWS, 0) {
                text.push_str(MIXED_TEXT);
            } else {
                write!(text, "R{}C{}", row + 1, col + 1).expect("writing to a String");
            }
            CellColors {
                fill: AUTOMATIC,
                text: AUTOMATIC,
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_block::{CELL_BYTES, HEADER_BYTES};

    fn fnv1a(bytes: &[u8]) -> u32 {
        bytes.iter().fold(0x811c_9dc5, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
        })
    }

    fn cell_field(bytes: &[u8], row: usize, col: usize, field: usize) -> u32 {
        let at = HEADER_BYTES + (row * TILE_COLS as usize + col) * CELL_BYTES + field * 4;
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn bars_have_explicit_fills_and_other_cells_follow_the_theme() {
        let block = block(0, 0);
        let bytes = block.as_bytes();
        for (bar, fill) in BAR_FILLS.iter().enumerate() {
            assert_eq!(cell_field(bytes, 0, bar * 2, 2), *fill);
            assert_eq!(cell_field(bytes, 3, bar * 2 + 1, 2), *fill);
        }
        assert_eq!(cell_field(bytes, 4, 3, 2), AUTOMATIC);
        assert_eq!(cell_field(bytes, 4, 3, 3), AUTOMATIC);
    }

    #[test]
    fn the_text_is_in_the_heap() {
        let block = block(0, 0);
        let bytes = block.as_bytes();
        let heap = HEADER_BYTES + (TILE_ROWS * TILE_COLS) as usize * CELL_BYTES;
        let text = |row, col| {
            let offset = cell_field(bytes, row, col, 0) as usize;
            let length = cell_field(bytes, row, col, 1) as usize;
            std::str::from_utf8(&bytes[heap + offset..heap + offset + length]).unwrap()
        };
        assert_eq!(text(4, 0), MIXED_TEXT);
        assert_eq!(text(5, 1), "R6C2");
        assert_eq!(text(0, 0), "");
    }

    #[test]
    fn later_tiles_use_their_own_addresses() {
        let block = block(64, 16);
        let bytes = block.as_bytes();
        let heap = HEADER_BYTES + (TILE_ROWS * TILE_COLS) as usize * CELL_BYTES;
        let offset = cell_field(bytes, 4, 0, 0) as usize;
        let length = cell_field(bytes, 4, 0, 1) as usize;
        assert_eq!(&bytes[heap + offset..heap + offset + length], b"R69C17");
        assert_eq!(cell_field(bytes, 0, 0, 2), BAR_FILLS[0]);
    }

    // Web and desktop draw the same bytes. tests/e2e/pattern.mjs holds the same
    // value, and the browser and desktop checks compare against it.
    #[test]
    fn the_first_tile_has_the_expected_hash() {
        assert_eq!(fnv1a(block(0, 0).as_bytes()), 2_069_359_672);
    }
}
