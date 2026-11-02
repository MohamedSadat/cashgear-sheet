//! Cell formatting as `styles.xml` holds it: indices stay as stored, so saving can append
//! to the table without changing what any existing cell points at.

use std::collections::HashMap;

use crate::color::{Color, ColorSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Underline {
    None,
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub name: String,
    /// In points.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: Underline,
    pub color: Color,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            name: "Calibri".to_string(),
            size: 11.0,
            bold: false,
            italic: false,
            strike: false,
            underline: Underline::None,
            color: Color::AUTO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    None,
    Solid,
    /// Share of the foreground colour in percent, for patterns drawn as a flat mix.
    Mixed(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub pattern: Pattern,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Fill {
    fn default() -> Self {
        Self {
            pattern: Pattern::None,
            fg: Color::AUTO,
            bg: Color::AUTO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    None,
    Hair,
    Dotted,
    Dashed,
    DashDot,
    DashDotDot,
    Thin,
    Medium,
    MediumDashed,
    MediumDashDot,
    MediumDashDotDot,
    SlantDashDot,
    Thick,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub style: LineStyle,
    pub color: Color,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            style: LineStyle::None,
            color: Color::AUTO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Border {
    pub left: Line,
    pub right: Line,
    pub top: Line,
    pub bottom: Line,
    pub diagonal: Line,
    pub diagonal_up: bool,
    pub diagonal_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
    Justify,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadingOrder {
    /// By the first strong character of the text.
    #[default]
    Context,
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Alignment {
    pub horizontal: HAlign,
    pub vertical: VAlign,
    pub wrap: bool,
    pub shrink: bool,
    pub indent: u8,
    /// As stored: 0 to 90 counter-clockwise, 91 to 180 clockwise, 255 stacked.
    pub rotation: u8,
    pub reading_order: ReadingOrder,
}

/// One `cellXfs` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Xf {
    pub font: u32,
    pub fill: u32,
    pub border: u32,
    pub num_fmt: u32,
    pub alignment: Alignment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleTable {
    pub fonts: Vec<Font>,
    pub fills: Vec<Fill>,
    pub borders: Vec<Border>,
    pub xfs: Vec<Xf>,
    /// Format codes the file defines, by id. Lower ids are built in.
    pub num_fmts: HashMap<u32, String>,
    pub colors: ColorSpace,
}

impl Default for StyleTable {
    fn default() -> Self {
        Self {
            fonts: vec![Font::default()],
            fills: vec![Fill::default()],
            borders: vec![Border::default()],
            xfs: vec![Xf::default()],
            num_fmts: HashMap::new(),
            colors: ColorSpace::default(),
        }
    }
}

impl StyleTable {
    pub fn xf(&self, index: u32) -> &Xf {
        self.xfs
            .get(index as usize)
            .or_else(|| self.xfs.first())
            .expect("the table always has a default xf")
    }

    pub fn font(&self, xf: &Xf) -> &Font {
        self.fonts
            .get(xf.font as usize)
            .or_else(|| self.fonts.first())
            .expect("the table always has a default font")
    }

    pub fn fill(&self, xf: &Xf) -> &Fill {
        self.fills
            .get(xf.fill as usize)
            .or_else(|| self.fills.first())
            .expect("the table always has a default fill")
    }

    pub fn border(&self, xf: &Xf) -> &Border {
        self.borders
            .get(xf.border as usize)
            .or_else(|| self.borders.first())
            .expect("the table always has a default border")
    }

    /// The Normal style's font, which column widths count in.
    pub fn normal_font(&self) -> &Font {
        self.font(self.xf(0))
    }
}

/// The widest digit in pixels at 96 DPI, as Excel measured it: each font at 8, 9, 10, 11, 12,
/// 14 and 16 points. Hinting makes these uneven, so a formula can't predict them.
const DIGIT_WIDTHS: [(&str, [u8; 7]); 15] = [
    ("calibri", [6, 6, 7, 7, 8, 10, 11]),
    ("calibri light", [6, 6, 7, 8, 8, 10, 11]),
    ("carlito", [6, 6, 7, 7, 8, 10, 11]),
    ("arial", [6, 7, 7, 8, 9, 11, 12]),
    ("arial narrow", [5, 5, 6, 7, 7, 9, 10]),
    ("times new roman", [6, 6, 6, 7, 8, 9, 11]),
    ("tahoma", [6, 7, 7, 8, 9, 10, 11]),
    ("verdana", [7, 8, 8, 10, 10, 12, 13]),
    ("courier new", [7, 7, 8, 9, 10, 11, 13]),
    ("cambria", [6, 7, 7, 8, 9, 11, 12]),
    ("segoe ui", [6, 6, 7, 8, 9, 10, 11]),
    ("consolas", [6, 7, 7, 8, 9, 10, 12]),
    ("georgia", [8, 8, 9, 9, 10, 12, 13]),
    ("trebuchet ms", [6, 6, 7, 8, 8, 10, 11]),
    ("ms sans serif", [6, 6, 7, 7, 9, 10, 11]),
];
const DIGIT_SIZES: [f32; 7] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0];

/// The widest digit of a font, which column widths are counted in. Fonts not measured here
/// follow Calibri's proportion, which is what most files use.
pub fn max_digit_width(font: &Font) -> f64 {
    let name = font.name.to_ascii_lowercase();
    // Aptos isn't on the machine that measured the table. Files Excel saved show 7 at 11 points.
    let table = DIGIT_WIDTHS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, widths)| widths);
    if let Some(widths) = table {
        if let Some(at) = DIGIT_SIZES.iter().position(|size| *size == font.size) {
            return f64::from(widths[at]);
        }
        // Between sizes, or past them: in proportion to the nearest measured size.
        let nearest = DIGIT_SIZES
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - font.size).abs().total_cmp(&(b.1 - font.size).abs()))
            .map_or(3, |(at, _)| at);
        let ratio = f64::from(widths[nearest]) / f64::from(DIGIT_SIZES[nearest]);
        return (f64::from(font.size) * ratio).round().max(1.0);
    }
    (f64::from(font.size) * 7.0 / 11.0).round().max(1.0)
}

/// A stored column width, in characters, to whole pixels at 100%.
pub fn col_width_to_px(width: f64, digit: f64) -> u32 {
    let px = ((256.0 * width + (128.0 / digit).trunc()) / 256.0 * digit).trunc();
    px.clamp(0.0, 4096.0) as u32
}

/// The width of a column nobody sized, from `baseColWidth`: padded, then rounded up to 8s.
pub fn base_col_width_to_px(base: f64, digit: f64) -> u32 {
    let padding = 2.0 * (digit / 4.0).ceil() + 1.0;
    let padded = (base * digit + padding).max(1.0);
    ((padded / 8.0).ceil() * 8.0).min(4096.0) as u32
}

/// Points to whole pixels at 100%.
pub fn points_to_px(points: f64) -> u32 {
    (points * 96.0 / 72.0).round().clamp(0.0, 4096.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_widths_match_excel_for_calibri_11() {
        let digit = max_digit_width(&Font::default());
        assert_eq!(digit, 7.0);
        assert_eq!(col_width_to_px(9.140625, digit), 64);
        assert_eq!(col_width_to_px(8.7109375, digit), 61);
        assert_eq!(col_width_to_px(20.7109375, digit), 145);
        assert_eq!(base_col_width_to_px(8.0, digit), 64);
    }

    /// Measured from Excel over COM, on Normal fonts it has here.
    #[test]
    fn default_columns_match_excel_for_other_fonts() {
        let default_px = |name: &str, size: f32| {
            let font = Font {
                name: name.to_string(),
                size,
                ..Font::default()
            };
            base_col_width_to_px(8.0, max_digit_width(&font))
        };
        assert_eq!(default_px("Calibri", 11.0), 64);
        assert_eq!(default_px("Calibri", 9.0), 56);
        assert_eq!(default_px("Calibri", 12.0), 72);
        assert_eq!(default_px("Calibri", 14.0), 88);
        assert_eq!(default_px("Arial", 10.0), 64);
        assert_eq!(default_px("Arial", 11.0), 72);
        assert_eq!(default_px("Times New Roman", 10.0), 56);
        assert_eq!(default_px("Verdana", 16.0), 120);
        assert_eq!(default_px("Georgia", 16.0), 120);
        assert_eq!(default_px("Arial Narrow", 8.0), 48);
        assert_eq!(default_px("Aptos Narrow", 11.0), 64);
    }

    #[test]
    fn a_size_nobody_measured_goes_in_proportion() {
        let font = Font {
            name: "Arial".to_string(),
            size: 10.5,
            ..Font::default()
        };
        assert_eq!(max_digit_width(&font), 7.0);
    }

    #[test]
    fn row_heights_match_excel() {
        assert_eq!(points_to_px(15.0), 20);
        assert_eq!(points_to_px(14.4), 19);
        assert_eq!(points_to_px(409.5), 546);
    }

    #[test]
    fn an_index_past_the_table_falls_back_to_the_default() {
        let table = StyleTable::default();
        assert_eq!(table.xf(9_999), &Xf::default());
    }
}
