//! Pivot tables and caches: reading their definitions when a file opens, and a cache's records when
//! a pivot is first refreshed or changed.

use std::collections::HashMap;

use cgs_model::pivot::{
    AxisLine, BaseItem, CacheField, CacheRecords, CacheSource, CacheValue, DataField, DateGrouping,
    FieldGroup, FieldSort, GroupRange, ItemType, PageField, PivotAxis, PivotCache, PivotField,
    PivotItem, PivotLocation, PivotOptions, PivotStyleInfo, PivotTable, RecordColumn, ShowAs,
    Summary,
};
use cgs_model::Book;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::package::Package;
use crate::structure::{parse_tree, Element, Node};
use crate::workbook::read_relationships;
use crate::xml::{attr, parse_rect};

/// The root element of a part, if it parses.
fn root(xml: &[u8]) -> Option<Element> {
    parse_tree(xml)
        .ok()?
        .into_iter()
        .find_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
}

fn child<'a>(element: &'a Element, name: &[u8]) -> Option<&'a Element> {
    element.elements().find(|e| e.local() == name)
}

fn text(element: &Element, name: &[u8]) -> Option<String> {
    element.attr_text(name)
}

fn flag(element: &Element, name: &[u8], default: bool) -> bool {
    match element.attr(name) {
        Some(b"1" | b"true") => true,
        Some(b"0" | b"false") => false,
        _ => default,
    }
}

fn number<T: std::str::FromStr>(element: &Element, name: &[u8]) -> Option<T> {
    std::str::from_utf8(element.attr(name)?).ok()?.parse().ok()
}

/// The value of an `r:id`, whatever its prefix.
fn relationship_id(element: &Element) -> Option<String> {
    let tag = String::from_utf8_lossy(&element.tag).into_owned();
    let at = tag.find(":id=")?;
    let rest = &tag[at + 4..];
    let quote = rest.chars().next()?;
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Days from 1899-12-30 to a civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + 25_569
}

/// An ISO date and time from a cache as a serial, in the 1900 or 1904 date system.
pub fn iso_serial(text: &str, date1904: bool) -> Option<f64> {
    let (date, time) = text.split_once('T').unwrap_or((text, ""));
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let mut serial = days_from_civil(year, month, day) as f64;
    if date1904 {
        serial -= 1462.0;
    } else if (1.0..61.0).contains(&serial) {
        // Excel counts a 29 February 1900 that never was. Times alone sit on 1899-12-30, day 0.
        serial -= 1.0;
    }
    if !time.is_empty() {
        let mut bits = time.trim_end_matches('Z').split(':');
        let hours: f64 = bits.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let minutes: f64 = bits.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let seconds: f64 = bits.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        serial += (hours * 3600.0 + minutes * 60.0 + seconds) / 86_400.0;
    }
    Some(serial)
}

/// A serial as the ISO text a cache holds.
pub fn serial_iso(serial: f64, date1904: bool) -> String {
    let mut days = serial.floor() as i64;
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        days += 1;
        seconds -= 86_400;
    }
    let mut civil = days;
    if date1904 {
        civil += 1462;
    } else if (1..61).contains(&civil) {
        civil += 1;
    }
    // Back from days since 1899-12-30.
    let z = civil - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// A shared item or record value.
fn cache_value(element: &Element, date1904: bool) -> Option<CacheValue> {
    let value = || text(element, b"v").unwrap_or_default();
    Some(match element.local() {
        b"m" => CacheValue::Missing,
        b"n" => CacheValue::Number(value().parse().unwrap_or(0.0)),
        b"s" => CacheValue::Text(value()),
        b"b" => CacheValue::Bool(matches!(value().as_str(), "1" | "true")),
        b"e" => CacheValue::Error(value()),
        b"d" => CacheValue::Date(iso_serial(&value(), date1904).unwrap_or(0.0)),
        _ => return None,
    })
}

fn read_group(element: &Element, field: u32, date1904: bool) -> FieldGroup {
    let base = number(element, b"base").unwrap_or(field);
    let parent = number(element, b"par");
    let mut range = None;
    let mut items = Vec::new();
    for part in element.elements() {
        match part.local() {
            b"rangePr" => {
                let interval = number(part, b"groupInterval").unwrap_or(1.0);
                let auto_start = flag(part, b"autoStart", true);
                let auto_end = flag(part, b"autoEnd", true);
                let by = text(part, b"groupBy").unwrap_or_else(|| "range".to_string());
                range = Some(match DateGrouping::from_xml(&by) {
                    Some(by) => GroupRange::Dates {
                        by,
                        start: text(part, b"startDate")
                            .and_then(|t| iso_serial(&t, date1904))
                            .unwrap_or(0.0),
                        end: text(part, b"endDate")
                            .and_then(|t| iso_serial(&t, date1904))
                            .unwrap_or(0.0),
                        interval,
                        auto_start,
                        auto_end,
                    },
                    None => GroupRange::Numbers {
                        start: number(part, b"startNum").unwrap_or(0.0),
                        end: number(part, b"endNum").unwrap_or(0.0),
                        interval,
                        auto_start,
                        auto_end,
                    },
                });
            }
            b"discretePr" => {
                range = Some(GroupRange::Discrete(
                    part.elements().filter_map(|x| number(x, b"v")).collect(),
                ));
            }
            b"groupItems" => {
                items = part
                    .elements()
                    .filter_map(|v| cache_value(v, date1904))
                    .collect();
            }
            _ => {}
        }
    }
    FieldGroup {
        base,
        parent,
        range,
        items,
    }
}

/// A cache definition part.
pub fn read_cache_definition(xml: &[u8], date1904: bool) -> Option<PivotCache> {
    let root = root(xml)?;
    if root.local() != b"pivotCacheDefinition" {
        return None;
    }
    let mut cache = PivotCache {
        record_count: number(&root, b"recordCount").unwrap_or(0),
        refresh_on_load: flag(&root, b"refreshOnLoad", false),
        save_data: flag(&root, b"saveData", true),
        ..PivotCache::default()
    };
    let mut why: Vec<&str> = Vec::new();
    let mut source_why: Vec<&str> = Vec::new();
    if let Some(source) = child(&root, b"cacheSource") {
        match source.attr(b"type") {
            Some(b"worksheet") => {
                let at = child(source, b"worksheetSource");
                let extended = at.and_then(relationship_id).is_some();
                cache.source = CacheSource::Worksheet {
                    sheet: at.and_then(|a| text(a, b"sheet")),
                    range: at.and_then(|a| text(a, b"ref")),
                    name: at.and_then(|a| text(a, b"name")),
                };
                if extended {
                    source_why.push("its source is in another workbook");
                }
            }
            Some(b"external") => source_why.push("its data comes from an external connection"),
            Some(b"consolidation") => source_why.push("it consolidates several ranges"),
            Some(b"scenario") => source_why.push("it summarizes scenarios"),
            _ => source_why.push("its source can't be read"),
        }
    }
    if child(&root, b"cacheHierarchies").is_some() || child(&root, b"tupleCache").is_some() {
        why.push("it is an OLAP cube");
    }
    if let Some(fields) = child(&root, b"cacheFields") {
        for (index, field) in fields.elements().enumerate() {
            if field.local() != b"cacheField" {
                continue;
            }
            let mut out = CacheField {
                name: text(field, b"name").unwrap_or_default(),
                num_fmt_id: number(field, b"numFmtId").unwrap_or(0),
                formula: text(field, b"formula"),
                database: flag(field, b"databaseField", true),
                ..CacheField::default()
            };
            if let Some(shared) = child(field, b"sharedItems") {
                out.listed = shared.attr(b"count").is_some() || shared.elements().next().is_some();
                out.shared = shared
                    .elements()
                    .filter_map(|v| cache_value(v, date1904))
                    .collect();
            }
            if let Some(group) = child(field, b"fieldGroup") {
                out.group = Some(read_group(group, index as u32, date1904));
            }
            if out.formula.is_some() {
                why.push("it has a calculated field");
            }
            cache.fields.push(out);
        }
    }
    why.dedup();
    if !why.is_empty() {
        cache.unsupported = Some(why.join(", "));
    }
    if !source_why.is_empty() {
        cache.no_refresh = Some(source_why.join(", "));
    }
    Some(cache)
}

fn axis(text: &[u8]) -> Option<PivotAxis> {
    Some(match text {
        b"axisRow" => PivotAxis::Row,
        b"axisCol" => PivotAxis::Col,
        b"axisPage" => PivotAxis::Page,
        b"axisValues" => PivotAxis::Values,
        _ => return None,
    })
}

const SUBTOTAL_FLAGS: [(&[u8], ItemType); 11] = [
    (b"sumSubtotal", ItemType::Sum),
    (b"countASubtotal", ItemType::CountA),
    (b"avgSubtotal", ItemType::Avg),
    (b"maxSubtotal", ItemType::Max),
    (b"minSubtotal", ItemType::Min),
    (b"productSubtotal", ItemType::Product),
    (b"countSubtotal", ItemType::Count),
    (b"stdDevSubtotal", ItemType::StdDev),
    (b"stdDevPSubtotal", ItemType::StdDevP),
    (b"varSubtotal", ItemType::Var),
    (b"varPSubtotal", ItemType::VarP),
];

/// The attribute that asks for a custom subtotal.
pub fn subtotal_flag(kind: ItemType) -> Option<&'static str> {
    SUBTOTAL_FLAGS
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(name, _)| std::str::from_utf8(name).unwrap_or_default())
}

fn read_field(element: &Element, repeat_default: bool) -> PivotField {
    let mut field = PivotField {
        axis: element.attr(b"axis").and_then(axis),
        data_field: flag(element, b"dataField", false),
        show_all: flag(element, b"showAll", true),
        compact: flag(element, b"compact", true),
        outline: flag(element, b"outline", true),
        subtotal_top: flag(element, b"subtotalTop", true),
        default_subtotal: flag(element, b"defaultSubtotal", true),
        insert_blank_row: flag(element, b"insertBlankRow", false),
        name: text(element, b"name"),
        subtotal_caption: text(element, b"subtotalCaption"),
        num_fmt_id: number(element, b"numFmtId"),
        multiple_item_selection: flag(element, b"multipleItemSelectionAllowed", false),
        sort: match element.attr(b"sortType") {
            Some(b"ascending") => FieldSort::Ascending,
            Some(b"descending") => FieldSort::Descending,
            _ => FieldSort::Manual,
        },
        ..PivotField::default()
    };
    field.repeat_labels = child(element, b"extLst")
        .and_then(|list| {
            list.elements()
                .flat_map(|ext| ext.elements())
                .find(|e| e.local() == b"pivotField")
        })
        .map_or(repeat_default, |e| {
            flag(e, b"fillDownLabels", repeat_default)
        });
    field.subtotals = SUBTOTAL_FLAGS
        .iter()
        .filter(|(name, _)| flag(element, name, false))
        .map(|(_, kind)| *kind)
        .collect();
    if let Some(items) = child(element, b"items") {
        field.items_listed = true;
        for item in items.elements().filter(|e| e.local() == b"item") {
            let kind = item
                .attr_text(b"t")
                .and_then(|t| ItemType::from_xml(&t))
                .unwrap_or(ItemType::Data);
            field.items.push(PivotItem {
                kind,
                x: number(item, b"x"),
                hidden: flag(item, b"h", false),
                show_details: flag(item, b"sd", true),
                name: text(item, b"n"),
                missing: flag(item, b"m", false),
            });
        }
    }
    if let Some(scope) = child(element, b"autoSortScope") {
        // <autoSortScope><pivotArea><references><reference field="4294967294"><x v="0"/>
        let data = scope
            .elements()
            .flat_map(|area| area.elements())
            .flat_map(|refs| refs.elements())
            .find(|r| r.attr(b"field") == Some(b"4294967294"))
            .and_then(|r| r.elements().next().and_then(|x| number(x, b"v")));
        field.sort_by_data = Some(data.unwrap_or(0));
    }
    field
}

fn read_lines(element: Option<&Element>) -> Vec<AxisLine> {
    let Some(element) = element else {
        return Vec::new();
    };
    element
        .elements()
        .filter(|e| e.local() == b"i")
        .map(|line| AxisLine {
            kind: line
                .attr_text(b"t")
                .and_then(|t| ItemType::from_xml(&t))
                .unwrap_or(ItemType::Data),
            repeat: number(line, b"r").unwrap_or(0),
            data: number(line, b"i").unwrap_or(0),
            members: line
                .elements()
                .filter(|x| x.local() == b"x")
                .map(|x| number(x, b"v").unwrap_or(0))
                .collect(),
        })
        .collect()
}

fn axis_fields(element: Option<&Element>) -> Vec<i32> {
    element
        .map(|e| {
            e.elements()
                .filter(|f| f.local() == b"field")
                .filter_map(|f| number(f, b"x"))
                .collect()
        })
        .unwrap_or_default()
}

/// Excel 2010's extension on the root, for `hideValuesRow` and newer Show Values As kinds.
fn root_extension<'a>(root: &'a Element, name: &[u8]) -> Option<&'a Element> {
    child(root, b"extLst")?
        .elements()
        .flat_map(|ext| ext.elements())
        .find(|e| e.local() == name)
}

/// A pivot table definition part. `cache` is the index of its cache in `Book::pivot_caches`.
pub fn read_table_definition(xml: &[u8], sheet: u32, cache: u32) -> Option<PivotTable> {
    let root = root(xml)?;
    if root.local() != b"pivotTableDefinition" {
        return None;
    }
    let location = child(&root, b"location")?;
    let range = parse_rect(&text(location, b"ref")?)?;
    let opt = |name: &[u8], default: bool| flag(&root, name, default);
    let x14 = root_extension(&root, b"pivotTableDefinition");
    let options = PivotOptions {
        data_caption: text(&root, b"dataCaption").unwrap_or_else(|| "Values".to_string()),
        grand_total_caption: text(&root, b"grandTotalCaption"),
        row_header_caption: text(&root, b"rowHeaderCaption"),
        col_header_caption: text(&root, b"colHeaderCaption"),
        missing_caption: text(&root, b"missingCaption"),
        show_missing: opt(b"showMissing", true),
        error_caption: text(&root, b"errorCaption"),
        show_error: opt(b"showError", false),
        row_grand_totals: opt(b"rowGrandTotals", true),
        col_grand_totals: opt(b"colGrandTotals", true),
        data_on_rows: opt(b"dataOnRows", false),
        data_position: number(&root, b"dataPosition"),
        compact: opt(b"compact", true),
        compact_data: opt(b"compactData", true),
        outline: opt(b"outline", false),
        outline_data: opt(b"outlineData", false),
        grid_drop_zones: opt(b"gridDropZones", false),
        indent: number(&root, b"indent").unwrap_or(1),
        show_headers: opt(b"showHeaders", true),
        merge_item: opt(b"mergeItem", false),
        page_over_then_down: opt(b"pageOverThenDown", false),
        page_wrap: number(&root, b"pageWrap").unwrap_or(0),
        show_empty_row: opt(b"showEmptyRow", false),
        show_empty_col: opt(b"showEmptyCol", false),
        hide_values_row: x14.is_some_and(|e| flag(e, b"hideValuesRow", false)),
        created_version: number(&root, b"createdVersion").unwrap_or(0),
        updated_version: number(&root, b"updatedVersion").unwrap_or(0),
    };
    let repeat_default = x14.is_some_and(|e| flag(e, b"fillDownLabelsDefault", false));
    let fields: Vec<PivotField> = child(&root, b"pivotFields")
        .map(|f| {
            f.elements()
                .filter(|e| e.local() == b"pivotField")
                .map(|e| read_field(e, repeat_default))
                .collect()
        })
        .unwrap_or_default();
    let page_fields = child(&root, b"pageFields")
        .map(|p| {
            p.elements()
                .filter(|e| e.local() == b"pageField")
                .map(|e| PageField {
                    field: number(e, b"fld").unwrap_or(0),
                    item: number(e, b"item"),
                    name: text(e, b"name"),
                })
                .collect()
        })
        .unwrap_or_default();
    // Newer Show Values As kinds sit in each data field's extension.
    let data_fields = child(&root, b"dataFields")
        .map(|d| {
            d.elements()
                .filter(|e| e.local() == b"dataField")
                .map(|e| {
                    let extended = child(e, b"extLst")
                        .and_then(|l| l.elements().flat_map(|x| x.elements()).next())
                        .and_then(|x| x.attr_text(b"pivotShowAs"))
                        .and_then(|t| ShowAs::from_xml(&t));
                    DataField {
                        name: text(e, b"name").unwrap_or_default(),
                        field: number(e, b"fld").unwrap_or(0),
                        summary: e
                            .attr_text(b"subtotal")
                            .and_then(|t| Summary::from_xml(&t))
                            .unwrap_or_default(),
                        show_as: extended.unwrap_or_else(|| {
                            e.attr_text(b"showDataAs")
                                .and_then(|t| ShowAs::from_xml(&t))
                                .unwrap_or_default()
                        }),
                        base_field: number(e, b"baseField").unwrap_or(0),
                        base_item: BaseItem::from_xml(number(e, b"baseItem").unwrap_or(0)),
                        num_fmt_id: number(e, b"numFmtId"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let style = child(&root, b"pivotTableStyleInfo")
        .map(|s| PivotStyleInfo {
            name: text(s, b"name"),
            row_headers: flag(s, b"showRowHeaders", false),
            col_headers: flag(s, b"showColHeaders", false),
            row_stripes: flag(s, b"showRowStripes", false),
            col_stripes: flag(s, b"showColStripes", false),
            last_column: flag(s, b"showLastColumn", false),
        })
        .unwrap_or(PivotStyleInfo {
            name: None,
            row_headers: false,
            col_headers: false,
            row_stripes: false,
            col_stripes: false,
            last_column: false,
        });
    let mut why = Vec::new();
    if child(&root, b"calculatedItems").is_some() {
        why.push("it has calculated items");
    }
    if child(&root, b"filters").is_some() {
        why.push("it has label or value filters");
    }
    if child(&root, b"pivotHierarchies").is_some() {
        why.push("it is an OLAP pivot table");
    }
    Some(PivotTable {
        name: text(&root, b"name").unwrap_or_default(),
        sheet,
        part: None,
        cache,
        location: PivotLocation {
            range,
            first_header_row: number(location, b"firstHeaderRow").unwrap_or(0),
            first_data_row: number(location, b"firstDataRow").unwrap_or(0),
            first_data_col: number(location, b"firstDataCol").unwrap_or(0),
            row_page_count: number(location, b"rowPageCount").unwrap_or(0),
            col_page_count: number(location, b"colPageCount").unwrap_or(0),
        },
        fields,
        row_fields: axis_fields(child(&root, b"rowFields")),
        col_fields: axis_fields(child(&root, b"colFields")),
        page_fields,
        data_fields,
        row_items: read_lines(child(&root, b"rowItems")),
        col_items: read_lines(child(&root, b"colItems")),
        options,
        style,
        unsupported: (!why.is_empty()).then(|| why.join(", ")),
        changed: false,
    })
}

/// The workbook's `<pivotCaches>`: cache id to definition part.
fn workbook_caches(package: &Package, main: &str) -> Vec<(u32, String)> {
    let Some(xml) = package.read(main) else {
        return Vec::new();
    };
    let rels = read_relationships(package, main);
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref() == b"pivotCache" =>
            {
                let id = attr(e, b"cacheId").and_then(|v| v.parse().ok());
                let rel = attr(e, b"id");
                if let (Some(id), Some((_, target))) = (id, rel.and_then(|r| rels.get(&r))) {
                    out.push((id, target.clone()));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Reads every pivot cache and table. Unreadable ones are left out, and their parts saved as
/// they are.
pub fn read_pivots(package: &Package, main: &str, book: &mut Book) {
    let mut caches: Vec<PivotCache> = Vec::new();
    let mut by_part: HashMap<String, u32> = HashMap::new();
    for (id, part) in workbook_caches(package, main) {
        let Some(xml) = package.read(&part) else {
            continue;
        };
        let Some(mut cache) = read_cache_definition(&xml, book.date1904) else {
            continue;
        };
        cache.id = id;
        cache.records_part = read_relationships(package, &part)
            .into_values()
            .find(|(kind, _)| kind.ends_with("/pivotCacheRecords"))
            .map(|(_, target)| target)
            .filter(|target| package.part(target).is_some());
        cache.part = Some(part.clone());
        by_part.insert(crate::zipdir::normalize(&part), caches.len() as u32);
        caches.push(cache);
    }
    let mut tables = Vec::new();
    for (index, sheet) in book.sheets.iter().enumerate() {
        let Some(part) = &sheet.part else { continue };
        let mut found: Vec<(String, String)> = read_relationships(package, part)
            .into_iter()
            .filter(|(_, (kind, _))| kind.ends_with("/pivotTable"))
            .map(|(id, (_, target))| (id, target))
            .collect();
        found.sort();
        for (_, target) in found {
            let Some(xml) = package.read(&target) else {
                continue;
            };
            let cache = read_relationships(package, &target)
                .into_values()
                .find(|(kind, _)| kind.ends_with("/pivotCacheDefinition"))
                .and_then(|(_, def)| by_part.get(&crate::zipdir::normalize(&def)).copied());
            let Some(cache) = cache else { continue };
            if let Some(mut table) = read_table_definition(&xml, index as u32, cache) {
                table.part = Some(target.clone());
                let source = &caches[cache as usize];
                if table.unsupported.is_none() {
                    if let Some(why) = &source.unsupported {
                        table.unsupported = Some(why.clone());
                    } else if table.fields.len() != source.fields.len() {
                        table.unsupported = Some("its fields don't match its cache".to_string());
                    }
                }
                tables.push(table);
            }
        }
    }
    book.pivot_caches = caches;
    book.pivot_tables = tables;
}

/// A cache's records part, by column. Values the records hold themselves are added to each
/// column after the shared items.
pub fn read_records(xml: &[u8], cache: &PivotCache, date1904: bool) -> CacheRecords {
    let source_fields: Vec<usize> = (0..cache.fields.len())
        .filter(|&i| cache.fields[i].database)
        .collect();
    let mut columns: Vec<RecordColumn> = cache
        .fields
        .iter()
        .map(|field| RecordColumn {
            values: if field.database {
                field.shared.clone()
            } else {
                Vec::new()
            },
            index: Vec::new(),
        })
        .collect();
    let mut interned: Vec<HashMap<ValueKey, u32>> = columns
        .iter()
        .map(|column| {
            column
                .values
                .iter()
                .enumerate()
                .map(|(i, v)| (ValueKey::of(v), i as u32))
                .collect()
        })
        .collect();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut at = 0usize;
    let mut count = 0u32;
    let mut in_record = false;
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == b"r" => {
                in_record = true;
                at = 0;
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"r" => {
                // A short record leaves its last fields blank.
                while at < source_fields.len() {
                    let column = &mut columns[source_fields[at]];
                    let index = intern(
                        column,
                        &mut interned[source_fields[at]],
                        CacheValue::Missing,
                    );
                    column.index.push(index);
                    at += 1;
                }
                in_record = false;
                count += 1;
            }
            Ok(Event::Empty(ref e) | Event::Start(ref e)) if in_record => {
                let Some(&field) = source_fields.get(at) else {
                    buf.clear();
                    continue;
                };
                let name = e.local_name();
                let raw = attr(e, b"v").unwrap_or_default();
                let column = &mut columns[field];
                let index = match name.as_ref() {
                    b"x" => raw.parse().unwrap_or(0),
                    b"m" => intern(column, &mut interned[field], CacheValue::Missing),
                    b"n" => intern(
                        column,
                        &mut interned[field],
                        CacheValue::Number(raw.parse().unwrap_or(0.0)),
                    ),
                    b"s" => intern(column, &mut interned[field], CacheValue::Text(raw)),
                    b"b" => intern(
                        column,
                        &mut interned[field],
                        CacheValue::Bool(matches!(raw.as_str(), "1" | "true")),
                    ),
                    b"e" => intern(column, &mut interned[field], CacheValue::Error(raw)),
                    b"d" => intern(
                        column,
                        &mut interned[field],
                        CacheValue::Date(iso_serial(&raw, date1904).unwrap_or(0.0)),
                    ),
                    _ => {
                        buf.clear();
                        continue;
                    }
                };
                column.index.push(index);
                at += 1;
            }
            _ => {}
        }
        buf.clear();
    }
    CacheRecords { columns, count }
}

/// A cache value as a hash key: numbers by their bits, text by its case-folded form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKey {
    Missing,
    Number(u64),
    Text(String),
    Bool(bool),
    Error(String),
    Date(u64),
}

impl ValueKey {
    pub fn of(value: &CacheValue) -> Self {
        match value {
            CacheValue::Missing => Self::Missing,
            CacheValue::Number(n) => Self::Number((n + 0.0).to_bits()),
            CacheValue::Text(t) => Self::Text(t.to_lowercase()),
            CacheValue::Bool(b) => Self::Bool(*b),
            CacheValue::Error(e) => Self::Error(e.clone()),
            CacheValue::Date(n) => Self::Date((n + 0.0).to_bits()),
        }
    }
}

fn intern(column: &mut RecordColumn, map: &mut HashMap<ValueKey, u32>, value: CacheValue) -> u32 {
    let key = ValueKey::of(&value);
    if let Some(&index) = map.get(&key) {
        return index;
    }
    let index = column.values.len() as u32;
    column.values.push(value);
    map.insert(key, index);
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_go_both_ways() {
        assert_eq!(iso_serial("1900-01-01T00:00:00", false), Some(1.0));
        assert_eq!(iso_serial("1900-03-01T00:00:00", false), Some(61.0));
        assert_eq!(iso_serial("2014-01-01T00:00:00", false), Some(41640.0));
        assert_eq!(iso_serial("2014-01-01T12:00:00", false), Some(41640.5));
        assert_eq!(iso_serial("1904-01-02T00:00:00", true), Some(1.0));
        assert_eq!(serial_iso(41640.0, false), "2014-01-01T00:00:00");
        assert_eq!(serial_iso(41640.5, false), "2014-01-01T12:00:00");
        assert_eq!(serial_iso(1.0, false), "1900-01-01T00:00:00");
        assert_eq!(serial_iso(1.0, true), "1904-01-02T00:00:00");
    }

    #[test]
    fn a_cache_and_its_records_read() {
        let def = br#"<pivotCacheDefinition xmlns="m" xmlns:r="r" r:id="rId1" recordCount="3"><cacheSource type="worksheet"><worksheetSource ref="A1:B4" sheet="Data"/></cacheSource><cacheFields count="2"><cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="North"/><s v="South"/></sharedItems></cacheField><cacheField name="Sales" numFmtId="0"><sharedItems containsNumber="1"/></cacheField></cacheFields></pivotCacheDefinition>"#;
        let cache = read_cache_definition(def, false).unwrap();
        assert_eq!(cache.fields.len(), 2);
        assert!(cache.unsupported.is_none());
        assert_eq!(
            cache.source,
            CacheSource::Worksheet {
                sheet: Some("Data".into()),
                range: Some("A1:B4".into()),
                name: None
            }
        );
        let records = br#"<pivotCacheRecords count="3"><r><x v="0"/><n v="10"/></r><r><x v="1"/><n v="20"/></r><r><x v="0"/><n v="10"/></r></pivotCacheRecords>"#;
        let read = read_records(records, &cache, false);
        assert_eq!(read.count, 3);
        assert_eq!(read.columns[0].index, vec![0, 1, 0]);
        assert_eq!(read.columns[1].index, vec![0, 1, 0]);
        assert_eq!(
            read.columns[1].values,
            vec![CacheValue::Number(10.0), CacheValue::Number(20.0)]
        );
    }
}
