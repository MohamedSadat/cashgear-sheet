//! Notes (Excel 2016's comments) and hyperlinks.

use crate::Rect;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    pub author: String,
    pub text: String,
    /// Show/Hide Note: the box shows all the time, not only on hover.
    pub visible: bool,
    /// Where its box is, as its VML shape's `x:Anchor` says: the column and pixels into it, the
    /// row and pixels into it, of the box's top left and then its bottom right, counted from 0.
    /// `None` for a new note, which goes where Excel puts one.
    pub anchor: Option<[u32; 8]>,
}

impl Note {
    /// Where Excel 2016 puts a new note's box, measured over COM: from 15 pixels into the next
    /// column and 10 into the row above, to 31 pixels into the column after next and 9 into the
    /// row three below: 144 by 79 pixels at default sizes.
    pub fn default_anchor(row: u32, col: u32) -> [u32; 8] {
        let (r, c) = (row - 1, col - 1);
        [c + 1, 15, r.saturating_sub(1), 10, c + 3, 31, r + 3, 9]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    pub area: Rect,
    /// A web address, file or `mailto:` link, outside the workbook.
    pub target: Option<String>,
    /// A place in the workbook, such as `Sheet2!A1` or a name.
    pub location: Option<String>,
    pub tooltip: Option<String>,
    pub display: Option<String>,
}
