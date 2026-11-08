//! Saving in slices: the same file as saving in one go, reads served between slices, and nothing
//! changing the workbook until the save has ended.

use std::io::{Cursor, Write};
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{EditCell, EditInput, Engine};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const ROWS: u32 = 4000;

/// One sheet of `ROWS` rows: a value, and a formula on it.
fn long_book() -> Vec<u8> {
    let types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    let mut sheet = format!(r#"<worksheet xmlns="{MAIN}"><sheetData>"#);
    for row in 1..=ROWS {
        sheet.push_str(&format!(
            r#"<row r="{row}"><c r="A{row}"><v>{row}</v></c><c r="B{row}"><f>A{row}*2</f><v>{}</v></c></row>"#,
            row * 2
        ));
    }
    sheet.push_str("</sheetData></worksheet>");
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut out);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in [
            ("[Content_Types].xml", types.as_bytes()),
            ("_rels/.rels", root.as_bytes()),
            ("xl/workbook.xml", workbook.as_bytes()),
            ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == cgs_engine::LoadPhase::Reading {}
    engine
}

fn edited() -> Engine {
    let mut engine = opened(long_book());
    engine
        .edit_cells(&[EditCell {
            sheet: 0,
            row: 1,
            col: 1,
            input: Some(EditInput::Value(CalcValue::Number(50.0))),
            style: None,
        }])
        .unwrap();
    engine
}

#[derive(Default)]
struct Cells(Vec<(u32, u32, CalcValue)>);

impl CellSink for Cells {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.push((row, col, value.clone()));
    }
}

fn value_at(engine: &Engine, row: u32, col: u32) -> Option<CalcValue> {
    let mut cells = Cells::default();
    engine.read_cells(
        SheetRect {
            sheet: 0,
            rect: Rect {
                top: row,
                left: col,
                bottom: row,
                right: col,
            },
        },
        &mut cells,
    );
    cells.0.into_iter().next().map(|(_, _, value)| value)
}

#[test]
fn a_save_in_slices_writes_what_one_in_one_go_writes() {
    let mut whole = Vec::new();
    edited().save(&mut whole).unwrap();

    let mut engine = edited();
    let mut sliced = Vec::new();
    engine.save_begin().unwrap();
    let mut slices = 0;
    let report = loop {
        slices += 1;
        if let Some(report) = engine.save_step(0.0, &mut sliced).unwrap() {
            break report;
        }
        // Between slices the workbook reads as it was, and nothing may change it.
        assert!(engine.is_saving());
        assert_eq!(value_at(&engine, 1, 2), Some(CalcValue::Number(100.0)));
        assert!(engine
            .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#)
            .unwrap_err()
            .contains("being saved"));
        assert!(!engine.calc_step(8, &[]).unwrap());
        let (done, total) = engine.save_progress();
        assert!(done <= total && total > 0);
    };
    assert!(slices > 10, "only {slices} slices");
    assert_eq!(sliced, whole);
    assert!(report.cells_written > 0);

    // The check runs in slices too, and then the save can be used.
    engine.verify_begin(Rc::new(sliced.clone())).unwrap();
    let mut checks = 0;
    while !engine.verify_step(0.0).unwrap() {
        checks += 1;
        assert!(engine.is_saving());
    }
    assert!(checks > 1);
    assert!(!engine.is_saving());
    engine.commit_saved(Rc::new(sliced)).unwrap();
    assert!(!engine.has_changes());

    // Edits go ahead once it has ended.
    engine
        .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#)
        .unwrap();
    while engine.calc_step(1000, &[]).unwrap() {}
    assert_eq!(value_at(&engine, 1, 2), Some(CalcValue::Number(18.0)));
}

#[test]
fn a_cancelled_save_leaves_the_workbook_as_it_was() {
    let mut engine = edited();
    engine.save_begin().unwrap();
    assert!(engine.save_step(0.0, &mut Vec::new()).unwrap().is_none());
    engine.save_cancel();
    assert!(!engine.is_saving());
    assert!(engine.has_changes());
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    engine.verify_saved(&out).unwrap();
}

#[test]
fn a_check_in_slices_catches_a_damaged_save() {
    let mut engine = edited();
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    // A byte of the sheet changed, inside its compressed data, after the save.
    let at = out.len() / 3;
    out[at] ^= 0x55;
    engine.verify_begin(Rc::new(out)).unwrap();
    let failed = loop {
        match engine.verify_step(0.0) {
            Ok(true) => break false,
            Ok(false) => {}
            Err(_) => break true,
        }
    };
    assert!(failed);
    assert!(!engine.is_saving());
}
