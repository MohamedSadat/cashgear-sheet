//! Table parts: a new table's part as Excel writes one, and an existing part brought into step with
//! the model, changing only what the model holds so that everything else in it stays as it was.

use std::io;

use cgs_model::{Table, TableColumnInfo};

use crate::filter::{auto_filter_xml, read_auto_filter, rect_ref};
use crate::patch_parts::table_column_attr;
use crate::scan::{escape_attr, prefix};
use crate::structure::{parse_tree, write_tree, Element, Node};

pub const TABLE_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";
pub const TABLE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// `totalsRowShown`, which says whether the row was ever shown: Excel leaves it out once it has.
fn totals_attrs(table: &Table) -> (Option<String>, Option<&'static str>) {
    if table.totals_rows > 0 {
        return (Some(table.totals_rows.to_string()), None);
    }
    let remembered = table
        .column_info
        .iter()
        .any(|info| info.totals_function.is_some() || info.totals_label.is_some());
    (None, (!remembered).then_some("0"))
}

fn filter_xml(table: &Table) -> Option<String> {
    let filter = table.filter.as_ref().filter(|_| table.header_rows > 0)?;
    Some(auto_filter_xml(filter))
}

fn style_xml(table: &Table) -> String {
    let flag = |on: bool| if on { "1" } else { "0" };
    let style = &table.style;
    let name = style
        .name
        .as_ref()
        .map(|name| format!(" name=\"{}\"", escape_attr(name)))
        .unwrap_or_default();
    format!(
        "<tableStyleInfo{name} showFirstColumn=\"{}\" showLastColumn=\"{}\" showRowStripes=\"{}\" showColumnStripes=\"{}\"/>",
        flag(style.first_column),
        flag(style.last_column),
        flag(style.row_stripes),
        flag(style.column_stripes)
    )
}

fn column_xml(id: u32, name: &str, info: &TableColumnInfo) -> String {
    let mut out = format!(
        "<tableColumn id=\"{id}\" name=\"{}\"",
        table_column_attr(name)
    );
    if let Some(label) = &info.totals_label {
        out.push_str(&format!(" totalsRowLabel=\"{}\"", escape_attr(label)));
    }
    if let Some(function) = info.totals_function {
        out.push_str(&format!(" totalsRowFunction=\"{}\"", function.xml()));
    }
    let mut inner = String::new();
    if let Some(formula) = &info.calculated {
        inner.push_str(&format!(
            "<calculatedColumnFormula>{}</calculatedColumnFormula>",
            escape_text(formula)
        ));
    }
    if let Some(formula) = &info.totals_formula {
        inner.push_str(&format!(
            "<totalsRowFormula>{}</totalsRowFormula>",
            escape_text(formula)
        ));
    }
    if inner.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&inner);
        out.push_str("</tableColumn>");
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A new table's whole part.
pub fn new_table_part(table: &Table) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    let name = escape_attr(&table.name);
    out.push_str(&format!(
        "<table xmlns=\"{MAIN}\" id=\"{}\" name=\"{name}\" displayName=\"{name}\" ref=\"{}\"",
        table.id,
        rect_ref(&table.range)
    ));
    if table.header_rows == 0 {
        out.push_str(" headerRowCount=\"0\"");
    }
    let (count, shown) = totals_attrs(table);
    if let Some(count) = count {
        out.push_str(&format!(" totalsRowCount=\"{count}\""));
    }
    if let Some(shown) = shown {
        out.push_str(&format!(" totalsRowShown=\"{shown}\""));
    }
    out.push('>');
    if let Some(filter) = filter_xml(table) {
        out.push_str(&filter);
    }
    out.push_str(&format!("<tableColumns count=\"{}\">", table.columns.len()));
    for (index, name) in table.columns.iter().enumerate() {
        let info = table.column_info.get(index).cloned().unwrap_or_default();
        out.push_str(&column_xml(index as u32 + 1, name, &info));
    }
    out.push_str("</tableColumns>");
    out.push_str(&style_xml(table));
    out.push_str("</table>");
    out
}

/// A child element made from XML that has no prefix, in a part whose elements may have one.
fn child(xml: &str, prefixed: bool) -> io::Result<Node> {
    let xml = if prefixed {
        let end = xml
            .bytes()
            .position(|byte| byte.is_ascii_whitespace() || byte == b'>' || byte == b'/')
            .unwrap_or(xml.len());
        format!("{} xmlns=\"{MAIN}\"{}", &xml[..end], &xml[end..])
    } else {
        xml.to_string()
    };
    let mut nodes = parse_tree(xml.as_bytes())?;
    nodes
        .pop()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no element"))
}

/// Gives an empty element room for children.
fn open(element: &mut Element) {
    if !element.empty {
        return;
    }
    let name = String::from_utf8_lossy(&element.tag)
        .trim_start_matches('<')
        .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>')
        .next()
        .unwrap_or_default()
        .to_string();
    let mut tag = element.tag.clone();
    if tag.ends_with(b"/>") {
        tag.truncate(tag.len() - 2);
        while tag.last().is_some_and(u8::is_ascii_whitespace) {
            tag.pop();
        }
        tag.push(b'>');
    }
    element.tag = tag;
    element.end = format!("</{name}>").into_bytes();
    element.empty = false;
}

fn position_of(children: &[Node], local: &[u8]) -> Option<usize> {
    children
        .iter()
        .position(|node| matches!(node, Node::Element(element) if element.local() == local))
}

/// Where a new child of `<table>` goes, before the first of these that's there.
fn insert_before(children: &[Node], after: &[&[u8]]) -> usize {
    after
        .iter()
        .find_map(|local| position_of(children, local))
        .unwrap_or(children.len())
}

fn set_child_text(
    column: &mut Element,
    local: &[u8],
    text: Option<&str>,
    prefixed: bool,
) -> io::Result<()> {
    match (position_of(&column.children, local), text) {
        (Some(at), None) => {
            column.children.remove(at);
        }
        (Some(at), Some(text)) => {
            if let Node::Element(element) = &mut column.children[at] {
                if element.text() != text {
                    open(element);
                    element.set_text(text);
                }
            }
        }
        (None, Some(text)) => {
            open(column);
            let name = String::from_utf8_lossy(local).into_owned();
            let node = child(&format!("<{name}>{}</{name}>", escape_text(text)), prefixed)?;
            // A calculated formula comes before a totals one, and both before anything else.
            let at = if local == b"calculatedColumnFormula" {
                0
            } else {
                insert_before(&column.children, &[b"xmlColumnPr", b"extLst"])
            };
            column.children.insert(at.min(column.children.len()), node);
        }
        (None, None) => {}
    }
    Ok(())
}

fn edit_column(
    column: &mut Element,
    name: &str,
    info: &TableColumnInfo,
    prefixed: bool,
) -> io::Result<()> {
    let wanted = table_column_attr(name);
    if column.attr(b"name").map(|raw| raw.to_vec()) != Some(wanted.clone().into_bytes()) {
        column.set_attr("name", Some(&wanted));
    }
    let label = info.totals_label.as_deref().map(escape_attr);
    if column.attr_text(b"totalsRowLabel").as_deref() != info.totals_label.as_deref() {
        column.set_attr("totalsRowLabel", label.as_deref());
    }
    let function = info.totals_function.map(|function| function.xml());
    if column.attr_text(b"totalsRowFunction").as_deref() != function {
        column.set_attr("totalsRowFunction", function);
    }
    let calculated = column
        .elements()
        .find(|element| element.local() == b"calculatedColumnFormula")
        .map(Element::text);
    if calculated.as_deref() != info.calculated.as_deref() {
        set_child_text(
            column,
            b"calculatedColumnFormula",
            info.calculated.as_deref(),
            prefixed,
        )?;
    }
    let totals = column
        .elements()
        .find(|element| element.local() == b"totalsRowFormula")
        .map(Element::text);
    if totals.as_deref() != info.totals_formula.as_deref() {
        set_child_text(
            column,
            b"totalsRowFormula",
            info.totals_formula.as_deref(),
            prefixed,
        )?;
    }
    Ok(())
}

/// An existing table part with what the model says about the table: its name, area, heading and
/// totals rows, filter, columns and style. Nothing else in it changes.
pub fn patch_table_part(xml: &[u8], table: &Table) -> io::Result<Vec<u8>> {
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else {
            continue;
        };
        if root.local() != b"table" {
            continue;
        }
        let prefixed = !prefix(&root.tag).is_empty();
        if root.attr_text(b"displayName").as_deref() != Some(table.name.as_str()) {
            let name = escape_attr(&table.name);
            root.set_attr("displayName", Some(&name));
            root.set_attr("name", Some(&name));
        }
        let reference = rect_ref(&table.range);
        if root.attr_text(b"ref").as_deref() != Some(reference.as_str()) {
            root.set_attr("ref", Some(&reference));
        }
        let header = (table.header_rows == 0).then_some("0");
        if root.attr_text(b"headerRowCount").as_deref() != header {
            root.set_attr("headerRowCount", header);
        }
        let (count, shown) = totals_attrs(table);
        if root.attr_text(b"totalsRowCount") != count {
            root.set_attr("totalsRowCount", count.as_deref());
        }
        if root.attr_text(b"totalsRowShown").as_deref() != shown {
            root.set_attr("totalsRowShown", shown);
        }
        open(root);
        // The filter, only if it says something different, so that what's in it stays.
        let wanted = table.filter.as_ref().filter(|_| table.header_rows > 0);
        match (position_of(&root.children, b"autoFilter"), wanted) {
            (Some(at), None) => {
                root.children.remove(at);
            }
            (Some(at), Some(filter)) => {
                let mut existing = Vec::new();
                write_tree(std::slice::from_ref(&root.children[at]), &mut existing);
                if read_auto_filter(&existing).as_ref() != Some(filter) {
                    root.children[at] = child(&auto_filter_xml(filter), prefixed)?;
                }
            }
            (None, Some(filter)) => {
                let node = child(&auto_filter_xml(filter), prefixed)?;
                let at = insert_before(
                    &root.children,
                    &[b"sortState", b"tableColumns", b"tableStyleInfo", b"extLst"],
                );
                root.children.insert(at, node);
            }
            (None, None) => {}
        }
        // Columns: by position while the count is the same, and by name once it isn't.
        if let Some(at) = position_of(&root.children, b"tableColumns") {
            if let Node::Element(columns) = &mut root.children[at] {
                let count = columns.elements().count();
                if count == table.columns.len() {
                    for (index, column) in columns.elements_mut().enumerate() {
                        let info = table.column_info.get(index).cloned().unwrap_or_default();
                        edit_column(column, &table.columns[index], &info, prefixed)?;
                    }
                } else {
                    let mut old: Vec<Element> = std::mem::take(&mut columns.children)
                        .into_iter()
                        .filter_map(|node| match node {
                            Node::Element(element) => Some(element),
                            Node::Text(_) => None,
                        })
                        .collect();
                    let mut next_id = old
                        .iter()
                        .filter_map(|element| element.attr_text(b"id"))
                        .filter_map(|id| id.parse::<u32>().ok())
                        .max()
                        .unwrap_or(0);
                    for (index, name) in table.columns.iter().enumerate() {
                        let info = table.column_info.get(index).cloned().unwrap_or_default();
                        let found = old.iter().position(|element| {
                            element.attr_text(b"name").is_some_and(|text| {
                                crate::xml::unescape_x(&text).eq_ignore_ascii_case(name)
                            })
                        });
                        let node = match found {
                            Some(found) => {
                                let mut element = old.remove(found);
                                edit_column(&mut element, name, &info, prefixed)?;
                                Node::Element(element)
                            }
                            None => {
                                next_id += 1;
                                child(&column_xml(next_id, name, &info), prefixed)?
                            }
                        };
                        columns.children.push(node);
                    }
                    let text = table.columns.len().to_string();
                    columns.set_attr("count", Some(&text));
                }
            }
        }
        // The style.
        let style = child(&style_xml(table), prefixed)?;
        match position_of(&root.children, b"tableStyleInfo") {
            Some(at) => {
                let same = match (&root.children[at], &style) {
                    (Node::Element(a), Node::Element(b)) => {
                        let read = |element: &Element| {
                            [
                                element.attr_text(b"name"),
                                element.attr_text(b"showFirstColumn"),
                                element.attr_text(b"showLastColumn"),
                                element.attr_text(b"showRowStripes"),
                                element.attr_text(b"showColumnStripes"),
                            ]
                        };
                        let normal = |values: [Option<String>; 5]| {
                            values.map(|value| match value.as_deref() {
                                Some("true") => Some("1".to_string()),
                                Some("false") | None => Some("0".to_string()),
                                _ => value,
                            })
                        };
                        let (mut first, mut second) = (normal(read(a)), normal(read(b)));
                        // A name that isn't there is no style, not "0".
                        first[0] = a.attr_text(b"name");
                        second[0] = b.attr_text(b"name");
                        first == second
                    }
                    _ => false,
                };
                if !same {
                    root.children[at] = style;
                }
            }
            None => {
                let at = insert_before(&root.children, &[b"extLst"]);
                root.children.insert(at, style);
            }
        }
    }
    let mut out = Vec::with_capacity(xml.len() + 64);
    write_tree(&nodes, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgs_model::{AutoFilter, Rect, TotalsFunction};

    fn table() -> Table {
        let range = Rect {
            top: 1,
            left: 8,
            bottom: 6,
            right: 10,
        };
        let mut table = Table::new(
            "Sales".to_string(),
            0,
            range,
            vec!["Item".to_string(), "Cost".to_string(), "Qty".to_string()],
        );
        table.id = 3;
        table.filter = Some(AutoFilter::new(range));
        table
    }

    #[test]
    fn a_new_part_is_what_excel_writes() {
        let xml = new_table_part(&table());
        assert!(xml.contains(r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="3" name="Sales" displayName="Sales" ref="H1:J6" totalsRowShown="0"><autoFilter ref="H1:J6"/><tableColumns count="3"><tableColumn id="1" name="Item"/>"#), "{xml}");
        assert!(xml.ends_with(r#"<tableStyleInfo name="TableStyleMedium2" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>"#), "{xml}");
    }

    #[test]
    fn patching_changes_only_what_the_model_holds() {
        let source = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:xr="x" id="3" name="Sales" displayName="Sales" ref="H1:J6" totalsRowShown="0" xr:uid="{A}"><autoFilter ref="H1:J6" xr:uid="{B}"/><tableColumns count="3"><tableColumn id="1" name="Item" xr:uid="{C}"/><tableColumn id="2" name="Cost" dataDxfId="4"/><tableColumn id="3" name="Qty"/></tableColumns><tableStyleInfo name="TableStyleMedium2" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>"#;
        let same = patch_table_part(source.as_bytes(), &table()).unwrap();
        assert_eq!(String::from_utf8(same).unwrap(), source);
        let mut changed = table();
        changed.totals_rows = 1;
        changed.range.bottom = 7;
        changed.column_info[0].totals_label = Some("Total".to_string());
        changed.column_info[2].totals_function = Some(TotalsFunction::Sum);
        changed.column_info[1].calculated = Some("Sales[[#This Row],[Qty]]*2".to_string());
        changed.style.column_stripes = true;
        let out =
            String::from_utf8(patch_table_part(source.as_bytes(), &changed).unwrap()).unwrap();
        assert!(out.contains(r#"ref="H1:J7""#), "{out}");
        assert!(
            out.contains(r#"totalsRowCount="1""#) && !out.contains("totalsRowShown"),
            "{out}"
        );
        assert!(
            out.contains(r#"<autoFilter ref="H1:J6" xr:uid="{B}"/>"#),
            "{out}"
        );
        assert!(
            out.contains(
                r#"<tableColumn id="1" name="Item" xr:uid="{C}" totalsRowLabel="Total"/>"#
            ),
            "{out}"
        );
        assert!(out.contains(r#"<tableColumn id="2" name="Cost" dataDxfId="4"><calculatedColumnFormula>Sales[[#This Row],[Qty]]*2</calculatedColumnFormula></tableColumn>"#), "{out}");
        assert!(out.contains(r#"showColumnStripes="1""#), "{out}");
    }

    #[test]
    fn a_resized_table_keeps_its_columns_by_name() {
        let source = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="3" name="Sales" displayName="Sales" ref="H1:J6"><tableColumns count="3"><tableColumn id="1" name="Item"/><tableColumn id="2" name="Cost" dataDxfId="4"/><tableColumn id="5" name="Qty"/></tableColumns></table>"#;
        let mut changed = table();
        changed.filter = None;
        changed.range.right = 9;
        changed.columns = vec!["Cost".to_string(), "New".to_string()];
        changed.column_info = vec![TableColumnInfo::default(); 2];
        let out =
            String::from_utf8(patch_table_part(source.as_bytes(), &changed).unwrap()).unwrap();
        assert!(out.contains(r#"<tableColumns count="2"><tableColumn id="2" name="Cost" dataDxfId="4"/><tableColumn id="6" name="New"/></tableColumns>"#), "{out}");
        assert!(out.contains("<tableStyleInfo"), "{out}");
    }
}
