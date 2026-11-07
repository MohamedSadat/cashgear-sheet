//! Writing PDFs from recorded pages, with the fonts the app ships.

use cgs_pdf::{write_pdf, Document};

fn font(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../packages/engine-client/src/pdf-fonts/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A document of one page with the fonts and a picture in its blob.
fn document(ops: &str) -> (Document, Vec<u8>) {
    let carlito = font("Carlito-Regular.ttf");
    let arabic = font("NotoSansArabic-Regular.ttf");
    let mut blob = carlito.clone();
    blob.extend_from_slice(&arabic);
    let picture: Vec<u8> = (0..4u8)
        .flat_map(|i| [i * 60, 255 - i * 60, 128, if i == 3 { 0 } else { 255 }])
        .collect();
    let picture_at = blob.len();
    blob.extend_from_slice(&picture);
    let json = format!(
        r#"{{"title":"Test","pages":[{{"width":612,"height":792,"ops":{ops}}}],
        "files":[{{"offset":0,"length":{},"name":"Carlito"}},{{"offset":{},"length":{},"name":"Noto Sans Arabic"}}],
        "faces":[{{"chain":[0,1]}}],
        "images":[{{"offset":{picture_at},"length":16,"width":2,"height":2,"kind":"rgba"}}]}}"#,
        carlito.len(),
        carlito.len(),
        arabic.len()
    );
    (serde_json::from_str(&json).unwrap(), blob)
}

const ID: &str = "[1,0,0,1,0,0]";

fn text(t: &str, extra: &str) -> String {
    format!(
        r##"{{"k":"text","t":"{t}","x":72,"y":100,"m":{ID},"face":0,"size":11,"fill":"#000000ff"{extra}}}"##
    )
}

#[test]
fn writes_a_page_with_text_shapes_and_a_picture() {
    let ops = format!(
        r##"[{{"k":"save"}},{{"k":"clip","d":[4,0,0,612,792]}},
        {{"k":"path","d":[4,72,72,100,20],"fill":"#ffff0080","stroke":"#ff0000ff","lw":0.75,"dash":[2,1]}},
        {{"k":"path","d":[4,72,200,100,20],"fill":{{"linear":[72,0,172,0],"stops":[[0,"#638ec6ff"],[1,"#ffffffff"]]}}}},
        {},
        {{"k":"image","image":0,"m":[50,0,0,50,300,300]}},
        {{"k":"restore"}}]"##,
        text("Hello, World", "")
    );
    let (doc, blob) = document(&ops);
    let pdf = write_pdf(&doc, &blob).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Type0"), "a CID font");
    assert!(text.contains("/ToUnicode"), "text copies");
    assert!(text.contains("/Identity-H"));
    assert!(text.contains("/SMask"), "the picture's transparency");
    assert!(text.contains("/ShadingType 2"), "the gradient");
    assert!(text.contains("CashGear Sheet"));
    // Only the subset goes in: far smaller than the 628 KB font.
    assert!(pdf.len() < 60_000, "{} bytes", pdf.len());
}

#[test]
fn arabic_falls_back_and_is_shaped() {
    let (mut doc, blob) = document("[]");
    let mixed = "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} Excel 2016";
    doc.pages[0].ops = serde_json::from_str(&format!(
        "[{}]",
        text(mixed, r#","rtl":true,"align":"right""#)
    ))
    .unwrap();
    let pdf = write_pdf(&doc, &blob).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    // Both fonts are used: Arabic from Noto, the Latin from Carlito.
    assert_eq!(text.matches("/Subtype /Type0").count(), 2, "{}", text.len());
}

#[test]
fn shaping_joins_arabic_letters() {
    let data = font("NotoSansArabic-Regular.ttf");
    let carlito = font("Carlito-Regular.ttf");
    let mut files = vec![
        cgs_pdf::FontFile::new(&carlito, 0, "Carlito".into()).unwrap(),
        cgs_pdf::FontFile::new(&data, 0, "Noto".into()).unwrap(),
    ];
    let alone = cgs_pdf::shape_for_tests(&mut files, &[0, 1], "\u{0628}", true);
    let word = cgs_pdf::shape_for_tests(&mut files, &[0, 1], "\u{0628}\u{0628}\u{0628}", true);
    // Joined letters take other forms than the letter alone, all from the Arabic font that
    // Carlito falls back on. Noto draws a beh's dot as a mark, so each letter can be two glyphs.
    assert!(!alone.is_empty());
    assert!(word.iter().all(|&(file, _)| file == 1));
    assert_ne!(word, alone.repeat(3));
    // Latin then Arabic in a left-to-right line keeps that order.
    let mixed = cgs_pdf::shape_for_tests(&mut files, &[0, 1], "ab \u{0628}\u{0628}", false);
    let files_used: Vec<usize> = mixed.iter().map(|g| g.0).collect();
    assert_eq!(&files_used[..3], &[0, 0, 0]);
    assert!(files_used[3..].iter().all(|&f| f == 1));
}

#[test]
fn a_damaged_font_is_refused_in_words() {
    let json = r#"{"pages":[],"files":[{"offset":0,"length":4,"name":"Broken"}]}"#;
    let doc: Document = serde_json::from_str(json).unwrap();
    let error = write_pdf(&doc, &[1, 2, 3, 4]).unwrap_err();
    assert!(error.contains("Broken"), "{error}");
}
