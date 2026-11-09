//! The parts read once when a file opens: relationships, `workbook.xml`, tables, drawings.

use std::collections::HashMap;

use cgs_model::{
    Anchor, AnchorPoint, Book, DefinedName, Drawing, DrawingKind, SheetInfo, SheetKind, SheetState,
    Table, TableColumnInfo, TableStyle, TotalsFunction,
};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::package::Package;
use crate::styles::{read_styles, read_theme};
use crate::xml::{
    attr, attr_bool, attr_u32, parse_rect, push_text, rels_path, resolve_target, unescape_x,
};
use crate::ReadError;

/// A part's relationships: id to (type, resolved target).
pub fn read_relationships(package: &Package, part: &str) -> HashMap<String, (String, String)> {
    match package.read(&rels_path(part)) {
        Some(xml) => relationships_in(&xml, part),
        None => HashMap::new(),
    }
}

/// The relationships a part's rels hold, from their XML.
pub fn relationships_in(xml: &[u8], part: &str) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(kind), Some(target)) = (
                    attr(&element, b"Id"),
                    attr(&element, b"Type"),
                    attr(&element, b"Target"),
                ) {
                    // External links name a URL, not a part.
                    let external = attr(&element, b"TargetMode").as_deref() == Some("External");
                    let target = if external {
                        target
                    } else {
                        resolve_target(part, &target.replace('\\', "/"))
                    };
                    out.insert(id, (kind, target));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The main part's path, from the package's relationships, with the usual place as a fallback.
fn workbook_part(package: &Package) -> Option<String> {
    let found = read_relationships(package, "")
        .into_values()
        .find(|(kind, _)| kind.ends_with("/officeDocument"))
        .map(|(_, target)| target)
        .filter(|target| package.part(target).is_some());
    found.or_else(|| {
        package
            .parts()
            .iter()
            .map(|part| part.name.replace('\\', "/"))
            .find(|name| {
                let lower = name.to_ascii_lowercase();
                lower == "xl/workbook.xml"
                    || lower == "workbook.xml"
                    || lower.ends_with("/workbook.xml")
            })
    })
}

/// The shared strings part, if the workbook has one.
pub fn shared_strings_part(package: &Package) -> Option<String> {
    let part = workbook_part(package)?;
    read_relationships(package, &part)
        .into_values()
        .find(|(kind, _)| kind.ends_with("/sharedStrings"))
        .map(|(_, target)| target)
        .filter(|target| package.part(target).is_some())
}

/// The styles part, if the workbook has one.
pub fn styles_part(package: &Package) -> Option<String> {
    let part = workbook_part(package)?;
    read_relationships(package, &part)
        .into_values()
        .find(|(kind, _)| kind.ends_with("/styles"))
        .map(|(_, target)| target)
        .filter(|target| package.part(target).is_some())
}

/// Reads everything but the cells. `Book::sheets` holds names and parts, not yet any cells.
pub fn read_workbook(package: &Package) -> Result<Book, ReadError> {
    let part = workbook_part(package).ok_or(ReadError::NotAWorkbook)?;
    let xml = package.read(&part).ok_or(ReadError::NotAWorkbook)?;
    let rels = read_relationships(package, &part);
    let mut book = Book::default();
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    let mut name: Option<DefinedName> = None;
    let mut name_text = String::new();
    let mut saw_sheets = false;
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Eof) => break,
            Err(error) => {
                if saw_sheets {
                    break;
                }
                return Err(ReadError::Damaged(error.to_string()));
            }
            Ok(Event::Start(ref element) | Event::Empty(ref element)) => {
                match element.local_name().as_ref() {
                    b"sheets" => saw_sheets = true,
                    b"sheet" => {
                        let sheet = sheet_entry(
                            attr(element, b"name").unwrap_or_default(),
                            attr(element, b"id"),
                            attr(element, b"state"),
                            &rels,
                        );
                        book.sheets.push(sheet);
                    }
                    b"workbookProtection" => {
                        book.protection =
                            Some(crate::protection::read_workbook_protection(element));
                    }
                    b"workbookPr" => {
                        book.date1904 = attr_bool(element, b"date1904").unwrap_or(false);
                    }
                    b"calcPr" => {
                        book.full_calc_on_load =
                            attr_bool(element, b"fullCalcOnLoad").unwrap_or(false);
                        book.manual_calc = attr(element, b"calcMode").as_deref() == Some("manual");
                    }
                    b"workbookView" => {
                        book.active_sheet = attr_u32(element, b"activeTab").unwrap_or(0);
                    }
                    b"definedName" => {
                        name = Some(DefinedName {
                            name: attr(element, b"name").unwrap_or_default(),
                            formula: String::new(),
                            local_sheet: attr_u32(element, b"localSheetId"),
                            hidden: attr_bool(element, b"hidden").unwrap_or(false),
                        });
                        name_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref element)) if element.local_name().as_ref() == b"definedName" => {
                if let Some(mut done) = name.take() {
                    done.formula = std::mem::take(&mut name_text);
                    book.names.push(done);
                }
            }
            Ok(ref other) => {
                if name.is_some() {
                    push_text(other, &mut name_text);
                }
            }
        }
        buf.clear();
    }
    if book.sheets.is_empty() {
        return Err(ReadError::NotAWorkbook);
    }
    if book.active_sheet as usize >= book.sheets.len() {
        book.active_sheet = 0;
    }

    if let Some((_, target)) = rels.values().find(|(kind, _)| kind.ends_with("/styles")) {
        if let Some(xml) = package.read(target) {
            book.styles = read_styles(&xml);
        }
    }
    if let Some((_, target)) = rels.values().find(|(kind, _)| kind.ends_with("/theme")) {
        if let Some(xml) = package.read(target) {
            book.styles.colors.theme = read_theme(&xml);
        }
    }
    read_tables(package, &mut book);
    crate::pivot::read_pivots(package, &part, &mut book);
    crate::read_chart_sheets(package, &mut book);
    Ok(book)
}

fn sheet_entry(
    name: String,
    id: Option<String>,
    state: Option<String>,
    rels: &HashMap<String, (String, String)>,
) -> SheetInfo {
    let state = match state.as_deref() {
        Some("hidden") => SheetState::Hidden,
        Some("veryHidden") => SheetState::VeryHidden,
        _ => SheetState::Visible,
    };
    let (kind, part) = match id.as_ref().and_then(|id| rels.get(id)) {
        Some((rel, target)) if rel.ends_with("/worksheet") => {
            (SheetKind::Worksheet, Some(target.clone()))
        }
        Some((rel, target)) if rel.ends_with("/chartsheet") => {
            let mut info = SheetInfo::new(name, state, SheetKind::Chart, None);
            info.chart_sheet = Some(target.clone());
            return info;
        }
        _ => (SheetKind::Other, None),
    };
    SheetInfo::new(name, state, kind, part)
}

/// Tables come before any cell, because a name or a formula can read one.
fn read_tables(package: &Package, book: &mut Book) {
    let mut tables = Vec::new();
    for (index, sheet) in book.sheets.iter().enumerate() {
        let Some(part) = &sheet.part else { continue };
        for (kind, target) in read_relationships(package, part).into_values() {
            if !kind.ends_with("/table") {
                continue;
            }
            if let Some(mut table) = package
                .read(&target)
                .and_then(|xml| read_table(&xml, index as u32))
            {
                table.part = Some(target.clone());
                tables.push(table);
            }
        }
    }
    book.tables = tables;
}

fn read_table(xml: &[u8], sheet: u32) -> Option<Table> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut table: Option<Table> = None;
    // Which formula of a column is being read: 1 calculated, 2 totals.
    let mut formula = 0u8;
    let mut text = String::new();
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event @ (Event::Start(_) | Event::Empty(_))) => {
                let empty = matches!(event, Event::Empty(_));
                let (Event::Start(element) | Event::Empty(element)) = event else {
                    unreachable!()
                };
                match element.local_name().as_ref() {
                    // Only the root: an extension can hold an <x14:table> too.
                    b"table" if table.is_none() => {
                        let range = parse_rect(&attr(&element, b"ref")?)?;
                        let name = attr(&element, b"displayName")
                            .or_else(|| attr(&element, b"name"))
                            .unwrap_or_default();
                        let mut next = Table::new(name, sheet, range, Vec::new());
                        next.header_rows = attr_u32(&element, b"headerRowCount").unwrap_or(1);
                        next.totals_rows = attr_u32(&element, b"totalsRowCount").unwrap_or(0);
                        next.id = attr_u32(&element, b"id").unwrap_or(0);
                        next.edited = false;
                        next.style = TableStyle {
                            name: None,
                            first_column: false,
                            last_column: false,
                            row_stripes: false,
                            column_stripes: false,
                        };
                        table = Some(next);
                    }
                    b"autoFilter" if table.as_ref().is_some_and(|t| t.filter.is_none()) => {
                        // Its own XML, read by itself.
                        let start = before;
                        let end = if empty {
                            reader.buffer_position() as usize
                        } else {
                            let _ = reader.read_to_end_into(element.name(), &mut Vec::new());
                            reader.buffer_position() as usize
                        };
                        if let (Some(table), Some(filter)) = (
                            table.as_mut(),
                            xml.get(start..end)
                                .and_then(crate::filter::read_auto_filter),
                        ) {
                            table.filter = Some(filter);
                        }
                    }
                    b"tableColumn" => {
                        if let (Some(table), Some(name)) = (table.as_mut(), attr(&element, b"name"))
                        {
                            table.columns.push(unescape_x(&name).into_owned());
                            table.column_info.push(TableColumnInfo {
                                totals_function: attr(&element, b"totalsRowFunction")
                                    .and_then(|text| TotalsFunction::from_xml(&text)),
                                totals_label: attr(&element, b"totalsRowLabel"),
                                totals_formula: None,
                                calculated: None,
                            });
                        }
                    }
                    b"calculatedColumnFormula" if !empty => {
                        formula = 1;
                        text.clear();
                    }
                    b"totalsRowFormula" if !empty => {
                        formula = 2;
                        text.clear();
                    }
                    b"tableStyleInfo" => {
                        if let Some(table) = table.as_mut() {
                            table.style = TableStyle {
                                name: attr(&element, b"name"),
                                first_column: attr_bool(&element, b"showFirstColumn")
                                    .unwrap_or(false),
                                last_column: attr_bool(&element, b"showLastColumn")
                                    .unwrap_or(false),
                                row_stripes: attr_bool(&element, b"showRowStripes")
                                    .unwrap_or(false),
                                column_stripes: attr_bool(&element, b"showColumnStripes")
                                    .unwrap_or(false),
                            };
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(end)) => {
                let local = end.local_name();
                if formula != 0
                    && matches!(
                        local.as_ref(),
                        b"calculatedColumnFormula" | b"totalsRowFormula"
                    )
                {
                    if let Some(info) = table
                        .as_mut()
                        .and_then(|table| table.column_info.last_mut())
                    {
                        let read = Some(std::mem::take(&mut text));
                        if formula == 1 {
                            info.calculated = read;
                        } else {
                            info.totals_formula = read;
                        }
                    }
                    formula = 0;
                }
            }
            Ok(event) => {
                if formula != 0 {
                    push_text(&event, &mut text);
                }
            }
        }
        buf.clear();
    }
    table
}

#[derive(Default, Clone, Copy)]
struct Marker {
    col: Option<i64>,
    col_off: i64,
    row: Option<i64>,
    row_off: i64,
}

impl Marker {
    fn point(self) -> AnchorPoint {
        AnchorPoint {
            row: self.row.unwrap_or(0) as u32 + 1,
            col: self.col.unwrap_or(0) as u32 + 1,
            row_off: self.row_off,
            col_off: self.col_off,
        }
    }
}

/// An anchor of a drawing part, with the relationships of what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingRead {
    pub drawing: Drawing,
    /// `c:chart r:id` or `cx:chart r:id`.
    pub chart_rel: Option<String>,
    /// `a:blip r:embed`.
    pub image_rel: Option<String>,
}

/// Reads the anchors of a drawing part, each with what it holds.
pub fn read_drawing(xml: &[u8]) -> Vec<Drawing> {
    read_drawing_parts(xml)
        .into_iter()
        .map(|read| read.drawing)
        .collect()
}

/// Reads the anchors of a drawing part, each with what it holds and where that is.
pub fn read_drawing_parts(xml: &[u8]) -> Vec<DrawingRead> {
    let mut chart_rel: Option<String> = None;
    let mut image_rel: Option<String> = None;
    let mut id = 0u32;
    let mut description = String::new();
    let (mut hidden, mut prints) = (false, true);
    // Anchors and `mc:AlternateContent` blocks at the top, counted as the writer counts them. A
    // block's choice is the drawing; its fallback is the same drawing for older programs.
    let mut anchors = 0u32;
    let mut alternate = 0u32;
    let mut fallback = false;
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // Inside an anchor: which of `from` and `to` we are in, and the field being read.
    let mut kind: Option<DrawingKind> = None;
    let mut name = String::new();
    let mut from = Marker::default();
    let mut to = Marker::default();
    let mut in_to = false;
    let mut field = 0u8;
    let mut text = String::new();
    let mut ext = (0i64, 0i64);
    let mut pos = (0i64, 0i64);
    let mut anchor_kind = 0u8;
    let mut depth = 0u32;
    loop {
        let event = reader.read_event_into(&mut buf);
        let (element, empty) = match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => (Some(element), false),
            Ok(Event::Empty(element)) => (Some(element), true),
            Ok(Event::End(end)) => {
                match end.local_name().as_ref() {
                    b"col" | b"row" | b"colOff" | b"rowOff" => {
                        let value: i64 = text.trim().parse().unwrap_or(0);
                        let marker = if in_to { &mut to } else { &mut from };
                        match field {
                            1 => marker.col = Some(value),
                            2 => marker.col_off = value,
                            3 => marker.row = Some(value),
                            4 => marker.row_off = value,
                            _ => {}
                        }
                        field = 0;
                        text.clear();
                    }
                    b"to" => in_to = false,
                    b"Fallback" if anchor_kind == 0 => fallback = false,
                    b"AlternateContent" if anchor_kind == 0 => {
                        alternate = alternate.saturating_sub(1);
                        if alternate == 0 {
                            anchors += 1;
                        }
                    }
                    b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" if fallback => {
                        kind = None;
                        name.clear();
                        (hidden, prints) = (false, true);
                        anchor_kind = 0;
                        depth = 0;
                        chart_rel = None;
                        image_rel = None;
                    }
                    b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                        let anchor = match anchor_kind {
                            1 => Anchor::TwoCell {
                                from: from.point(),
                                to: to.point(),
                            },
                            2 => Anchor::OneCell {
                                from: from.point(),
                                width: ext.0,
                                height: ext.1,
                            },
                            _ => Anchor::Absolute {
                                x: pos.0,
                                y: pos.1,
                                width: ext.0,
                                height: ext.1,
                            },
                        };
                        if let Some(kind) = kind.take() {
                            let mut drawing = Drawing::new(kind, std::mem::take(&mut name), anchor);
                            drawing.id = id;
                            drawing.description = std::mem::take(&mut description);
                            drawing.source = Some(anchors);
                            drawing.hidden = hidden;
                            drawing.prints = prints;
                            out.push(DrawingRead {
                                drawing,
                                chart_rel: chart_rel.take(),
                                image_rel: image_rel.take(),
                            });
                        }
                        if alternate == 0 {
                            anchors += 1;
                        }
                        id = 0;
                        chart_rel = None;
                        image_rel = None;
                        anchor_kind = 0;
                        depth = 0;
                        (hidden, prints) = (false, true);
                    }
                    b"grpSp" | b"graphicFrame" | b"pic" | b"sp" | b"cxnSp" => {
                        depth = depth.saturating_sub(1);
                    }
                    _ => {}
                }
                buf.clear();
                continue;
            }
            Ok(other) => {
                if field != 0 {
                    push_text(other, &mut text);
                }
                buf.clear();
                continue;
            }
        };
        let Some(element) = element else { continue };
        match element.local_name().as_ref() {
            b"AlternateContent" if anchor_kind == 0 && !empty => alternate += 1,
            b"Fallback" if anchor_kind == 0 && alternate > 0 && !empty => fallback = true,
            b"twoCellAnchor" => {
                anchor_kind = 1;
                (from, to) = (Marker::default(), Marker::default());
            }
            b"oneCellAnchor" => {
                anchor_kind = 2;
                from = Marker::default();
            }
            b"absoluteAnchor" => anchor_kind = 3,
            b"to" => in_to = true,
            b"from" => in_to = false,
            b"col" if !empty => field = 1,
            b"colOff" if !empty => field = 2,
            b"row" if !empty => field = 3,
            b"rowOff" if !empty => field = 4,
            b"ext" | b"pos" if anchor_kind != 0 && depth == 0 => {
                let is_ext = element.local_name().as_ref() == b"ext";
                let read = |a: &[u8], b: &[u8]| -> (i64, i64) {
                    let number = |key: &[u8]| {
                        attr(element, key)
                            .and_then(|text| text.parse().ok())
                            .unwrap_or(0)
                    };
                    (number(a), number(b))
                };
                if is_ext {
                    ext = read(b"cx", b"cy");
                } else {
                    pos = read(b"x", b"y");
                }
            }
            b"grpSp" | b"graphicFrame" | b"pic" | b"sp" | b"cxnSp" => {
                if depth == 0 && kind.is_none() {
                    kind = Some(match element.local_name().as_ref() {
                        b"graphicFrame" => DrawingKind::Other,
                        b"pic" => DrawingKind::Picture,
                        b"grpSp" => DrawingKind::Group,
                        _ => DrawingKind::Shape,
                    });
                }
                if !empty {
                    depth += 1;
                }
            }
            b"cNvPr" if name.is_empty() && kind.is_some() => {
                name = attr(element, b"name").unwrap_or_default();
                id = attr(element, b"id")
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0);
                description = attr(element, b"descr").unwrap_or_default();
                // A control or an embedded object keeps a hidden stand-in in `mc:Choice` while
                // the real one is drawn from VML, and that one still prints.
                hidden = alternate == 0
                    && matches!(attr(element, b"hidden").as_deref(), Some("1" | "true"));
            }
            b"clientData" if anchor_kind != 0 => {
                prints = !matches!(
                    attr(element, b"fPrintsWithSheet").as_deref(),
                    Some("0" | "false")
                );
            }
            b"chart" if kind == Some(DrawingKind::Other) && depth == 1 => {
                kind = Some(DrawingKind::Chart);
                chart_rel = attr(element, b"id");
            }
            b"chart" if kind == Some(DrawingKind::Other) => kind = Some(DrawingKind::Chart),
            b"chartex" => kind = Some(DrawingKind::Chart),
            b"blip" if kind == Some(DrawingKind::Picture) && depth == 1 && image_rel.is_none() => {
                image_rel = attr(element, b"embed");
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alternate_block_is_one_drawing() {
        let xml = br#"<xdr:wsDr xmlns:xdr="x" xmlns:a="y" xmlns:mc="m">
          <mc:AlternateContent><mc:Choice Requires="cx1"><xdr:twoCellAnchor>
            <xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
            <xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
            <xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Waterfall"/></xdr:nvGraphicFramePr>
              <a:graphic><a:graphicData uri="u"><cx:chart xmlns:cx="c" r:id="rId1"/></a:graphicData></a:graphic>
            </xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></mc:Choice>
          <mc:Fallback><xdr:twoCellAnchor>
            <xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
            <xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
            <xdr:sp><xdr:nvSpPr><xdr:cNvPr id="0" name=""/></xdr:nvSpPr></xdr:sp><xdr:clientData/></xdr:twoCellAnchor></mc:Fallback></mc:AlternateContent>
          <xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
            <xdr:ext cx="1" cy="1"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Logo"/></xdr:nvPicPr>
            <xdr:blipFill><a:blip r:embed="rId2"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>
        </xdr:wsDr>"#;
        let read = read_drawing_parts(xml);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].drawing.kind, DrawingKind::Chart);
        assert_eq!(read[0].chart_rel.as_deref(), Some("rId1"));
        assert_eq!(read[0].drawing.source, Some(0));
        assert_eq!(read[1].image_rel.as_deref(), Some("rId2"));
        assert_eq!(read[1].drawing.source, Some(1));
    }

    #[test]
    fn hidden_and_unprinted_drawings_are_marked() {
        let anchor = |name: &str, nv: &str, client: &str| {
            format!(
                r#"<xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="2" name="{name}"{nv}/></xdr:nvSpPr></xdr:sp><xdr:clientData{client}/></xdr:twoCellAnchor>"#
            )
        };
        let xml = format!(
            r#"<xdr:wsDr xmlns:xdr="x" xmlns:mc="m">{}{}{}<mc:AlternateContent><mc:Choice Requires="a14">{}</mc:Choice><mc:Fallback/></mc:AlternateContent></xdr:wsDr>"#,
            anchor("Shown", "", ""),
            anchor("Hidden", r#" hidden="1""#, ""),
            anchor("Kept off paper", "", r#" fPrintsWithSheet="0""#),
            anchor("Check Box 1", r#" hidden="1""#, ""),
        );
        let read = read_drawing_parts(xml.as_bytes());
        let flags: Vec<_> = read
            .iter()
            .map(|r| (r.drawing.name.as_str(), r.drawing.hidden, r.drawing.prints))
            .collect();
        assert_eq!(
            flags,
            [
                ("Shown", false, true),
                ("Hidden", true, true),
                ("Kept off paper", false, false),
                // A control's stand-in, which Excel prints from its VML.
                ("Check Box 1", false, true),
            ]
        );
    }

    #[test]
    fn reads_a_drawing_of_each_kind() {
        let xml = br#"<xdr:wsDr xmlns:xdr="x" xmlns:a="y" xmlns:c="z">
          <xdr:twoCellAnchor editAs="oneCell">
            <xdr:from><xdr:col>1</xdr:col><xdr:colOff>10</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>20</xdr:rowOff></xdr:from>
            <xdr:to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>9</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
            <xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Sales chart"/></xdr:nvGraphicFramePr>
              <xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm>
              <a:graphic><a:graphicData uri="u"><c:chart r:id="rId1"/></a:graphicData></a:graphic>
            </xdr:graphicFrame><xdr:clientData/>
          </xdr:twoCellAnchor>
          <xdr:oneCellAnchor>
            <xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
            <xdr:ext cx="1000" cy="2000"/>
            <xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Logo"/></xdr:nvPicPr></xdr:pic><xdr:clientData/>
          </xdr:oneCellAnchor>
          <xdr:absoluteAnchor><xdr:pos x="5" y="6"/><xdr:ext cx="7" cy="8"/>
            <xdr:sp><xdr:nvSpPr><xdr:cNvPr id="4" name="Box"/></xdr:nvSpPr></xdr:sp><xdr:clientData/>
          </xdr:absoluteAnchor>
        </xdr:wsDr>"#;
        let drawings = read_drawing(xml);
        assert_eq!(drawings.len(), 3);
        assert_eq!(drawings[0].kind, DrawingKind::Chart);
        assert_eq!(drawings[0].name, "Sales chart");
        assert_eq!(
            drawings[0].anchor,
            Anchor::TwoCell {
                from: AnchorPoint {
                    row: 3,
                    col: 2,
                    row_off: 20,
                    col_off: 10
                },
                to: AnchorPoint {
                    row: 10,
                    col: 6,
                    row_off: 0,
                    col_off: 0
                },
            }
        );
        assert_eq!(drawings[1].kind, DrawingKind::Picture);
        assert!(matches!(
            drawings[1].anchor,
            Anchor::OneCell {
                width: 1000,
                height: 2000,
                ..
            }
        ));
        assert_eq!(drawings[2].kind, DrawingKind::Shape);
        assert_eq!(
            drawings[2].anchor,
            Anchor::Absolute {
                x: 5,
                y: 6,
                width: 7,
                height: 8
            }
        );
    }
}
