//! Reads small packages through the real loader and the real formula engine.

use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use cgs_calc::formualizer::FormualizerEngine;
use cgs_calc::{CalcEngine, CalcValue, CellRef, CellSink, Rect, SheetRect};
use cgs_model::{Book, DrawingKind, HAlign, SheetKind, SheetState};
use cgs_xlsx::{book_info, Loader, ReadError};

const CONTENT_TYPES: &str = r#"<Types xmlns="x"/>"#;
const ROOT_RELS: &str = r#"<Relationships xmlns="x"><Relationship Id="rId1" Type="http://x/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
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

/// A workbook of these sheets, with shared strings and styles.
fn workbook(sheets: &[(&str, &str)], strings: &str, styles: &str) -> Vec<u8> {
    let entries: String = sheets
        .iter()
        .enumerate()
        .map(|(index, (name, _))| {
            let number = index + 1;
            format!(r#"<sheet name="{name}" sheetId="{number}" r:id="rId{number}"/>"#)
        })
        .collect();
    let rels: String = sheets
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let number = index + 1;
            format!(
                r#"<Relationship Id="rId{number}" Type="http://x/worksheet" Target="worksheets/sheet{number}.xml"/>"#
            )
        })
        .collect();
    let workbook = format!(r#"<workbook xmlns="x"><sheets>{entries}</sheets></workbook>"#);
    let workbook_rels = format!(
        r#"<Relationships xmlns="x">{rels}<Relationship Id="rS" Type="http://x/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rT" Type="http://x/styles" Target="styles.xml"/></Relationships>"#
    );
    let names: Vec<String> = (0..sheets.len())
        .map(|index| format!("xl/worksheets/sheet{}.xml", index + 1))
        .collect();
    let mut parts: Vec<(&str, &str)> = vec![
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &workbook_rels),
        ("xl/sharedStrings.xml", strings),
        ("xl/styles.xml", styles),
    ];
    for (index, (_, xml)) in sheets.iter().enumerate() {
        parts.push((&names[index], xml));
    }
    zip_of(&parts)
}

struct Opened {
    book: Book,
    engine: FormualizerEngine,
}

fn open(bytes: Vec<u8>) -> Opened {
    let (mut loader, mut book) = Loader::open(Rc::new(bytes)).unwrap();
    let mut engine = FormualizerEngine::new();
    engine.begin_load(&book_info(&book)).unwrap();
    // A budget that stops at every chance proves the reader resumes cleanly.
    while !loader.step(&mut book, &mut engine, &mut || false).unwrap() {}
    engine.finish_load().unwrap();
    Opened { book, engine }
}

#[derive(Default)]
struct Grid(HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn read(engine: &dyn CalcEngine, sheet: u32) -> Grid {
    let mut out = Grid::default();
    engine.read_block(
        SheetRect {
            sheet,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: 40,
                right: 40,
            },
        },
        &mut out,
    );
    out
}

fn number(value: f64) -> CalcValue {
    CalcValue::Number(value)
}

fn text(value: &str) -> CalcValue {
    CalcValue::Text(value.to_string())
}

const NO_STRINGS: &str = "<sst/>";
const NO_STYLES: &str = "<styleSheet/>";

#[test]
fn reads_every_kind_of_cell() {
    let sheet = r#"<worksheet><dimension ref="A1:F5"/><sheetData>
      <row r="1"><c r="A1"><v>1.5</v></c><c r="B1" t="s"><v>0</v></c><c r="C1" t="b"><v>1</v></c>
        <c r="D1" t="e"><v>#N/A</v></c><c r="E1" t="inlineStr"><is><t>inline &amp; text</t></is></c>
        <c r="F1" t="str"><v>a_x000D_b</v></c></row>
      <row r="3"><c r="A3" t="s"><v>1</v></c></row>
      <row r="5"><c r="F5" t="d"><v>2024-01-15</v></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(
        &[("Data", sheet)],
        "<sst><si><t>hello</t></si><si><t>مرحبا</t></si></sst>",
        NO_STYLES,
    ));
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(1, 1)], number(1.5));
    assert_eq!(grid.0[&(1, 2)], text("hello"));
    assert_eq!(grid.0[&(1, 3)], CalcValue::Bool(true));
    assert_eq!(grid.0[&(1, 4)], CalcValue::Error("#N/A".to_string()));
    assert_eq!(grid.0[&(1, 5)], text("inline & text"));
    assert_eq!(grid.0[&(1, 6)], text("a\rb"));
    assert_eq!(grid.0[&(3, 1)], text("مرحبا"));
    assert_eq!(grid.0[&(5, 6)], number(45306.0));
    assert!(opened.book.sheets[0].loaded);
    assert_eq!(opened.book.sheets[0].used, (5, 6));
}

#[test]
fn opening_calculates_nothing_and_shared_formulas_expand() {
    // The saved values are wrong on purpose: seeing them proves nothing calculated.
    let sheet = r#"<worksheet><sheetData>
      <row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>100</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>200</v></c></row>
      <row r="3"><c r="A3"><v>3</v></c><c r="B3" t="str"><f t="shared" si="0"/><v>saved</v></c>
        <c r="C3" t="str"><f>"x"</f></c><c r="D3"><f>A1</f></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(1, 2)], number(100.0));
    assert_eq!(grid.0[&(2, 2)], number(200.0));
    assert_eq!(grid.0[&(3, 2)], text("saved"));
    // An empty string result is a value, which stops text overflowing into it.
    assert_eq!(grid.0[&(3, 3)], text(""));
    // No saved value: blank until something calculates it.
    assert_eq!(grid.0.get(&(3, 4)), None);
    let formula = |row, col| opened.engine.formula_text(CellRef { sheet: 0, row, col });
    assert_eq!(formula(1, 2).as_deref(), Some("A1*2"));
    assert_eq!(formula(3, 2).as_deref(), Some("A3*2"));
}

#[test]
fn a_shared_follower_can_come_before_its_master() {
    let sheet = r#"<worksheet><sheetData>
      <row r="1"><c r="A1"><f t="shared" si="4"/><v>1</v></c></row>
      <row r="2"><c r="A2"><f t="shared" ref="A1:A2" si="4">B2+1</f><v>2</v></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let at = CellRef {
        sheet: 0,
        row: 1,
        col: 1,
    };
    assert_eq!(opened.engine.formula_text(at).as_deref(), Some("B1+1"));
}

#[test]
fn array_output_is_kept_and_edits_rebuild_it() {
    let sheet = r#"<worksheet><sheetData>
      <row r="1"><c r="A1"><v>1</v></c><c r="C1"><f t="array" ref="C1:C2">A1:A2*10</f><v>50</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="C2"><v>60</v></c></row>
    </sheetData></worksheet>"#;
    let mut opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(1, 3)], number(50.0));
    assert_eq!(grid.0[&(2, 3)], number(60.0));
    opened.engine.recalc_all().unwrap();
    while opened.engine.calc_slice(8, &[]).unwrap() != cgs_calc::CalcProgress::Done {}
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(2, 3)], number(20.0));
}

#[test]
fn a_wrong_dimension_starts_the_sheet_over_with_room() {
    let sheet = r#"<worksheet><dimension ref="A1"/><sheetData>
      <row r="1"><c r="A1"><v>1</v></c><c r="H1"><v>8</v></c></row>
      <row r="2"><c r="ZZ2"><v>9</v></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(1, 8)], number(8.0));
    assert_eq!(opened.book.sheets[0].used, (2, 702));
    assert_eq!(opened.book.sheets[0].style_at(2, 702), 0);
}

#[test]
fn rows_out_of_order_still_land_in_place() {
    let sheet = r#"<worksheet><dimension ref="A1:A3"/><sheetData>
      <row r="3"><c r="A3"><v>3</v></c></row>
      <row r="1"><c r="A1"><v>1</v></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let grid = read(&opened.engine, 0);
    assert_eq!(grid.0[&(1, 1)], number(1.0));
    assert_eq!(grid.0[&(3, 1)], number(3.0));
}

#[test]
fn a_huge_sparse_dimension_goes_in_cell_by_cell() {
    let sheet = r#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData>
      <row r="1048576"><c r="XFD1048576"><v>7</v></c></row>
    </sheetData></worksheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    let mut grid = Grid::default();
    opened.engine.read_block(
        SheetRect {
            sheet: 0,
            rect: Rect {
                top: 1_048_576,
                left: 16_384,
                bottom: 1_048_576,
                right: 16_384,
            },
        },
        &mut grid,
    );
    assert_eq!(grid.0[&(1_048_576, 16_384)], number(7.0));
}

#[test]
fn keeps_sizes_styles_merges_and_the_view() {
    let sheet = r#"<worksheet>
      <dimension ref="A1:C4"/>
      <sheetViews><sheetView showGridLines="0" zoomScale="150" workbookViewId="0">
        <pane xSplit="1" ySplit="2" topLeftCell="B3" activePane="bottomRight" state="frozen"/>
        <selection pane="bottomRight" activeCell="C4" sqref="C4"/>
      </sheetView></sheetViews>
      <sheetFormatPr defaultRowHeight="18"/>
      <cols><col min="1" max="1" width="20.7109375" customWidth="1"/>
            <col min="2" max="3" width="9" hidden="1" style="1"/></cols>
      <sheetData>
        <row r="1" ht="30" customHeight="1"><c r="A1" s="1"><v>1</v></c></row>
        <row r="2" hidden="1"><c r="A2"><v>2</v></c></row>
        <row r="4" s="1" customFormat="1"><c r="A4"/></row>
      </sheetData>
      <mergeCells count="2"><mergeCell ref="B1:C2"/><mergeCell ref="A9:A9"/></mergeCells>
    </worksheet>"#;
    let styles = r#"<styleSheet><cellXfs count="2"><xf/><xf numFmtId="4"><alignment horizontal="right"/></xf></cellXfs></styleSheet>"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, styles));
    let info = &opened.book.sheets[0];
    assert_eq!(info.rows.default_px, 24);
    assert_eq!(info.rows.px(1), 40);
    assert_eq!(info.rows.px(2), 0);
    assert_eq!(info.rows.px(3), 24);
    assert_eq!(info.cols.px(1), 145);
    assert_eq!(info.cols.px(2), 0);
    assert_eq!(info.cols.px(4), 64);
    assert_eq!(info.style_at(1, 1), 1);
    assert_eq!(info.style_at(2, 1), 0);
    assert_eq!(info.style_at(3, 2), 1, "from the column");
    assert_eq!(info.style_at(4, 2), 1, "from the row");
    assert_eq!(info.merges.len(), 1, "a single-cell merge is no merge");
    assert!(info.merge_at(2, 3).is_some());
    assert!(!info.view.show_grid);
    assert_eq!(info.view.zoom, 150);
    let freeze = info.view.freeze.unwrap();
    assert_eq!((freeze.rows, freeze.cols), (2, 1));
    assert_eq!(info.view.top_left, (3, 2));
    assert_eq!(info.view.active_cell, (4, 3));
    assert_eq!(opened.book.styles.xf(1).alignment.horizontal, HAlign::Right);
}

#[test]
fn hidden_rows_reach_the_engine() {
    let sheet = r#"<worksheet><sheetData>
      <row r="1"><c r="A1"><v>1</v></c></row>
      <row r="2" hidden="1"><c r="A2"><v>2</v></c></row>
      <row r="3"><c r="A3"><f>SUBTOTAL(109,A1:A2)</f><v>0</v></c></row>
    </sheetData></worksheet>"#;
    let mut opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    opened.engine.recalc_all().unwrap();
    while opened.engine.calc_slice(8, &[]).unwrap() != cgs_calc::CalcProgress::Done {}
    assert_eq!(read(&opened.engine, 0).0[&(3, 1)], number(1.0));
}

#[test]
fn finds_a_workbook_in_an_unusual_place() {
    let bytes = zip_of(&[
        (
            "_rels/.rels",
            r#"<Relationships><Relationship Id="r" Type="http://x/officeDocument" Target="/content/main.xml"/></Relationships>"#,
        ),
        (
            "content/main.xml",
            r#"<workbook><sheets><sheet name="Only" sheetId="1" r:id="r1"/></sheets></workbook>"#,
        ),
        (
            "content/_rels/main.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="s.xml"/></Relationships>"#,
        ),
        (
            "content/s.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1"><v>5</v></c></row></sheetData></worksheet>"#,
        ),
    ]);
    let opened = open(bytes);
    assert_eq!(opened.book.sheets[0].name, "Only");
    assert_eq!(read(&opened.engine, 0).0[&(1, 1)], number(5.0));
}

#[test]
fn hidden_sheets_chart_sheets_and_the_active_tab_come_through() {
    let bytes = zip_of(&[
        (
            "xl/workbook.xml",
            r#"<workbook><bookViews><workbookView activeTab="1"/></bookViews><sheets>
              <sheet name="Visible" sheetId="1" r:id="r1"/>
              <sheet name="Secret" sheetId="2" state="veryHidden" r:id="r2"/>
              <sheet name="Pie" sheetId="3" r:id="r3"/>
            </sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="a.xml"/>
              <Relationship Id="r2" Type="http://x/worksheet" Target="b.xml"/>
              <Relationship Id="r3" Type="http://x/chartsheet" Target="c.xml"/></Relationships>"#,
        ),
        ("xl/a.xml", "<worksheet/>"),
        ("xl/b.xml", "<worksheet/>"),
        ("xl/c.xml", "<chartsheet/>"),
    ]);
    let opened = open(bytes);
    assert_eq!(opened.book.active_sheet, 1);
    assert_eq!(opened.book.sheets[1].state, SheetState::VeryHidden);
    assert_eq!(opened.book.sheets[2].kind, SheetKind::Chart);
    assert!(opened.book.sheets[0].loaded && opened.book.sheets[1].loaded);
}

#[test]
fn reads_tables_and_names_before_cells() {
    let bytes = zip_of(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="S" sheetId="1" r:id="r1"/></sheets>
              <definedNames><definedName name="Rate">S!$A$1</definedName>
              <definedName name="_xlnm.Print_Area" localSheetId="0">S!$A$1:$B$2</definedName></definedNames></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="r1" Type="http://x/worksheet" Target="worksheets/s.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/s.xml",
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Col</t></is></c></row>
              <row r="2"><c r="A2"><v>4</v></c></row></sheetData>
              <drawing r:id="d1"/><tableParts count="1"><tablePart r:id="t1"/></tableParts></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/s.xml.rels",
            r#"<Relationships><Relationship Id="t1" Type="http://x/table" Target="../tables/t.xml"/>
              <Relationship Id="d1" Type="http://x/drawing" Target="../drawings/d.xml"/></Relationships>"#,
        ),
        (
            "xl/tables/t.xml",
            r#"<table name="T" displayName="Sales" ref="A1:A2" totalsRowShown="0"><tableColumns count="1"><tableColumn id="1" name="Col"/></tableColumns>
              <extLst><ext><x14:table altText="no"/></ext></extLst></table>"#,
        ),
        (
            "xl/drawings/d.xml",
            r#"<wsDr><absoluteAnchor><pos x="1" y="2"/><ext cx="3" cy="4"/><pic><nvPicPr><cNvPr name="Logo"/></nvPicPr></pic></absoluteAnchor></wsDr>"#,
        ),
    ]);
    let opened = open(bytes);
    assert_eq!(opened.book.tables.len(), 1);
    assert_eq!(opened.book.tables[0].name, "Sales");
    assert_eq!(opened.book.tables[0].columns, ["Col"]);
    assert_eq!(
        book_info(&opened.book).names.len(),
        1,
        "print areas are left out"
    );
    assert_eq!(opened.book.sheets[0].drawings.len(), 1);
    assert_eq!(opened.book.sheets[0].drawings[0].kind, DrawingKind::Picture);
    assert_eq!(opened.book.sheets[0].drawings[0].name, "Logo");
}

#[test]
fn a_part_cut_short_keeps_what_it_read() {
    let sheet =
        r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v"#;
    let opened = open(workbook(&[("S", sheet)], NO_STRINGS, NO_STYLES));
    assert_eq!(read(&opened.engine, 0).0[&(1, 1)], number(1.0));
    assert!(opened.book.sheets[0].loaded);
}

#[test]
fn files_it_cannot_read_say_why() {
    let error = |bytes: Vec<u8>| Loader::open(Rc::new(bytes)).err().unwrap();
    assert!(matches!(
        error(b"plain text".to_vec()),
        ReadError::NotAPackage(_)
    ));
    assert_eq!(error(zip_of(&[("a.txt", "x")])), ReadError::NotAWorkbook);
    let mut old = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    old.resize(4096, 0);
    assert_eq!(error(old.clone()), ReadError::OldFormat);
    let marker: Vec<u8> = "EncryptedPackage"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    old.extend_from_slice(&marker);
    assert_eq!(error(old), ReadError::Encrypted);
}

#[test]
fn another_sheet_can_be_asked_for_first() {
    let sheet =
        r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
    let (mut loader, mut book) = Loader::open(Rc::new(workbook(
        &[("A", sheet), ("B", sheet), ("C", sheet)],
        NO_STRINGS,
        NO_STYLES,
    )))
    .unwrap();
    let mut engine = FormualizerEngine::new();
    engine.begin_load(&book_info(&book)).unwrap();
    loader.prioritize(2);
    assert!(!loader.step(&mut book, &mut engine, &mut || false).unwrap());
    assert!(book.sheets[2].loaded);
    assert!(!book.sheets[0].loaded);
    while !loader.step(&mut book, &mut engine, &mut || false).unwrap() {}
    assert!(book.sheets.iter().all(|sheet| sheet.loaded));
    let progress = loader.progress();
    assert_eq!(progress.done, progress.total);
}
