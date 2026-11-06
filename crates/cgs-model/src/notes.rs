//! Notes (Excel 2016's comments) and hyperlinks.

use crate::Rect;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    pub author: String,
    pub text: String,
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
