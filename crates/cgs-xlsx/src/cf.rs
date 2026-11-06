//! `conditionalFormatting`, and the extended (x14) rules a worksheet's `extLst` holds: read into
//! the model, and written back as Excel writes them.

use cgs_model::{
    CfBlock, CfKind, CfOperator, CfRule, CfValueKind, Cfvo, Color, DataBar, Dxf, IconSet, Rect,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::filter::rect_ref;
use crate::scan::escape_attr;
use crate::styles::parse_color;
use crate::xml::{attr, attr_bool, attr_u32, push_text};

/// The extension that holds extended conditional formats.
pub const CF_EXT: &str = "{78C0D931-6437-407d-A8EE-F0AAD7539E65}";
/// The extension a classic data bar names its extended partner in.
const BAR_EXT: &str = "{B025F937-C7B1-47D3-B67F-A62EFF666E3E}";
const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";

/// `A1:B2 C3 D:D 5:5`, as areas.
pub fn parse_sqref(text: &str) -> Vec<Rect> {
    text.split_ascii_whitespace()
        .filter_map(|part| {
            let part = part.trim_matches('$');
            if let Some(rect) = crate::xml::parse_rect(part) {
                return Some(rect);
            }
            // Whole columns or rows.
            let (first, last) = part.split_once(':')?;
            let clean = |text: &str| text.replace('$', "");
            let (first, last) = (clean(first), clean(last));
            if let (Ok(top), Ok(bottom)) = (first.parse::<u32>(), last.parse::<u32>()) {
                return Some(Rect {
                    top: top.min(bottom),
                    left: 1,
                    bottom: top.max(bottom),
                    right: cgs_model::MAX_COLS,
                });
            }
            let left = cgs_refs::col_from_letters(&first)? as u32;
            let right = cgs_refs::col_from_letters(&last)? as u32;
            Some(Rect {
                top: 1,
                left: left.min(right),
                bottom: cgs_model::MAX_ROWS,
                right: left.max(right),
            })
        })
        .collect()
}

pub fn sqref_text(areas: &[Rect]) -> String {
    areas.iter().map(rect_ref).collect::<Vec<_>>().join(" ")
}

fn cfvo(element: &BytesStart<'_>) -> Cfvo {
    Cfvo {
        kind: CfValueKind::from_xml(&attr(element, b"type").unwrap_or_default()),
        value: attr(element, b"val"),
        gte: attr_bool(element, b"gte").unwrap_or(true),
    }
}

fn kind_of(element: &BytesStart<'_>) -> CfKind {
    let text = attr(element, b"text").unwrap_or_default();
    match attr(element, b"type").as_deref().unwrap_or_default() {
        "cellIs" => attr(element, b"operator")
            .and_then(|op| CfOperator::from_xml(&op))
            .map_or(CfKind::Other(String::new()), CfKind::CellIs),
        "expression" => CfKind::Expression,
        "containsText" => CfKind::ContainsText(text),
        "notContainsText" => CfKind::NotContainsText(text),
        "beginsWith" => CfKind::BeginsWith(text),
        "endsWith" => CfKind::EndsWith(text),
        "containsBlanks" => CfKind::ContainsBlanks,
        "notContainsBlanks" => CfKind::NotContainsBlanks,
        "containsErrors" => CfKind::ContainsErrors,
        "notContainsErrors" => CfKind::NotContainsErrors,
        "timePeriod" => CfKind::TimePeriod(attr(element, b"timePeriod").unwrap_or_default()),
        "top10" => CfKind::Top10 {
            bottom: attr_bool(element, b"bottom").unwrap_or(false),
            percent: attr_bool(element, b"percent").unwrap_or(false),
            rank: attr_u32(element, b"rank").unwrap_or(10),
        },
        "aboveAverage" => CfKind::AboveAverage {
            above: attr_bool(element, b"aboveAverage").unwrap_or(true),
            equal: attr_bool(element, b"equalAverage").unwrap_or(false),
            std_dev: attr_u32(element, b"stdDev").unwrap_or(0),
        },
        "duplicateValues" => CfKind::Duplicates,
        "uniqueValues" => CfKind::Uniques,
        "colorScale" => CfKind::ColorScale {
            cfvos: Vec::new(),
            colors: Vec::new(),
        },
        "dataBar" => CfKind::DataBar(Box::new(DataBar {
            min: Cfvo::new(CfValueKind::Min, None),
            max: Cfvo::new(CfValueKind::Max, None),
            color: Color::AUTO,
            show_value: true,
            min_length: 10,
            max_length: 90,
            gradient: true,
            border: None,
            negative_color: None,
            axis_color: None,
            right_to_left: false,
        })),
        "iconSet" => CfKind::IconSet(Box::new(IconSet {
            set: "3TrafficLights1".to_string(),
            cfvos: Vec::new(),
            reverse: false,
            show_value: true,
            custom: Vec::new(),
        })),
        other => CfKind::Other(other.to_string()),
    }
}

/// Reads one block's rules, classic or extended, from its element's XML.
fn read_block(xml: &[u8], extended: bool) -> Option<CfBlock> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut block = CfBlock {
        areas: Vec::new(),
        rules: Vec::new(),
        extended,
        raw: (!extended).then(|| String::from_utf8_lossy(xml).into_owned()),
    };
    let mut rule: Option<CfRule> = None;
    // How many thresholds the rule being read has.
    let mut thresholds = 0usize;
    let mut text = String::new();
    let mut reading: Option<&'static str> = None;
    // An extended rule's own format, read as a dxf of its own.
    let mut dxf_start: Option<usize> = None;
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf);
        let (element, empty) = match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => (Some(element.clone()), false),
            Ok(Event::Empty(element)) => (Some(element.clone()), true),
            Ok(Event::End(end)) => {
                match end.local_name().as_ref() {
                    b"cfRule" => {
                        if let Some(done) = rule.take() {
                            block.rules.push(done);
                        }
                    }
                    b"formula" | b"f" => {
                        match reading {
                            Some("formula") => {
                                if let Some(rule) = rule.as_mut() {
                                    rule.formulas.push(std::mem::take(&mut text));
                                }
                                reading = None;
                            }
                            // An extended threshold keeps its value in an `xm:f`.
                            Some("cfvo") => {
                                let value = Some(std::mem::take(&mut text));
                                match rule.as_mut().map(|rule| &mut rule.kind) {
                                    Some(CfKind::ColorScale { cfvos, .. }) => {
                                        if let Some(last) = cfvos.last_mut() {
                                            last.value = value;
                                        }
                                    }
                                    Some(CfKind::IconSet(set)) => {
                                        if let Some(last) = set.cfvos.last_mut() {
                                            last.value = value;
                                        }
                                    }
                                    Some(CfKind::DataBar(bar)) => {
                                        if thresholds <= 1 {
                                            bar.min.value = value;
                                        } else {
                                            bar.max.value = value;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                    b"cfvo" => {
                        if reading == Some("cfvo") {
                            reading = None;
                        }
                    }
                    b"sqref" => {
                        if reading == Some("sqref") {
                            block.areas = parse_sqref(&text);
                            text.clear();
                        }
                        reading = None;
                    }
                    b"id" => {
                        if reading == Some("id") {
                            if let Some(rule) = rule.as_mut() {
                                rule.x14_id = Some(std::mem::take(&mut text));
                            }
                        }
                        reading = None;
                    }
                    b"dxf" => {
                        if let (Some(start), Some(rule)) = (dxf_start.take(), rule.as_mut()) {
                            let end = reader.buffer_position() as usize;
                            let inner = format!(
                                "<styleSheet><dxfs>{}</dxfs></styleSheet>",
                                String::from_utf8_lossy(&xml[start..end])
                                    .replace("x14:", "")
                                    .replace("xm:", "")
                            );
                            rule.own_dxf = crate::dxf::read_dxf_tables(inner.as_bytes())
                                .dxfs
                                .into_iter()
                                .next();
                        }
                    }
                    _ => {}
                }
                buf.clear();
                continue;
            }
            Ok(other) => {
                if reading.is_some() {
                    push_text(other, &mut text);
                }
                buf.clear();
                continue;
            }
        };
        let Some(element) = element else { continue };
        match element.local_name().as_ref() {
            b"conditionalFormatting" => {
                if let Some(sqref) = attr(&element, b"sqref") {
                    block.areas = parse_sqref(&sqref);
                }
            }
            b"cfRule" if dxf_start.is_none() => {
                let next = CfRule {
                    kind: kind_of(&element),
                    priority: attr_u32(&element, b"priority").map_or(1, |p| p as i32),
                    stop: attr_bool(&element, b"stopIfTrue").unwrap_or(false),
                    dxf: attr_u32(&element, b"dxfId"),
                    own_dxf: None,
                    formulas: Vec::new(),
                    x14_id: attr(&element, b"id"),
                };
                thresholds = 0;
                if empty {
                    block.rules.push(next);
                } else {
                    rule = Some(next);
                }
            }
            b"formula" | b"f" if !empty && dxf_start.is_none() => {
                if reading != Some("cfvo") {
                    reading = Some("formula");
                }
                text.clear();
            }
            b"sqref" if !empty => {
                reading = Some("sqref");
                text.clear();
            }
            b"id" if !empty => {
                reading = Some("id");
                text.clear();
            }
            b"dxf" if !empty => dxf_start = Some(before),
            b"cfvo" if dxf_start.is_none() => {
                let value = cfvo(&element);
                thresholds += 1;
                match rule.as_mut().map(|rule| &mut rule.kind) {
                    Some(CfKind::ColorScale { cfvos, .. }) => cfvos.push(value),
                    // The first is the low end and the second the high.
                    Some(CfKind::DataBar(bar)) if thresholds == 1 => bar.min = value,
                    Some(CfKind::DataBar(bar)) => bar.max = value,
                    Some(CfKind::IconSet(set)) => set.cfvos.push(value),
                    _ => {}
                }
                // An extended cfvo can hold its value as an `xm:f`.
                if !empty {
                    reading = Some("cfvo");
                    text.clear();
                }
            }
            b"color" | b"fillColor" if dxf_start.is_none() => {
                let color = parse_color(&element);
                match rule.as_mut().map(|rule| &mut rule.kind) {
                    Some(CfKind::ColorScale { colors, .. }) => colors.push(color),
                    Some(CfKind::DataBar(bar)) => bar.color = color,
                    _ => {}
                }
            }
            b"borderColor" => {
                if let Some(CfKind::DataBar(bar)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    bar.border = Some(parse_color(&element));
                }
            }
            b"negativeFillColor" => {
                if let Some(CfKind::DataBar(bar)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    bar.negative_color = Some(parse_color(&element));
                }
            }
            b"axisColor" => {
                if let Some(CfKind::DataBar(bar)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    bar.axis_color = Some(parse_color(&element));
                }
            }
            b"dataBar" => {
                if let Some(CfKind::DataBar(bar)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    bar.show_value = attr_bool(&element, b"showValue").unwrap_or(true);
                    if let Some(length) = attr_u32(&element, b"minLength") {
                        bar.min_length = length;
                    }
                    if let Some(length) = attr_u32(&element, b"maxLength") {
                        bar.max_length = length;
                    }
                    if extended {
                        bar.gradient = attr_bool(&element, b"gradient").unwrap_or(true);
                        bar.right_to_left =
                            attr(&element, b"direction").as_deref() == Some("rightToLeft");
                        if attr_bool(&element, b"border").unwrap_or(false) && bar.border.is_none() {
                            bar.border = Some(Color::AUTO);
                        }
                    }
                }
            }
            b"iconSet" => {
                if let Some(CfKind::IconSet(set)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    if let Some(name) = attr(&element, b"iconSet") {
                        set.set = name;
                    }
                    set.reverse = attr_bool(&element, b"reverse").unwrap_or(false);
                    set.show_value = attr_bool(&element, b"showValue").unwrap_or(true);
                }
            }
            b"cfIcon" => {
                if let Some(CfKind::IconSet(set)) = rule.as_mut().map(|rule| &mut rule.kind) {
                    let name = attr(&element, b"iconSet").unwrap_or_default();
                    let id = attr_u32(&element, b"iconId").unwrap_or(0);
                    set.custom.push((name != "NoIcons").then_some((name, id)));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    (!block.areas.is_empty()).then_some(block)
}

/// Every block a sheet has: the classic ones, each with its extended partner's details, and the
/// extended ones of their own.
pub fn read_cf(classic: &[&[u8]], ext_list: Option<&[u8]>) -> Vec<CfBlock> {
    let mut blocks: Vec<CfBlock> = classic
        .iter()
        .filter_map(|xml| read_block(xml, false))
        .collect();
    let Some(ext_list) = ext_list else {
        return blocks;
    };
    for xml in extended_blocks(ext_list) {
        let Some(block) = read_block(&xml, true) else {
            continue;
        };
        // A rule whose id a classic data bar names fills that bar in.
        let mut own = Vec::new();
        for rule in block.rules {
            let partner = rule.x14_id.as_ref().and_then(|id| {
                blocks
                    .iter_mut()
                    .flat_map(|block| block.rules.iter_mut())
                    .find(|classic| classic.x14_id.as_ref() == Some(id))
            });
            match (partner, &rule.kind) {
                (Some(classic), CfKind::DataBar(extended)) => {
                    if let CfKind::DataBar(bar) = &mut classic.kind {
                        bar.gradient = extended.gradient;
                        bar.border = extended.border;
                        bar.negative_color = extended.negative_color;
                        bar.axis_color = extended.axis_color;
                        bar.right_to_left = extended.right_to_left;
                        bar.min_length = extended.min_length;
                        bar.max_length = extended.max_length;
                        if matches!(extended.min.kind, CfValueKind::AutoMin) {
                            bar.min.kind = CfValueKind::AutoMin;
                        }
                        if matches!(extended.max.kind, CfValueKind::AutoMax) {
                            bar.max.kind = CfValueKind::AutoMax;
                        }
                    }
                }
                _ => own.push(rule),
            }
        }
        if !own.is_empty() {
            blocks.push(CfBlock {
                areas: block.areas,
                rules: own,
                extended: true,
                raw: None,
            });
        }
    }
    blocks
}

/// The `x14:conditionalFormatting` elements of a worksheet's `extLst`, each as its own XML.
fn extended_blocks(ext_list: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(ext_list);
    let mut buf = Vec::new();
    let mut depth = 0u32;
    let mut start = None;
    let mut in_cf_ext = false;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => {
                let local = element.local_name().as_ref().to_vec();
                if local == b"ext" {
                    in_cf_ext = attr(&element, b"uri").as_deref() == Some(CF_EXT);
                }
                if in_cf_ext && local == b"conditionalFormatting" && start.is_none() {
                    start = Some((before, depth));
                }
                depth += 1;
            }
            Ok(Event::End(element)) => {
                depth -= 1;
                if element.local_name().as_ref() == b"conditionalFormatting" {
                    if let Some((at, level)) = start {
                        if level == depth {
                            out.push(ext_list[at..reader.buffer_position() as usize].to_vec());
                            start = None;
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

fn color_xml(name: &str, color: &Color) -> String {
    match color.kind {
        cgs_model::ColorKind::Rgb(rgb) => format!("<{name} rgb=\"FF{rgb:06X}\"/>"),
        cgs_model::ColorKind::Theme(theme) if color.tint != 0.0 => {
            let tint = (f64::from(color.tint) * 32767.0).trunc() / 32767.0;
            format!("<{name} theme=\"{theme}\" tint=\"{tint}\"/>")
        }
        cgs_model::ColorKind::Theme(theme) => format!("<{name} theme=\"{theme}\"/>"),
        cgs_model::ColorKind::Indexed(index) => format!("<{name} indexed=\"{index}\"/>"),
        cgs_model::ColorKind::Auto => format!("<{name} auto=\"1\"/>"),
    }
}

fn cfvo_xml(value: &Cfvo, prefix: &str) -> String {
    // A classic cfvo has no automatic ends: those are the extended partner's.
    let kind = match (prefix, value.kind) {
        ("", CfValueKind::AutoMin) => CfValueKind::Min,
        ("", CfValueKind::AutoMax) => CfValueKind::Max,
        (_, kind) => kind,
    };
    let mut out = format!("<{prefix}cfvo type=\"{}\"", kind.xml());
    if !value.gte {
        out.push_str(" gte=\"0\"");
    }
    match (&value.value, prefix) {
        (Some(text), "") => {
            out.push_str(&format!(" val=\"{}\"/>", escape_attr(text)));
        }
        (Some(text), _) => {
            out.push_str(&format!(
                "><xm:f>{}</xm:f></{prefix}cfvo>",
                escape_text(text)
            ));
        }
        (None, _) => out.push_str("/>"),
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A classic rule's XML.
fn rule_xml(rule: &CfRule) -> String {
    let mut attrs = String::new();
    let mut inner = String::new();
    let kind = match &rule.kind {
        CfKind::CellIs(op) => {
            attrs.push_str(&format!(" operator=\"{}\"", op.xml()));
            "cellIs"
        }
        CfKind::Expression => "expression",
        CfKind::ContainsText(text) => {
            attrs.push_str(&format!(
                " operator=\"containsText\" text=\"{}\"",
                escape_attr(text)
            ));
            "containsText"
        }
        CfKind::NotContainsText(text) => {
            attrs.push_str(&format!(
                " operator=\"notContains\" text=\"{}\"",
                escape_attr(text)
            ));
            "notContainsText"
        }
        CfKind::BeginsWith(text) => {
            attrs.push_str(&format!(
                " operator=\"beginsWith\" text=\"{}\"",
                escape_attr(text)
            ));
            "beginsWith"
        }
        CfKind::EndsWith(text) => {
            attrs.push_str(&format!(
                " operator=\"endsWith\" text=\"{}\"",
                escape_attr(text)
            ));
            "endsWith"
        }
        CfKind::ContainsBlanks => "containsBlanks",
        CfKind::NotContainsBlanks => "notContainsBlanks",
        CfKind::ContainsErrors => "containsErrors",
        CfKind::NotContainsErrors => "notContainsErrors",
        CfKind::TimePeriod(period) => {
            attrs.push_str(&format!(" timePeriod=\"{}\"", escape_attr(period)));
            "timePeriod"
        }
        CfKind::Top10 {
            bottom,
            percent,
            rank,
        } => {
            if *percent {
                attrs.push_str(" percent=\"1\"");
            }
            if *bottom {
                attrs.push_str(" bottom=\"1\"");
            }
            attrs.push_str(&format!(" rank=\"{rank}\""));
            "top10"
        }
        CfKind::AboveAverage {
            above,
            equal,
            std_dev,
        } => {
            if !above {
                attrs.push_str(" aboveAverage=\"0\"");
            }
            if *equal {
                attrs.push_str(" equalAverage=\"1\"");
            }
            if *std_dev > 0 {
                attrs.push_str(&format!(" stdDev=\"{std_dev}\""));
            }
            "aboveAverage"
        }
        CfKind::Duplicates => "duplicateValues",
        CfKind::Uniques => "uniqueValues",
        CfKind::ColorScale { cfvos, colors } => {
            inner.push_str("<colorScale>");
            for value in cfvos {
                inner.push_str(&cfvo_xml(value, ""));
            }
            for color in colors {
                inner.push_str(&color_xml("color", color));
            }
            inner.push_str("</colorScale>");
            "colorScale"
        }
        CfKind::DataBar(bar) => {
            inner.push_str("<dataBar");
            if !bar.show_value {
                inner.push_str(" showValue=\"0\"");
            }
            inner.push('>');
            inner.push_str(&cfvo_xml(&bar.min, ""));
            inner.push_str(&cfvo_xml(&bar.max, ""));
            inner.push_str(&color_xml("color", &bar.color));
            inner.push_str("</dataBar>");
            if let Some(id) = &rule.x14_id {
                inner.push_str(&format!(
                    "<extLst><ext uri=\"{BAR_EXT}\" xmlns:x14=\"{X14}\"><x14:id>{}</x14:id></ext></extLst>",
                    escape_text(id)
                ));
            }
            "dataBar"
        }
        CfKind::IconSet(set) => {
            inner.push_str(&format!("<iconSet iconSet=\"{}\"", escape_attr(&set.set)));
            if set.reverse {
                inner.push_str(" reverse=\"1\"");
            }
            if !set.show_value {
                inner.push_str(" showValue=\"0\"");
            }
            inner.push('>');
            for value in &set.cfvos {
                inner.push_str(&cfvo_xml(value, ""));
            }
            inner.push_str("</iconSet>");
            "iconSet"
        }
        CfKind::Other(kind) => kind.as_str(),
    };
    let mut out = format!("<cfRule type=\"{kind}\"");
    if let Some(dxf) = rule.dxf {
        out.push_str(&format!(" dxfId=\"{dxf}\""));
    }
    out.push_str(&format!(" priority=\"{}\"", rule.priority.max(1)));
    if rule.stop {
        out.push_str(" stopIfTrue=\"1\"");
    }
    out.push_str(&attrs);
    let formulas: String = rule
        .formulas
        .iter()
        .map(|formula| format!("<formula>{}</formula>", escape_text(formula)))
        .collect();
    if formulas.is_empty() && inner.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&formulas);
        out.push_str(&inner);
        out.push_str("</cfRule>");
    }
    out
}

/// A classic block's XML: the file's own while it is as read.
pub fn block_xml(block: &CfBlock) -> String {
    if let Some(raw) = &block.raw {
        return raw.clone();
    }
    let mut out = format!(
        "<conditionalFormatting sqref=\"{}\">",
        sqref_text(&block.areas)
    );
    for rule in &block.rules {
        out.push_str(&rule_xml(rule));
    }
    out.push_str("</conditionalFormatting>");
    out
}

/// The extended partner Excel 2016 writes for a classic data bar.
pub fn bar_partner_xml(block: &CfBlock, rule: &CfRule) -> Option<String> {
    let CfKind::DataBar(bar) = &rule.kind else {
        return None;
    };
    let id = rule.x14_id.as_ref()?;
    let mut out = format!(
        "<x14:conditionalFormatting xmlns:xm=\"{XM}\"><x14:cfRule type=\"dataBar\" id=\"{}\"><x14:dataBar minLength=\"{}\" maxLength=\"{}\"",
        escape_attr(id),
        bar.min_length,
        bar.max_length
    );
    if !bar.gradient {
        out.push_str(" gradient=\"0\"");
    }
    if bar.border.is_some() {
        out.push_str(" border=\"1\"");
    }
    out.push_str(" negativeBarBorderColorSameAsPositive=\"0\">");
    out.push_str(&cfvo_xml(&bar.min, "x14:"));
    out.push_str(&cfvo_xml(&bar.max, "x14:"));
    if let Some(color) = &bar.border {
        out.push_str(&color_xml("x14:borderColor", color));
    }
    let negative = bar.negative_color.unwrap_or(Color {
        kind: cgs_model::ColorKind::Rgb(0xFF0000),
        tint: 0.0,
    });
    out.push_str(&color_xml("x14:negativeFillColor", &negative));
    if bar.border.is_some() {
        out.push_str(&color_xml("x14:negativeBorderColor", &negative));
    }
    let axis = bar.axis_color.unwrap_or(Color {
        kind: cgs_model::ColorKind::Rgb(0x000000),
        tint: 0.0,
    });
    out.push_str(&color_xml("x14:axisColor", &axis));
    out.push_str(&format!(
        "</x14:dataBar></x14:cfRule><xm:sqref>{}</xm:sqref></x14:conditionalFormatting>",
        sqref_text(&block.areas)
    ));
    Some(out)
}

/// The rules' extension as it should stand: the file's extended blocks of their own, then the
/// partners of the classic data bars. Empty when there are none.
pub fn cf_ext_xml(blocks: &[CfBlock], kept: &[Vec<u8>]) -> String {
    let mut inner = String::new();
    for xml in kept {
        inner.push_str(&String::from_utf8_lossy(xml));
    }
    for block in blocks.iter().filter(|block| !block.extended) {
        for rule in &block.rules {
            if let Some(partner) = bar_partner_xml(block, rule) {
                inner.push_str(&partner);
            }
        }
    }
    if inner.is_empty() {
        return String::new();
    }
    format!(
        "<ext uri=\"{CF_EXT}\" xmlns:x14=\"{X14}\"><x14:conditionalFormattings>{inner}</x14:conditionalFormattings></ext>"
    )
}

/// The extended blocks of an `extLst` that no classic rule pairs with, kept as their XML.
pub fn unpaired_extended(ext_list: &[u8], blocks: &[CfBlock]) -> Vec<Vec<u8>> {
    let ids: Vec<&String> = blocks
        .iter()
        .filter(|block| !block.extended)
        .flat_map(|block| block.rules.iter())
        .filter_map(|rule| rule.x14_id.as_ref())
        .collect();
    extended_blocks(ext_list)
        .into_iter()
        .filter(|xml| {
            let text = String::from_utf8_lossy(xml);
            !ids.iter().any(|id| text.contains(id.as_str()))
        })
        .collect()
}

/// A worksheet's `extLst` with its rules' extension replaced, and everything else in it as it was.
pub fn ext_list_with(ext_list: Option<&[u8]>, cf_ext: &str) -> String {
    let Some(ext_list) = ext_list else {
        return if cf_ext.is_empty() {
            String::new()
        } else {
            format!("<extLst>{cf_ext}</extLst>")
        };
    };
    // Each `ext` of the list, by its place.
    let mut reader = Reader::from_reader(ext_list);
    let mut buf = Vec::new();
    let mut depth = 0u32;
    let mut kept = String::new();
    let mut start: Option<(usize, bool)> = None;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => {
                if depth == 1 && element.local_name().as_ref() == b"ext" {
                    let ours = attr(&element, b"uri").as_deref() == Some(CF_EXT);
                    start = Some((before, ours));
                }
                depth += 1;
            }
            Ok(Event::Empty(element)) => {
                if depth == 1 && element.local_name().as_ref() == b"ext" {
                    kept.push_str(&String::from_utf8_lossy(
                        &ext_list[before..reader.buffer_position() as usize],
                    ));
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 1 {
                    if let Some((at, ours)) = start.take() {
                        if !ours {
                            kept.push_str(&String::from_utf8_lossy(
                                &ext_list[at..reader.buffer_position() as usize],
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    kept.push_str(cf_ext);
    if kept.is_empty() {
        String::new()
    } else {
        format!("<extLst>{kept}</extLst>")
    }
}

/// An empty differential format, for rules that only draw.
pub fn no_dxf() -> Dxf {
    Dxf::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIC: &str = r#"<conditionalFormatting sqref="A1:A10 C1"><cfRule type="cellIs" dxfId="0" priority="2" operator="greaterThan"><formula>5</formula></cfRule><cfRule type="dataBar" priority="1"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar><extLst><ext uri="{B025F937-C7B1-47D3-B67F-A62EFF666E3E}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:id>{00000000-0001-0000-0000-000000000000}</x14:id></ext></extLst></cfRule></conditionalFormatting>"#;
    const EXT: &str = r#"<extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:conditionalFormattings><x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="dataBar" id="{00000000-0001-0000-0000-000000000000}"><x14:dataBar minLength="0" maxLength="100" gradient="0" border="1"><x14:cfvo type="autoMin"/><x14:cfvo type="autoMax"/><x14:borderColor rgb="FF638EC6"/><x14:negativeFillColor rgb="FFFF0000"/><x14:axisColor rgb="FF000000"/></x14:dataBar></x14:cfRule><xm:sqref>A1:A10 C1</xm:sqref></x14:conditionalFormatting><x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="iconSet" priority="3" id="{2}"><x14:iconSet iconSet="3Stars"><x14:cfvo type="percent"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>33</xm:f></x14:cfvo><x14:cfvo type="percent"><xm:f>67</xm:f></x14:cfvo></x14:iconSet></x14:cfRule><xm:sqref>B1:B5</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext><ext uri="{other}"><y/></ext></extLst>"#;

    #[test]
    fn reads_classic_rules_and_their_extended_partners() {
        let blocks = read_cf(&[CLASSIC.as_bytes()], Some(EXT.as_bytes()));
        assert_eq!(blocks.len(), 2);
        let classic = &blocks[0];
        assert_eq!(classic.areas.len(), 2);
        assert_eq!(
            classic.rules[0].kind,
            CfKind::CellIs(CfOperator::GreaterThan)
        );
        assert_eq!(classic.rules[0].formulas, ["5"]);
        let CfKind::DataBar(bar) = &classic.rules[1].kind else {
            panic!("a data bar");
        };
        assert!(!bar.gradient && bar.border.is_some() && bar.min_length == 0);
        assert_eq!(bar.min.kind, CfValueKind::AutoMin);
        let extended = &blocks[1];
        assert!(extended.extended);
        let CfKind::IconSet(set) = &extended.rules[0].kind else {
            panic!("an icon set");
        };
        assert_eq!(set.set, "3Stars");
        assert_eq!(set.cfvos[1].value.as_deref(), Some("33"));
        assert_eq!(block_xml(classic), CLASSIC);
    }

    #[test]
    fn a_new_bar_writes_its_partner_and_other_extensions_stay() {
        let mut blocks = read_cf(&[CLASSIC.as_bytes()], Some(EXT.as_bytes()));
        blocks[0].raw = None;
        let kept = unpaired_extended(EXT.as_bytes(), &blocks);
        assert_eq!(kept.len(), 1);
        let ext = cf_ext_xml(&blocks, &kept);
        assert!(
            ext.contains("3Stars") && ext.contains("gradient=\"0\""),
            "{ext}"
        );
        let list = ext_list_with(Some(EXT.as_bytes()), &ext);
        assert!(
            list.starts_with("<extLst><ext uri=\"{other}\"><y/></ext><ext uri=\"{78C0D931"),
            "{list}"
        );
        let written = block_xml(&blocks[0]);
        assert!(written.contains("<cfRule type=\"dataBar\" priority=\"1\"><dataBar><cfvo type=\"min\"/><cfvo type=\"max\"/><color rgb=\"FF638EC6\"/></dataBar><extLst>"), "{written}");
    }
}
