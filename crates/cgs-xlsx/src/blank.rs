//! A new blank workbook: the smallest package Excel opens with no complaint and that can then be
//! saved like any other, because it is one. Its parts are ones Excel itself would write for an
//! empty book, down to the default font, so nothing is special about a new file.

use std::io;

use crate::zipdir::write_new;

const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn with_declaration(body: String) -> Vec<u8> {
    format!("{DECLARATION}{body}").into_bytes()
}

/// The bytes of a workbook with one empty sheet.
pub fn blank_workbook() -> io::Result<Vec<u8>> {
    let content_types = with_declaration("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>".to_string());
    let package_rels = with_declaration(format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{RELS}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    ));
    let workbook = with_declaration(format!(
        "<workbook xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\"><bookViews><workbookView/></bookViews><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets><calcPr calcId=\"191029\"/></workbook>"
    ));
    let workbook_rels = with_declaration(format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{RELS}/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"{RELS}/styles\" Target=\"styles.xml\"/></Relationships>"
    ));
    let sheet = with_declaration(format!(
        "<worksheet xmlns=\"{MAIN}\"><dimension ref=\"A1\"/><sheetViews><sheetView tabSelected=\"1\" workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><sheetData/></worksheet>"
    ));
    let styles = minimal_styles();
    write_new(&[
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", &package_rels),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &workbook_rels),
        ("xl/worksheets/sheet1.xml", &sheet),
        ("xl/styles.xml", &styles),
    ])
}

/// The styles of a workbook that has only the default ones.
pub fn minimal_styles() -> Vec<u8> {
    with_declaration(format!(
        "<styleSheet xmlns=\"{MAIN}\"><fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/><family val=\"2\"/><scheme val=\"minor\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles><dxfs count=\"0\"/><tableStyles count=\"0\" defaultTableStyle=\"TableStyleMedium2\" defaultPivotStyle=\"PivotStyleLight16\"/></styleSheet>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package;
    use std::rc::Rc;

    #[test]
    fn a_blank_workbook_opens_as_one_empty_sheet() {
        let bytes = blank_workbook().unwrap();
        let package = Package::open(Rc::new(bytes)).unwrap();
        let book = crate::workbook::read_workbook(&package).unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.sheets[0].name, "Sheet1");
        assert_eq!(book.styles.xfs.len(), 1);
    }
}
