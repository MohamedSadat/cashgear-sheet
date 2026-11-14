//! Page setup through the engine: commands change it with undo, a save writes it by editing the
//! file's own elements, and what was saved reads back the same.

use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use cgs_engine::{Command, Engine, LoadPhase};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip_of(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut out);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

fn book(sheet_body: &str) -> Vec<u8> {
    let types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="My Sheet" sheetId="1" r:id="rId1"/><sheet name="Two" sheetId="2" r:id="rId2"/></sheets><calcPr calcId="1"/></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{RELS}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#
    );
    let sheet_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/printerSettings" Target="../printerSettings/printerSettings1.bin"/></Relationships>"#
    );
    let mut rows = String::new();
    for row in 1..=40 {
        rows.push_str(&format!(
            r#"<row r="{row}"><c r="A{row}"><v>{row}</v></c></row>"#
        ));
    }
    zip_of(&[
        ("[Content_Types].xml", types.as_bytes().to_vec()),
        ("_rels/.rels", root.into_bytes()),
        ("xl/workbook.xml", workbook.into_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        (
            "xl/worksheets/sheet1.xml",
            format!(
                r#"<worksheet xmlns="{MAIN}" xmlns:r="{RELS}">{}</worksheet>"#,
                sheet_body.replace("ROWS", &rows)
            )
            .into_bytes(),
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            sheet_rels.into_bytes(),
        ),
        ("xl/printerSettings/printerSettings1.bin", vec![1, 2, 3, 4]),
        (
            "xl/worksheets/sheet2.xml",
            format!(r#"<worksheet xmlns="{MAIN}"><sheetData/></worksheet>"#).into_bytes(),
        ),
    ])
}

/// The same workbook with a styles part.
fn with_styles(bytes: Vec<u8>, styles: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        let text = String::from_utf8(data.clone()).unwrap_or_default();
        let data = match name.as_str() {
            "[Content_Types].xml" => text
                .replace("</Types>", r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#)
                .into_bytes(),
            "xl/_rels/workbook.xml.rels" => text
                .replace("</Relationships>", &format!(r#"<Relationship Id="rId9" Type="{RELS}/styles" Target="styles.xml"/></Relationships>"#))
                .into_bytes(),
            _ => data,
        };
        parts.push((name, data));
    }
    parts.push(("xl/styles.xml".to_string(), styles.as_bytes().to_vec()));
    let named: Vec<(&str, Vec<u8>)> = parts.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
    zip_of(&named)
}

const BODY: &str = r#"<sheetPr codeName="Sheet1"><tabColor rgb="FFFF0000"/></sheetPr><dimension ref="A1:A40"/><sheetData>ROWS</sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="portrait" horizontalDpi="1200" verticalDpi="1200" r:id="rId1"/><legacyDrawingHF r:id="rId7"/>"#;

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    while engine.prepare_step().unwrap() {}
    engine
}

fn saved(engine: &mut Engine) -> Vec<u8> {
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    engine.verify_saved(&out).unwrap();
    out
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn raw(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn setup(engine: &Engine, sheet: u32) -> Value {
    let text = engine
        .query(&format!(r#"{{"kind":"pageSetup","sheet":{sheet}}}"#))
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn run(engine: &mut Engine, json: &str) {
    engine.apply_json(json).unwrap();
}

const CHANGES: &[&str] = &[
    r#"{"kind":"pageSetup","sheets":[0],"spec":{"orientation":"landscape","fitToPage":true,"fitWidth":1,"fitHeight":0,"margins":{"left":0.25,"right":0.25,"top":1,"bottom":1,"header":0.5,"footer":0.5},"centerHorizontally":true,"gridlines":true,"headings":true,"firstPage":3,"pageOrder":"overThenDown","headerFooter":{"oddHeader":"&L&\"Arial,Bold\"&14Report&R&D","oddFooter":"&CPage &P of &N","differentFirst":true,"firstFooter":"&CFirst"}}}"#,
    r#"{"kind":"printArea","sheet":0,"areas":[{"top":1,"left":1,"bottom":30,"right":4}],"action":"set"}"#,
    r#"{"kind":"printArea","sheet":0,"areas":[{"top":35,"left":1,"bottom":40,"right":2}],"action":"add"}"#,
    r#"{"kind":"printTitles","sheet":0,"rows":[1,2],"cols":[1,1]}"#,
    r#"{"kind":"pageBreak","sheet":0,"row":11,"col":3,"action":"insert"}"#,
];

#[test]
fn page_setup_saves_by_editing_the_files_own_elements() {
    let mut engine = opened(book(BODY));
    for change in CHANGES {
        run(&mut engine, change);
    }
    let bytes = saved(&mut engine);
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    // The printer's settings stay linked, and the part itself is copied as it was.
    assert!(sheet.contains(r#"<pageSetup paperSize="9" orientation="landscape" horizontalDpi="1200" verticalDpi="1200" r:id="rId1" firstPageNumber="3" fitToHeight="0" pageOrder="overThenDown" useFirstPageNumber="1"/>"#), "{sheet}");
    assert_eq!(
        raw(&bytes, "xl/printerSettings/printerSettings1.bin"),
        vec![1, 2, 3, 4]
    );
    assert!(sheet.contains(r#"<sheetPr codeName="Sheet1"><tabColor rgb="FFFF0000"/><pageSetUpPr fitToPage="1"/></sheetPr>"#), "{sheet}");
    assert!(sheet.contains(r#"<printOptions horizontalCentered="1" headings="1" gridLines="1"/><pageMargins left="0.25" right="0.25" top="1" bottom="1" header="0.5" footer="0.5"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<headerFooter differentFirst="1"><oddHeader>&amp;L&amp;"Arial,Bold"&amp;14Report&amp;R&amp;D</oddHeader><oddFooter>&amp;CPage &amp;P of &amp;N</oddFooter><firstFooter>&amp;CFirst</firstFooter></headerFooter><rowBreaks count="1" manualBreakCount="1"><brk id="10" max="16383" man="1"/></rowBreaks><colBreaks count="1" manualBreakCount="1"><brk id="2" max="1048575" man="1"/></colBreaks><legacyDrawingHF r:id="rId7"/>"#), "{sheet}");
    let workbook = part(&bytes, "xl/workbook.xml");
    assert!(workbook.contains(r#"<definedName name="_xlnm.Print_Area" localSheetId="0">'My Sheet'!$A$1:$D$30,'My Sheet'!$A$35:$B$40</definedName>"#), "{workbook}");
    assert!(workbook.contains(r#"<definedName name="_xlnm.Print_Titles" localSheetId="0">'My Sheet'!$A:$A,'My Sheet'!$1:$2</definedName>"#), "{workbook}");
    // The other sheet wasn't touched.
    assert_eq!(
        part(&bytes, "xl/worksheets/sheet2.xml"),
        format!(r#"<worksheet xmlns="{MAIN}"><sheetData/></worksheet>"#)
    );

    let before = setup(&engine, 0);
    let again = opened(bytes);
    let after = setup(&again, 0);
    assert_eq!(before, after);
    assert_eq!(after["orientation"], "landscape");
    assert_eq!(after["fitToPage"], true);
    assert_eq!(after["firstPage"], 3);
    assert_eq!(after["rowBreaks"], serde_json::json!([10]));
    assert_eq!(after["colBreaks"], serde_json::json!([2]));
    assert_eq!(after["printArea"], "$A$1:$D$30,$A$35:$B$40");
    assert_eq!(after["titleRows"], "$1:$2");
    assert_eq!(after["titleCols"], "$A:$A");
    assert_eq!(after["headerFooter"]["firstFooter"], "&CFirst");
}

#[test]
fn page_setup_undoes_and_redoes() {
    let mut engine = opened(book(BODY));
    let first = setup(&engine, 0);
    assert_eq!(first["paper"], 9);
    assert_eq!(first["orientation"], "portrait");
    for change in CHANGES {
        run(&mut engine, change);
    }
    let changed = setup(&engine, 0);
    for _ in CHANGES {
        engine.apply(Command::Undo).unwrap();
    }
    assert_eq!(setup(&engine, 0), first);
    for _ in CHANGES {
        engine.apply(Command::Redo).unwrap();
    }
    assert_eq!(setup(&engine, 0), changed);
}

#[test]
fn breaks_and_titles_move_with_rows() {
    let mut engine = opened(book(BODY));
    run(&mut engine, CHANGES[3]);
    run(&mut engine, CHANGES[4]);
    run(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":3,"delete":false}"#,
    );
    let moved = setup(&engine, 0);
    assert_eq!(moved["rowBreaks"], serde_json::json!([13]));
    assert_eq!(moved["titleRows"], "$4:$5");
    let bytes = saved(&mut engine);
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(r#"<brk id="13" max="16383" man="1"/>"#),
        "{sheet}"
    );
    let again = opened(bytes);
    assert_eq!(setup(&again, 0)["rowBreaks"], serde_json::json!([13]));
    // A deleted row takes its break with it.
    let mut engine = again;
    run(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":13,"count":1,"delete":true}"#,
    );
    assert_eq!(setup(&engine, 0)["rowBreaks"], serde_json::json!([]));
}

#[test]
fn a_sheet_untouched_saves_its_page_setup_byte_for_byte() {
    let mut engine = opened(book(BODY));
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"x"}"#,
    );
    let bytes = saved(&mut engine);
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="portrait" horizontalDpi="1200" verticalDpi="1200" r:id="rId1"/>"#), "{sheet}");
}

fn pages(engine: &Engine, sheet: u32, request: &str) -> Value {
    let text = engine
        .query(&format!(
            r#"{{"kind":"pages","sheet":{sheet},"request":{request}}}"#
        ))
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn pages_follow_the_print_area_titles_and_breaks() {
    let mut engine = opened(book(BODY));
    // A4 portrait at Excel's normal margins: 841.89 - 108 points hold 48 rows of 15.
    let all = pages(&engine, 0, "{}");
    assert_eq!(all["pages"].as_array().unwrap().len(), 1);
    assert_eq!(all["pages"][0]["rows"], serde_json::json!([1, 40]));
    run(&mut engine, CHANGES[4]);
    let broken = pages(&engine, 0, "{}");
    assert_eq!(broken["rowBreaks"], serde_json::json!([10]));
    assert_eq!(broken["pages"][1]["rows"], serde_json::json!([11, 40]));
    run(&mut engine, CHANGES[1]);
    run(&mut engine, CHANGES[3]);
    let titled = pages(&engine, 0, "{}");
    assert_eq!(titled["pages"][1]["titleRows"], serde_json::json!([1, 2]));
    assert_eq!(titled["pages"][1]["rows"], serde_json::json!([11, 30]));
    // A selection prints instead of the print area, and an empty sheet has no pages.
    let chosen = pages(
        &engine,
        0,
        r#"{"selection":[{"top":5,"left":1,"bottom":6,"right":1}]}"#,
    );
    assert_eq!(chosen["pages"][0]["rows"], serde_json::json!([5, 6]));
    assert_eq!(pages(&engine, 1, "{}")["pages"], serde_json::json!([]));
}

#[test]
fn text_running_into_empty_cells_prints_with_them() {
    let long = "Text long enough to run on across many empty columns to its right ".repeat(2);
    let sheet = |extra: &str| {
        format!(
            r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{long}</t></is></c>{extra}</row><row r="2"><c r="A2"><v>1</v></c></row></sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="portrait"/>"#
        )
    };
    let engine = opened(book(&sheet("")));
    let wide = pages(&engine, 0, "{}");
    // About 900 pixels of text from A1: past the eight columns an A4 page holds.
    assert_eq!(wide["pages"].as_array().unwrap().len(), 2, "{wide}");
    // A value in B1 stops it.
    let engine = opened(book(&sheet(r#"<c r="B1"><v>2</v></c>"#)));
    assert_eq!(
        pages(&engine, 0, "{}")["pages"].as_array().unwrap().len(),
        1
    );
    // So does wrapping, which keeps the text in its cell.
    let wrapped = sheet("").replace(
        r#"<c r="A1" t="inlineStr">"#,
        r#"<c r="A1" s="1" t="inlineStr">"#,
    );
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellXfs count="2"><xf fontId="0" fillId="0" borderId="0"/><xf fontId="0" fillId="0" borderId="0" applyAlignment="1"><alignment wrapText="1"/></xf></cellXfs></styleSheet>"#;
    let engine = opened(with_styles(book(&wrapped), styles));
    assert_eq!(
        pages(&engine, 0, "{}")["pages"].as_array().unwrap().len(),
        1
    );
}

/// What a view of a sheet shows when it names no range (Track S3): the cells holding values,
/// fills and borders, from the first of them rather than from A1, and nothing on an empty sheet.
#[test]
fn the_used_range_starts_at_the_first_cell_used() {
    let body = r#"<sheetData><row r="3"><c r="C3"><v>1</v></c></row><row r="7"><c r="B7" s="1"/><c r="E7"><v>2</v></c></row></sheetData>"#;
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf/><xf fillId="2" applyFill="1"/></cellXfs></styleSheet>"#;
    let engine = opened(with_styles(book(body), styles));
    let ask = |sheet: u32| -> Value {
        let question = format!(r#"{{"kind":"usedRange","sheet":{sheet}}}"#);
        serde_json::from_str(&engine.query(&question).unwrap()).unwrap()
    };
    // The filled blank at B7 counts, as printing counts it.
    assert_eq!(
        ask(0),
        serde_json::json!({ "top": 3, "left": 2, "bottom": 7, "right": 5 })
    );
    assert_eq!(ask(1), Value::Null);
}
