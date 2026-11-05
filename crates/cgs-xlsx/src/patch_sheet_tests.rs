use super::*;

fn value(number: f64) -> CellPatch {
    CellPatch {
        change: CellChange::Value(CalcValue::Number(number)),
        style: None,
    }
}

fn text(text: &str) -> CellPatch {
    CellPatch {
        change: CellChange::Value(CalcValue::Text(text.to_string())),
        style: None,
    }
}

fn formula(text: &str, result: f64) -> CellPatch {
    CellPatch {
        change: CellChange::Formula {
            text: text.to_string(),
            value: CalcValue::Number(result),
            array: ArrayKind::None,
        },
        style: None,
    }
}

fn result(number: f64) -> CellPatch {
    CellPatch {
        change: CellChange::Result(CalcValue::Number(number)),
        style: None,
    }
}

fn run_with(
    xml: &str,
    patches: &[((u32, u32), CellPatch)],
    elements: &[NewElement],
) -> (String, StringTable, Outcome) {
    let map: BTreeMap<_, _> = patches.iter().cloned().collect();
    let mut strings = StringTable::new(0);
    let (out, outcome) = patch_into(
        || Ok(xml.as_bytes()),
        Vec::new,
        &map,
        elements,
        &SheetExtras::default(),
        &mut strings,
        false,
    )
    .unwrap();
    (String::from_utf8(out).unwrap(), strings, outcome)
}

fn run(xml: &str, patches: &[((u32, u32), CellPatch)]) -> String {
    run_with(xml, patches, &[]).0
}

/// Wraps `sheetData`'s contents in a worksheet.
fn sheet(data: &str) -> String {
    format!(r#"<worksheet xmlns="m"><sheetData>{data}</sheetData></worksheet>"#)
}

#[test]
fn no_patches_change_no_bytes() {
    let xml = r#"<?xml version="1.0"?>
<worksheet xmlns="m" xmlns:x14ac="z" foo:bar='1'><dimension ref="A1:B2"/><!-- a note -->
  <cols><col min="1" max="2" width="9"/></cols>
  <sheetData>
    <row r="1" spans="1:2" x14ac:dyDescent="0.25"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1"><f>A1</f><v>1</v><extLst><ext uri="u"/></extLst></c></row>
    <row r="2"/>
  </sheetData><unknownThing a="1"><inner/></unknownThing>
  <drawing r:id="rId1"/></worksheet>"#;
    assert_eq!(run(xml, &[]), xml);
}

#[test]
fn a_value_replaces_only_its_cell() {
    let xml = sheet(
        r#"<row r="1" x="y"><c r="A1"><v>1</v></c><c r="B1" s="4" t="s" foo="bar"><v>9</v><extLst/></c><c r="C1"><v>3</v></c></row>"#,
    );
    assert_eq!(
        run(&xml, &[((1, 2), value(5.5))]),
        sheet(
            r#"<row r="1" x="y"><c r="A1"><v>1</v></c><c r="B1" s="4" foo="bar"><v>5.5</v><extLst/></c><c r="C1"><v>3</v></c></row>"#
        )
    );
}

#[test]
fn text_goes_to_shared_strings_and_results_stay_in_the_cell() {
    let (out, strings, _) = run_with(
        &sheet(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1</f><v>1</v></c></row>"#),
        &[
            ((1, 1), text("hello")),
            (
                (1, 2),
                CellPatch {
                    change: CellChange::Result(CalcValue::Text("<&>".to_string())),
                    style: None,
                },
            ),
        ],
        &[],
    );
    assert_eq!(
        out,
        sheet(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="str"><f>A1</f><v>&lt;&amp;&gt;</v></c></row>"#
        )
    );
    assert_eq!(strings.added, ["hello"]);
    assert_eq!(strings.references, 1);
}

#[test]
fn new_cells_and_rows_land_in_order() {
    let xml = sheet(
        r#"<row r="2"><c r="B2"><v>1</v></c><c r="D2"><v>2</v></c></row><row r="5"><c r="A5"><v>3</v></c></row>"#,
    );
    let out = run(
        &xml,
        &[
            ((1, 3), value(10.0)),
            ((2, 1), value(11.0)),
            ((2, 3), value(12.0)),
            ((2, 6), value(13.0)),
            ((3, 1), value(14.0)),
            ((9, 2), value(15.0)),
        ],
    );
    assert_eq!(
        out,
        sheet(concat!(
            r#"<row r="1"><c r="C1"><v>10</v></c></row>"#,
            r#"<row r="2"><c r="A2"><v>11</v></c><c r="B2"><v>1</v></c><c r="C2"><v>12</v></c><c r="D2"><v>2</v></c><c r="F2"><v>13</v></c></row>"#,
            r#"<row r="3"><c r="A3"><v>14</v></c></row>"#,
            r#"<row r="5"><c r="A5"><v>3</v></c></row>"#,
            r#"<row r="9"><c r="B9"><v>15</v></c></row>"#,
        ))
    );
}

#[test]
fn an_empty_row_and_an_empty_sheet_can_take_cells() {
    assert_eq!(
        run(&sheet(r#"<row r="1" ht="20"/>"#), &[((1, 1), value(1.0))]),
        sheet(r#"<row r="1" ht="20"><c r="A1"><v>1</v></c></row>"#)
    );
    assert_eq!(
        run(
            r#"<worksheet xmlns="m"><sheetData/></worksheet>"#,
            &[((2, 2), value(1.0))]
        ),
        sheet(r#"<row r="2"><c r="B2"><v>1</v></c></row>"#)
    );
    // The schema requires `sheetData`, but readers don't, so a file can lack it.
    assert_eq!(
        run(
            r#"<worksheet xmlns="m"><pageMargins/></worksheet>"#,
            &[((1, 1), value(1.0))]
        ),
        r#"<worksheet xmlns="m"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><pageMargins/></worksheet>"#
    );
}

#[test]
fn a_prefixed_worksheet_gets_prefixed_cells() {
    let xml = r#"<x:worksheet xmlns:x="m"><x:sheetData><x:row r="1"><x:c r="B1"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
    assert_eq!(
        run(
            xml,
            &[((1, 1), formula("B1&\"<\"", 2.0)), ((2, 1), value(3.0))]
        ),
        r#"<x:worksheet xmlns:x="m"><x:sheetData><x:row r="1"><x:c r="A1"><x:f>B1&amp;"&lt;"</x:f><x:v>2</x:v></x:c><x:c r="B1"><x:v>1</x:v></x:c></x:row><x:row r="2"><x:c r="A2"><x:v>3</x:v></x:c></x:row></x:sheetData></x:worksheet>"#
    );
}

#[test]
fn clearing_keeps_a_cells_style_and_what_we_dont_know() {
    let clear = CellPatch {
        change: CellChange::Clear,
        style: None,
    };
    let xml = sheet(
        r#"<row r="1"><c r="A1" s="3" t="s" cm="1" vm="2"><v>1</v></c><c r="B1"><f>1+1</f><v>2</v><extLst/></c></row>"#,
    );
    assert_eq!(
        run(&xml, &[((1, 1), clear.clone()), ((1, 2), clear)]),
        sheet(r#"<row r="1"><c r="A1" s="3"/><c r="B1"><extLst/></c></row>"#)
    );
}

#[test]
fn a_style_alone_leaves_the_contents() {
    let patch = CellPatch {
        change: CellChange::Keep,
        style: Some(7),
    };
    let xml =
        sheet(r#"<row r="1"><c r="A1" s="3" t="s"><v>1</v></c><c r="B1" t="b"><v>0</v></c></row>"#);
    assert_eq!(
        run(
            &xml,
            &[
                ((1, 1), patch.clone()),
                ((1, 2), patch.clone()),
                ((1, 3), patch)
            ]
        ),
        sheet(
            r#"<row r="1"><c r="A1" s="7" t="s"><v>1</v></c><c r="B1" t="b" s="7"><v>0</v></c><c r="C1" s="7"/></row>"#
        )
    );
}

#[test]
fn new_cells_take_the_rows_style_and_then_the_columns() {
    let xml = r#"<worksheet xmlns="m"><cols><col min="2" max="3" style="5" width="9"/></cols><sheetData><row r="1" s="8" customFormat="1"><c r="A1"><v>1</v></c></row><row r="2" s="9"><c r="A2"><v>1</v></c></row></sheetData></worksheet>"#;
    let out = run(
        xml,
        &[
            ((1, 2), value(1.0)),
            ((2, 2), value(1.0)),
            ((2, 4), value(1.0)),
            ((3, 3), value(1.0)),
        ],
    );
    assert!(out.contains(r#"<c r="B1" s="8">"#), "{out}");
    // `s` without `customFormat` doesn't count, as in Excel.
    assert!(out.contains(r#"<c r="B2" s="5">"#), "{out}");
    assert!(out.contains(r#"<c r="D2"><v>"#), "{out}");
    assert!(out.contains(r#"<row r="3"><c r="C3" s="5">"#), "{out}");
}

#[test]
fn a_result_that_matches_changes_nothing_and_keeps_shared_formulas() {
    let xml = sheet(
        r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1*2</f><v>2</v></c></row><row r="2"><c r="A2" cm="1"><f t="shared" si="0"/><v>4</v></c></row>"#,
    );
    let (out, _, outcome) = run_with(&xml, &[((1, 1), result(2.0)), ((2, 1), result(4.0))], &[]);
    assert_eq!(out, xml);
    assert!(!outcome.changed);
    assert_eq!(
        run(&xml, &[((1, 1), result(3.0)), ((2, 1), result(5.0))]),
        sheet(
            r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1*2</f><v>3</v></c></row><row r="2"><c r="A2" cm="1"><f t="shared" si="0"/><v>5</v></c></row>"#
        )
    );
}

#[test]
fn a_result_with_no_cell_is_left_for_excel() {
    let (out, _, outcome) = run_with(&sheet(""), &[((1, 1), result(3.0))], &[]);
    assert_eq!(out, sheet(""));
    assert_eq!(outcome.results_skipped, 1);
    assert!(!outcome.changed);
}

#[test]
fn changing_a_shared_master_gives_its_followers_their_own_formulas() {
    let xml = sheet(
        r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A3" si="0">B1*2</f><v>2</v></c></row><row r="2"><c r="A2"><f t="shared" si="0"/><v>4</v></c></row><row r="3"><c r="A3"><f t="shared" ca="1" si="0"></f><v>6</v></c></row>"#,
    );
    assert_eq!(
        run(&xml, &[((1, 1), value(1.0))]),
        sheet(
            r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><f>B2*2</f><v>4</v></c></row><row r="3"><c r="A3"><f ca="1">B3*2</f><v>6</v></c></row>"#
        )
    );
}

#[test]
fn followers_before_their_master_are_found_on_a_second_pass() {
    let xml = sheet(
        r#"<row r="1"><c r="A1"><f t="shared" si="0"/><v>4</v></c></row><row r="2"><c r="A2"><f t="shared" ref="A1:A2" si="0">B2*2</f><v>2</v></c></row>"#,
    );
    assert_eq!(
        run(&xml, &[((2, 1), value(1.0))]),
        sheet(
            r#"<row r="1"><c r="A1"><f>B1*2</f><v>4</v></c></row><row r="2"><c r="A2"><v>1</v></c></row>"#
        )
    );
}

#[test]
fn a_shared_follower_can_change_on_its_own() {
    let xml = sheet(
        r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1*2</f><v>2</v></c><c r="A2"><f t="shared" si="0"/><v>4</v></c></row>"#,
    );
    // A1's row claims A2 here, which is untidy but legal, and it's the cell's address that counts.
    assert_eq!(
        run(&xml, &[((2, 1), value(9.0))]),
        sheet(
            r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1*2</f><v>2</v></c><c r="A2"><v>9</v></c></row>"#
        )
    );
}

#[test]
fn rows_out_of_order_never_get_a_second_row_with_the_same_number() {
    let xml =
        sheet(r#"<row r="5"><c r="A5"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row>"#);
    assert_eq!(
        run(&xml, &[((2, 1), value(7.0)), ((3, 1), value(8.0))]),
        sheet(
            r#"<row r="3"><c r="A3"><v>8</v></c></row><row r="5"><c r="A5"><v>1</v></c></row><row r="2"><c r="A2"><v>7</v></c></row>"#
        )
    );
}

#[test]
fn cells_and_rows_with_no_address_get_one_when_their_row_changes() {
    let xml = sheet(
        r#"<row><c><v>1</v></c><c><v>2</v></c><c r="E1"><v>3</v></c><c><v>4</v></c></row><row><c><v>5</v></c></row>"#,
    );
    assert_eq!(
        run(&xml, &[((1, 2), value(9.0)), ((1, 4), value(8.0))]),
        sheet(
            r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>9</v></c><c r="D1"><v>8</v></c><c r="E1"><v>3</v></c><c r="F1"><v>4</v></c></row><row><c><v>5</v></c></row>"#
        )
    );
}

#[test]
fn a_new_cell_outside_a_rows_spans_removes_the_hint() {
    let xml = sheet(r#"<row r="1" spans="2:3"><c r="B1"><v>1</v></c></row>"#);
    assert_eq!(
        run(&xml, &[((1, 3), value(2.0))]),
        sheet(r#"<row r="1" spans="2:3"><c r="B1"><v>1</v></c><c r="C1"><v>2</v></c></row>"#)
    );
    assert_eq!(
        run(&xml, &[((1, 9), value(2.0))]),
        sheet(r#"<row r="1"><c r="B1"><v>1</v></c><c r="I1"><v>2</v></c></row>"#)
    );
}

#[test]
fn the_dimension_grows_to_hold_new_cells() {
    let xml = r#"<worksheet xmlns="m"><dimension ref="B2:C3"/><sheetData/></worksheet>"#;
    let out = run(xml, &[((9, 1), value(1.0))]);
    assert!(out.contains(r#"<dimension ref="A2:C9"/>"#), "{out}");
    // `A1` is how an empty sheet says it holds nothing.
    let empty = r#"<worksheet xmlns="m"><dimension ref="A1"/><sheetData/></worksheet>"#;
    assert!(run(empty, &[((2, 2), value(1.0))]).contains(r#"<dimension ref="B2"/>"#));
    // A result is no reason to grow.
    assert_eq!(
        run(xml, &[((9, 1), result(1.0))]).matches("B2:C3").count(),
        1
    );
}

#[test]
fn new_elements_go_where_the_schema_wants_them() {
    let merge = NewElement {
        name: "mergeCells".to_string(),
        xml: r#"<mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells>"#.to_string(),
    };
    let xml = r#"<worksheet xmlns="m"><sheetData/><autoFilter ref="A1"/><conditionalFormatting/><pageMargins/><unknownThing/><tableParts/></worksheet>"#;
    let (out, _, outcome) = run_with(xml, &[], std::slice::from_ref(&merge));
    assert_eq!(
        out,
        r#"<worksheet xmlns="m"><sheetData/><autoFilter ref="A1"/><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells><conditionalFormatting/><pageMargins/><unknownThing/><tableParts/></worksheet>"#
    );
    assert!(outcome.changed);
    // Last of all when nothing later is there, and it keeps its namespace under a prefix.
    let prefixed = r#"<x:worksheet xmlns:x="m"><x:sheetData/></x:worksheet>"#;
    assert_eq!(
        run_with(prefixed, &[], &[merge]).0,
        r#"<x:worksheet xmlns:x="m"><x:sheetData/><mergeCells xmlns="m" count="1"><mergeCell ref="A1:B1"/></mergeCells></x:worksheet>"#
    );
}

#[test]
fn an_element_goes_after_others_of_its_own_kind() {
    let rule = NewElement {
        name: "conditionalFormatting".to_string(),
        xml: "<conditionalFormatting/>".to_string(),
    };
    let xml = r#"<worksheet xmlns="m"><sheetData/><conditionalFormatting sqref="A1"/><dataValidations/></worksheet>"#;
    assert_eq!(
        run_with(xml, &[], &[rule]).0,
        r#"<worksheet xmlns="m"><sheetData/><conditionalFormatting sqref="A1"/><conditionalFormatting/><dataValidations/></worksheet>"#
    );
}

#[test]
fn every_order_table_name_is_known_and_alternate_content_has_a_place() {
    assert!(worksheet_order(b"sheetData") < worksheet_order(b"mergeCells"));
    assert!(worksheet_order(b"AlternateContent") > worksheet_order(b"drawing"));
    assert!(worksheet_order(b"AlternateContent") < worksheet_order(b"tableParts"));
    assert_eq!(worksheet_order(b"madeUp"), None);
}

#[test]
fn a_non_utf8_part_gets_character_references() {
    let map: BTreeMap<_, _> = [(
        (1, 1),
        CellPatch {
            change: CellChange::Formula {
                text: "\"ن\"".to_string(),
                value: CalcValue::Text("ن".to_string()),
                array: ArrayKind::None,
            },
            style: None,
        },
    )]
    .into_iter()
    .collect();
    let mut strings = StringTable::new(0);
    let xml = sheet("");
    let (out, _) = patch_into(
        || Ok(xml.as_bytes()),
        Vec::new,
        &map,
        &[],
        &SheetExtras::default(),
        &mut strings,
        true,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("<f>\"&#1606;\"</f><v>&#1606;</v>"), "{out}");
}

#[test]
fn a_damaged_sheet_fails_rather_than_saving_something_wrong() {
    let map: BTreeMap<_, _> = [((1, 1), value(1.0))].into_iter().collect();
    let mut strings = StringTable::new(0);
    let xml = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1"#;
    assert!(patch_into(
        || Ok(xml.as_bytes()),
        Vec::new,
        &map,
        &[],
        &SheetExtras::default(),
        &mut strings,
        false
    )
    .is_err());
}

#[test]
fn errors_only_rich_values_can_hold_never_reach_a_plain_cell() {
    let error = |text: &str| CalcValue::Error(text.to_string());
    let xml =
        sheet(r#"<row r="1"><c r="A1"><f>1/0</f><v>1</v></c><c r="B1"><f>2</f><v>2</v></c></row>"#);
    let (out, strings, outcome) = run_with(
        &xml,
        &[
            (
                (1, 1),
                CellPatch {
                    change: CellChange::Result(error("#DIV/0!")),
                    style: None,
                },
            ),
            (
                (1, 2),
                CellPatch {
                    change: CellChange::Result(error("#SPILL!")),
                    style: None,
                },
            ),
            (
                (2, 1),
                CellPatch {
                    change: CellChange::Value(error("#CALC!")),
                    style: None,
                },
            ),
            (
                (2, 2),
                CellPatch {
                    change: CellChange::Formula {
                        text: "SEQUENCE(0)".to_string(),
                        value: error("#CALC!"),
                        array: ArrayKind::None,
                    },
                    style: None,
                },
            ),
        ],
        &[],
    );
    assert_eq!(
        out,
        sheet(concat!(
            r#"<row r="1"><c r="A1" t="e"><f>1/0</f><v>#DIV/0!</v></c><c r="B1"><f>2</f></c></row>"#,
            r#"<row r="2"><c r="A2" t="s"><v>0</v></c><c r="B2"><f>SEQUENCE(0)</f></c></row>"#,
        ))
    );
    assert_eq!(strings.added, ["#CALC!"]);
    assert_eq!(outcome.results_skipped, 2);
}

#[test]
fn numbers_read_back_exactly() {
    for number in [
        0.0,
        1.0,
        -2.5,
        1e-300,
        1.7976931348623157e308,
        0.1 + 0.2,
        1e21,
        123456789012345680.0,
    ] {
        assert_eq!(number_text(number).parse::<f64>().unwrap(), number);
    }
    assert_eq!(number_text(1e21), "1e21");
    assert_eq!(number_text(0.00001), "0.00001");
    assert_eq!(cell_name(1, 1), "A1");
    assert_eq!(cell_name(1048576, 16384), "XFD1048576");
}

fn with_extras(xml: &str, extras: &SheetExtras) -> String {
    let map = BTreeMap::new();
    let mut strings = StringTable::new(0);
    let (out, _) = patch_into(
        || Ok(xml.as_bytes()),
        Vec::new,
        &map,
        &[],
        extras,
        &mut strings,
        false,
    )
    .unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_row_says_what_changed_and_keeps_the_rest() {
    let xml = sheet(r#"<row r="2" ht="14.4" x14ac:dyDescent="0.3"><c r="A2"><v>1</v></c></row>"#);
    let mut extras = SheetExtras::default();
    // The height comes to the same pixels, so it stays as stored.
    extras.rows.insert(
        2,
        RowPatch {
            size_px: Some(19),
            custom: false,
            hidden: true,
            style: Some(4),
        },
    );
    extras.rows.insert(
        5,
        RowPatch {
            size_px: Some(40),
            custom: true,
            ..RowPatch::default()
        },
    );
    let out = with_extras(&xml, &extras);
    assert!(out.contains(r#"ht="14.4""#), "{out}");
    assert!(out.contains(r#"x14ac:dyDescent="0.3""#), "{out}");
    assert!(out.contains(r#"hidden="1""#), "{out}");
    assert!(out.contains(r#"s="4" customFormat="1""#), "{out}");
    assert!(out.contains(r#"<c r="A2"><v>1</v></c>"#), "{out}");
    assert!(
        out.contains(r#"<row r="5" ht="30" customHeight="1"></row>"#),
        "{out}"
    );
}

#[test]
fn columns_split_where_a_change_lands_and_keep_what_they_said() {
    let xml = r#"<worksheet><cols><col min="2" max="6" width="20" customWidth="1" outlineLevel="1"/></cols><sheetData/></worksheet>"#;
    let extras = SheetExtras {
        cols: vec![ColPatch {
            first: 4,
            last: 8,
            size_px: Some(100),
            default_px: 64,
            digit: 7.0,
            custom: true,
            hidden: false,
            style: Some(3),
        }],
        ..SheetExtras::default()
    };
    let out = with_extras(xml, &extras);
    assert!(
        out.contains(r#"<col min="2" max="3" width="20" customWidth="1" outlineLevel="1"/>"#),
        "{out}"
    );
    assert!(out.contains(r#"min="4" max="6""#), "{out}");
    assert!(out.contains(r#"outlineLevel="1" style="3""#), "{out}");
    assert!(
        out.contains(r#"<col min="7" max="8" width="14.28515625" customWidth="1" style="3"/>"#),
        "{out}"
    );
    assert_eq!(cgs_model::style::col_width_to_px(14.28515625, 7.0), 100);
}

#[test]
fn every_pixel_width_reads_back_as_itself() {
    for digit in [5.0, 6.0, 7.0, 8.0, 9.0, 11.0] {
        for px in 1..2000 {
            let width = px_to_col_width(px, digit);
            assert_eq!(cgs_model::style::col_width_to_px(width, digit), px);
        }
    }
}

#[test]
fn a_sheet_with_no_cols_gets_them_in_their_place() {
    let xml = r#"<worksheet><sheetFormatPr defaultRowHeight="15"/><sheetData/></worksheet>"#;
    let extras = SheetExtras {
        cols: vec![ColPatch {
            first: 1,
            last: 1,
            size_px: Some(64),
            default_px: 64,
            digit: 7.0,
            hidden: true,
            ..ColPatch::default()
        }],
        ..SheetExtras::default()
    };
    let out = with_extras(xml, &extras);
    assert!(
        out.contains(r#"<sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="1" width="9.140625" hidden="1"/></cols><sheetData"#),
        "{out}"
    );
}

#[test]
fn styles_come_from_the_sheet_for_cells_in_an_area() {
    let xml = sheet(r#"<row r="1"><c r="A1" s="1"><v>1</v></c><c r="C1" s="2"><v>3</v></c></row>"#);
    let mut info = cgs_model::SheetInfo::default();
    info.set_style(1, 1, 7);
    info.set_style(1, 2, 8);
    info.set_style(1, 3, 2);
    info.set_style(3, 2, 9);
    info.used = (3, 3);
    let extras = SheetExtras {
        styled: Some((
            info,
            vec![cgs_model::Rect {
                top: 1,
                left: 1,
                bottom: 3,
                right: 2,
            }],
        )),
        ..SheetExtras::default()
    };
    let out = with_extras(&xml, &extras);
    assert!(out.contains(r#"<c r="A1" s="7"><v>1</v></c>"#), "{out}");
    assert!(out.contains(r#"<c r="B1" s="8"/>"#), "{out}");
    assert!(out.contains(r#"<c r="C1" s="2"><v>3</v></c>"#), "{out}");
    assert!(
        out.contains(r#"<row r="3"><c r="B3" s="9"/></row>"#),
        "{out}"
    );
}

#[test]
fn a_style_that_does_not_change_leaves_the_row_alone() {
    let xml = sheet(r#"<row r="1" spans="1:1"><c r="A1"   s="7" ><v>1</v></c></row>"#);
    let mut info = cgs_model::SheetInfo::default();
    info.set_style(1, 1, 7);
    info.used = (1, 1);
    let extras = SheetExtras {
        styled: Some((
            info,
            vec![cgs_model::Rect {
                top: 1,
                left: 1,
                bottom: 1,
                right: 1,
            }],
        )),
        ..SheetExtras::default()
    };
    let out = with_extras(&xml, &extras);
    assert!(out.contains(r#"<c r="A1"   s="7" ><v>1</v></c>"#), "{out}");
}

#[test]
fn a_frozen_view_writes_its_pane_and_selections() {
    let xml = r#"<worksheet><sheetViews><sheetView tabSelected="1" workbookViewId="0"><selection activeCell="B2" sqref="B2"/></sheetView></sheetViews><sheetData/></worksheet>"#;
    let extras = SheetExtras {
        view: Some(ViewPatch {
            freeze: Some(Some(cgs_model::Freeze {
                rows: 2,
                cols: 1,
                top: 1,
                left: 1,
            })),
            show_grid: Some(false),
            show_headers: None,
            active_cell: (3, 2),
        }),
        ..SheetExtras::default()
    };
    let out = with_extras(xml, &extras);
    assert!(out.contains(r#"showGridLines="0""#), "{out}");
    assert!(
        out.contains(r#"<pane xSplit="1" ySplit="2" topLeftCell="B3" activePane="bottomRight" state="frozen"/><selection pane="topRight" activeCell="B3" sqref="B3"/><selection pane="bottomLeft" activeCell="B3" sqref="B3"/><selection pane="bottomRight" activeCell="B3" sqref="B3"/></sheetView>"#),
        "{out}"
    );
    let unfrozen = SheetExtras {
        view: Some(ViewPatch {
            freeze: Some(None),
            active_cell: (3, 2),
            ..ViewPatch::default()
        }),
        ..SheetExtras::default()
    };
    let back = with_extras(&out, &unfrozen);
    assert!(!back.contains("<pane"), "{back}");
    assert!(
        back.contains(r#"<selection activeCell="B3" sqref="B3"/>"#),
        "{back}"
    );
}

#[test]
fn a_sheet_with_no_views_gets_one_in_its_place() {
    let xml = r#"<worksheet><sheetFormatPr defaultRowHeight="15"/><sheetData/></worksheet>"#;
    let extras = SheetExtras {
        view: Some(ViewPatch {
            show_headers: Some(false),
            active_cell: (1, 1),
            ..ViewPatch::default()
        }),
        ..SheetExtras::default()
    };
    let out = with_extras(xml, &extras);
    assert!(
        out.contains(r#"<worksheet><sheetViews><sheetView workbookViewId="0" showRowColHeaders="0"/></sheetViews><sheetFormatPr"#),
        "{out}"
    );
}
