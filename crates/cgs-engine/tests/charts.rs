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
