//! `sheetProtection` and `workbookProtection`, read and written as Excel 2016 writes them.

use cgs_model::{PasswordHash, SheetProtection, WorkbookProtection};
use quick_xml::events::BytesStart;

use crate::xml::{attr, attr_bool, attr_u32};

/// A password as the file holds it, from its attributes' names.
fn read_password(
    element: &BytesStart<'_>,
    [algorithm, hash, salt, spin, legacy]: [&[u8]; 5],
) -> Option<PasswordHash> {
    if let (Some(algorithm), Some(hash)) = (attr(element, algorithm), attr(element, hash)) {
        return Some(PasswordHash::Hashed {
            algorithm,
            hash,
            salt: attr(element, salt).unwrap_or_default(),
            spin: attr_u32(element, spin).unwrap_or(0),
        });
    }
    attr(element, legacy)
        .and_then(|text| u16::from_str_radix(text.trim(), 16).ok())
        .filter(|&hash| hash != 0)
        .map(PasswordHash::Legacy)
}

pub fn read_sheet_protection(element: &BytesStart<'_>) -> SheetProtection {
    // The file says what is protected; `allowed` turns that round.
    let protected = |name: &[u8], default: bool| attr_bool(element, name).unwrap_or(default);
    let allowed = |name: &[u8]| !protected(name, true);
    SheetProtection {
        sheet: protected(b"sheet", false),
        objects: protected(b"objects", false),
        scenarios: protected(b"scenarios", false),
        format_cells: allowed(b"formatCells"),
        format_columns: allowed(b"formatColumns"),
        format_rows: allowed(b"formatRows"),
        insert_columns: allowed(b"insertColumns"),
        insert_rows: allowed(b"insertRows"),
        insert_hyperlinks: allowed(b"insertHyperlinks"),
        delete_columns: allowed(b"deleteColumns"),
        delete_rows: allowed(b"deleteRows"),
        select_locked: !protected(b"selectLockedCells", false),
        select_unlocked: !protected(b"selectUnlockedCells", false),
        sort: allowed(b"sort"),
        auto_filter: allowed(b"autoFilter"),
        pivot_tables: allowed(b"pivotTables"),
        password: read_password(
            element,
            [
                b"algorithmName",
                b"hashValue",
                b"saltValue",
                b"spinCount",
                b"password",
            ],
        ),
    }
}

pub fn read_workbook_protection(element: &BytesStart<'_>) -> WorkbookProtection {
    WorkbookProtection {
        structure: attr_bool(element, b"lockStructure").unwrap_or(false),
        windows: attr_bool(element, b"lockWindows").unwrap_or(false),
        password: read_password(
            element,
            [
                b"workbookAlgorithmName",
                b"workbookHashValue",
                b"workbookSaltValue",
                b"workbookSpinCount",
                b"workbookPassword",
            ],
        ),
    }
}

fn push_password(out: &mut String, password: &Option<PasswordHash>, prefix: &str) {
    match password {
        Some(PasswordHash::Hashed {
            algorithm,
            hash,
            salt,
            spin,
        }) => {
            let lead = |name: &str| {
                if prefix.is_empty() {
                    let mut chars = name.chars();
                    chars
                        .next()
                        .map(|first| first.to_ascii_lowercase().to_string() + chars.as_str())
                        .unwrap_or_default()
                } else {
                    format!("{prefix}{name}")
                }
            };
            out.push_str(&format!(
                " {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{spin}\"",
                lead("AlgorithmName"),
                crate::scan::escape_attr(algorithm),
                lead("HashValue"),
                crate::scan::escape_attr(hash),
                lead("SaltValue"),
                crate::scan::escape_attr(salt),
                lead("SpinCount"),
            ));
        }
        Some(PasswordHash::Legacy(hash)) => {
            let name = if prefix.is_empty() {
                "password".to_string()
            } else {
                format!("{prefix}Password")
            };
            out.push_str(&format!(" {name}=\"{hash:04X}\""));
        }
        None => {}
    }
}

/// `<sheetProtection>` as Excel writes it: only what differs from the schema's defaults.
pub fn sheet_protection_xml(protection: &SheetProtection) -> String {
    let mut out = String::from("<sheetProtection");
    push_password(&mut out, &protection.password, "");
    let mut flag = |name: &str, on: bool, value: &str| {
        if on {
            out.push_str(&format!(" {name}=\"{value}\""));
        }
    };
    flag("sheet", protection.sheet, "1");
    flag("objects", protection.objects, "1");
    flag("scenarios", protection.scenarios, "1");
    flag("formatCells", protection.format_cells, "0");
    flag("formatColumns", protection.format_columns, "0");
    flag("formatRows", protection.format_rows, "0");
    flag("insertColumns", protection.insert_columns, "0");
    flag("insertRows", protection.insert_rows, "0");
    flag("insertHyperlinks", protection.insert_hyperlinks, "0");
    flag("deleteColumns", protection.delete_columns, "0");
    flag("deleteRows", protection.delete_rows, "0");
    flag("selectLockedCells", !protection.select_locked, "1");
    flag("sort", protection.sort, "0");
    flag("autoFilter", protection.auto_filter, "0");
    flag("pivotTables", protection.pivot_tables, "0");
    flag("selectUnlockedCells", !protection.select_unlocked, "1");
    out.push_str("/>");
    out
}

/// `<workbookProtection>` as Excel writes it.
pub fn workbook_protection_xml(protection: &WorkbookProtection, prefix: &str) -> String {
    let mut out = format!("<{prefix}workbookProtection");
    push_password(&mut out, &protection.password, "workbook");
    if protection.structure {
        out.push_str(" lockStructure=\"1\"");
    }
    if protection.windows {
        out.push_str(" lockWindows=\"1\"");
    }
    out.push_str("/>");
    out
}

/// `workbook.xml` with its `workbookProtection` made this, or taken out, where the schema wants it:
/// after `workbookPr` and what Excel writes beside it, before `bookViews`.
pub fn set_workbook_protection(
    xml: &[u8],
    protection: Option<&WorkbookProtection>,
) -> std::io::Result<Vec<u8>> {
    use crate::structure::{parse_tree, write_tree, Element, Node};
    let mut tree = parse_tree(xml)?;
    for node in &mut tree {
        let Node::Element(root) = node else { continue };
        let prefix = crate::scan::prefix(&root.tag).to_string();
        root.children.retain(|child| {
            !matches!(child, Node::Element(element) if element.local() == b"workbookProtection")
        });
        let Some(protection) = protection else {
            continue;
        };
        let element = Node::Element(Element {
            tag: workbook_protection_xml(protection, &prefix).into_bytes(),
            empty: true,
            children: Vec::new(),
            end: Vec::new(),
        });
        const AFTER: &[&[u8]] = &[
            b"bookViews",
            b"sheets",
            b"functionGroups",
            b"externalReferences",
            b"definedNames",
            b"calcPr",
        ];
        let at = root
            .children
            .iter()
            .position(
                |child| matches!(child, Node::Element(element) if AFTER.contains(&element.local())),
            )
            .unwrap_or(root.children.len());
        root.children.insert(at, element);
    }
    let mut out = Vec::with_capacity(xml.len() + 256);
    write_tree(&tree, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(xml: &str) -> BytesStart<'static> {
        let mut reader = quick_xml::Reader::from_str(xml);
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(element) => element.into_owned(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_and_writes_what_excel_2016_wrote() {
        // From Excel 2016 over COM: Protect with "abc", then with every option allowed, then
        // with nothing selectable.
        let excel = [
            r#"<sheetProtection algorithmName="SHA-512" hashValue="lE+oY/K8O9RzxhHbfj+59f6OKjW/HyUpDp1tBw9LKbUXQ2NlFMdltyninyJ7dov5g+VPTaGpfgt/Z1zYKcAFyQ==" saltValue="mZgorKXCxX0pnb7n04x0bA==" spinCount="100000" sheet="1" objects="1" scenarios="1"/>"#,
            r#"<sheetProtection sheet="1" objects="1" scenarios="1" formatCells="0" formatColumns="0" formatRows="0" insertColumns="0" insertRows="0" insertHyperlinks="0" deleteColumns="0" deleteRows="0" selectLockedCells="1" sort="0" autoFilter="0" pivotTables="0"/>"#,
            r#"<sheetProtection sheet="1" objects="1" scenarios="1" selectLockedCells="1" selectUnlockedCells="1"/>"#,
            r#"<sheetProtection objects="1" scenarios="1"/>"#,
        ];
        for xml in excel {
            let read = read_sheet_protection(&element(xml));
            assert_eq!(sheet_protection_xml(&read), xml);
        }
        let first = read_sheet_protection(&element(excel[0]));
        assert!(first.sheet && first.select_locked && !first.format_cells);
        let second = read_sheet_protection(&element(excel[1]));
        assert!(second.format_cells && second.pivot_tables && !second.select_locked);
        assert!(!read_sheet_protection(&element(excel[3])).sheet);
        assert_eq!(
            read_sheet_protection(&element(r#"<sheetProtection password="CBEB" sheet="1"/>"#))
                .password,
            Some(PasswordHash::Legacy(0xCBEB))
        );
    }

    #[test]
    fn the_workbooks_goes_before_its_views() {
        let excel = r#"<workbookProtection workbookAlgorithmName="SHA-512" workbookHashValue="SHy4/btZfFwmRsjQc5i0Tt0EsAatkk5D41w/SBmvMDUwoOD1Pzy3kRvtIUMUbrwoZyeuVxX0qBjbFgpke2OOdg==" workbookSaltValue="C8wlzqnyyPxIKcG6QlN/HQ==" workbookSpinCount="100000" lockStructure="1"/>"#;
        let read = read_workbook_protection(&element(excel));
        assert!(read.structure && !read.windows);
        assert_eq!(workbook_protection_xml(&read, ""), excel);
        let book = br#"<workbook xmlns="m"><workbookPr/><bookViews><workbookView/></bookViews><sheets/></workbook>"#;
        let protected = set_workbook_protection(book, Some(&read)).unwrap();
        let text = String::from_utf8(protected.clone()).unwrap();
        assert_eq!(
            text,
            format!(
                r#"<workbook xmlns="m"><workbookPr/>{excel}<bookViews><workbookView/></bookViews><sheets/></workbook>"#
            )
        );
        let back = set_workbook_protection(&protected, None).unwrap();
        assert_eq!(back, book.to_vec());
    }
}
