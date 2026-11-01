//! Ties the workbook together: viewport snapshots and, later, calculation
//! scheduling. Phase 0 serves a test pattern through the real tile format.
pub mod test_pattern;
pub mod view_block;

pub use view_block::ViewBlock;

/// The engine a worker holds. It has no workbook yet.
#[derive(Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    /// The tile whose top-left cell is (`first_row`, `first_col`), counted from 0.
    pub fn view_block(&self, first_row: u32, first_col: u32) -> ViewBlock {
        test_pattern::block(first_row, first_col)
    }
}
