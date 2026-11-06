//! Pivot parts written from the model: an existing part patched so that only what the model holds
//! changes, and new parts for a pivot table made here. A cache's records part is never rewritten:
//! the definition asks Excel to refresh when it opens the file instead.

use std::fmt::Write as _;
use std::io;

use cgs_model::pivot::{
    AxisLine, CacheField, CacheSource, CacheValue, DataField, FieldSort, GroupRange, ItemType,
    PivotAxis, PivotCache, PivotField, PivotTable, ShowAs, VALUES_FIELD,
};

use crate::pivot::serial_iso;
use crate::scan::{escape_attr, prefix};
use crate::structure::{parse_tree, write_tree, Element, Node};

pub const TABLE_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
pub const CACHE_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
pub const RECORDS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml";
pub const TABLE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable";
pub const CACHE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition";
pub const RECORDS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";

/// The order of a pivot table definition's children.
const TABLE_ORDER: [&[u8]; 17] = [
    b"location",
    b"pivotFields",
    b"rowFields",
    b"rowItems",
    b"colFields",
    b"colItems",
    b"pageFields",
    b"dataFields",
    b"formats",
    b"conditionalFormats",
    b"chartFormats",
    b"pivotHierarchies",
    b"pivotTableStyleInfo",
    b"filters",
    b"rowHierarchiesUsage",
    b"colHierarchiesUsage",
    b"extLst",
];

const FIELD_ORDER: [&[u8]; 3] = [b"items", b"autoSortScope", b"extLst"];

fn flag(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let text = format!("{n}");
        text
    }
}

fn cell_ref(row: u32, col: u32) -> String {
    let mut c = col;
    let mut letters = Vec::new();
    while c > 0 {
        letters.push(b'A' + ((c - 1) % 26) as u8);
        c = (c - 1) / 26;
    }
    letters.reverse();
    format!("{}{row}", String::from_utf8(letters).unwrap_or_default())
}

fn rect_ref(r: &cgs_model::Rect) -> String {
    format!(
        "{}:{}",
        cell_ref(r.top, r.left),
        cell_ref(r.bottom, r.right)
    )
}

/// Parses elements written without a prefix, giving them the part's own prefix.
fn elements(xml: &str, prefix: &str) -> Vec<Node> {
    let text = if prefix.is_empty() {
        xml.to_string()
    } else {
        // Only our own elements are in `xml`, so a plain rewrite of their tags is enough.
        let mut out = String::with_capacity(xml.len() + 64);
        let bytes = xml.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
                out.push('<');
                out.push_str(prefix);
                i += 1;
            } else if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                out.push_str("</");
                out.push_str(prefix);
                i += 2;
            } else {
                let ch = xml[i..].chars().next().unwrap_or(' ');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    };
    parse_tree(text.as_bytes()).unwrap_or_default()
}

/// Puts `nodes` where `name` goes among `parent`'s children, in place of any it had.
fn place(parent: &mut Element, name: &[u8], nodes: Vec<Node>, order: &[&[u8]]) {
    parent
        .children
        .retain(|c| !matches!(c, Node::Element(e) if e.local() == name));
    if nodes.is_empty() {
        return;
    }
    let rank = order.iter().position(|n| *n == name).unwrap_or(order.len());
    let at = parent
        .children
        .iter()
        .position(|c| match c {
            Node::Element(e) => order
                .iter()
                .position(|n| *n == e.local())
                .is_some_and(|r| r > rank),
            Node::Text(_) => false,
        })
        .unwrap_or(parent.children.len());
    if parent.empty {
        parent.empty = false;
        let local = String::from_utf8_lossy(crate::scan::local_name(&parent.tag)).into_owned();
        let pre = prefix(&parent.tag);
        // `<x/>` becomes `<x>`.
        let mut tag = parent.tag.clone();
        if tag.ends_with(b"/>") {
            tag.truncate(tag.len() - 2);
            while tag.last() == Some(&b' ') {
                tag.pop();
            }
            tag.push(b'>');
        }
        parent.tag = tag;
        parent.end = format!("</{pre}{local}>").into_bytes();
    }
    for (offset, node) in nodes.into_iter().enumerate() {
        parent
            .children
            .insert(at.min(parent.children.len()) + offset, node);
    }
}

fn lines_xml(name: &str, lines: &[AxisLine]) -> String {
    let mut out = format!("<{name} count=\"{}\">", lines.len().max(1));
    if lines.is_empty() {
        out.push_str("<i/>");
    }
    for line in lines {
        out.push_str("<i");
        if line.kind != ItemType::Data {
            let _ = write!(out, " t=\"{}\"", line.kind.xml());
        }
        if line.repeat > 0 {
            let _ = write!(out, " r=\"{}\"", line.repeat);
        }
        if line.data > 0 {
            let _ = write!(out, " i=\"{}\"", line.data);
        }
        if line.members.is_empty() {
            out.push_str("/>");
            continue;
        }
        out.push('>');
        for &m in &line.members {
            if m == 0 {
                out.push_str("<x/>");
            } else {
                let _ = write!(out, "<x v=\"{m}\"/>");
            }
        }
        out.push_str("</i>");
    }
    let _ = write!(out, "</{name}>");
    out
}

fn axis_xml(axis: Option<PivotAxis>) -> Option<&'static str> {
    axis.map(|a| match a {
        PivotAxis::Row => "axisRow",
        PivotAxis::Col => "axisCol",
        PivotAxis::Page => "axisPage",
        PivotAxis::Values => "axisValues",
    })
}

fn items_xml(field: &PivotField) -> String {
    if field.items.is_empty() {
        return String::new();
    }
    let mut out = format!("<items count=\"{}\">", field.items.len());
    for item in &field.items {
        out.push_str("<item");
        if let Some(name) = &item.name {
            let _ = write!(out, " n=\"{}\"", escape_attr(name));
        }
        if item.kind != ItemType::Data {
            let _ = write!(out, " t=\"{}\"", item.kind.xml());
        }
        if item.hidden {
            out.push_str(" h=\"1\"");
        }
        if !item.show_details {
            out.push_str(" sd=\"0\"");
        }
        if item.missing {
            out.push_str(" m=\"1\"");
        }
        if let Some(x) = item.x {
            let _ = write!(out, " x=\"{x}\"");
        }
        out.push_str("/>");
    }
    out.push_str("</items>");
    out
}

fn sort_scope_xml(data: u32) -> String {
    format!(
        "<autoSortScope><pivotArea dataOnly=\"0\" outline=\"0\" fieldPosition=\"0\"><references count=\"1\"><reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"{data}\"/></reference></references></pivotArea></autoSortScope>"
    )
}

/// The attributes a pivot field's tag gets from the model, as (name, value or nothing).
fn field_attrs(field: &PivotField) -> Vec<(&'static str, Option<String>)> {
    let mut attrs: Vec<(&'static str, Option<String>)> = vec![
        ("name", field.name.as_ref().map(|n| escape_attr(n))),
        ("axis", axis_xml(field.axis).map(str::to_string)),
        ("dataField", field.data_field.then(|| "1".to_string())),
        ("compact", (!field.compact).then(|| "0".to_string())),
        ("outline", (!field.outline).then(|| "0".to_string())),
        (
            "subtotalTop",
            (!field.subtotal_top).then(|| "0".to_string()),
        ),
        ("showAll", Some(flag(field.show_all).to_string())),
        (
            "sortType",
            match field.sort {
                FieldSort::Manual => None,
                FieldSort::Ascending => Some("ascending".to_string()),
                FieldSort::Descending => Some("descending".to_string()),
            },
        ),
        (
            "defaultSubtotal",
            (!field.default_subtotal).then(|| "0".to_string()),
        ),
        (
            "insertBlankRow",
            field.insert_blank_row.then(|| "1".to_string()),
        ),
        (
            "multipleItemSelectionAllowed",
            field.multiple_item_selection.then(|| "1".to_string()),
        ),
        (
            "includeNewItemsInFilter",
            field.include_new_items.then(|| "1".to_string()),
        ),
        (
            "subtotalCaption",
            field.subtotal_caption.as_ref().map(|c| escape_attr(c)),
        ),
    ];
    for (name, kind) in [
        ("sumSubtotal", ItemType::Sum),
        ("countASubtotal", ItemType::CountA),
        ("avgSubtotal", ItemType::Avg),
        ("maxSubtotal", ItemType::Max),
        ("minSubtotal", ItemType::Min),
        ("productSubtotal", ItemType::Product),
        ("countSubtotal", ItemType::Count),
        ("stdDevSubtotal", ItemType::StdDev),
        ("stdDevPSubtotal", ItemType::StdDevP),
        ("varSubtotal", ItemType::Var),
        ("varPSubtotal", ItemType::VarP),
    ] {
        attrs.push((
            name,
            field.subtotals.contains(&kind).then(|| "1".to_string()),
        ));
    }
    attrs
}

fn fill_down_ext(on: bool) -> String {
    format!(
        "<extLst><ext uri=\"{{2946ED86-A175-432a-8AC1-64E0C546D7DE}}\" xmlns:x14=\"{X14}\"><x14:pivotField fillDownLabels=\"{}\"/></ext></extLst>",
        flag(on)
    )
}

/// Sets or adds `fillDownLabels` on a field's Excel 2010 extension.
fn set_fill_down(element: &mut Element, on: bool, pre: &str) {
    let found = element
        .elements_mut()
        .filter(|e| e.local() == b"extLst")
        .flat_map(|list| list.elements_mut())
        .flat_map(|ext| ext.elements_mut())
        .find(|e| e.local() == b"pivotField");
    match found {
        Some(e) => e.set_attr("fillDownLabels", Some(flag(on))),
        None if on => {
            let nodes = elements(&fill_down_ext(true), pre);
            place(element, b"extLst", nodes, &FIELD_ORDER);
        }
        None => {}
    }
}

fn field_xml(field: &PivotField) -> String {
    let mut out = String::from("<pivotField");
    for (name, value) in field_attrs(field) {
        if let Some(value) = value {
            let _ = write!(out, " {name}=\"{value}\"");
        }
    }
    let items = items_xml(field);
    let scope = field.sort_by_data.map(sort_scope_xml).unwrap_or_default();
    let ext = if field.repeat_labels {
        fill_down_ext(true)
    } else {
        String::new()
    };
    if items.is_empty() && scope.is_empty() && ext.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&items);
        out.push_str(&scope);
        out.push_str(&ext);
        out.push_str("</pivotField>");
    }
    out
}

fn fields_xml(name: &str, fields: &[i32]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut out = format!("<{name} count=\"{}\">", fields.len());
    for f in fields {
        let _ = write!(out, "<field x=\"{f}\"/>");
    }
    let _ = write!(out, "</{name}>");
    out
}

fn data_fields_xml(fields: &[DataField]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut out = format!("<dataFields count=\"{}\">", fields.len());
    for d in fields {
        let _ = write!(
            out,
            "<dataField name=\"{}\" fld=\"{}\"",
            escape_attr(&d.name),
            d.field
        );
        if d.summary != cgs_model::pivot::Summary::Sum {
            let _ = write!(out, " subtotal=\"{}\"", d.summary.xml());
        }
        if d.show_as != ShowAs::Normal && !d.show_as.extended() {
            let _ = write!(out, " showDataAs=\"{}\"", d.show_as.xml());
        }
        let _ = write!(
            out,
            " baseField=\"{}\" baseItem=\"{}\"",
            d.base_field,
            d.base_item.xml()
        );
        if let Some(id) = d.num_fmt_id {
            let _ = write!(out, " numFmtId=\"{id}\"");
        }
        if d.show_as.extended() {
            let _ = write!(
                out,
                "><extLst><ext uri=\"{{E15A36E0-9728-4e99-A89B-3F7291B0FE68}}\" xmlns:x14=\"{X14}\"><x14:dataField pivotShowAs=\"{}\"/></ext></extLst></dataField>",
                d.show_as.xml()
            );
        } else {
            out.push_str("/>");
        }
    }
    out.push_str("</dataFields>");
    out
}

fn page_fields_xml(table: &PivotTable) -> String {
    if table.page_fields.is_empty() {
        return String::new();
    }
    let mut out = format!("<pageFields count=\"{}\">", table.page_fields.len());
    for p in &table.page_fields {
        let _ = write!(out, "<pageField fld=\"{}\"", p.field);
        if let Some(item) = p.item {
            let _ = write!(out, " item=\"{item}\"");
        }
        out.push_str(" hier=\"-1\"/>");
    }
    out.push_str("</pageFields>");
    out
}

fn location_xml(table: &PivotTable) -> String {
    let l = &table.location;
    let mut out = format!(
        "<location ref=\"{}\" firstHeaderRow=\"{}\" firstDataRow=\"{}\" firstDataCol=\"{}\"",
        rect_ref(&l.range),
        l.first_header_row,
        l.first_data_row,
        l.first_data_col
    );
    if l.row_page_count > 0 {
        let _ = write!(
            out,
            " rowPageCount=\"{}\" colPageCount=\"{}\"",
            l.row_page_count, l.col_page_count
        );
    }
    out.push_str("/>");
    out
}

fn style_xml(table: &PivotTable) -> String {
    let s = &table.style;
    let name = s
        .name
        .as_ref()
        .map(|n| format!(" name=\"{}\"", escape_attr(n)))
        .unwrap_or_default();
    format!(
        "<pivotTableStyleInfo{name} showRowHeaders=\"{}\" showColHeaders=\"{}\" showRowStripes=\"{}\" showColStripes=\"{}\" showLastColumn=\"{}\"/>",
        flag(s.row_headers),
        flag(s.col_headers),
        flag(s.row_stripes),
        flag(s.col_stripes),
        flag(s.last_column)
    )
}

/// The root's attributes the model holds.
fn root_attrs(table: &PivotTable) -> Vec<(&'static str, Option<String>)> {
    let o = &table.options;
    let caption = |c: &Option<String>| c.as_ref().map(|t| escape_attr(t));
    vec![
        ("name", Some(escape_attr(&table.name))),
        ("dataOnRows", o.data_on_rows.then(|| "1".to_string())),
        ("dataPosition", o.data_position.map(|p| p.to_string())),
        ("dataCaption", Some(escape_attr(&o.data_caption))),
        ("grandTotalCaption", caption(&o.grand_total_caption)),
        ("rowHeaderCaption", caption(&o.row_header_caption)),
        ("colHeaderCaption", caption(&o.col_header_caption)),
        ("missingCaption", caption(&o.missing_caption)),
        ("showMissing", (!o.show_missing).then(|| "0".to_string())),
        ("errorCaption", caption(&o.error_caption)),
        ("showError", o.show_error.then(|| "1".to_string())),
        (
            "rowGrandTotals",
            (!o.row_grand_totals).then(|| "0".to_string()),
        ),
        (
            "colGrandTotals",
            (!o.col_grand_totals).then(|| "0".to_string()),
        ),
        ("showHeaders", (!o.show_headers).then(|| "0".to_string())),
        ("compact", (!o.compact).then(|| "0".to_string())),
        ("compactData", (!o.compact_data).then(|| "0".to_string())),
        ("outline", o.outline.then(|| "1".to_string())),
        ("outlineData", o.outline_data.then(|| "1".to_string())),
    ]
}

/// Brings an existing pivot table part in line with the model, keeping everything it doesn't
/// hold: formats, chart formats, conditional formats, extensions.
pub fn patch_table_part(xml: &[u8], table: &PivotTable) -> io::Result<Vec<u8>> {
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else { continue };
        if root.local() != b"pivotTableDefinition" {
            continue;
        }
        let pre = prefix(&root.tag);
        for (name, value) in root_attrs(table) {
            root.set_attr(name, value.as_deref());
        }
        // The hidden values row is Excel 2010's, in the root's extension.
        if let Some(ext) = root
            .elements_mut()
            .filter(|e| e.local() == b"extLst")
            .flat_map(|l| l.elements_mut())
            .flat_map(|e| e.elements_mut())
            .find(|e| e.local() == b"pivotTableDefinition")
        {
            ext.set_attr(
                "hideValuesRow",
                table.options.hide_values_row.then_some("1"),
            );
        }
        place(
            root,
            b"location",
            elements(&location_xml(table), &pre),
            &TABLE_ORDER,
        );
        // Each field keeps its own children apart from its items and sort.
        let mut existing: Vec<Element> = Vec::new();
        let mut list_tag = None;
        if let Some(list) = root.elements_mut().find(|e| e.local() == b"pivotFields") {
            list_tag = Some((list.tag.clone(), list.end.clone()));
            existing = std::mem::take(&mut list.children)
                .into_iter()
                .filter_map(|c| match c {
                    Node::Element(e) if e.local() == b"pivotField" => Some(e),
                    _ => None,
                })
                .collect();
        }
        let mut fields_xml_list = Vec::new();
        for (i, field) in table.fields.iter().enumerate() {
            match existing.get_mut(i) {
                Some(element) => {
                    for (name, value) in field_attrs(field) {
                        element.set_attr(name, value.as_deref());
                    }
                    let items = elements(&items_xml(field), &pre);
                    place(element, b"items", items, &FIELD_ORDER);
                    let scope = field
                        .sort_by_data
                        .map(|d| elements(&sort_scope_xml(d), &pre))
                        .unwrap_or_default();
                    place(element, b"autoSortScope", scope, &FIELD_ORDER);
                    set_fill_down(element, field.repeat_labels, &pre);
                    if element.children.is_empty() && !element.empty {
                        // Nothing left inside: written as `<pivotField .../>`.
                        let mut tag = element.tag.clone();
                        if tag.last() == Some(&b'>') {
                            tag.pop();
                            tag.extend_from_slice(b"/>");
                        }
                        element.tag = tag;
                        element.empty = true;
                        element.end.clear();
                    }
                    fields_xml_list.push(Node::Element(std::mem::replace(
                        element,
                        Element {
                            tag: Vec::new(),
                            empty: true,
                            children: Vec::new(),
                            end: Vec::new(),
                        },
                    )));
                }
                None => fields_xml_list.extend(elements(&field_xml(field), &pre)),
            }
        }
        let (tag, end) = list_tag.unwrap_or_else(|| {
            (
                format!("<{pre}pivotFields count=\"{}\">", table.fields.len()).into_bytes(),
                format!("</{pre}pivotFields>").into_bytes(),
            )
        });
        let mut list = Element {
            tag,
            empty: false,
            children: fields_xml_list,
            end,
        };
        if list.tag.ends_with(b"/>") {
            list.tag.truncate(list.tag.len() - 2);
            list.tag.push(b'>');
            list.end = format!("</{pre}pivotFields>").into_bytes();
        }
        list.set_attr("count", Some(&table.fields.len().to_string()));
        place(
            root,
            b"pivotFields",
            vec![Node::Element(list)],
            &TABLE_ORDER,
        );
        place(
            root,
            b"rowFields",
            elements(&fields_xml("rowFields", &table.row_fields), &pre),
            &TABLE_ORDER,
        );
        let rows = if table.row_fields.is_empty() && table.row_items.is_empty() {
            String::new()
        } else {
            lines_xml("rowItems", &table.row_items)
        };
        place(root, b"rowItems", elements(&rows, &pre), &TABLE_ORDER);
        place(
            root,
            b"colFields",
            elements(&fields_xml("colFields", &table.col_fields), &pre),
            &TABLE_ORDER,
        );
        place(
            root,
            b"colItems",
            elements(&lines_xml("colItems", &table.col_items), &pre),
            &TABLE_ORDER,
        );
        place(
            root,
            b"pageFields",
            elements(&page_fields_xml(table), &pre),
            &TABLE_ORDER,
        );
        place(
            root,
            b"dataFields",
            elements(&data_fields_xml(&table.data_fields), &pre),
            &TABLE_ORDER,
        );
        place(
            root,
            b"pivotTableStyleInfo",
            elements(&style_xml(table), &pre),
            &TABLE_ORDER,
        );
    }
    let mut out = Vec::with_capacity(xml.len() + 256);
    write_tree(&nodes, &mut out);
    Ok(out)
}

/// A new pivot table part, as Excel 2016 writes one.
pub fn new_table_part(table: &PivotTable, cache_id: u32) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    let _ = write!(out, "<pivotTableDefinition xmlns=\"{MAIN}\"");
    for (name, value) in root_attrs(table) {
        if name == "dataCaption" || name == "name" {
            continue;
        }
        if let Some(value) = value {
            let _ = write!(out, " {name}=\"{value}\"");
        }
    }
    let _ = write!(
        out,
        " name=\"{}\" cacheId=\"{cache_id}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"{}\" updatedVersion=\"6\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"6\" indent=\"0\" multipleFieldFilters=\"0\">",
        escape_attr(&table.name),
        escape_attr(&table.options.data_caption)
    );
    out.push_str(&location_xml(table));
    let _ = write!(out, "<pivotFields count=\"{}\">", table.fields.len());
    for field in &table.fields {
        out.push_str(&field_xml(field));
    }
    out.push_str("</pivotFields>");
    out.push_str(&fields_xml("rowFields", &table.row_fields));
    if !(table.row_fields.is_empty() && table.row_items.is_empty()) {
        out.push_str(&lines_xml("rowItems", &table.row_items));
    }
    out.push_str(&fields_xml("colFields", &table.col_fields));
    out.push_str(&lines_xml("colItems", &table.col_items));
    out.push_str(&page_fields_xml(table));
    out.push_str(&data_fields_xml(&table.data_fields));
    out.push_str(&style_xml(table));
    let _ = write!(
        out,
        "<extLst><ext uri=\"{{962EF5D1-5CA2-4c93-8EF4-DBF5C05439D2}}\" xmlns:x14=\"{X14}\"><x14:pivotTableDefinition{} xmlns:xm=\"http://schemas.microsoft.com/office/excel/2006/main\"/></ext></extLst>",
        if table.options.hide_values_row { " hideValuesRow=\"1\"" } else { "" }
    );
    out.push_str("</pivotTableDefinition>");
    out
}

fn value_xml(value: &CacheValue, date1904: bool) -> String {
    match value {
        CacheValue::Missing => "<m/>".to_string(),
        CacheValue::Number(n) => format!("<n v=\"{}\"/>", number(*n)),
        CacheValue::Text(t) => format!("<s v=\"{}\"/>", escape_attr(t)),
        CacheValue::Bool(b) => format!("<b v=\"{}\"/>", flag(*b)),
        CacheValue::Error(e) => format!("<e v=\"{}\"/>", escape_attr(e)),
        CacheValue::Date(d) => format!("<d v=\"{}\"/>", serial_iso(*d, date1904)),
    }
}

/// `sharedItems` as Excel writes it: the kinds of value it holds, and the items when listed.
/// `values` is everything the field holds, whether listed or not.
fn shared_items_xml(field: &CacheField, values: &[CacheValue], date1904: bool) -> String {
    let mut blank = false;
    let mut text = false;
    let mut long = false;
    let mut numbers = 0usize;
    let mut integers = true;
    let mut dates = 0usize;
    let mut other = 0usize;
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut dmin, mut dmax) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        match v {
            CacheValue::Missing => blank = true,
            CacheValue::Text(t) => {
                text = true;
                long |= t.chars().count() > 255;
            }
            CacheValue::Number(n) => {
                numbers += 1;
                integers &= n.fract() == 0.0;
                min = min.min(*n);
                max = max.max(*n);
            }
            CacheValue::Date(d) => {
                dates += 1;
                dmin = dmin.min(*d);
                dmax = dmax.max(*d);
            }
            // Excel counts errors and logicals as text here.
            CacheValue::Bool(_) | CacheValue::Error(_) => {
                other += 1;
                text = true;
            }
        }
    }
    let kinds = usize::from(text) + usize::from(numbers > 0) + usize::from(dates > 0);
    let mut out = String::from("<sharedItems");
    if !text && !blank {
        out.push_str(" containsSemiMixedTypes=\"0\"");
    }
    if dates > 0 && numbers == 0 && !text && other == 0 {
        out.push_str(" containsNonDate=\"0\"");
    }
    if dates > 0 {
        out.push_str(" containsDate=\"1\"");
    }
    if !text {
        out.push_str(" containsString=\"0\"");
    }
    if blank {
        out.push_str(" containsBlank=\"1\"");
    }
    if kinds > 1 {
        out.push_str(" containsMixedTypes=\"1\"");
    }
    if numbers > 0 {
        out.push_str(" containsNumber=\"1\"");
        if integers {
            out.push_str(" containsInteger=\"1\"");
        }
        let _ = write!(
            out,
            " minValue=\"{}\" maxValue=\"{}\"",
            number(min),
            number(max)
        );
    }
    if dates > 0 {
        let _ = write!(
            out,
            " minDate=\"{}\" maxDate=\"{}\"",
            serial_iso(dmin, date1904),
            serial_iso(dmax.floor() + 1.0, date1904)
        );
    }
    if long {
        out.push_str(" longText=\"1\"");
    }
    if !field.listed {
        out.push_str("/>");
        return out;
    }
    let _ = write!(out, " count=\"{}\">", field.shared.len());
    for v in &field.shared {
        out.push_str(&value_xml(v, date1904));
    }
    out.push_str("</sharedItems>");
    out
}

fn group_xml(field: &CacheField, date1904: bool) -> String {
    let Some(group) = &field.group else {
        return String::new();
    };
    let mut out = "<fieldGroup".to_string();
    if let Some(par) = group.parent {
        let _ = write!(out, " par=\"{par}\"");
    }
    if group.base_named {
        let _ = write!(out, " base=\"{}\"", group.base);
    }
    out.push('>');
    match &group.range {
        Some(GroupRange::Numbers {
            start,
            end,
            interval,
            auto_start,
            auto_end,
        }) => {
            let _ = write!(
                out,
                "<rangePr{}{} startNum=\"{}\" endNum=\"{}\" groupInterval=\"{}\"/>",
                if *auto_start { "" } else { " autoStart=\"0\"" },
                if *auto_end { "" } else { " autoEnd=\"0\"" },
                number(*start),
                number(*end),
                number(*interval)
            );
        }
        Some(GroupRange::Dates {
            by,
            start,
            end,
            interval,
            auto_start,
            auto_end,
        }) => {
            let _ = write!(
                out,
                "<rangePr{}{} groupBy=\"{}\" startDate=\"{}\" endDate=\"{}\"{}/>",
                if *auto_start { "" } else { " autoStart=\"0\"" },
                if *auto_end { "" } else { " autoEnd=\"0\"" },
                by.xml(),
                serial_iso(*start, date1904),
                serial_iso(*end, date1904),
                if *interval != 1.0 {
                    format!(" groupInterval=\"{}\"", number(*interval))
                } else {
                    String::new()
                }
            );
        }
        Some(GroupRange::Discrete(map)) => {
            let _ = write!(out, "<discretePr count=\"{}\">", map.len());
            for x in map {
                let _ = write!(out, "<x v=\"{x}\"/>");
            }
            out.push_str("</discretePr>");
        }
        None => {}
    }
    // A field another groups by, such as a date grouped into months, has no items of its own.
    if group.range.is_none() && group.items.is_empty() {
        if out.ends_with('>') {
            out.pop();
            out.push_str("/>");
        }
        return out;
    }
    if !group.items.is_empty() {
        let _ = write!(out, "<groupItems count=\"{}\">", group.items.len());
        for v in &group.items {
            out.push_str(&value_xml(v, date1904));
        }
        out.push_str("</groupItems>");
    }
    out.push_str("</fieldGroup>");
    out
}

fn cache_field_xml(field: &CacheField, values: &[CacheValue], date1904: bool) -> String {
    let mut out = format!(
        "<cacheField name=\"{}\" numFmtId=\"{}\"",
        escape_attr(&field.name),
        field.num_fmt_id
    );
    if !field.database {
        out.push_str(" databaseField=\"0\"");
    }
    out.push('>');
    if field.database {
        out.push_str(&shared_items_xml(field, values, date1904));
    }
    out.push_str(&group_xml(field, date1904));
    out.push_str("</cacheField>");
    out
}

fn source_xml(cache: &PivotCache) -> String {
    match &cache.source {
        CacheSource::Worksheet { sheet, range, name } => {
            let mut out = String::from("<cacheSource type=\"worksheet\"><worksheetSource");
            if let Some(range) = range {
                let _ = write!(out, " ref=\"{}\"", escape_attr(range));
            }
            if let Some(name) = name {
                let _ = write!(out, " name=\"{}\"", escape_attr(name));
            }
            if let Some(sheet) = sheet {
                let _ = write!(out, " sheet=\"{}\"", escape_attr(sheet));
            }
            out.push_str("/></cacheSource>");
            out
        }
        CacheSource::Other => String::new(),
    }
}

/// Brings a cache definition in line with the model: its source, its fields, and a refresh when
/// the file opens. `values` holds each field's values, listed or not. `drop_records` says the
/// records part goes, since it no longer fits the fields.
pub fn patch_cache_part(
    xml: &[u8],
    cache: &PivotCache,
    values: &[Vec<CacheValue>],
    drop_records: bool,
    date1904: bool,
) -> io::Result<Vec<u8>> {
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else { continue };
        if root.local() != b"pivotCacheDefinition" {
            continue;
        }
        let pre = prefix(&root.tag);
        root.set_attr("refreshOnLoad", Some("1"));
        if drop_records {
            root.set_attr("saveData", Some("0"));
            root.set_attr("r:id", None);
            root.set_attr("recordCount", Some(&cache.record_count.to_string()));
        }
        // The source's place.
        if let CacheSource::Worksheet { sheet, range, name } = &cache.source {
            if let Some(at) = root
                .elements_mut()
                .find(|e| e.local() == b"cacheSource")
                .and_then(|s| s.elements_mut().find(|e| e.local() == b"worksheetSource"))
            {
                at.set_attr("ref", range.as_ref().map(|r| escape_attr(r)).as_deref());
                at.set_attr("sheet", sheet.as_ref().map(|s| escape_attr(s)).as_deref());
                at.set_attr("name", name.as_ref().map(|n| escape_attr(n)).as_deref());
            }
        }
        // Fields: each one's shared items and group, its other children kept.
        let Some(list) = root.elements_mut().find(|e| e.local() == b"cacheFields") else {
            continue;
        };
        let old: Vec<Element> = std::mem::take(&mut list.children)
            .into_iter()
            .filter_map(|c| match c {
                Node::Element(e) if e.local() == b"cacheField" => Some(e),
                _ => None,
            })
            .collect();
        let mut children = Vec::new();
        for (i, field) in cache.fields.iter().enumerate() {
            let empty: Vec<CacheValue> = Vec::new();
            let field_values = values.get(i).unwrap_or(&empty);
            let same_name = old
                .get(i)
                .and_then(|e| e.attr_text(b"name"))
                .is_some_and(|n| n == field.name);
            match (same_name, old.get(i)) {
                (true, Some(element)) if field.database || field.group.is_some() => {
                    let mut element = Element {
                        tag: element.tag.clone(),
                        empty: element.empty,
                        children: Vec::new(),
                        end: element.end.clone(),
                    };
                    let source = &old[i];
                    element.set_attr("numFmtId", Some(&field.num_fmt_id.to_string()));
                    // Keep children other than the items and the group, in place.
                    let mut kept: Vec<Node> = Vec::new();
                    let mut put_items = false;
                    for child in &source.children {
                        match child {
                            Node::Element(e) if e.local() == b"sharedItems" && !field.database => {
                                kept.push(clone_node(child));
                                put_items = true;
                            }
                            Node::Element(e) if e.local() == b"sharedItems" => {
                                kept.extend(elements(
                                    &shared_items_xml(field, field_values, date1904),
                                    &pre,
                                ));
                                put_items = true;
                            }
                            Node::Element(e) if e.local() == b"fieldGroup" => {
                                kept.extend(elements(&group_xml(field, date1904), &pre));
                            }
                            other => kept.push(clone_node(other)),
                        }
                    }
                    if !put_items && field.database {
                        let mut items =
                            elements(&shared_items_xml(field, field_values, date1904), &pre);
                        items.append(&mut kept);
                        kept = items;
                    }
                    if element.empty {
                        element.empty = false;
                        let mut tag = element.tag.clone();
                        if tag.ends_with(b"/>") {
                            tag.truncate(tag.len() - 2);
                            tag.push(b'>');
                        }
                        element.tag = tag;
                        element.end = format!("</{pre}cacheField>").into_bytes();
                    }
                    element.children = kept;
                    children.push(Node::Element(element));
                }
                (true, Some(element)) => children.push(Node::Element(clone_element(element))),
                _ => children.extend(elements(
                    &cache_field_xml(field, field_values, date1904),
                    &pre,
                )),
            }
        }
        list.children = children;
        list.set_attr("count", Some(&cache.fields.len().to_string()));
    }
    let mut out = Vec::with_capacity(xml.len() + 256);
    write_tree(&nodes, &mut out);
    Ok(out)
}

fn clone_node(node: &Node) -> Node {
    match node {
        Node::Text(t) => Node::Text(t.clone()),
        Node::Element(e) => Node::Element(clone_element(e)),
    }
}

fn clone_element(e: &Element) -> Element {
    Element {
        tag: e.tag.clone(),
        empty: e.empty,
        children: e.children.iter().map(clone_node).collect(),
        end: e.end.clone(),
    }
}

/// A new cache definition, with no records: Excel reads the source when it opens the file.
pub fn new_cache_part(cache: &PivotCache, values: &[Vec<CacheValue>], date1904: bool) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    let _ = write!(
        out,
        "<pivotCacheDefinition xmlns=\"{MAIN}\" xmlns:r=\"{RELS}\" saveData=\"0\" refreshOnLoad=\"1\" createdVersion=\"6\" refreshedVersion=\"6\" minRefreshableVersion=\"3\" recordCount=\"{}\">",
        cache.record_count
    );
    out.push_str(&source_xml(cache));
    let _ = write!(out, "<cacheFields count=\"{}\">", cache.fields.len());
    let empty = Vec::new();
    for (i, field) in cache.fields.iter().enumerate() {
        out.push_str(&cache_field_xml(
            field,
            values.get(i).unwrap_or(&empty),
            date1904,
        ));
    }
    out.push_str("</cacheFields></pivotCacheDefinition>");
    out
}

/// `<pivotCaches>` with one more cache in it, for the workbook part.
pub fn add_workbook_cache(xml: &[u8], cache_id: u32, rel_id: &str) -> io::Result<Vec<u8>> {
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else { continue };
        if root.local() != b"workbook" {
            continue;
        }
        let pre = prefix(&root.tag);
        let rel_prefix = "r:";
        let entry = format!(
            "<pivotCache cacheId=\"{cache_id}\" {rel_prefix}id=\"{}\"/>",
            escape_attr(rel_id)
        );
        if !String::from_utf8_lossy(&root.tag).contains("xmlns:r=") {
            let mut tag = root.tag.clone();
            let close = if tag.ends_with(b"/>") { 2 } else { 1 };
            let at = tag.len() - close;
            tag.splice(at..at, format!(" xmlns:r=\"{RELS}\"").bytes());
            root.tag = tag;
        }
        let has_list = root.elements().any(|e| e.local() == b"pivotCaches");
        match has_list
            .then(|| root.elements_mut().find(|e| e.local() == b"pivotCaches"))
            .flatten()
        {
            Some(list) => {
                if list.empty {
                    list.empty = false;
                    let mut tag = list.tag.clone();
                    if tag.ends_with(b"/>") {
                        tag.truncate(tag.len() - 2);
                        tag.push(b'>');
                    }
                    list.tag = tag;
                    list.end = format!("</{pre}pivotCaches>").into_bytes();
                }
                list.children.extend(elements(&entry, &pre));
            }
            None => {
                let nodes = elements(&format!("<pivotCaches>{entry}</pivotCaches>"), &pre);
                // Before what CT_Workbook puts after the caches.
                const AFTER: [&[u8]; 6] = [
                    b"smartTagPr",
                    b"smartTagTypes",
                    b"webPublishing",
                    b"fileRecoveryPr",
                    b"webPublishObjects",
                    b"extLst",
                ];
                let at = root
                    .children
                    .iter()
                    .position(|c| matches!(c, Node::Element(e) if AFTER.contains(&e.local())))
                    .unwrap_or(root.children.len());
                for (offset, node) in nodes.into_iter().enumerate() {
                    root.children.insert(at + offset, node);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(xml.len() + 128);
    write_tree(&nodes, &mut out);
    Ok(out)
}

/// The workbook part without the caches no pivot table uses any more.
pub fn remove_workbook_caches(xml: &[u8], ids: &[u32]) -> io::Result<Vec<u8>> {
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else { continue };
        let mut drop_list = false;
        if let Some(list) = root.elements_mut().find(|e| e.local() == b"pivotCaches") {
            list.children.retain(|c| match c {
                Node::Element(e) => !e
                    .attr_text(b"cacheId")
                    .and_then(|v| v.parse::<u32>().ok())
                    .is_some_and(|id| ids.contains(&id)),
                Node::Text(_) => true,
            });
            drop_list = list.elements().next().is_none();
        }
        if drop_list {
            root.children
                .retain(|c| !matches!(c, Node::Element(e) if e.local() == b"pivotCaches"));
        }
    }
    let mut out = Vec::with_capacity(xml.len());
    write_tree(&nodes, &mut out);
    Ok(out)
}

/// The field list a refreshed cache's values come in, for writing: each field's values whether
/// listed or not.
pub fn values_of(
    fields: &[CacheField],
    records: Option<&cgs_model::pivot::CacheRecords>,
) -> Vec<Vec<CacheValue>> {
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            records
                .and_then(|r| r.columns.get(i))
                .filter(|c| !c.values.is_empty())
                .map(|c| {
                    let used: std::collections::HashSet<u32> = c.index.iter().copied().collect();
                    c.values
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| used.contains(&(*i as u32)))
                        .map(|(_, v)| v.clone())
                        .collect()
                })
                .unwrap_or_else(|| f.shared.clone())
        })
        .collect()
}

/// Whether a table's lines and fields point only at what its cache has.
pub fn fits(table: &PivotTable, cache: &PivotCache) -> bool {
    if table.fields.len() != cache.fields.len() {
        return false;
    }
    table.fields.iter().enumerate().all(|(f, field)| {
        let count = match &cache.fields[f].group {
            Some(g) if !g.items.is_empty() => g.items.len(),
            _ => cache.fields[f].shared.len(),
        };
        field
            .items
            .iter()
            .all(|i| i.x.is_none_or(|x| (x as usize) < count))
    }) && table
        .row_fields
        .iter()
        .chain(&table.col_fields)
        .all(|&f| f == VALUES_FIELD || (f as usize) < cache.fields.len())
}
