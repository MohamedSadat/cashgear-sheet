//! Page setup: `printOptions`, `pageMargins`, `pageSetup`, `headerFooter`, `rowBreaks`,
//! `colBreaks` and `sheetPr`'s `pageSetUpPr`. Reading fills the model, and writing edits the
//! file's own tags, so attributes this app doesn't know, such as `pageSetup`'s link to the
//! printer's settings, stay as they were.

use std::io;

use cgs_model::{
    HeaderFooter, Margins, Orientation, PageBreak, PageOrder, PageSetup, PrintComments, PrintErrors,
};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::patch_sheet::NewElement;
use crate::scan::{closed, edit_tag, escape_text, opened};
use crate::structure::{parse_tree, write_tree, Element, Node};
use crate::xml::{attr, attr_bool, attr_f64, attr_u32, push_text};

/// The worksheet elements page setup is kept in, in the schema's order.
pub const PAGE_ELEMENTS: &[&str] = &[
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
];

pub fn is_page_element(name: &[u8]) -> bool {
    PAGE_ELEMENTS.iter().any(|known| known.as_bytes() == name)
}

/// Reads one of `PAGE_ELEMENTS`, as the file holds it, into the sheet's page setup.
pub fn read_page_element(name: &[u8], xml: &[u8], page: &mut PageSetup) {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut text_of: Option<Vec<u8>> = None;
    let mut text = String::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let local = element.local_name().as_ref().to_vec();
                match local.as_slice() {
                    b"pageSetup" if name == b"pageSetup" => {
                        if let Some(paper) = attr_u32(element, b"paperSize") {
                            page.paper = paper;
                        }
                        if let Some(scale) = attr_u32(element, b"scale") {
                            page.scale = scale.clamp(10, 400);
                        }
                        if let Some(width) = attr_u32(element, b"fitToWidth") {
                            page.fit_width = width;
                        }
                        if let Some(height) = attr_u32(element, b"fitToHeight") {
                            page.fit_height = height;
                        }
                        page.orientation = match attr(element, b"orientation").as_deref() {
                            Some("portrait") => Orientation::Portrait,
                            Some("landscape") => Orientation::Landscape,
                            _ => Orientation::Default,
                        };
                        page.page_order = match attr(element, b"pageOrder").as_deref() {
                            Some("overThenDown") => PageOrder::OverThenDown,
                            _ => PageOrder::DownThenOver,
                        };
                        page.black_and_white =
                            attr_bool(element, b"blackAndWhite").unwrap_or(false);
                        page.draft = attr_bool(element, b"draft").unwrap_or(false);
                        page.comments = match attr(element, b"cellComments").as_deref() {
                            Some("atEnd") => PrintComments::AtEnd,
                            Some("asDisplayed") => PrintComments::AsDisplayed,
                            _ => PrintComments::None,
                        };
                        page.errors = match attr(element, b"errors").as_deref() {
                            Some("blank") => PrintErrors::Blank,
                            Some("dash") => PrintErrors::Dash,
                            Some("NA") => PrintErrors::Na,
                            _ => PrintErrors::Displayed,
                        };
                        let first = attr(element, b"firstPageNumber")
                            .and_then(|text| text.trim().parse::<i64>().ok());
                        page.first_page =
                            if attr_bool(element, b"useFirstPageNumber").unwrap_or(false) {
                                Some(first.unwrap_or(1))
                            } else {
                                None
                            };
                        page.dpi = attr_u32(element, b"horizontalDpi");
                        page.copies = attr_u32(element, b"copies").unwrap_or(1).max(1);
                    }
                    b"pageMargins" if name == b"pageMargins" => {
                        let normal = Margins::NORMAL;
                        let get = |key: &[u8], fallback: f64| {
                            attr_f64(element, key)
                                .filter(|v| v.is_finite() && *v >= 0.0)
                                .unwrap_or(fallback)
                        };
                        page.margins = Margins {
                            left: get(b"left", normal.left),
                            right: get(b"right", normal.right),
                            top: get(b"top", normal.top),
                            bottom: get(b"bottom", normal.bottom),
                            header: get(b"header", normal.header),
                            footer: get(b"footer", normal.footer),
                        };
                    }
                    b"printOptions" if name == b"printOptions" => {
                        page.center_horizontally =
                            attr_bool(element, b"horizontalCentered").unwrap_or(false);
                        page.center_vertically =
                            attr_bool(element, b"verticalCentered").unwrap_or(false);
                        page.headings = attr_bool(element, b"headings").unwrap_or(false);
                        // Excel reads gridlines only when `gridLinesSet` isn't turned off.
                        page.gridlines = attr_bool(element, b"gridLines").unwrap_or(false)
                            && attr_bool(element, b"gridLinesSet").unwrap_or(true);
                    }
                    b"headerFooter" if name == b"headerFooter" => {
                        let hf = &mut page.header_footer;
                        hf.different_odd_even =
                            attr_bool(element, b"differentOddEven").unwrap_or(false);
                        hf.different_first = attr_bool(element, b"differentFirst").unwrap_or(false);
                        hf.scale_with_doc = attr_bool(element, b"scaleWithDoc").unwrap_or(true);
                        hf.align_with_margins =
                            attr_bool(element, b"alignWithMargins").unwrap_or(true);
                    }
                    b"oddHeader" | b"oddFooter" | b"evenHeader" | b"evenFooter"
                    | b"firstHeader" | b"firstFooter"
                        if name == b"headerFooter" =>
                    {
                        if matches!(event, Ok(Event::Start(_))) {
                            text_of = Some(local.clone());
                            text.clear();
                        }
                    }
                    b"brk" if name == b"rowBreaks" || name == b"colBreaks" => {
                        let Some(id) = attr_u32(element, b"id") else {
                            continue;
                        };
                        let rows = name == b"rowBreaks";
                        let brk = PageBreak {
                            id,
                            min: attr_u32(element, b"min").unwrap_or(0),
                            max: attr_u32(element, b"max").unwrap_or(if rows {
                                cgs_model::MAX_COLS - 1
                            } else {
                                cgs_model::MAX_ROWS - 1
                            }),
                            manual: attr_bool(element, b"man").unwrap_or(false),
                            pivot: attr_bool(element, b"pt").unwrap_or(false),
                        };
                        let list = if rows {
                            &mut page.row_breaks
                        } else {
                            &mut page.col_breaks
                        };
                        list.push(brk);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(end)) => {
                if text_of.as_deref() == Some(end.local_name().as_ref()) {
                    let hf = &mut page.header_footer;
                    let slot = match text_of.take().as_deref() {
                        Some(b"oddHeader") => &mut hf.odd_header,
                        Some(b"oddFooter") => &mut hf.odd_footer,
                        Some(b"evenHeader") => &mut hf.even_header,
                        Some(b"evenFooter") => &mut hf.even_footer,
                        Some(b"firstHeader") => &mut hf.first_header,
                        _ => &mut hf.first_footer,
                    };
                    *slot = std::mem::take(&mut text);
                }
            }
            Ok(other) => {
                if text_of.is_some() {
                    push_text(other, &mut text);
                }
            }
        }
        buf.clear();
    }
    page.row_breaks.sort_by_key(|b| b.id);
    page.col_breaks.sort_by_key(|b| b.id);
}

/// A number as Excel writes margins: as short as it reads back.
fn number(value: f64) -> String {
    let text = format!("{value}");
    if text.contains('e') {
        format!("{value:.6}")
    } else {
        text
    }
}

/// The file's own tag for an element, if it had one, or a new tag.
fn first_tag(captured: &[(String, Vec<u8>)], name: &str) -> Option<Vec<u8>> {
    let (_, xml) = captured.iter().find(|(n, _)| n == name)?;
    let end = xml.iter().position(|&b| b == b'>')?;
    Some(xml[..=end].to_vec())
}

/// Sets each attribute: a value where it differs from the schema's default or the file already
/// had it, and nothing where it is the default and the file didn't say.
fn tag_with(
    original: Option<Vec<u8>>,
    name: &str,
    attrs: &[(&str, Option<String>, bool)],
) -> Vec<u8> {
    let had = original.is_some();
    let tag = original.unwrap_or_else(|| format!("<{name}/>").into_bytes());
    let changes: Vec<(&str, Option<String>)> = attrs
        .iter()
        .map(|(key, value, is_default)| {
            let present = had && crate::scan::attr_value(&tag, key.as_bytes()).is_some();
            if *is_default && !present {
                (*key, None)
            } else {
                (*key, value.clone())
            }
        })
        .collect();
    let refs: Vec<(&str, Option<&str>)> = changes
        .iter()
        .map(|(key, value)| (*key, value.as_deref()))
        .collect();
    edit_tag(&tag, &refs)
}

fn element_name(tag: &[u8]) -> String {
    let end = tag
        .iter()
        .skip(1)
        .position(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        .map_or(tag.len(), |at| at + 1);
    String::from_utf8_lossy(&tag[1..end]).into_owned()
}

fn print_options_xml(page: &PageSetup, captured: &[(String, Vec<u8>)]) -> String {
    let original = first_tag(captured, "printOptions");
    let had = original.is_some();
    let quiet =
        !page.center_horizontally && !page.center_vertically && !page.headings && !page.gridlines;
    if quiet && !had {
        return String::new();
    }
    let tag = tag_with(
        original,
        "printOptions",
        &[
            (
                "horizontalCentered",
                Some(bit(page.center_horizontally)),
                !page.center_horizontally,
            ),
            (
                "verticalCentered",
                Some(bit(page.center_vertically)),
                !page.center_vertically,
            ),
            ("headings", Some(bit(page.headings)), !page.headings),
            ("gridLines", Some(bit(page.gridlines)), !page.gridlines),
            // Excel ignores `gridLines` when this is off, so it goes once gridlines are set here.
            ("gridLinesSet", None, true),
        ],
    );
    String::from_utf8_lossy(&closed(&tag)).into_owned()
}

fn bit(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

fn page_margins_xml(page: &PageSetup, captured: &[(String, Vec<u8>)]) -> String {
    let m = page.margins;
    let tag = tag_with(
        first_tag(captured, "pageMargins"),
        "pageMargins",
        &[
            ("left", Some(number(m.left)), false),
            ("right", Some(number(m.right)), false),
            ("top", Some(number(m.top)), false),
            ("bottom", Some(number(m.bottom)), false),
            ("header", Some(number(m.header)), false),
            ("footer", Some(number(m.footer)), false),
        ],
    );
    String::from_utf8_lossy(&closed(&tag)).into_owned()
}

fn page_setup_xml(page: &PageSetup, captured: &[(String, Vec<u8>)]) -> String {
    let orientation = match page.orientation {
        Orientation::Default => None,
        Orientation::Portrait => Some("portrait".to_string()),
        Orientation::Landscape => Some("landscape".to_string()),
    };
    let comments = match page.comments {
        PrintComments::None => "none",
        PrintComments::AtEnd => "atEnd",
        PrintComments::AsDisplayed => "asDisplayed",
    };
    let errors = match page.errors {
        PrintErrors::Displayed => "displayed",
        PrintErrors::Blank => "blank",
        PrintErrors::Dash => "dash",
        PrintErrors::Na => "NA",
    };
    let order = match page.page_order {
        PageOrder::DownThenOver => "downThenOver",
        PageOrder::OverThenDown => "overThenDown",
    };
    let first = page.first_page.unwrap_or(1);
    let tag = tag_with(
        first_tag(captured, "pageSetup"),
        "pageSetup",
        &[
            ("paperSize", Some(page.paper.to_string()), page.paper == 1),
            ("scale", Some(page.scale.to_string()), page.scale == 100),
            (
                "firstPageNumber",
                Some(first.to_string()),
                page.first_page.is_none(),
            ),
            (
                "fitToWidth",
                Some(page.fit_width.to_string()),
                page.fit_width == 1,
            ),
            (
                "fitToHeight",
                Some(page.fit_height.to_string()),
                page.fit_height == 1,
            ),
            (
                "pageOrder",
                Some(order.to_string()),
                page.page_order == PageOrder::DownThenOver,
            ),
            ("orientation", orientation.clone(), orientation.is_none()),
            (
                "blackAndWhite",
                Some(bit(page.black_and_white)),
                !page.black_and_white,
            ),
            ("draft", Some(bit(page.draft)), !page.draft),
            (
                "cellComments",
                Some(comments.to_string()),
                page.comments == PrintComments::None,
            ),
            (
                "useFirstPageNumber",
                Some(bit(page.first_page.is_some())),
                page.first_page.is_none(),
            ),
            (
                "errors",
                Some(errors.to_string()),
                page.errors == PrintErrors::Displayed,
            ),
            ("copies", Some(page.copies.to_string()), page.copies == 1),
        ],
    );
    String::from_utf8_lossy(&closed(&tag)).into_owned()
}

fn header_footer_xml(page: &PageSetup, captured: &[(String, Vec<u8>)]) -> String {
    let hf = &page.header_footer;
    let original = first_tag(captured, "headerFooter");
    if hf.is_empty() && original.is_none() {
        return String::new();
    }
    let defaults = HeaderFooter::default();
    let tag = tag_with(
        original,
        "headerFooter",
        &[
            (
                "differentOddEven",
                Some(bit(hf.different_odd_even)),
                !hf.different_odd_even,
            ),
            (
                "differentFirst",
                Some(bit(hf.different_first)),
                !hf.different_first,
            ),
            (
                "scaleWithDoc",
                Some(bit(hf.scale_with_doc)),
                hf.scale_with_doc == defaults.scale_with_doc,
            ),
            (
                "alignWithMargins",
                Some(bit(hf.align_with_margins)),
                hf.align_with_margins == defaults.align_with_margins,
            ),
        ],
    );
    let name = element_name(&tag);
    let mut body = Vec::new();
    for (child, text) in [
        ("oddHeader", &hf.odd_header),
        ("oddFooter", &hf.odd_footer),
        ("evenHeader", &hf.even_header),
        ("evenFooter", &hf.even_footer),
        ("firstHeader", &hf.first_header),
        ("firstFooter", &hf.first_footer),
    ] {
        if text.is_empty() {
            continue;
        }
        body.extend_from_slice(format!("<{child}").as_bytes());
        // Leading and trailing spaces mean something in a header.
        if text.starts_with(' ') || text.ends_with(' ') {
            body.extend_from_slice(b" xml:space=\"preserve\"");
        }
        body.push(b'>');
        escape_text(text, &mut body);
        body.extend_from_slice(format!("</{child}>").as_bytes());
    }
    let out = if body.is_empty() {
        closed(&tag)
    } else {
        let mut out = opened(&tag);
        out.extend_from_slice(&body);
        out.extend_from_slice(format!("</{name}>").as_bytes());
        out
    };
    String::from_utf8_lossy(&out).into_owned()
}

fn breaks_xml(list: &[PageBreak], name: &str, captured: &[(String, Vec<u8>)]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let original = first_tag(captured, name);
    let manual = list.iter().filter(|b| b.manual).count();
    let tag = tag_with(
        original,
        name,
        &[
            ("count", Some(list.len().to_string()), false),
            ("manualBreakCount", Some(manual.to_string()), false),
        ],
    );
    let element = element_name(&tag);
    let mut out = opened(&tag);
    for brk in list {
        let mut item = format!("<brk id=\"{}\"", brk.id);
        if brk.min != 0 {
            item.push_str(&format!(" min=\"{}\"", brk.min));
        }
        item.push_str(&format!(" max=\"{}\"", brk.max));
        if brk.manual {
            item.push_str(" man=\"1\"");
        }
        if brk.pivot {
            item.push_str(" pt=\"1\"");
        }
        item.push_str("/>");
        out.extend_from_slice(item.as_bytes());
    }
    out.extend_from_slice(format!("</{element}>").as_bytes());
    String::from_utf8_lossy(&out).into_owned()
}

/// The six elements as the model now has them, each replacing what the file holds. An element
/// with nothing to say goes, where it has no attribute this app doesn't know.
pub fn page_elements(page: &PageSetup, captured: &[(String, Vec<u8>)]) -> Vec<NewElement> {
    let element = |name: &str, xml: String| NewElement {
        name: name.to_string(),
        xml,
        replace: true,
    };
    vec![
        element("printOptions", print_options_xml(page, captured)),
        element("pageMargins", page_margins_xml(page, captured)),
        element("pageSetup", page_setup_xml(page, captured)),
        element("headerFooter", header_footer_xml(page, captured)),
        element(
            "rowBreaks",
            breaks_xml(&page.row_breaks, "rowBreaks", captured),
        ),
        element(
            "colBreaks",
            breaks_xml(&page.col_breaks, "colBreaks", captured),
        ),
    ]
}

/// `sheetPr` with `pageSetUpPr`'s `fitToPage` as given, the element made where it is needed
/// and every other part kept.
pub fn edit_sheet_pr(xml: &[u8], fit: bool) -> io::Result<Vec<u8>> {
    let mut tree = parse_tree(xml)?;
    for node in &mut tree {
        let Node::Element(sheet_pr) = node else {
            continue;
        };
        let prefix = {
            let name = element_name(&sheet_pr.tag);
            name.rsplit_once(':')
                .map(|(p, _)| format!("{p}:"))
                .unwrap_or_default()
        };
        if let Some(setup) = sheet_pr
            .elements_mut()
            .find(|child| child.local() == b"pageSetUpPr")
        {
            setup.set_attr("fitToPage", fit.then_some("1"));
            continue;
        }
        if !fit {
            continue;
        }
        let new = Element {
            tag: format!("<{prefix}pageSetUpPr fitToPage=\"1\"/>").into_bytes(),
            empty: true,
            children: Vec::new(),
            end: Vec::new(),
        };
        // After `tabColor` and `outlinePr`, the only children that come before it.
        let at = sheet_pr
            .children
            .iter()
            .rposition(|child| {
                matches!(child, Node::Element(e) if e.local() == b"tabColor" || e.local() == b"outlinePr")
            })
            .map_or(0, |at| at + 1);
        if sheet_pr.empty {
            sheet_pr.empty = false;
            sheet_pr.tag = opened(&sheet_pr.tag);
            sheet_pr.end = format!("</{prefix}sheetPr>").into_bytes();
        }
        sheet_pr.children.insert(at, Node::Element(new));
    }
    let mut out = Vec::with_capacity(xml.len() + 40);
    write_tree(&tree, &mut out);
    Ok(out)
}

/// A new `sheetPr` for a sheet that has none, if fitting needs one.
pub fn new_sheet_pr(fit: bool, filter_mode: bool) -> Option<String> {
    let filter = if filter_mode { " filterMode=\"1\"" } else { "" };
    match (fit, filter_mode) {
        (false, false) => None,
        (false, true) => Some(format!("<sheetPr{filter}/>")),
        (true, _) => Some(format!(
            "<sheetPr{filter}><pageSetUpPr fitToPage=\"1\"/></sheetPr>"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str, xml: &str) -> PageSetup {
        let mut page = PageSetup::default();
        read_page_element(name.as_bytes(), xml.as_bytes(), &mut page);
        page
    }

    #[test]
    fn reads_page_setup() {
        let page = read(
            "pageSetup",
            r#"<pageSetup paperSize="9" scale="85" fitToWidth="2" fitToHeight="0" orientation="landscape" pageOrder="overThenDown" firstPageNumber="5" useFirstPageNumber="1" blackAndWhite="1" cellComments="atEnd" errors="dash" horizontalDpi="600" r:id="rId1"/>"#,
        );
        assert_eq!(page.paper, 9);
        assert_eq!(page.scale, 85);
        assert_eq!((page.fit_width, page.fit_height), (2, 0));
        assert!(page.landscape());
        assert_eq!(page.page_order, PageOrder::OverThenDown);
        assert_eq!(page.first_page, Some(5));
        assert!(page.black_and_white);
        assert_eq!(page.comments, PrintComments::AtEnd);
        assert_eq!(page.errors, PrintErrors::Dash);
        assert_eq!(page.dpi, Some(600));
    }

    #[test]
    fn reads_margins_options_headers_and_breaks() {
        let mut page = read(
            "pageMargins",
            r#"<pageMargins left="0.25" right="0.25" top="1" bottom="1" header="0.5" footer="0.5"/>"#,
        );
        read_page_element(
            b"printOptions",
            br#"<printOptions horizontalCentered="1" gridLines="1" headings="1"/>"#,
            &mut page,
        );
        read_page_element(
            b"headerFooter",
            br#"<headerFooter differentFirst="1"><oddHeader>&amp;CPage &amp;P</oddHeader><firstFooter>&amp;LFirst</firstFooter></headerFooter>"#,
            &mut page,
        );
        read_page_element(
            b"rowBreaks",
            br#"<rowBreaks count="2" manualBreakCount="1"><brk id="30" max="16383" man="1"/><brk id="10" max="16383"/></rowBreaks>"#,
            &mut page,
        );
        assert_eq!(page.margins.left, 0.25);
        assert_eq!(page.margins.header, 0.5);
        assert!(page.center_horizontally && page.gridlines && page.headings);
        assert!(!page.center_vertically);
        assert_eq!(page.header_footer.odd_header, "&CPage &P");
        assert_eq!(page.header_footer.first_footer, "&LFirst");
        assert!(page.header_footer.different_first);
        assert_eq!(
            page.row_breaks
                .iter()
                .map(|b| (b.id, b.manual))
                .collect::<Vec<_>>(),
            vec![(10, false), (30, true)]
        );
    }

    #[test]
    fn writing_keeps_attributes_it_doesnt_know() {
        let captured = vec![(
            "pageSetup".to_string(),
            br#"<pageSetup paperSize="9" orientation="portrait" horizontalDpi="1200" r:id="rId3"/>"#.to_vec(),
        )];
        let mut page = PageSetup::default();
        read_page_element(b"pageSetup", &captured[0].1, &mut page);
        page.orientation = Orientation::Landscape;
        page.scale = 70;
        let xml = page_setup_xml(&page, &captured);
        assert_eq!(
            xml,
            r#"<pageSetup paperSize="9" orientation="landscape" horizontalDpi="1200" r:id="rId3" scale="70"/>"#
        );
    }

    #[test]
    fn new_elements_say_only_what_differs() {
        let page = PageSetup {
            fit_to_page: true,
            fit_height: 0,
            gridlines: true,
            ..PageSetup::default()
        };
        let list = page_elements(&page, &[]);
        let xml: Vec<&str> = list.iter().map(|e| e.xml.as_str()).collect();
        assert_eq!(xml[0], r#"<printOptions gridLines="1"/>"#);
        assert_eq!(
            xml[1],
            r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#
        );
        assert_eq!(xml[2], r#"<pageSetup fitToHeight="0"/>"#);
        assert_eq!(xml[3], "");
        assert_eq!(xml[4], "");
    }

    #[test]
    fn writes_headers_and_breaks() {
        let mut page = PageSetup::default();
        page.header_footer.odd_footer = "&CPage &P of &N".to_string();
        page.header_footer.odd_header = " &A".to_string();
        page.insert_break(Some(20), Some(5));
        let list = page_elements(&page, &[]);
        assert_eq!(
            list[3].xml,
            r#"<headerFooter><oddHeader xml:space="preserve"> &amp;A</oddHeader><oddFooter>&amp;CPage &amp;P of &amp;N</oddFooter></headerFooter>"#
        );
        assert_eq!(
            list[4].xml,
            r#"<rowBreaks count="1" manualBreakCount="1"><brk id="20" max="16383" man="1"/></rowBreaks>"#
        );
        assert_eq!(
            list[5].xml,
            r#"<colBreaks count="1" manualBreakCount="1"><brk id="5" max="1048575" man="1"/></colBreaks>"#
        );
        let mut back = PageSetup::default();
        read_page_element(b"headerFooter", list[3].xml.as_bytes(), &mut back);
        assert_eq!(back.header_footer, page.header_footer);
    }

    #[test]
    fn fit_to_page_goes_in_sheet_pr() {
        let edited = edit_sheet_pr(
            br#"<sheetPr codeName="Sheet1"><tabColor rgb="FFFF0000"/></sheetPr>"#,
            true,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(edited).unwrap(),
            r#"<sheetPr codeName="Sheet1"><tabColor rgb="FFFF0000"/><pageSetUpPr fitToPage="1"/></sheetPr>"#
        );
        let edited = edit_sheet_pr(br#"<x:sheetPr/>"#, true).unwrap();
        assert_eq!(
            String::from_utf8(edited).unwrap(),
            r#"<x:sheetPr><x:pageSetUpPr fitToPage="1"/></x:sheetPr>"#
        );
        let edited = edit_sheet_pr(
            br#"<sheetPr><pageSetUpPr autoPageBreaks="0" fitToPage="1"/></sheetPr>"#,
            false,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(edited).unwrap(),
            r#"<sheetPr><pageSetUpPr autoPageBreaks="0"/></sheetPr>"#
        );
    }
}
