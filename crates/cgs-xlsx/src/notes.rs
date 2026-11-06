//! Notes (the comments part and the VML shapes Excel draws them with) and hyperlinks: read into
//! the model, and written back as Excel 2016 writes them.

use std::collections::BTreeMap;

use cgs_model::{Hyperlink, Note};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::package::Package;
use crate::patch_sheet::cell_name;
use crate::scan::escape_attr;
use crate::workbook::read_relationships;
use crate::xml::{attr, parse_cell, parse_rect, push_text};

pub const COMMENTS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
pub const COMMENTS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
pub const VML_TYPE: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";
pub const VML_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing";
pub const LINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// A comments part's notes, by cell.
pub fn read_comments(xml: &[u8]) -> BTreeMap<(u32, u32), Note> {
    let mut out = BTreeMap::new();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut authors: Vec<String> = Vec::new();
    let mut in_author = false;
    let mut author = String::new();
    let mut current: Option<((u32, u32), usize)> = None;
    let mut in_text = false;
    let mut in_t = false;
    let mut in_phonetic = false;
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => match element.local_name().as_ref() {
                b"author" => {
                    in_author = true;
                    author.clear();
                }
                b"comment" => {
                    let at = attr(&element, b"ref").and_then(|r| parse_cell(r.as_bytes()));
                    let who = attr(&element, b"authorId")
                        .and_then(|id| id.parse::<usize>().ok())
                        .unwrap_or(0);
                    current = at.map(|at| (at, who));
                    text.clear();
                }
                b"text" => in_text = true,
                b"t" if in_text && !in_phonetic => in_t = true,
                b"rPh" => in_phonetic = true,
                _ => {}
            },
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"author" => {
                    in_author = false;
                    authors.push(std::mem::take(&mut author));
                }
                b"t" => in_t = false,
                b"rPh" => in_phonetic = false,
                b"text" => in_text = false,
                b"comment" => {
                    if let Some((at, who)) = current.take() {
                        out.insert(
                            at,
                            Note {
                                author: authors.get(who).cloned().unwrap_or_default(),
                                text: std::mem::take(&mut text),
                            },
                        );
                    }
                }
                _ => {}
            },
            Ok(event) => {
                if in_author {
                    push_text(&event, &mut author);
                } else if in_t {
                    push_text(&event, &mut text);
                }
            }
        }
        buf.clear();
    }
    out
}

/// The cells a threaded comments part covers.
pub fn read_threaded(xml: &[u8]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"threadedComment" =>
            {
                if let Some(at) = attr(&element, b"ref").and_then(|r| parse_cell(r.as_bytes())) {
                    if !out.contains(&at) {
                        out.push(at);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Notes by cell.
pub type Notes = BTreeMap<(u32, u32), Note>;

/// A sheet's notes, its threaded comments' cells, and the comments part, from its relationships.
pub fn read_sheet_notes(package: &Package, part: &str) -> (Notes, Vec<(u32, u32)>) {
    let mut notes = BTreeMap::new();
    let mut threaded = Vec::new();
    for (kind, target) in read_relationships(package, part).into_values() {
        if kind.ends_with("/comments") {
            if let Some(xml) = package.read(&target) {
                notes = read_comments(&xml);
            }
        } else if kind.ends_with("/threadedComment") {
            if let Some(xml) = package.read(&target) {
                threaded = read_threaded(&xml);
            }
        }
    }
    (notes, threaded)
}

/// A sheet's hyperlinks, from its `hyperlinks` element and relationships.
pub fn read_links(xml: &[u8], package: &Package, part: &str) -> Vec<Hyperlink> {
    let rels = read_relationships(package, part);
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"hyperlink" =>
            {
                let Some(area) = attr(&element, b"ref").and_then(|r| parse_rect(&r)) else {
                    continue;
                };
                let mut target = None;
                for (key, value) in element
                    .attributes()
                    .flatten()
                    .map(|a| (a.key.as_ref().to_vec(), a.value.to_vec()))
                {
                    if key.ends_with(b":id") || key == b"id" {
                        let id = String::from_utf8_lossy(&value).into_owned();
                        target = rels.get(&id).map(|(_, target)| target.clone());
                    }
                }
                out.push(Hyperlink {
                    area,
                    target,
                    location: attr(&element, b"location"),
                    tooltip: attr(&element, b"tooltip"),
                    display: attr(&element, b"display"),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A comments part for these notes, as Excel writes one: the author's name in bold, then the text.
pub fn comments_xml(notes: &BTreeMap<(u32, u32), Note>) -> String {
    let mut authors: Vec<&str> = Vec::new();
    for note in notes.values() {
        if !authors.contains(&note.author.as_str()) {
            authors.push(&note.author);
        }
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<comments xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><authors>",
    );
    for author in &authors {
        out.push_str(&format!("<author>{}</author>", escape_text(author)));
    }
    out.push_str("</authors><commentList>");
    for (&(row, col), note) in notes {
        let id = authors.iter().position(|a| *a == note.author).unwrap_or(0);
        out.push_str(&format!(
            "<comment ref=\"{}\" authorId=\"{id}\"><text><r><rPr><sz val=\"9\"/><color indexed=\"81\"/><rFont val=\"Tahoma\"/><family val=\"2\"/></rPr><t xml:space=\"preserve\">{}</t></r></text></comment>",
            cell_name(row, col),
            escape_text(&note.text)
        ));
    }
    out.push_str("</commentList></comments>");
    out
}

/// One note's shape, hidden as Excel's are until hovered, beside its cell.
fn note_shape(row: u32, col: u32, id: u32) -> String {
    let (r, c) = (row - 1, col - 1);
    format!(
        "<v:shape id=\"_x0000_s{id}\" type=\"#_x0000_t202\" style='position:absolute;margin-left:59.25pt;margin-top:1.5pt;width:108pt;height:59.25pt;z-index:{z};visibility:hidden' fillcolor=\"#ffffe1\" o:insetmode=\"auto\"><v:fill color2=\"#ffffe1\"/><v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/><v:textbox style='mso-direction-alt:auto'><div style='text-align:left'></div></v:textbox><x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{}, 15, {}, 2, {}, 15, {}, 16</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{r}</x:Row><x:Column>{c}</x:Column></x:ClientData></v:shape>",
        c + 1,
        r.saturating_sub(1),
        c + 3,
        r + 3,
        z = id - 1024,
    )
}

/// A VML drawing for these notes. An existing drawing keeps every shape that isn't a note's,
/// such as a form control's, and its notes' shapes are made again.
pub fn vml_xml(notes: &BTreeMap<(u32, u32), Note>, existing: Option<&[u8]>) -> String {
    let head = "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\"><o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"1\"/></o:shapelayout>";
    let shapetype = "<v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>";
    let mut kept = String::new();
    let mut have_type = false;
    let mut prefix = head.to_string();
    if let Some(existing) = existing {
        let text = String::from_utf8_lossy(existing).into_owned();
        // Everything up to the first shape stays, and shapes that aren't notes stay.
        let first = text.find("<v:shape").unwrap_or(text.len());
        prefix = text[..first].to_string();
        have_type = text.contains("_x0000_t202");
        let mut rest = &text[first..];
        while let Some(start) = rest.find("<v:shape") {
            let tail = &rest[start..];
            // A shapetype is a separate element and is kept as it is.
            let is_type = tail.starts_with("<v:shapetype");
            let end_tag = if is_type {
                "</v:shapetype>"
            } else {
                "</v:shape>"
            };
            let open_end = tail.find('>').map_or(tail.len(), |at| at + 1);
            let end = if tail[..open_end].ends_with("/>") {
                open_end
            } else {
                tail.find(end_tag)
                    .map_or(tail.len(), |at| at + end_tag.len())
            };
            let piece = &tail[..end];
            if is_type || !piece.contains("ObjectType=\"Note\"") {
                kept.push_str(piece);
            }
            rest = &tail[end..];
        }
        if let Some(close) = prefix.rfind("</xml>") {
            prefix.truncate(close);
        }
    }
    let mut out = prefix;
    if !have_type {
        out.push_str(shapetype);
    }
    out.push_str(&kept);
    // New shapes' ids come after any a kept shape has.
    let mut next = 1025;
    let mut rest = kept.as_str();
    while let Some(at) = rest.find("_x0000_s") {
        let digits: String = rest[at + 8..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(id) = digits.parse::<u32>() {
            next = next.max(id + 1);
        }
        rest = &rest[at + 8..];
    }
    for (index, &(row, col)) in notes.keys().enumerate() {
        out.push_str(&note_shape(row, col, next + index as u32));
    }
    out.push_str("</xml>");
    out
}

/// A sheet's `hyperlinks`, with each external target's relationship id, or nothing for none.
pub fn hyperlinks_xml(links: &[(Hyperlink, Option<String>)]) -> String {
    if links.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "<hyperlinks xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
    );
    for (link, id) in links {
        let area = link.area;
        let reference = if (area.top, area.left) == (area.bottom, area.right) {
            cell_name(area.top, area.left)
        } else {
            format!(
                "{}:{}",
                cell_name(area.top, area.left),
                cell_name(area.bottom, area.right)
            )
        };
        out.push_str(&format!("<hyperlink ref=\"{reference}\""));
        if let Some(id) = id {
            out.push_str(&format!(" r:id=\"{}\"", escape_attr(id)));
        }
        if let Some(location) = &link.location {
            out.push_str(&format!(" location=\"{}\"", escape_attr(location)));
        }
        if let Some(tooltip) = &link.tooltip {
            out.push_str(&format!(" tooltip=\"{}\"", escape_attr(tooltip)));
        }
        if let Some(display) = &link.display {
            out.push_str(&format!(" display=\"{}\"", escape_attr(display)));
        }
        out.push_str("/>");
    }
    out.push_str("</hyperlinks>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_read_back_from_what_is_written() {
        let mut notes = BTreeMap::new();
        notes.insert(
            (2, 3),
            Note {
                author: "Ann".to_string(),
                text: "Check <this> & that".to_string(),
            },
        );
        let read = read_comments(comments_xml(&notes).as_bytes());
        assert_eq!(read, notes);
        let vml = vml_xml(&notes, None);
        assert!(
            vml.contains("<x:Row>1</x:Row><x:Column>2</x:Column>"),
            "{vml}"
        );
    }

    #[test]
    fn a_drawing_keeps_its_controls_and_makes_its_notes_again() {
        let existing = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml"><o:shapelayout/><v:shapetype id="_x0000_t201"/><v:shape id="_x0000_s1025"><x:ClientData ObjectType="Button"/></v:shape><v:shape id="_x0000_s1026"><x:ClientData ObjectType="Note"><x:Row>9</x:Row></x:ClientData></v:shape></xml>"#;
        let mut notes = BTreeMap::new();
        notes.insert((1, 1), Note::default());
        let vml = vml_xml(&notes, Some(existing.as_bytes()));
        assert!(vml.contains("ObjectType=\"Button\""), "{vml}");
        assert!(!vml.contains("<x:Row>9</x:Row>"), "{vml}");
        assert!(vml.contains("<x:Row>0</x:Row>"), "{vml}");
    }
}
