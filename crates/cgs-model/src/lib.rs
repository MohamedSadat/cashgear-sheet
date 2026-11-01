//! Everything about a workbook except its cells: styles, dimensions, merges,
//! tables, drawings and the package index, plus commands and undo. No I/O.
//! Phase 0 holds only the grid's limits and tile size.

/// Excel's row limit.
pub const MAX_ROWS: u32 = 1_048_576;
/// Excel's column limit.
pub const MAX_COLS: u32 = 16_384;
/// Rows in one grid tile.
pub const TILE_ROWS: u32 = 64;
/// Columns in one grid tile.
pub const TILE_COLS: u32 = 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_divides_into_whole_tiles() {
        assert_eq!(MAX_ROWS % TILE_ROWS, 0);
        assert_eq!(MAX_COLS % TILE_COLS, 0);
    }
}
