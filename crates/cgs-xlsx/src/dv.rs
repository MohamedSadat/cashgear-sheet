//! `dataValidations`, and the extended ones (x14) a worksheet's `extLst` can hold, which name lists
//! on other sheets.

use cgs_model::{DvKind, DvOp, DvStyle, Validation};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::cf::{parse_sqref, sqref_text};
use crate::scan::escape_attr;
use crate::xml::{attr, attr_bool, push_text};

/// The extension that holds extended validation.
const DV_EXT: &str = "{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}";

fn read(xml: &[u8], extended: bool) -> Vec<Validation> {
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut current: Option<(Validation, usize)> = None;
    let mut reading: Option<u8> = None;
    let mut text = String::new();
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf);
        match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element)) => {
                let empty = matches!(event, Ok(Event::Empty(_)));
                match element.local_name().as_ref() {
                    b"dataValidation" => {
                        let validation = Validation {
                            areas: attr(element, b"sqref")
                                .map(|s| parse_sqref(&s))
                                .unwrap_or_default(),
                            kind: DvKind::from_xml(&attr(element, b"type").unwrap_or_default()),
                            op: DvOp::from_xml(&attr(element, b"operator").unwrap_or_default()),
                            allow_blank: attr_bool(element, b"allowBlank").unwrap_or(false),
                            dropdown: !attr_bool(element, b"showDropDown").unwrap_or(false),
                            show_input: attr_bool(element, b"showInputMessage").unwrap_or(false),
                            input_title: attr(element, b"promptTitle").unwrap_or_default(),
                            input_text: attr(element, b"prompt").unwrap_or_default(),
                            show_error: attr_bool(element, b"showErrorMessage").unwrap_or(false),
                            error_title: attr(element, b"errorTitle").unwrap_or_default(),
                            error_text: attr(element, b"error").unwrap_or_default(),
                            style: DvStyle::from_xml(
                                &attr(element, b"errorStyle").unwrap_or_default(),
                            ),
                            extended,
                            ..Validation::default()
                        };
                        if empty {
                            let mut validation = validation;
                            if !extended {
                                validation.raw = Some(
                                    String::from_utf8_lossy(
                                        &xml[before..reader.buffer_position() as usize],
                                    )
                                    .into_owned(),
                                );
                            }
                            out.push(validation);
                        } else {
                            current = Some((validation, before));
                        }
                    }
                    b"formula1" if !empty => {
                        reading = Some(1);
                        text.clear();
                    }
                    b"formula2" if !empty => {
                        reading = Some(2);
                        text.clear();
                    }
                    b"sqref" if !empty => {
                        reading = Some(3);
                        text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"formula1" | b"formula2" | b"sqref" => {
                    if let (Some((validation, _)), Some(which)) = (current.as_mut(), reading.take())
                    {
                        let value = std::mem::take(&mut text);
                        match which {
                            1 => validation.formula1 = Some(value),
                            2 => validation.formula2 = Some(value),
                            _ => validation.areas = parse_sqref(&value),
                        }
                    }
                }
                // An extended one's formula sits in an `xm:f` inside `x14:formula1`.
                b"f" => {}
                b"dataValidation" => {
                    if let Some((mut validation, start)) = current.take() {
                        if !extended {
                            validation.raw = Some(
                                String::from_utf8_lossy(
                                    &xml[start..reader.buffer_position() as usize],
                                )
                                .into_owned(),
                            );
                        }
                        if !validation.areas.is_empty() {
                            out.push(validation);
                        }
                    }
                }
                _ => {}
            },
            Ok(other) => {
                if reading.is_some() {
                    push_text(other, &mut text);
                }
            }
        }
        buf.clear();
    }
    out
}

/// A sheet's validation: its own, then its extension's.
pub fn read_validations(classic: Option<&[u8]>, ext_list: Option<&[u8]>) -> Vec<Validation> {
    let mut out = classic.map(|xml| read(xml, false)).unwrap_or_default();
    if let Some(ext) = ext_list {
        let text = String::from_utf8_lossy(ext);
        if let Some(at) = text.find(DV_EXT) {
            out.extend(read(&ext[at..], true));
        }
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One rule's XML: the file's own while it is as read.
pub fn validation_xml(validation: &Validation) -> String {
    if let Some(raw) = &validation.raw {
        return raw.clone();
    }
    let mut out = String::from("<dataValidation");
    if validation.kind != DvKind::Any {
        out.push_str(&format!(" type=\"{}\"", validation.kind.xml()));
    }
    if validation.style != DvStyle::Stop {
        out.push_str(&format!(" errorStyle=\"{}\"", validation.style.xml()));
    }
    if validation.op != DvOp::Between
        && !matches!(validation.kind, DvKind::List | DvKind::Custom | DvKind::Any)
    {
        out.push_str(&format!(" operator=\"{}\"", validation.op.xml()));
    }
    let flag = |out: &mut String, name: &str, on: bool| {
        if on {
            out.push_str(&format!(" {name}=\"1\""));
        }
    };
    flag(&mut out, "allowBlank", validation.allow_blank);
    flag(
        &mut out,
        "showDropDown",
        validation.kind == DvKind::List && !validation.dropdown,
    );
    flag(&mut out, "showInputMessage", validation.show_input);
    flag(&mut out, "showErrorMessage", validation.show_error);
    for (name, text) in [
        ("errorTitle", &validation.error_title),
        ("error", &validation.error_text),
        ("promptTitle", &validation.input_title),
        ("prompt", &validation.input_text),
    ] {
        if !text.is_empty() {
            out.push_str(&format!(" {name}=\"{}\"", escape_attr(text)));
        }
    }
    out.push_str(&format!(" sqref=\"{}\"", sqref_text(&validation.areas)));
    let mut inner = String::new();
    if let Some(formula) = &validation.formula1 {
        inner.push_str(&format!("<formula1>{}</formula1>", escape_text(formula)));
    }
    if let Some(formula) = &validation.formula2 {
        inner.push_str(&format!("<formula2>{}</formula2>", escape_text(formula)));
    }
    if inner.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&inner);
        out.push_str("</dataValidation>");
    }
    out
}

/// The sheet's `dataValidations`, or nothing when it has none of its own.
pub fn validations_xml(list: &[Validation]) -> String {
    let own: Vec<&Validation> = list.iter().filter(|v| !v.extended).collect();
    if own.is_empty() {
        return String::new();
    }
    let mut out = format!("<dataValidations count=\"{}\">", own.len());
    for validation in own {
        out.push_str(&validation_xml(validation));
    }
    out.push_str("</dataValidations>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_back_the_same() {
        let xml = r#"<dataValidations count="2"><dataValidation type="list" allowBlank="1" showInputMessage="1" showErrorMessage="1" promptTitle="Pick" prompt="One of them" sqref="A1:A10"><formula1>"Yes,No"</formula1></dataValidation><dataValidation type="whole" errorStyle="warning" operator="greaterThan" showErrorMessage="1" sqref="B1 C3:C4"><formula1>5</formula1></dataValidation></dataValidations>"#;
        let mut list = read_validations(Some(xml.as_bytes()), None);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, DvKind::List);
        assert_eq!(list[0].formula1.as_deref(), Some("\"Yes,No\""));
        assert!(list[0].dropdown && list[0].allow_blank);
        assert_eq!(list[1].op, DvOp::GreaterThan);
        assert_eq!(list[1].style, DvStyle::Warning);
        assert_eq!(list[1].areas.len(), 2);
        assert_eq!(validations_xml(&list), xml);
        for validation in &mut list {
            validation.raw = None;
        }
        assert_eq!(validations_xml(&list), xml);
    }

    #[test]
    fn an_extended_list_on_another_sheet_is_read_too() {
        let ext = r#"<extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}" xmlns:x14="x"><x14:dataValidations count="1" xmlns:xm="m"><x14:dataValidation type="list" allowBlank="1"><x14:formula1><xm:f>Lists!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>D1:D5</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst>"#;
        let list = read_validations(None, Some(ext.as_bytes()));
        assert_eq!(list.len(), 1);
        assert!(list[0].extended);
        assert_eq!(list[0].formula1.as_deref(), Some("Lists!$A$1:$A$3"));
        assert_eq!(list[0].areas[0].left, 4);
    }
}
