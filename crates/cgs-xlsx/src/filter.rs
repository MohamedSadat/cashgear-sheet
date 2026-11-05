//! `autoFilter` and `sortState`, read into the model and written back as Excel writes them.

use cgs_model::{
    AutoFilter, DateGroup, FilterColumn, FilterCriteria, FilterOp, Rect, SortBy, SortKey, SortState,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::patch_sheet::cell_name;
use crate::scan::escape_attr;
use crate::xml::{attr, attr_bool, attr_f64, attr_u32, parse_rect};

/// A rectangle as a `ref` writes it.
pub fn rect_ref(rect: &Rect) -> String {
    if (rect.top, rect.left) == (rect.bottom, rect.right) {
        cell_name(rect.top, rect.left)
    } else {
        format!(
            "{}:{}",
            cell_name(rect.top, rect.left),
            cell_name(rect.bottom, rect.right)
        )
    }
}

fn read_sort_condition(element: &BytesStart<'_>) -> Option<SortKey> {
    let area = parse_rect(&attr(element, b"ref")?)?;
    let by = match attr(element, b"sortBy").as_deref() {
        Some("cellColor") => SortBy::CellColor(attr_u32(element, b"dxfId")),
        Some("fontColor") => SortBy::FontColor(attr_u32(element, b"dxfId")),
        Some("icon") => SortBy::Icon {
            set: attr(element, b"iconSet").unwrap_or_else(|| "3Arrows".to_string()),
            id: attr_u32(element, b"iconId").unwrap_or(0),
        },
        _ => SortBy::Value,
    };
    Some(SortKey {
        area,
        descending: attr_bool(element, b"descending").unwrap_or(false),
        by,
        custom_list: attr(element, b"customList"),
    })
}

/// Reads an `autoFilter` element, given its own XML.
pub fn read_auto_filter(xml: &[u8]) -> Option<AutoFilter> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut filter: Option<AutoFilter> = None;
    let mut column: Option<FilterColumn> = None;
    let mut sort: Option<SortState> = None;
    // Inside an element this doesn't read, whose XML is kept as it is.
    let mut depth = 0u32;
    let mut column_start = 0usize;
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf);
        let (element, empty) = match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => (Some(element.clone()), false),
            Ok(Event::Empty(element)) => (Some(element.clone()), true),
            Ok(Event::End(end)) => {
                depth = depth.saturating_sub(1);
                match end.local_name().as_ref() {
                    b"filterColumn" => {
                        if let (Some(filter), Some(mut done)) = (filter.as_mut(), column.take()) {
                            if let Some(FilterCriteria::Other(raw)) = &mut done.criteria {
                                let end = reader.buffer_position() as usize;
                                *raw =
                                    String::from_utf8_lossy(&xml[column_start..end]).into_owned();
                            }
                            filter.columns.push(done);
                        }
                    }
                    b"sortState" => {
                        if let (Some(filter), Some(done)) = (filter.as_mut(), sort.take()) {
                            filter.sort = Some(done);
                        }
                    }
                    _ => {}
                }
                buf.clear();
                continue;
            }
            _ => {
                buf.clear();
                continue;
            }
        };
        let Some(element) = element else { continue };
        if !empty {
            depth += 1;
        }
        let local = element.local_name().as_ref().to_vec();
        match local.as_slice() {
            b"autoFilter" if filter.is_none() => {
                filter = Some(AutoFilter::new(parse_rect(&attr(&element, b"ref")?)?));
            }
            b"filterColumn" => {
                column_start = before;
                let next = FilterColumn {
                    col_id: attr_u32(&element, b"colId").unwrap_or(0),
                    hidden_button: attr_bool(&element, b"hiddenButton").unwrap_or(false)
                        || attr_bool(&element, b"showButton") == Some(false),
                    criteria: None,
                };
                if empty {
                    if let Some(filter) = filter.as_mut() {
                        filter.columns.push(next);
                    }
                } else {
                    column = Some(next);
                }
            }
            b"filters" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Values {
                        values: Vec::new(),
                        blank: attr_bool(&element, b"blank").unwrap_or(false),
                        dates: Vec::new(),
                    });
                }
            }
            b"filter" => {
                if let Some(FilterCriteria::Values { values, .. }) =
                    column.as_mut().and_then(|column| column.criteria.as_mut())
                {
                    values.push(attr(&element, b"val").unwrap_or_default());
                }
            }
            b"dateGroupItem" => {
                if let Some(FilterCriteria::Values { dates, .. }) =
                    column.as_mut().and_then(|column| column.criteria.as_mut())
                {
                    let part = |name: &[u8]| attr_u32(&element, name).map(|value| value as u8);
                    let grouping = attr(&element, b"dateTimeGrouping").unwrap_or_default();
                    let level = ["year", "month", "day", "hour", "minute", "second"]
                        .iter()
                        .position(|name| *name == grouping)
                        .unwrap_or(0);
                    let wanted =
                        |at: usize, name: &[u8]| if level >= at { part(name) } else { None };
                    dates.push(DateGroup {
                        year: attr_u32(&element, b"year").unwrap_or(0) as u16,
                        month: wanted(1, b"month"),
                        day: wanted(2, b"day"),
                        hour: wanted(3, b"hour"),
                        minute: wanted(4, b"minute"),
                        second: wanted(5, b"second"),
                    });
                }
            }
            b"customFilters" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Custom {
                        and: attr_bool(&element, b"and").unwrap_or(false),
                        conditions: Vec::new(),
                    });
                }
            }
            b"customFilter" => {
                if let Some(FilterCriteria::Custom { conditions, .. }) =
                    column.as_mut().and_then(|column| column.criteria.as_mut())
                {
                    conditions.push((
                        FilterOp::from_xml(&attr(&element, b"operator").unwrap_or_default()),
                        attr(&element, b"val").unwrap_or_default(),
                    ));
                }
            }
            b"top10" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Top10 {
                        top: attr_bool(&element, b"top").unwrap_or(true),
                        percent: attr_bool(&element, b"percent").unwrap_or(false),
                        value: attr_f64(&element, b"val").unwrap_or(10.0),
                        filter_value: attr_f64(&element, b"filterVal"),
                    });
                }
            }
            b"dynamicFilter" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Dynamic {
                        kind: attr(&element, b"type").unwrap_or_default(),
                        value: attr_f64(&element, b"val"),
                        max_value: attr_f64(&element, b"maxVal"),
                    });
                }
            }
            b"colorFilter" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Color {
                        cell: attr_bool(&element, b"cellColor").unwrap_or(true),
                        dxf: attr_u32(&element, b"dxfId"),
                    });
                }
            }
            b"iconFilter" => {
                if let Some(column) = column.as_mut() {
                    column.criteria = Some(FilterCriteria::Icon {
                        set: attr(&element, b"iconSet").unwrap_or_default(),
                        id: attr_u32(&element, b"iconId"),
                    });
                }
            }
            b"sortState" => {
                let area = attr(&element, b"ref").and_then(|text| parse_rect(&text));
                let state = area.map(|area| SortState {
                    area,
                    case_sensitive: attr_bool(&element, b"caseSensitive").unwrap_or(false),
                    by_columns: attr_bool(&element, b"columnSort").unwrap_or(false),
                    keys: Vec::new(),
                });
                if empty {
                    if let (Some(filter), Some(state)) = (filter.as_mut(), state) {
                        filter.sort = Some(state);
                    }
                } else {
                    sort = state;
                }
            }
            b"sortCondition" => {
                if let (Some(sort), Some(key)) = (sort.as_mut(), read_sort_condition(&element)) {
                    sort.keys.push(key);
                }
            }
            _ => {
                // Anything else inside a column, such as an extension, keeps the column as it was.
                if let Some(column) = column.as_mut() {
                    if depth <= 3 && column.criteria.is_none() {
                        column.criteria = Some(FilterCriteria::Other(String::new()));
                    }
                }
            }
        }
        buf.clear();
    }
    filter
}

fn number(value: f64) -> String {
    crate::patch_sheet::number_text(value)
}

fn write_criteria(criteria: &FilterCriteria, out: &mut String) {
    match criteria {
        FilterCriteria::Values {
            values,
            blank,
            dates,
        } => {
            out.push_str("<filters");
            if *blank {
                out.push_str(" blank=\"1\"");
            }
            if values.is_empty() && dates.is_empty() {
                out.push_str("/>");
                return;
            }
            out.push('>');
            for value in values {
                out.push_str(&format!("<filter val=\"{}\"/>", escape_attr(value)));
            }
            for date in dates {
                let parts = [
                    ("month", date.month),
                    ("day", date.day),
                    ("hour", date.hour),
                    ("minute", date.minute),
                    ("second", date.second),
                ];
                out.push_str(&format!("<dateGroupItem year=\"{}\"", date.year));
                let mut grouping = "year";
                for (name, value) in parts {
                    match value {
                        Some(value) => {
                            out.push_str(&format!(" {name}=\"{value}\""));
                            grouping = name;
                        }
                        None => break,
                    }
                }
                out.push_str(&format!(" dateTimeGrouping=\"{grouping}\"/>"));
            }
            out.push_str("</filters>");
        }
        FilterCriteria::Custom { and, conditions } => {
            out.push_str("<customFilters");
            if *and {
                out.push_str(" and=\"1\"");
            }
            out.push('>');
            for (op, value) in conditions {
                out.push_str("<customFilter");
                if *op != FilterOp::Equal {
                    out.push_str(&format!(" operator=\"{}\"", op.xml()));
                }
                out.push_str(&format!(" val=\"{}\"/>", escape_attr(value)));
            }
            out.push_str("</customFilters>");
        }
        FilterCriteria::Top10 {
            top,
            percent,
            value,
            filter_value,
        } => {
            out.push_str("<top10");
            if !*top {
                out.push_str(" top=\"0\"");
            }
            if *percent {
                out.push_str(" percent=\"1\"");
            }
            out.push_str(&format!(" val=\"{}\"", number(*value)));
            if let Some(cut) = filter_value {
                out.push_str(&format!(" filterVal=\"{}\"", number(*cut)));
            }
            out.push_str("/>");
        }
        FilterCriteria::Dynamic {
            kind,
            value,
            max_value,
        } => {
            out.push_str(&format!("<dynamicFilter type=\"{}\"", escape_attr(kind)));
            if let Some(value) = value {
                out.push_str(&format!(" val=\"{}\"", number(*value)));
            }
            if let Some(value) = max_value {
                out.push_str(&format!(" maxVal=\"{}\"", number(*value)));
            }
            out.push_str("/>");
        }
        FilterCriteria::Color { cell, dxf } => {
            out.push_str("<colorFilter");
            if let Some(dxf) = dxf {
                out.push_str(&format!(" dxfId=\"{dxf}\""));
            }
            if !*cell {
                out.push_str(" cellColor=\"0\"");
            }
            out.push_str("/>");
        }
        FilterCriteria::Icon { set, id } => {
            out.push_str(&format!("<iconFilter iconSet=\"{}\"", escape_attr(set)));
            if let Some(id) = id {
                out.push_str(&format!(" iconId=\"{id}\""));
            }
            out.push_str("/>");
        }
        FilterCriteria::Other(_) => {}
    }
}

fn write_sort(sort: &SortState, out: &mut String) {
    out.push_str("<sortState");
    if sort.by_columns {
        out.push_str(" columnSort=\"1\"");
    }
    if sort.case_sensitive {
        out.push_str(" caseSensitive=\"1\"");
    }
    out.push_str(&format!(" ref=\"{}\"", rect_ref(&sort.area)));
    if sort.keys.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for key in &sort.keys {
        out.push_str("<sortCondition");
        if key.descending {
            out.push_str(" descending=\"1\"");
        }
        match &key.by {
            SortBy::Value => {}
            SortBy::CellColor(dxf) | SortBy::FontColor(dxf) => {
                let kind = if matches!(key.by, SortBy::CellColor(_)) {
                    "cellColor"
                } else {
                    "fontColor"
                };
                out.push_str(&format!(" sortBy=\"{kind}\""));
                if let Some(dxf) = dxf {
                    out.push_str(&format!(" dxfId=\"{dxf}\""));
                }
            }
            SortBy::Icon { set, id } => {
                out.push_str(&format!(
                    " sortBy=\"icon\" iconSet=\"{}\" iconId=\"{id}\"",
                    escape_attr(set)
                ));
            }
        }
        out.push_str(&format!(" ref=\"{}\"", rect_ref(&key.area)));
        if let Some(list) = &key.custom_list {
            out.push_str(&format!(" customList=\"{}\"", escape_attr(list)));
        }
        out.push_str("/>");
    }
    out.push_str("</sortState>");
}

/// A `sortState` on its own, as a worksheet holds one for a sort with no filter.
pub fn sort_state_xml(sort: &SortState) -> String {
    let mut out = String::new();
    write_sort(sort, &mut out);
    out
}

/// An `autoFilter` element as Excel writes it, with no namespace prefix.
pub fn auto_filter_xml(filter: &AutoFilter) -> String {
    let mut out = format!("<autoFilter ref=\"{}\"", rect_ref(&filter.range));
    let columns: Vec<&FilterColumn> = filter
        .columns
        .iter()
        .filter(|column| column.criteria.is_some() || column.hidden_button)
        .collect();
    if columns.is_empty() && filter.sort.is_none() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    for column in columns {
        if let Some(FilterCriteria::Other(raw)) = &column.criteria {
            if !raw.is_empty() {
                out.push_str(raw);
                continue;
            }
        }
        out.push_str(&format!("<filterColumn colId=\"{}\"", column.col_id));
        if column.hidden_button {
            out.push_str(" hiddenButton=\"1\"");
        }
        match &column.criteria {
            Some(criteria) if !matches!(criteria, FilterCriteria::Other(_)) => {
                out.push('>');
                write_criteria(criteria, &mut out);
                out.push_str("</filterColumn>");
            }
            _ => out.push_str("/>"),
        }
    }
    if let Some(sort) = &filter.sort {
        write_sort(sort, &mut out);
    }
    out.push_str("</autoFilter>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_read_and_write_back_the_same() {
        for xml in [
            r#"<autoFilter ref="A1:C9"/>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="1"><filters blank="1"><filter val="a &amp; b"/><filter val="2"/><dateGroupItem year="2020" month="3" dateTimeGrouping="month"/></filters></filterColumn></autoFilter>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="0"><customFilters and="1"><customFilter operator="greaterThan" val="5"/><customFilter operator="lessThanOrEqual" val="9"/></customFilters></filterColumn></autoFilter>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="2"><top10 top="0" percent="1" val="10" filterVal="3"/></filterColumn></autoFilter>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="2"><dynamicFilter type="aboveAverage" val="4.5"/></filterColumn></autoFilter>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="2"><colorFilter dxfId="3"/></filterColumn><filterColumn colId="1" hiddenButton="1"/></autoFilter>"#,
            r#"<autoFilter ref="A1:C9"><filterColumn colId="2"><iconFilter iconSet="3Arrows" iconId="1"/></filterColumn><sortState ref="A2:C9"><sortCondition descending="1" ref="B2:B9"/><sortCondition sortBy="cellColor" dxfId="2" ref="C2:C9"/></sortState></autoFilter>"#,
        ] {
            let filter = read_auto_filter(xml.as_bytes()).unwrap_or_else(|| panic!("{xml}"));
            assert_eq!(auto_filter_xml(&filter), xml);
        }
    }

    #[test]
    fn a_column_this_cant_read_is_kept_as_it_was() {
        let xml = r#"<autoFilter ref="A1:C9"><filterColumn colId="1"><extLst><ext uri="x"><y/></ext></extLst></filterColumn></autoFilter>"#;
        let filter = read_auto_filter(xml.as_bytes()).unwrap();
        assert_eq!(auto_filter_xml(&filter), xml);
    }
}
