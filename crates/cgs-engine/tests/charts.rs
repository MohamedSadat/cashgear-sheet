//! Charts and pictures through the engine's commands: made from a selection, edited, moved,
//! saved, read back and saved again, as the file's parts say.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::rc::Rc;

use cgs_engine::{Command, Engine, LoadPhase, Screen};
use cgs_model::chart::{ChartText, GroupKind, LegendPosition};
use cgs_model::ChartBody;
use zip::ZipArchive;

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    engine
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
    engine.take_dirty();
}

fn typed(engine: &mut Engine, row: u32, col: u32, text: &str) {
    engine
        .apply(Command::Input {
            sheet: 0,
            row,
            col,
            text: text.to_string(),
            areas: Vec::new(),
            force: false,
        })
        .unwrap();
}

fn parts(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).unwrap();
            let mut body = Vec::new();
            file.read_to_end(&mut body).unwrap();
            (file.name().unwrap().to_string(), body)
        })
        .collect()
}

fn text(parts: &HashMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts[name].clone()).unwrap()
}

fn saved(engine: &mut Engine) -> Vec<u8> {
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    engine.verify_saved(&out).unwrap();
    engine.commit_saved(Rc::new(out.clone())).unwrap();
    out
}

fn json(engine: &mut Engine, text: &str) {
    engine.apply_json(text).unwrap();
}

/// Region, sales and costs, with headings.
fn with_data() -> Engine {
    let mut engine = opened(cgs_xlsx::blank::blank_workbook().unwrap());
    for (row, cells) in [
        ["", "Sales", "Costs"],
        ["North", "10", "4"],
        ["South", "20", "8"],
        ["East", "30", "12"],
    ]
    .iter()
    .enumerate()
    {
        for (col, text) in cells.iter().enumerate() {
            if !text.is_empty() {
                typed(&mut engine, row as u32 + 1, col as u32 + 1, text);
            }
        }
    }
    settle(&mut engine);
    engine
}

const SCREEN: Screen = Screen {
    dark: false,
    background: 0xFFFFFF,
    text: 0x000000,
};

#[test]
fn a_chart_from_a_selection_draws_its_cells() {
    let mut engine = with_data();
    json(
        &mut engine,
        r#"{"kind":"insertChart","sheet":0,"area":{"top":1,"left":1,"bottom":4,"right":3},"chart":"columnClustered","x":300,"y":20}"#,
    );
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(0, 0, SCREEN)).unwrap();
    assert_eq!(
        view["categories"],
        serde_json::json!(["North", "South", "East"])
    );
    assert_eq!(view["series"][0]["name"], "Sales");
    assert_eq!(
        view["series"][1]["values"],
        serde_json::json!([4.0, 8.0, 12.0])
    );
    assert_eq!(view["series"][0]["fill"], "#4472C4");
    assert_eq!(view["valueAxes"][0]["max"], 35.0);
    assert_eq!(view["legend"]["position"], "bottom");
    // A changed cell shows at once.
    typed(&mut engine, 2, 2, "40");
    settle(&mut engine);
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(0, 0, SCREEN)).unwrap();
    assert_eq!(view["series"][0]["values"][0], 40.0);
    // In dark mode the chart's background is the screen's.
    let dark: serde_json::Value = serde_json::from_str(&engine.chart_json(
        0,
        0,
        Screen {
            dark: true,
            background: 0x202020,
            text: 0xEEEEEE,
        },
    ))
    .unwrap();
    assert_eq!(dark["background"], "#202020");
    assert_eq!(dark["series"][0]["fill"], "#4472C4");
}

#[test]
fn a_chart_saves_reads_back_and_saves_again() {
    let mut engine = with_data();
    json(
        &mut engine,
        r#"{"kind":"insertChart","sheet":0,"area":{"top":1,"left":1,"bottom":4,"right":3},"chart":"columnClustered","x":300,"y":20}"#,
    );
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"title","text":"Regions"}}"#,
    );
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"legend","position":"right"}}"#,
    );
    json(
        &mut engine,
        r#"{"kind":"setDrawingBox","sheet":0,"index":0,"x":320,"y":40,"width":400,"height":250}"#,
    );
    let out = saved(&mut engine);
    let files = parts(&out);
    let types = text(&files, "[Content_Types].xml");
    assert!(types.contains("/xl/charts/chart1.xml"), "{types}");
    assert!(types.contains("/xl/drawings/drawing1.xml"), "{types}");
    let sheet = text(&files, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("<drawing "), "{sheet}");
    let chart = text(&files, "xl/charts/chart1.xml");
    assert!(chart.contains("<c:f>Sheet1!$B$2:$B$4</c:f>"), "{chart}");
    // The cache holds the values, for programs that read it.
    assert!(chart.contains("<c:v>30</c:v>"), "{chart}");

    // Read back: the same chart, at the same place.
    let mut again = opened(out.clone());
    let book = again.book().unwrap();
    let drawing = &book.sheets[0].drawings[0];
    assert_eq!(drawing.chart, Some(0));
    let ChartBody::Classic(chart) = &book.charts[0].body else {
        panic!()
    };
    assert_eq!(
        chart.title.as_ref().unwrap().text,
        Some(ChartText::Rich("Regions".into()))
    );
    assert_eq!(
        chart.legend.as_ref().unwrap().position,
        LegendPosition::Right
    );
    let layout = again.layout(0).unwrap();
    let placed = &layout.drawings[0];
    assert_eq!(
        (placed.x, placed.y, placed.width, placed.height),
        (320, 40, 400, 250)
    );

    // Edited and saved again: the part keeps its name, and nothing is added twice.
    json(
        &mut again,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"type","chart":"lineMarkers"}}"#,
    );
    let second = saved(&mut again);
    let files = parts(&second);
    assert!(!files.contains_key("xl/charts/chart2.xml"));
    let chart = text(&files, "xl/charts/chart1.xml");
    assert!(
        chart.contains("<c:lineChart>") && !chart.contains("barChart"),
        "{chart}"
    );
    let rels = text(&files, "xl/drawings/_rels/drawing1.xml.rels");
    assert_eq!(rels.matches("<Relationship ").count(), 1, "{rels}");
    let mut third = opened(second);
    let book = third.book().unwrap();
    let ChartBody::Classic(chart) = &book.charts[0].body else {
        panic!()
    };
    assert!(matches!(
        chart.groups[0].kind,
        GroupKind::Line { markers: true, .. }
    ));

    // Deleted: its parts go, and the file still opens.
    json(
        &mut third,
        r#"{"kind":"deleteDrawings","sheet":0,"indexes":[0]}"#,
    );
    let fourth = saved(&mut third);
    let files = parts(&fourth);
    assert!(!files.contains_key("xl/charts/chart1.xml"));
    let types = text(&files, "[Content_Types].xml");
    assert!(!types.contains("chart1.xml"), "{types}");
    let reopened = opened(fourth);
    assert!(reopened.book().unwrap().sheets[0].drawings.is_empty());
}

#[test]
fn rows_inserted_above_move_what_the_chart_reads() {
    let mut engine = with_data();
    json(
        &mut engine,
        r#"{"kind":"insertChart","sheet":0,"area":{"top":1,"left":1,"bottom":4,"right":3},"chart":"columnClustered","x":300,"y":20}"#,
    );
    json(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":2,"delete":false}"#,
    );
    settle(&mut engine);
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(0, 0, SCREEN)).unwrap();
    assert_eq!(
        view["series"][0]["values"],
        serde_json::json!([10.0, 20.0, 30.0])
    );
    let book = engine.book().unwrap();
    let ChartBody::Classic(chart) = &book.charts[0].body else {
        panic!()
    };
    let values = chart.groups[0].series[0].values.as_ref().unwrap();
    assert_eq!(values.formula.as_deref(), Some("Sheet1!$B$4:$B$6"));
    // Undo puts the formula back exactly.
    json(&mut engine, r#"{"kind":"undo"}"#);
    let book = engine.book().unwrap();
    let ChartBody::Classic(chart) = &book.charts[0].body else {
        panic!()
    };
    let values = chart.groups[0].series[0].values.as_ref().unwrap();
    assert_eq!(values.formula.as_deref(), Some("Sheet1!$B$2:$B$4"));
}

#[test]
fn a_picture_goes_in_and_comes_back() {
    let mut engine = with_data();
    // A 1x1 PNG.
    let png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0,
        0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 13, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0, 0, 0x03, 0x01, 0x01, 0, 0xC9, 0xFE, 0x92,
        0xEF, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let part = engine.add_media(png.clone()).unwrap();
    assert_eq!(part, "xl/media/image1.png");
    engine
        .apply(serde_json::from_str(&format!(
            r#"{{"kind":"insertPicture","sheet":0,"part":"{part}","x":10,"y":100,"width":48,"height":48,"name":"dot.png"}}"#
        ))
        .unwrap())
        .unwrap();
    let out = saved(&mut engine);
    let files = parts(&out);
    assert_eq!(files["xl/media/image1.png"], png);
    assert!(text(&files, "[Content_Types].xml").contains("image/png"));
    let reopened = opened(out);
    let drawing = &reopened.book().unwrap().sheets[0].drawings[0];
    assert_eq!(drawing.image.as_deref(), Some("xl/media/image1.png"));
    assert_eq!(drawing.description, "dot.png");
    assert_eq!(reopened.media("xl/media/image1.png"), png);
}

/// A data sheet and a chart sheet whose chart reads it.
fn chart_sheet_book() -> Vec<u8> {
    use std::io::Write;
    const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let parts: Vec<(String, String)> = vec![
        ("[Content_Types].xml".into(), r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/chartsheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml"/><Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/><Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/></Types>"#.into()),
        ("_rels/.rels".into(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)),
        ("xl/workbook.xml".into(), format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Chart1" sheetId="2" r:id="rId2"/></sheets></workbook>"#)),
        ("xl/_rels/workbook.xml.rels".into(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{RELS}/chartsheet" Target="chartsheets/sheet1.xml"/></Relationships>"#)),
        ("xl/worksheets/sheet1.xml".into(), format!(r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1"><v>5</v></c></row><row r="2"><c r="A2"><v>7</v></c></row></sheetData></worksheet>"#)),
        ("xl/chartsheets/sheet1.xml".into(), format!(r#"<chartsheet xmlns="{MAIN}" xmlns:r="{RELS}"><sheetPr/><sheetViews><sheetView workbookViewId="0"/></sheetViews><drawing r:id="rId1"/></chartsheet>"#)),
        ("xl/chartsheets/_rels/sheet1.xml.rels".into(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#)),
        ("xl/drawings/drawing1.xml".into(), format!(r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="8661400" cy="6286500"/><xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="{RELS}" r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>"#)),
        ("xl/drawings/_rels/drawing1.xml.rels".into(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/chart" Target="../charts/chart1.xml"/></Relationships>"#)),
        ("xl/charts/chart1.xml".into(), r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:f>Data!$A$1:$A$2</c:f></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:scaling/><c:delete val="0"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling/><c:delete val="0"/><c:axPos val="l"/><c:crossAx val="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#.into()),
    ];
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in &parts {
            writer.start_file(name.as_str(), options).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

#[test]
fn a_chart_sheet_shows_its_chart_and_saves_an_edit() {
    let mut engine = opened(chart_sheet_book());
    let book = engine.book().unwrap();
    let sheet = &book.sheets[1];
    assert_eq!(sheet.drawings.len(), 1);
    assert_eq!(sheet.drawings[0].chart, Some(0));
    assert!(sheet.part.is_none());
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(1, 0, SCREEN)).unwrap();
    assert_eq!(view["series"][0]["values"], serde_json::json!([5.0, 7.0]));
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":1,"index":0,"edit":{"kind":"title","text":"On its own sheet"}}"#,
    );
    let out = saved(&mut engine);
    let files = parts(&out);
    assert!(text(&files, "xl/charts/chart1.xml").contains("On its own sheet"));
    // The chart sheet's own parts are as they were.
    assert_eq!(
        text(&files, "xl/drawings/drawing1.xml"),
        text(&parts(&chart_sheet_book()), "xl/drawings/drawing1.xml")
    );
    let again = opened(out);
    assert_eq!(again.book().unwrap().sheets[1].drawings.len(), 1);
}

#[test]
fn a_trendline_is_saved_as_excel_2016_writes_it_and_drawn_from_its_fit() {
    let mut engine = with_data();
    json(
        &mut engine,
        r#"{"kind":"insertChart","sheet":0,"area":{"top":1,"left":1,"bottom":4,"right":3},"chart":"columnClustered","x":300,"y":20}"#,
    );
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"trendline","index":0,"trend":"forecast"}}"#,
    );
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(0, 0, SCREEN)).unwrap();
    let trend = &view["series"][0]["trendlines"][0];
    assert_eq!(trend["name"], "Linear (Sales)");
    assert_eq!(trend["values"], serde_json::json!([10.0, 20.0, 30.0]));
    assert_eq!(trend["line"]["dash"], "dot");
    let out = saved(&mut engine);
    let chart = text(&parts(&out), "xl/charts/chart1.xml");
    assert!(chart.contains("<c:trendline><c:spPr><a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"accent1\"/></a:solidFill><a:prstDash val=\"sysDot\"/></a:ln></c:spPr><c:trendlineType val=\"linear\"/><c:forward val=\"2\"/><c:dispRSqr val=\"0\"/><c:dispEq val=\"0\"/></c:trendline><c:cat>"), "{chart}");
    // A moving average in its place, then none.
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"trendline","index":0,"trend":"movingAverage"}}"#,
    );
    let view: serde_json::Value = serde_json::from_str(&engine.chart_json(0, 0, SCREEN)).unwrap();
    assert_eq!(
        view["series"][0]["trendlines"][0]["name"],
        "2 per. Mov. Avg. (Sales)"
    );
    assert_eq!(
        view["series"][0]["trendlines"][0]["values"],
        serde_json::json!([null, 15.0, 25.0])
    );
    let reopened = opened(saved(&mut engine));
    let chart = &reopened.book().unwrap().charts[0];
    let cgs_model::ChartBody::Classic(chart) = &chart.body else {
        panic!("a classic chart")
    };
    assert_eq!(
        chart.groups[0].series[0].trendlines[0].kind,
        cgs_model::chart::TrendKind::MovingAverage(2)
    );
    json(
        &mut engine,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"trendline","index":0}}"#,
    );
    assert!(!text(&parts(&saved(&mut engine)), "xl/charts/chart1.xml").contains("trendline"));
}

#[test]
fn a_picture_turns_flips_takes_alt_text_and_moves_in_front_as_excel_writes_them() {
    let mut engine = with_data();
    let png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0,
        0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 13, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0, 0, 0x03, 0x01, 0x01, 0, 0xC9, 0xFE, 0x92,
        0xEF, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let part = engine.add_media(png).unwrap();
    for (name, y) in [("one.png", 100), ("two.png", 200)] {
        engine
            .apply(serde_json::from_str(&format!(
                r#"{{"kind":"insertPicture","sheet":0,"part":"{part}","x":10,"y":{y},"width":48,"height":48,"name":"{name}"}}"#
            ))
            .unwrap())
            .unwrap();
    }
    json(
        &mut engine,
        r#"{"kind":"pictureFormat","sheet":0,"index":0,"change":{"rotate":90,"flip":"horizontal","description":"Logo"}}"#,
    );
    let out = saved(&mut engine);
    let drawing = text(&parts(&out), "xl/drawings/drawing1.xml");
    assert!(drawing.contains("descr=\"Logo\""), "{drawing}");
    assert!(
        drawing.contains("<a:xfrm rot=\"5400000\" flipH=\"1\">"),
        "{drawing}"
    );
    // Turned again and moved to the front: the file's own anchor changes in place.
    let mut again = opened(out);
    json(
        &mut again,
        r#"{"kind":"pictureFormat","sheet":0,"index":0,"change":{"rotate":-90,"order":"front"}}"#,
    );
    let out = saved(&mut again);
    let drawing = text(&parts(&out), "xl/drawings/drawing1.xml");
    assert!(drawing.contains("<a:xfrm flipH=\"1\">"), "{drawing}");
    let reopened = opened(out);
    let drawings = &reopened.book().unwrap().sheets[0].drawings;
    assert_eq!(drawings[1].description, "Logo");
    assert!(drawings[1].flip_h && drawings[1].rotation == 0);
    assert!(json_err(
        &mut again,
        r#"{"kind":"editChart","sheet":0,"index":0,"edit":{"kind":"trendline","index":0,"trend":"linear"}}"#
    ));
}

fn json_err(engine: &mut Engine, text: &str) -> bool {
    engine.apply_json(text).is_err()
}
