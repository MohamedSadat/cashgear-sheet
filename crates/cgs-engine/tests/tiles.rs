//! Opens small workbooks and reads back what their tiles say.

use std::io::Write;
use std::rc::Rc;

use cgs_engine::view_block::*;
use cgs_engine::{Engine, LoadPhase, Screen};

const LIGHT: Screen = Screen {
    dark: false,
    background: 0xFFFFFF,
    text: 0x000000,
};
const DARK: Screen = Screen {
    dark: true,
    background: 0x1E1E1E,
    text: 0xE6E6E6,
};

const STYLES: &str = r##"<styleSheet>
  <numFmts count="3"><numFmt numFmtId="164" formatCode="0.00;[Red]-0.00"/><numFmt numFmtId="165" formatCode="yyyy-mm-dd"/><numFmt numFmtId="166" formatCode="&quot;$&quot;* #,##0.00"/></numFmts>
  <fonts count="4">
    <font><sz val="11"/><name val="Calibri"/></font>
    <font><b/><sz val="14"/><color rgb="FF0000FF"/><name val="Arial"/></font>
    <font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
    <font><sz val="11"/><color rgb="FF000000"/><name val="Calibri"/></font>
  </fonts>
  <fills count="4">
    <fill><patternFill patternType="none"/></fill>
    <fill><patternFill patternType="gray125"/></fill>
    <fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill>
    <fill><patternFill patternType="solid"><fgColor theme="0"/></patternFill></fill>
  </fills>
  <borders count="3">
    <border><left/><right/><top/><bottom/><diagonal/></border>
    <border><left/><right style="thin"><color rgb="FF00FF00"/></right><top/><bottom/><diagonal/></border>
    <border><left style="medium"><color rgb="FF0000FF"/></left><right/><top/><bottom/><diagonal/></border>
  </borders>
  <cellXfs count="11">
    <xf/>
    <xf fontId="1" fillId="2"><alignment horizontal="center" vertical="top" wrapText="1"/></xf>
    <xf numFmtId="164"/>
    <xf numFmtId="165"/>
    <xf fontId="2"/>
    <xf fontId="3"/>
    <xf fillId="3"/>
    <xf borderId="1"/>
    <xf borderId="2"/>
    <xf fillId="2"/>
    <xf numFmtId="166"/>
  </cellXfs>
</styleSheet>"##;

fn zip_of(parts: &[(&str, String)]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

fn workbook(cells: &str, after: &str) -> Vec<u8> {
    let sheet = format!("<worksheet><sheetData>{cells}</sheetData>{after}</worksheet>");
    zip_of(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="One" sheetId="1" r:id="r1"/></sheets></workbook>"#
                .to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="s.xml"/><Relationship Id="r2" Type="http://x/styles" Target="styles.xml"/></Relationships>"#.to_string(),
        ),
        ("xl/styles.xml", STYLES.to_string()),
        ("xl/s.xml", sheet),
    ])
}

fn open(cells: &str, after: &str) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(workbook(cells, after))).unwrap();
    while engine.load_step(1.0).unwrap().phase == LoadPhase::Reading {}
    engine
}

/// A tile, read back the way `view-block.ts` reads it.
struct Tile {
    bytes: Vec<u8>,
    rows: usize,
    cols: usize,
    styles: usize,
    extras: usize,
    runs: usize,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[derive(Debug, PartialEq)]
struct Cell {
    text: String,
    fill: u32,
    color: u32,
    style: u16,
    flags: u16,
}

impl Tile {
    fn new(engine: &mut Engine, sheet: u32, row: u32, col: u32, screen: Screen) -> Self {
        let bytes = engine.view_block(sheet, row, col, screen).into_bytes();
        let rows = u16_at(&bytes, 6) as usize;
        let cols = u16_at(&bytes, 8) as usize;
        let styles = u32_at(&bytes, 20) as usize;
        let extras = u32_at(&bytes, 24) as usize;
        let runs = u32_at(&bytes, 32) as usize;
        Self {
            bytes,
            rows,
            cols,
            styles,
            extras,
            runs,
        }
    }

    fn sizes(&self) -> usize {
        let sizes = HEADER_BYTES + (self.rows + self.cols) * 2;
        sizes + (4 - sizes % 4) % 4
    }

    fn row_px(&self, row: usize) -> u16 {
        u16_at(&self.bytes, HEADER_BYTES + row * 2)
    }

    fn col_px(&self, col: usize) -> u16 {
        u16_at(&self.bytes, HEADER_BYTES + (self.rows + col) * 2)
    }

    fn runs_at(&self) -> usize {
        self.sizes()
            + self.rows * self.cols * CELL_BYTES
            + self.styles * STYLE_BYTES
            + self.extras * EXTRA_BYTES
    }

    fn heap(&self) -> usize {
        self.runs_at() + self.runs * RUN_BYTES
    }

    /// (text offset, start, length, colour, style) of a run.
    fn run(&self, index: usize) -> (u32, u32, u32, u32, u16) {
        let at = self.runs_at() + index * RUN_BYTES;
        (
            u32_at(&self.bytes, at),
            u32_at(&self.bytes, at + 4),
            u32_at(&self.bytes, at + 8),
            u32_at(&self.bytes, at + 12),
            u16_at(&self.bytes, at + 16),
        )
    }

    fn text(&self, offset: usize, length: usize) -> String {
        let heap = self.heap();
        String::from_utf8(self.bytes[heap + offset..heap + offset + length].to_vec()).unwrap()
    }

    fn cell(&self, row: usize, col: usize) -> Cell {
        let at = self.sizes() + (row * self.cols + col) * CELL_BYTES;
        Cell {
            text: self.text(
                u32_at(&self.bytes, at) as usize,
                u32_at(&self.bytes, at + 4) as usize,
            ),
            fill: u32_at(&self.bytes, at + 8),
            color: u32_at(&self.bytes, at + 12),
            style: u16_at(&self.bytes, at + 16),
            flags: u16_at(&self.bytes, at + 18),
        }
    }

    fn style_at(&self, index: u16) -> usize {
        self.sizes() + self.rows * self.cols * CELL_BYTES + index as usize * STYLE_BYTES
    }

    fn font(&self, index: u16) -> String {
        let at = self.style_at(index);
        self.text(
            u32_at(&self.bytes, at) as usize,
            u32_at(&self.bytes, at + 4) as usize,
        )
    }

    /// (kind, x, y, width, height, text) of an extra.
    fn extra(&self, index: usize) -> (u8, i32, i32, i32, i32, String) {
        let at = self.sizes()
            + self.rows * self.cols * CELL_BYTES
            + self.styles * STYLE_BYTES
            + index * EXTRA_BYTES;
        (
            self.bytes[at],
            u32_at(&self.bytes, at + 4) as i32,
            u32_at(&self.bytes, at + 8) as i32,
            u32_at(&self.bytes, at + 12) as i32,
            u32_at(&self.bytes, at + 16) as i32,
            self.text(
                u32_at(&self.bytes, at + 20) as usize,
                u32_at(&self.bytes, at + 24) as usize,
            ),
        )
    }
}

#[test]
fn a_tile_shows_formatted_values_in_their_own_colours() {
    let mut engine = open(
        r#"<row r="1">
          <c r="A1" s="1" t="inlineStr"><is><t>Title</t></is></c>
          <c r="B1" s="2"><v>-1.5</v></c>
          <c r="C1" s="3"><v>45306</v></c>
          <c r="D1"><v>3.14159265358979</v></c>
          <c r="E1" t="b"><v>1</v></c>
          <c r="F1" t="e"><v>#N/A</v></c>
          <c r="G1" t="inlineStr"><is><t>مرحبا</t></is></c>
          <c r="H1" s="3"><v>-1</v></c>
        </row>"#,
        "",
    );
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    assert_eq!((tile.rows, tile.cols), (64, 16));
    assert_eq!(
        tile.row_px(0),
        24,
        "fitted to its 14-point Arial, as Excel fits it"
    );
    assert_eq!(tile.row_px(1), 20);
    assert_eq!(tile.col_px(0), 64);

    let title = tile.cell(0, 0);
    assert_eq!(title.text, "Title");
    assert_eq!(title.fill, 0xFFFF00FF, "yellow");
    assert_eq!(title.color, 0x0000FFFF, "blue");
    assert_eq!(title.flags & KIND_MASK, KIND_TEXT);
    assert_eq!(tile.font(title.style), "Arial");
    let style = tile.style_at(title.style);
    assert_eq!(u16_at(&tile.bytes, style + 8), 1400);
    assert_eq!(tile.bytes[style + 10], FONT_BOLD);
    assert_eq!(tile.bytes[style + 12], 1, "centred");
    assert_eq!(tile.bytes[style + 13], 0, "at the top");
    assert_eq!(tile.bytes[style + 16] & LAYOUT_WRAP, LAYOUT_WRAP);

    let negative = tile.cell(0, 1);
    assert_eq!(negative.text, "-1.50");
    assert_eq!(negative.color, 0xFF0000FF, "the format's own red");
    assert_eq!(negative.flags & KIND_MASK, KIND_NUMBER);

    assert_eq!(tile.cell(0, 2).text, "2024-01-15");
    let general = tile.cell(0, 3);
    assert_eq!(general.flags & KIND_MASK, KIND_GENERAL);
    assert_eq!(general.color, AUTOMATIC);
    assert_eq!(general.fill, 0);
    assert_eq!(tile.cell(0, 4).text, "TRUE");
    assert_eq!(tile.cell(0, 4).flags & KIND_MASK, KIND_OTHER);
    assert_eq!(tile.cell(0, 5).text, "#N/A");
    let arabic = tile.cell(0, 6);
    assert_eq!(arabic.flags & CELL_RTL, CELL_RTL);
    let horizontal = tile.bytes[tile.style_at(arabic.style) + 12];
    assert_eq!(horizontal, 2, "Arabic text aligns right by itself");
    assert_eq!(
        tile.cell(0, 7).flags & CELL_HASHES,
        CELL_HASHES,
        "a negative date"
    );
    assert_eq!(tile.cell(1, 0).text, "");
}

#[test]
fn dark_mode_swaps_theme_colours_and_rescues_low_contrast_text() {
    let cells = r#"<row r="1">
      <c r="A1" t="inlineStr"><is><t>automatic</t></is></c>
      <c r="B1" s="4" t="inlineStr"><is><t>theme black</t></is></c>
      <c r="C1" s="5" t="inlineStr"><is><t>fixed black</t></is></c>
      <c r="D1" s="6"/>
      <c r="E1" s="1" t="inlineStr"><is><t>blue on yellow</t></is></c>
    </row>"#;
    let mut engine = open(cells, "");
    let light = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    assert_eq!(light.cell(0, 0).color, AUTOMATIC);
    assert_eq!(light.cell(0, 1).color, 0x000000FF);
    assert_eq!(light.cell(0, 2).color, 0x000000FF);
    assert_eq!(light.cell(0, 3).fill, 0xFFFFFFFF);

    let dark = Tile::new(&mut engine, 0, 0, 0, DARK);
    assert_eq!(dark.cell(0, 0).color, AUTOMATIC, "the palette supplies it");
    assert_eq!(dark.cell(0, 1).color, 0xE6E6E6FF, "dk1 swaps");
    assert_eq!(dark.cell(0, 3).fill, 0x1E1E1EFF, "lt1 swaps");
    let rescued = dark.cell(0, 2).color >> 8;
    assert!(
        cgs_model::color::contrast(rescued, DARK.background) >= cgs_model::color::MIN_CONTRAST,
        "fixed black moved to {rescued:06x}"
    );
    // Fills stay, and text that reads well on them stays too.
    assert_eq!(dark.cell(0, 4).fill, 0xFFFF00FF);
    assert_eq!(dark.cell(0, 4).color, 0x0000FFFF);
}

#[test]
fn general_alignment_goes_by_the_text_whatever_the_reading_order_says() {
    let styles = STYLES.replace(
        "<xf fillId=\"2\"/>",
        "<xf fillId=\"2\"/><xf><alignment readingOrder=\"2\"/></xf><xf><alignment readingOrder=\"1\"/></xf>",
    );
    let sheet = r#"<worksheet><sheetData><row r="1">
      <c r="A1" s="10" t="inlineStr"><is><t>left</t></is></c>
      <c r="B1" s="11" t="inlineStr"><is><t>يسار</t></is></c>
    </row></sheetData></worksheet>"#;
    let bytes = zip_of(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="One" sheetId="1" r:id="r1"/></sheets></workbook>"#
                .to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="s.xml"/><Relationship Id="r2" Type="http://x/styles" Target="styles.xml"/></Relationships>"#.to_string(),
        ),
        ("xl/styles.xml", styles),
        ("xl/s.xml", sheet.to_string()),
    ]);
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1.0).unwrap().phase == LoadPhase::Reading {}
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    let horizontal = |col| tile.bytes[tile.style_at(tile.cell(0, col).style) + 12];
    assert_eq!(horizontal(0), 0, "Latin stays left");
    assert_eq!(
        tile.cell(0, 0).flags & CELL_RTL,
        CELL_RTL,
        "though it runs right to left"
    );
    assert_eq!(horizontal(1), 2, "Arabic stays right");
    assert_eq!(
        tile.cell(0, 1).flags & CELL_RTL,
        0,
        "though it runs left to right"
    );
}

#[test]
fn a_formula_reads_the_same_before_and_after_the_graph_is_built() {
    let mut engine = open(
        r#"<row r="1"><c r="A1"><f>IF(1&gt;0,"a b",2)</f><v>1</v></c></row>"#,
        "",
    );
    assert_eq!(engine.edit_text(0, 1, 1), r#"=IF(1>0,"a b",2)"#);
    while engine.prepare_step().unwrap() {}
    assert_eq!(engine.edit_text(0, 1, 1), r#"=IF(1>0,"a b",2)"#);
}

#[test]
fn automatic_text_on_a_pale_fill_stays_black_in_dark_mode() {
    let cells = r#"<row r="1"><c r="A1" s="9" t="inlineStr"><is><t>on yellow</t></is></c></row>"#;
    let mut engine = open(cells, "");
    let dark = Tile::new(&mut engine, 0, 0, 0, DARK);
    assert_eq!(dark.cell(0, 0).fill, 0xFFFF00FF);
    assert_eq!(dark.cell(0, 0).color, 0x000000FF);
}

#[test]
fn a_fill_character_travels_inside_the_text() {
    let cells = r#"<row r="1"><c r="A1" s="10"><v>1234.5</v></c></row>"#;
    let mut engine = open(cells, "");
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    let at = tile.sizes();
    assert_eq!(u32_at(&tile.bytes, at + 20), 2, "the position, plus 1");
    assert_eq!(tile.cell(0, 0).text, "$ 1,234.50");
    assert_eq!(engine.display_text(0, 1, 1), "$1,234.50");
}

#[test]
fn hidden_lines_custom_sizes_and_merges_reach_the_tile() {
    let cells = r#"<row r="1" ht="30"><c r="A1" t="inlineStr"><is><t>merged</t></is></c></row>
      <row r="2" hidden="1"><c r="A2"><v>2</v></c></row>"#;
    let after = r#"<mergeCells count="1"><mergeCell ref="A1:B3"/></mergeCells>"#;
    let mut engine = open(cells, after);
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    assert_eq!(tile.row_px(0), 40);
    assert_eq!(tile.row_px(1), 0);
    let anchor = tile.cell(0, 0);
    assert_eq!(anchor.text, "", "merged text is an extra");
    assert_eq!(anchor.flags & CELL_MERGED, CELL_MERGED);
    assert_eq!(anchor.flags & CELL_NO_RIGHT_GRID, CELL_NO_RIGHT_GRID);
    assert_eq!(anchor.flags & CELL_NO_BOTTOM_GRID, CELL_NO_BOTTOM_GRID);
    let corner = tile.cell(2, 1);
    assert_eq!(corner.flags & CELL_NO_RIGHT_GRID, 0);
    assert_eq!(corner.flags & CELL_NO_BOTTOM_GRID, 0);
    assert_eq!(tile.cell(1, 0).text, "", "a covered value stays hidden");
    assert_eq!(tile.extras, 1);
    assert_eq!(
        tile.extra(0),
        (EXTRA_MERGE, 0, 0, 128, 60, "merged".to_string())
    );
}

#[test]
fn a_merged_area_shows_in_every_tile_it_touches() {
    let cells = r#"<row r="60"><c r="O60" t="inlineStr"><is><t>across</t></is></c></row>"#;
    let after = r#"<mergeCells count="1"><mergeCell ref="O60:R70"/></mergeCells>"#;
    let mut engine = open(cells, after);
    let far = Tile::new(&mut engine, 0, 64, 16, LIGHT);
    assert_eq!(far.extras, 1);
    let (kind, x, y, width, height, text) = far.extra(0);
    assert_eq!(kind, EXTRA_MERGE);
    assert_eq!((x, y), (-128, -100));
    assert_eq!((width, height), (4 * 64, 11 * 20));
    assert_eq!(text, "across");
}

#[test]
fn text_near_a_tile_edge_comes_along_as_an_overflow() {
    let cells = r#"<row r="1">
      <c r="P1" t="inlineStr"><is><t>going right</t></is></c>
      <c r="AG1" s="0" t="inlineStr"><is><t>مرحبا going left</t></is></c>
    </row>
    <row r="2"><c r="P2"><v>5</v></c></row>
    <row r="3"><c r="P3" t="inlineStr"><is><t>blocked</t></is></c><c r="Q3"><v>1</v></c></row>"#;
    let mut engine = open(cells, "");
    let tile = Tile::new(&mut engine, 0, 0, 16, LIGHT);
    assert_eq!(tile.cell(0, 0).text, "");
    let extras: Vec<_> = (0..tile.extras).map(|index| tile.extra(index)).collect();
    assert!(
        extras
            .iter()
            .any(|e| e.0 == EXTRA_OVERFLOW && e.1 == -64 && e.2 == 0 && e.5 == "going right"),
        "{extras:?}"
    );
    assert!(
        extras
            .iter()
            .any(|e| e.1 == 16 * 64 && e.5.contains("going left")),
        "{extras:?}"
    );
    assert!(
        !extras.iter().any(|e| e.5 == "5" || e.5 == "blocked"),
        "numbers don't overflow, and a filled cell stops text: {extras:?}"
    );
}

#[test]
fn each_line_is_drawn_once_with_its_neighbours_borders() {
    let cells = r#"<row r="1"><c r="A1" s="7"/><c r="C1"/><c r="D1" s="8"/></row>"#;
    let mut engine = open(cells, "");
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    let right_of = |col| {
        let style = tile.style_at(tile.cell(0, col).style);
        (tile.bytes[style + 17], u32_at(&tile.bytes, style + 20))
    };
    assert_eq!(right_of(0), (6, 0x00FF00FF), "its own thin line");
    assert_eq!(right_of(1), (0, 0), "nothing");
    assert_eq!(right_of(2), (7, 0x0000FFFF), "D1's medium left line");
}

#[test]
fn rich_text_comes_with_the_looks_of_its_runs() {
    let strings = r#"<sst count="1" uniqueCount="1"><si><r><t xml:space="preserve">Rich: </t></r><r><rPr><b/><sz val="11"/><color rgb="FF0070C0"/><rFont val="Calibri"/></rPr><t xml:space="preserve">bold blue, </t></r><r><rPr><i/><sz val="14"/><color rgb="FFC00000"/><rFont val="Times New Roman"/></rPr><t>big red italic</t></r></si></sst>"#;
    let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c></row></sheetData></worksheet>"#;
    let bytes = zip_of(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="One" sheetId="1" r:id="r1"/></sheets></workbook>"#
                .to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="s.xml"/><Relationship Id="r2" Type="http://x/styles" Target="styles.xml"/><Relationship Id="r3" Type="http://x/sharedStrings" Target="strings.xml"/></Relationships>"#.to_string(),
        ),
        ("xl/styles.xml", STYLES.to_string()),
        ("xl/strings.xml", strings.to_string()),
        ("xl/s.xml", sheet.to_string()),
    ]);
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1.0).unwrap().phase == LoadPhase::Reading {}
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    let cell = tile.cell(0, 0);
    assert_eq!(cell.text, "Rich: bold blue, big red italic");
    // The 14 point run makes the row taller, as Excel fits it.
    assert!(tile.row_px(0) > 20, "{}", tile.row_px(0));
    // Three runs a cell, both cells.
    assert_eq!(tile.runs, 6);
    let offset = u32_at(&tile.bytes, tile.sizes());
    let first = tile.run(0);
    assert_eq!((first.0, first.1, first.2), (offset, 0, 6));
    let bold = tile.run(1);
    assert_eq!((bold.1, bold.2, bold.3), (6, 11, 0x0070C0FF));
    let italic = tile.run(2);
    assert_eq!((italic.1, italic.2, italic.3), (17, 14, 0xC00000FF));
    assert_eq!(tile.font(bold.4), "Calibri");
    assert_eq!(tile.font(italic.4), "Times New Roman");
    let size = |style: u16| u16_at(&tile.bytes, tile.style_at(style) + 8);
    assert_eq!((size(bold.4), size(italic.4)), (1100, 1400));
    let flags = |style: u16| tile.bytes[tile.style_at(style) + 10];
    assert_ne!(flags(bold.4), flags(italic.4));
    // Dark mode keeps a run's colour where it reads, as it does a cell's.
    let dark = Tile::new(&mut engine, 0, 0, 0, DARK);
    assert_eq!(dark.runs, 6);
}

#[test]
fn a_tile_past_the_edge_of_the_data_is_blank_and_fast() {
    let mut engine = open(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#, "");
    let tile = Tile::new(&mut engine, 0, 1_048_512, 16_368, LIGHT);
    assert_eq!((tile.rows, tile.cols), (64, 16));
    assert_eq!(tile.cell(63, 15).text, "");
}

#[test]
fn a_tile_of_an_unread_sheet_says_so() {
    let mut engine = Engine::new();
    engine.open(Rc::new(workbook("", ""))).unwrap();
    let bytes = engine.view_block(0, 0, 0, LIGHT).into_bytes();
    assert_eq!(u16_at(&bytes, 10) & TILE_NOT_READY, TILE_NOT_READY);
}

#[test]
fn the_shell_gets_sheets_layout_and_cell_text() {
    let cells = r#"<row r="1" ht="30"><c r="A1"><f>1+1</f><v>2</v></c><c r="B1" s="3"><v>45306</v></c></row>"#;
    let mut engine = open(
        cells,
        r#"<mergeCells><mergeCell ref="D1:E2"/></mergeCells>"#,
    );
    assert_eq!(
        engine.book_json(),
        r#"{"active":0,"sheets":[{"name":"One","state":"visible","kind":"worksheet","loaded":true,"deleted":false}],"order":[0]}"#
    );
    let numbers = engine.layout_numbers(0);
    assert_eq!(&numbers[..4], &[20, 64, 1, 2]);
    let layout = engine.layout(0).unwrap();
    assert_eq!(layout.rows, vec![(1, 40)]);
    assert_eq!(layout.merges, vec![(1, 4, 2, 5)]);
    assert_eq!(engine.edit_text(0, 1, 1), "=1+1");
    assert_eq!(engine.display_text(0, 1, 1), "2");
    assert_eq!(engine.display_text(0, 1, 2), "2024-01-15");
    assert_eq!(engine.edit_text(0, 1, 2), "1/15/2024");
    assert_eq!(engine.edit_text(0, 9, 9), "");
}

#[test]
fn nothing_calculates_until_preparing_finishes_and_after_it_either() {
    // The saved value is wrong on purpose.
    let mut engine = open(r#"<row r="1"><c r="A1"><f>1+1</f><v>99</v></c></row>"#, "");
    assert_eq!(engine.display_text(0, 1, 1), "99");
    while engine.prepare_step().unwrap() {}
    assert_eq!(engine.load_step(1.0).unwrap().phase, LoadPhase::Ready);
    assert_eq!(engine.display_text(0, 1, 1), "99");
}

#[test]
fn opening_a_bad_file_leaves_no_workbook() {
    let mut engine = open(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#, "");
    assert!(engine.open(Rc::new(b"not a workbook".to_vec())).is_err());
    assert!(engine.book().is_none());
    assert_eq!(engine.book_json(), "null");
}

/// Medium 2, Excel's default: a blue heading in white bold, and pale blue on every other row.
#[test]
fn a_tables_style_draws_under_its_cells() {
    let mut engine = open(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row>
        <row r="2"><c r="A2"><v>1</v></c><c r="B2" s="1"><v>2</v></c></row>
        <row r="3"><c r="A3"><v>3</v></c><c r="B3"><v>4</v></c></row>"#,
        "",
    );
    engine
        .apply_json(
            r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":1,"bottom":3,"right":2}}"#,
        )
        .unwrap();
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    let heading = tile.cell(0, 0);
    assert_eq!(heading.fill, 0x4472C4FF);
    assert_eq!(heading.color, 0xFFFFFFFF);
    assert_eq!(tile.cell(1, 0).fill, 0xD9E1F2FF);
    assert_eq!(tile.cell(2, 0).fill, 0);
    // A cell's own fill stays.
    assert_eq!(tile.cell(1, 1).fill, 0xFFFF00FF);
}

/// Excel keeps a table's look as formats of the cells' own once it's turned back into cells.
#[test]
fn a_table_turned_into_cells_keeps_its_look() {
    let mut engine = open(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c></row>
        <row r="2"><c r="A2"><v>1</v></c></row>
        <row r="3"><c r="A3"><v>3</v></c></row>"#,
        "",
    );
    engine
        .apply_json(
            r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":1,"bottom":3,"right":1}}"#,
        )
        .unwrap();
    engine
        .apply_json(r#"{"kind":"convertToRange","table":"Table1"}"#)
        .unwrap();
    let tile = Tile::new(&mut engine, 0, 0, 0, LIGHT);
    assert_eq!(tile.cell(0, 0).fill, 0x4472C4FF);
    assert_eq!(tile.cell(0, 0).color, 0xFFFFFFFF);
    assert_eq!(tile.cell(2, 0).fill, 0);
    let format: serde_json::Value =
        serde_json::from_str(&engine.cell_format_json(0, 1, 1)).unwrap();
    assert_eq!(format["bold"], true, "{format}");
}
