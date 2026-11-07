//! A sheet's page setup, as `pageSetup`, `pageMargins`, `printOptions`, `headerFooter`,
//! `rowBreaks` and `colBreaks` hold it, and the codes headers and footers are written in.

/// Which way a page is turned. `Default` is the printer's, which is portrait for every printer
/// this app models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Default,
    Portrait,
    Landscape,
}

/// The order pages are numbered in when a sheet is more than a page wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageOrder {
    #[default]
    DownThenOver,
    OverThenDown,
}

/// Where notes print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintComments {
    #[default]
    None,
    AtEnd,
    AsDisplayed,
}

/// How error values print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintErrors {
    #[default]
    Displayed,
    Blank,
    Dash,
    Na,
}

/// In inches, as the file holds them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub header: f64,
    pub footer: f64,
}

impl Margins {
    /// Excel's "Normal" margins, which a new sheet has.
    pub const NORMAL: Self = Self {
        left: 0.7,
        right: 0.7,
        top: 0.75,
        bottom: 0.75,
        header: 0.3,
        footer: 0.3,
    };
}

impl Default for Margins {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// A manual or automatic page break. A row break's `id` is the last row before it, 1-based,
/// as Excel writes it, so the next page starts at `id + 1`. Columns are the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageBreak {
    pub id: u32,
    pub min: u32,
    pub max: u32,
    pub manual: bool,
    pub pivot: bool,
}

impl PageBreak {
    pub fn manual_row(id: u32) -> Self {
        Self {
            id,
            min: 0,
            max: crate::MAX_COLS - 1,
            manual: true,
            pivot: false,
        }
    }

    pub fn manual_col(id: u32) -> Self {
        Self {
            id,
            min: 0,
            max: crate::MAX_ROWS - 1,
            manual: true,
            pivot: false,
        }
    }
}

/// The six texts of a header and footer, in Excel's codes, and how they apply.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooter {
    pub odd_header: String,
    pub odd_footer: String,
    pub even_header: String,
    pub even_footer: String,
    pub first_header: String,
    pub first_footer: String,
    pub different_odd_even: bool,
    pub different_first: bool,
    pub scale_with_doc: bool,
    pub align_with_margins: bool,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        Self {
            odd_header: String::new(),
            odd_footer: String::new(),
            even_header: String::new(),
            even_footer: String::new(),
            first_header: String::new(),
            first_footer: String::new(),
            different_odd_even: false,
            different_first: false,
            scale_with_doc: true,
            align_with_margins: true,
        }
    }
}

impl HeaderFooter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The header and footer of a page, counted from 1.
    pub fn for_page(&self, page: u32) -> (&str, &str) {
        if self.different_first && page == 1 {
            (&self.first_header, &self.first_footer)
        } else if self.different_odd_even && page.is_multiple_of(2) {
            (&self.even_header, &self.even_footer)
        } else {
            (&self.odd_header, &self.odd_footer)
        }
    }
}

/// Everything about how a sheet prints, apart from its print area and titles, which are defined
/// names.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSetup {
    /// Excel's paper code. 1 is Letter and 9 is A4.
    pub paper: u32,
    pub orientation: Orientation,
    /// Percent, 10 to 400.
    pub scale: u32,
    /// `sheetPr`'s `pageSetUpPr fitToPage`: fit to pages instead of scaling.
    pub fit_to_page: bool,
    /// Pages wide and tall to fit to. 0 is automatic.
    pub fit_width: u32,
    pub fit_height: u32,
    /// The first page's number, when it isn't automatic.
    pub first_page: Option<i64>,
    pub page_order: PageOrder,
    pub black_and_white: bool,
    pub draft: bool,
    pub comments: PrintComments,
    pub errors: PrintErrors,
    /// Print quality in dots per inch, as the file says it.
    pub dpi: Option<u32>,
    pub copies: u32,
    pub margins: Margins,
    pub center_horizontally: bool,
    pub center_vertically: bool,
    pub headings: bool,
    pub gridlines: bool,
    pub header_footer: HeaderFooter,
    pub row_breaks: Vec<PageBreak>,
    pub col_breaks: Vec<PageBreak>,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            paper: 1,
            orientation: Orientation::Default,
            scale: 100,
            fit_to_page: false,
            fit_width: 1,
            fit_height: 1,
            first_page: None,
            page_order: PageOrder::DownThenOver,
            black_and_white: false,
            draft: false,
            comments: PrintComments::None,
            errors: PrintErrors::Displayed,
            dpi: None,
            copies: 1,
            margins: Margins::NORMAL,
            center_horizontally: false,
            center_vertically: false,
            headings: false,
            gridlines: false,
            header_footer: HeaderFooter::default(),
            row_breaks: Vec::new(),
            col_breaks: Vec::new(),
        }
    }
}

impl PageSetup {
    pub fn landscape(&self) -> bool {
        self.orientation == Orientation::Landscape
    }

    /// The paper's width and height in points, turned as the page is.
    pub fn paper_points(&self) -> (f64, f64) {
        let (width, height) = paper_size(self.paper).unwrap_or(LETTER);
        if self.landscape() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Which manual breaks fall on rows, for the rows a page can end on.
    pub fn manual_row_breaks(&self) -> impl Iterator<Item = u32> + '_ {
        self.row_breaks.iter().filter(|b| b.manual).map(|b| b.id)
    }

    pub fn manual_col_breaks(&self) -> impl Iterator<Item = u32> + '_ {
        self.col_breaks.iter().filter(|b| b.manual).map(|b| b.id)
    }

    /// Adds a manual break after `row` and `col` where they are past the first, as Insert Page
    /// Break does at a cell. Returns whether anything changed.
    pub fn insert_break(&mut self, row: Option<u32>, col: Option<u32>) -> bool {
        let mut changed = false;
        if let Some(row) = row.filter(|&r| r > 0) {
            changed |= put_break(&mut self.row_breaks, PageBreak::manual_row(row));
        }
        if let Some(col) = col.filter(|&c| c > 0) {
            changed |= put_break(&mut self.col_breaks, PageBreak::manual_col(col));
        }
        changed
    }

    /// Takes away the manual breaks after `row` and `col`, as Remove Page Break does.
    pub fn remove_break(&mut self, row: Option<u32>, col: Option<u32>) -> bool {
        let before = self.row_breaks.len() + self.col_breaks.len();
        if let Some(row) = row {
            self.row_breaks.retain(|b| !(b.manual && b.id == row));
        }
        if let Some(col) = col {
            self.col_breaks.retain(|b| !(b.manual && b.id == col));
        }
        before != self.row_breaks.len() + self.col_breaks.len()
    }

    /// Moves breaks after rows or columns go in or come out. `point` gives each break's new
    /// place, or `None` where its row went.
    pub fn move_breaks(&mut self, rows: bool, point: impl Fn(u32) -> Option<u32>) {
        let list = if rows {
            &mut self.row_breaks
        } else {
            &mut self.col_breaks
        };
        list.retain_mut(|b| match point(b.id) {
            Some(id) if id > 0 => {
                b.id = id;
                true
            }
            _ => false,
        });
        list.sort_by_key(|b| b.id);
        list.dedup_by_key(|b| b.id);
    }
}

fn put_break(list: &mut Vec<PageBreak>, brk: PageBreak) -> bool {
    match list.binary_search_by_key(&brk.id, |b| b.id) {
        Ok(at) if list[at].manual => false,
        Ok(at) => {
            list[at] = brk;
            true
        }
        Err(at) => {
            list.insert(at, brk);
            true
        }
    }
}

const INCH: f64 = 72.0;
const MM: f64 = 72.0 / 25.4;
const LETTER: (f64, f64) = (8.5 * INCH, 11.0 * INCH);

/// A paper code's width and height in points, portrait, from Excel's `XlPaperSize`.
pub fn paper_size(code: u32) -> Option<(f64, f64)> {
    let inches = |w: f64, h: f64| Some((w * INCH, h * INCH));
    let mm = |w: f64, h: f64| Some((w * MM, h * MM));
    match code {
        1 | 2 | 18 => inches(8.5, 11.0),
        3 | 17 => inches(11.0, 17.0),
        4 => inches(17.0, 11.0),
        5 => inches(8.5, 14.0),
        6 => inches(5.5, 8.5),
        7 => inches(7.25, 10.5),
        8 => mm(297.0, 420.0),
        9 | 10 => mm(210.0, 297.0),
        11 => mm(148.0, 210.0),
        12 => mm(257.0, 364.0),
        13 => mm(182.0, 257.0),
        14 => inches(8.5, 13.0),
        15 => mm(215.0, 275.0),
        16 => inches(10.0, 14.0),
        19 => inches(3.875, 8.875),
        20 => inches(4.125, 9.5),
        21 => inches(4.5, 10.375),
        22 => inches(4.75, 11.0),
        23 => inches(5.0, 11.5),
        24 => inches(17.0, 22.0),
        25 => inches(22.0, 34.0),
        26 => inches(34.0, 44.0),
        27 => mm(110.0, 220.0),
        28 => mm(162.0, 229.0),
        29 => mm(324.0, 458.0),
        30 => mm(229.0, 324.0),
        31 => mm(114.0, 162.0),
        32 => mm(114.0, 229.0),
        33 => mm(250.0, 353.0),
        34 => mm(176.0, 250.0),
        35 => mm(176.0, 125.0),
        36 => mm(110.0, 230.0),
        37 => inches(3.875, 7.5),
        38 => inches(3.625, 6.5),
        39 => inches(14.875, 11.0),
        40 => inches(8.5, 12.0),
        41 => inches(8.5, 13.0),
        _ => None,
    }
}

/// The paper sizes the app offers, with Excel's names for them.
pub const PAPERS: &[(u32, &str)] = &[
    (1, "Letter"),
    (5, "Legal"),
    (7, "Executive"),
    (3, "Tabloid"),
    (6, "Statement"),
    (8, "A3"),
    (9, "A4"),
    (11, "A5"),
    (12, "B4 (JIS)"),
    (13, "B5 (JIS)"),
    (14, "Folio"),
    (20, "Envelope #10"),
    (27, "Envelope DL"),
    (28, "Envelope C5"),
    (34, "Envelope B5"),
    (37, "Envelope Monarch"),
];

/// What a piece of a header or footer shows.
#[derive(Debug, Clone, PartialEq)]
pub enum HfText {
    Literal(String),
    /// `&P`, with what `&P+n` adds.
    Page(i64),
    /// `&N`.
    Pages,
    /// `&D`.
    Date,
    /// `&T`.
    Time,
    /// `&F`.
    File,
    /// `&Z`.
    Path,
    /// `&A`.
    Sheet,
    /// `&G`.
    Picture,
}

/// A run of a header or footer, in the font it is drawn in. `None` is the workbook's normal
/// font, size or colour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HfStyle {
    pub font: Option<String>,
    /// From `&"font,style"`, as the font's own style name says it.
    pub bold: bool,
    pub italic: bool,
    pub size: Option<f64>,
    /// 0 none, 1 single (`&U`), 2 double (`&E`).
    pub underline: u8,
    pub strike: bool,
    pub superscript: bool,
    pub subscript: bool,
    /// `RRGGBB`, or a theme colour as `&K` writes it (`TTSNNN`).
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HfRun {
    pub text: HfText,
    pub style: HfStyle,
}

/// A header or footer's left, centre and right sections.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HfSections {
    pub left: Vec<HfRun>,
    pub center: Vec<HfRun>,
    pub right: Vec<HfRun>,
}

impl HfSections {
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.center.is_empty() && self.right.is_empty()
    }

    pub fn has_picture(&self) -> bool {
        [&self.left, &self.center, &self.right]
            .iter()
            .any(|runs| runs.iter().any(|run| run.text == HfText::Picture))
    }
}

/// What fields show when a header or footer is drawn.
#[derive(Debug, Clone, Default)]
pub struct HfFields {
    pub page: i64,
    pub pages: i64,
    pub date: String,
    pub time: String,
    pub file: String,
    pub path: String,
    pub sheet: String,
}

impl HfText {
    /// The text this shows on a page. A picture shows none.
    pub fn resolve(&self, fields: &HfFields) -> String {
        match self {
            Self::Literal(text) => text.clone(),
            Self::Page(offset) => (fields.page + offset).to_string(),
            Self::Pages => fields.pages.to_string(),
            Self::Date => fields.date.clone(),
            Self::Time => fields.time.clone(),
            Self::File => fields.file.clone(),
            Self::Path => fields.path.clone(),
            Self::Sheet => fields.sheet.clone(),
            Self::Picture => String::new(),
        }
    }
}

/// Reads Excel's header and footer codes: `&L`, `&C` and `&R` start sections, `&P`, `&N`, `&D`,
/// `&T`, `&F`, `&Z`, `&A` and `&G` are fields, `&B`, `&I`, `&U`, `&E`, `&S`, `&X` and `&Y` turn
/// styles on and off, `&"font,style"` and `&nn` set the font and size, `&K` the colour, and `&&`
/// is an ampersand. Text before any section code is centred, as Excel shows it.
pub fn parse_header_footer(text: &str) -> HfSections {
    let mut sections = HfSections::default();
    let mut section = 1usize;
    let mut style = HfStyle::default();
    let mut literal = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    fn flush(sections: &mut HfSections, section: usize, literal: &mut String, style: &HfStyle) {
        if !literal.is_empty() {
            let run = HfRun {
                text: HfText::Literal(std::mem::take(literal)),
                style: style.clone(),
            };
            target(sections, section).push(run);
        }
    }
    fn target(sections: &mut HfSections, section: usize) -> &mut Vec<HfRun> {
        match section {
            0 => &mut sections.left,
            2 => &mut sections.right,
            _ => &mut sections.center,
        }
    }
    while at < chars.len() {
        let ch = chars[at];
        if ch != '&' || at + 1 >= chars.len() {
            literal.push(ch);
            at += 1;
            continue;
        }
        let code = chars[at + 1];
        at += 2;
        let push_field = |text: HfText, literal: &mut String, sections: &mut HfSections| {
            flush(sections, section, literal, &style);
            target(sections, section).push(HfRun {
                text,
                style: style.clone(),
            });
        };
        match code {
            '&' => literal.push('&'),
            'L' | 'C' | 'R' => {
                flush(&mut sections, section, &mut literal, &style);
                section = match code {
                    'L' => 0,
                    'C' => 1,
                    _ => 2,
                };
                style = HfStyle::default();
            }
            'P' => {
                let mut offset = 0i64;
                if at < chars.len() && (chars[at] == '+' || chars[at] == '-') {
                    let sign = if chars[at] == '-' { -1 } else { 1 };
                    let mut end = at + 1;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end > at + 1 {
                        let digits: String = chars[at + 1..end].iter().collect();
                        offset = sign * digits.parse::<i64>().unwrap_or(0);
                        at = end;
                    }
                }
                push_field(HfText::Page(offset), &mut literal, &mut sections);
            }
            'N' => push_field(HfText::Pages, &mut literal, &mut sections),
            'D' => push_field(HfText::Date, &mut literal, &mut sections),
            'T' => push_field(HfText::Time, &mut literal, &mut sections),
            'F' => push_field(HfText::File, &mut literal, &mut sections),
            'Z' => push_field(HfText::Path, &mut literal, &mut sections),
            'A' => push_field(HfText::Sheet, &mut literal, &mut sections),
            'G' => push_field(HfText::Picture, &mut literal, &mut sections),
            'B' | 'I' | 'U' | 'E' | 'S' | 'X' | 'Y' => {
                flush(&mut sections, section, &mut literal, &style);
                match code {
                    'B' => style.bold = !style.bold,
                    'I' => style.italic = !style.italic,
                    'U' => style.underline = if style.underline == 1 { 0 } else { 1 },
                    'E' => style.underline = if style.underline == 2 { 0 } else { 2 },
                    'S' => style.strike = !style.strike,
                    'X' => {
                        style.superscript = !style.superscript;
                        style.subscript = false;
                    }
                    _ => {
                        style.subscript = !style.subscript;
                        style.superscript = false;
                    }
                }
            }
            '"' => {
                flush(&mut sections, section, &mut literal, &style);
                let mut end = at;
                while end < chars.len() && chars[end] != '"' {
                    end += 1;
                }
                let inside: String = chars[at..end].iter().collect();
                at = (end + 1).min(chars.len());
                let (name, face) = inside.split_once(',').unwrap_or((inside.as_str(), ""));
                if name != "-" && !name.is_empty() {
                    style.font = Some(name.to_string());
                }
                let face = face.to_ascii_lowercase();
                if !face.is_empty() {
                    style.bold = face.contains("bold");
                    style.italic = face.contains("italic") || face.contains("oblique");
                }
            }
            'K' => {
                flush(&mut sections, section, &mut literal, &style);
                let end = (at + 6).min(chars.len());
                let color: String = chars[at..end].iter().collect();
                at = end;
                style.color = Some(color);
            }
            digit if digit.is_ascii_digit() => {
                flush(&mut sections, section, &mut literal, &style);
                let mut end = at;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
                let digits: String = std::iter::once(digit)
                    .chain(chars[at..end].iter().copied())
                    .collect();
                at = end;
                style.size = digits.parse::<f64>().ok();
            }
            // An unknown code shows nothing, as in Excel.
            _ => {}
        }
    }
    flush(&mut sections, section, &mut literal, &style);
    sections
}

/// The codes for a header or footer with these three sections of plain codes, as the Page Setup
/// dialog's boxes make them.
pub fn join_sections(left: &str, center: &str, right: &str) -> String {
    let mut out = String::new();
    for (code, text) in [("&L", left), ("&C", center), ("&R", right)] {
        if !text.is_empty() {
            out.push_str(code);
            out.push_str(text);
        }
    }
    out
}

/// Splits codes into the text of each section, codes kept, for the dialog's three boxes.
pub fn split_sections(text: &str) -> (String, String, String) {
    let mut parts = [String::new(), String::new(), String::new()];
    let mut section = 1usize;
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    while at < chars.len() {
        if chars[at] == '&' && at + 1 < chars.len() {
            match chars[at + 1] {
                'L' => section = 0,
                'C' => section = 1,
                'R' => section = 2,
                other => {
                    parts[section].push('&');
                    parts[section].push(other);
                }
            }
            at += 2;
        } else {
            parts[section].push(chars[at]);
            at += 1;
        }
    }
    let [left, center, right] = parts;
    (left, center, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(runs: &[HfRun]) -> Vec<HfText> {
        runs.iter().map(|run| run.text.clone()).collect()
    }

    #[test]
    fn reads_sections_and_fields() {
        let parsed = parse_header_footer("&LLeft &A&CPage &P of &N&R&D &T");
        assert_eq!(
            texts(&parsed.left),
            vec![HfText::Literal("Left ".into()), HfText::Sheet]
        );
        assert_eq!(
            texts(&parsed.center),
            vec![
                HfText::Literal("Page ".into()),
                HfText::Page(0),
                HfText::Literal(" of ".into()),
                HfText::Pages
            ]
        );
        assert_eq!(
            texts(&parsed.right),
            vec![HfText::Date, HfText::Literal(" ".into()), HfText::Time]
        );
    }

    #[test]
    fn text_with_no_section_is_centred() {
        let parsed = parse_header_footer("Confidential && private");
        assert!(parsed.left.is_empty());
        assert_eq!(
            texts(&parsed.center),
            vec![HfText::Literal("Confidential & private".into())]
        );
    }

    #[test]
    fn reads_fonts_sizes_styles_and_colours() {
        let parsed =
            parse_header_footer("&C&\"Arial,Bold Italic\"&14&KFF0000Big&B plain &Utail&P+2");
        let runs = &parsed.center;
        assert_eq!(runs[0].style.font.as_deref(), Some("Arial"));
        assert!(runs[0].style.bold && runs[0].style.italic);
        assert_eq!(runs[0].style.size, Some(14.0));
        assert_eq!(runs[0].style.color.as_deref(), Some("FF0000"));
        assert!(!runs[1].style.bold);
        assert_eq!(runs[2].style.underline, 1);
        assert_eq!(runs[3].text, HfText::Page(2));
    }

    #[test]
    fn a_size_right_after_a_field_is_a_size() {
        let parsed = parse_header_footer("&P&8x");
        assert_eq!(parsed.center[1].style.size, Some(8.0));
        assert_eq!(parsed.center[1].text, HfText::Literal("x".into()));
    }

    #[test]
    fn splits_and_joins_sections() {
        let (l, c, r) = split_sections("&L&BA&C&P&RB&&C");
        assert_eq!((l.as_str(), c.as_str(), r.as_str()), ("&BA", "&P", "B&&C"));
        assert_eq!(join_sections(&l, &c, &r), "&L&BA&C&P&RB&&C");
    }

    #[test]
    fn picks_first_and_even_pages() {
        let hf = HeaderFooter {
            odd_header: "odd".into(),
            even_header: "even".into(),
            first_header: "first".into(),
            different_first: true,
            different_odd_even: true,
            ..HeaderFooter::default()
        };
        assert_eq!(hf.for_page(1).0, "first");
        assert_eq!(hf.for_page(2).0, "even");
        assert_eq!(hf.for_page(3).0, "odd");
    }

    #[test]
    fn breaks_go_in_once_and_follow_rows() {
        let mut setup = PageSetup::default();
        assert!(setup.insert_break(Some(10), Some(0)));
        assert!(!setup.insert_break(Some(10), None));
        assert!(setup.insert_break(Some(4), Some(3)));
        assert_eq!(setup.manual_row_breaks().collect::<Vec<_>>(), vec![4, 10]);
        setup.move_breaks(true, |id| if id == 4 { None } else { Some(id + 2) });
        assert_eq!(setup.manual_row_breaks().collect::<Vec<_>>(), vec![12]);
        assert!(setup.remove_break(Some(12), Some(3)));
        assert!(setup.row_breaks.is_empty() && setup.col_breaks.is_empty());
    }

    #[test]
    fn paper_turns_with_the_page() {
        let mut setup = PageSetup {
            paper: 9,
            ..PageSetup::default()
        };
        let (w, h) = setup.paper_points();
        assert!((w - 595.27).abs() < 0.01 && (h - 841.89).abs() < 0.01);
        setup.orientation = Orientation::Landscape;
        assert_eq!(setup.paper_points(), (h, w));
    }
}
