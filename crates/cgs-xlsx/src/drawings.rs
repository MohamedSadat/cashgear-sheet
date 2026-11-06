//! Saves drawings and charts as the model has them. A sheet whose drawings changed has its drawing
//! part written from its anchors: each one the file had keeps its bytes, with only its corners
//! patched, new ones are added, and deleted ones go, with the parts only they used. Charts the
//! app changed are brought in line by `chart_write`, and new ones written whole.

use std::collections::{BTreeMap, HashSet};

use cgs_model::{Anchor, AnchorPoint, Book, ChartBody, Drawing, DrawingKind};

use crate::chart_write::{sync_chart, write_chart};
use crate::patch_parts::{add_relationship_with_id, remove_relationship_ids};
use crate::patch_sheet::NewElement;
use crate::scan::escape_attr;
use crate::structure::{parse_tree, write_tree, Element, Node};
use crate::workbook::relationships_in;
use crate::xml::rels_path;
use crate::zipdir::{normalize, pack, ZipChanges};

pub const DRAWING_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
pub const DRAWING_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
pub const CHART_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
pub const CHART_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
pub const IMAGE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const EMPTY_RELS: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";

/// A picture's content type, by its part's extension.
pub fn media_type(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpeg" | "jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// What saving drawings did, for the content types and for the engine afterwards.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DrawingsSave {
    /// New parts, with their content types.
    pub added: Vec<(String, String)>,
    pub removed: Vec<String>,
    /// New chart parts, by their index in `Book::charts`.
    pub chart_parts: Vec<(u32, String)>,
    /// New drawing parts, by sheet.
    pub drawing_parts: Vec<(u32, String)>,
    /// Where each drawing of a sheet now is among its part's anchors.
    pub sources: Vec<(u32, Vec<u32>)>,
}

/// A part's path as a relationship from `from` names it.
fn relative(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect();
    let from_dir = &from_dir[..from_dir.len().saturating_sub(1)];
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from_dir
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count();
    let mut out: Vec<String> = Vec::new();
    for _ in common..from_dir.len() {
        out.push("..".to_string());
    }
    for part in &to_parts[common..] {
        out.push((*part).to_string());
    }
    out.join("/")
}

fn fresh(taken: &mut HashSet<String>, stem: &str, ext: &str) -> String {
    let mut number = 1;
    loop {
        let name = format!("{stem}{number}.{ext}");
        if taken.insert(normalize(&name).to_ascii_lowercase()) {
            break name;
        }
        number += 1;
    }
}

fn marker(prefix: &str, name: &str, point: AnchorPoint) -> String {
    format!(
        "<{p}{name}><{p}col>{}</{p}col><{p}colOff>{}</{p}colOff><{p}row>{}</{p}row><{p}rowOff>{}</{p}rowOff></{p}{name}>",
        point.col.saturating_sub(1),
        point.col_off,
        point.row.saturating_sub(1),
        point.row_off,
        p = prefix
    )
}

/// An anchor's opening, its corners and size, and its end tag name.
fn anchor_frame(
    prefix: &str,
    anchor: &Anchor,
    edit_as: Option<&str>,
) -> (String, String, &'static str) {
    match anchor {
        Anchor::TwoCell { from, to } => (
            format!(
                "<{prefix}twoCellAnchor{}>",
                edit_as
                    .map(|e| format!(" editAs=\"{e}\""))
                    .unwrap_or_default()
            ),
            marker(prefix, "from", *from) + &marker(prefix, "to", *to),
            "twoCellAnchor",
        ),
        Anchor::OneCell {
            from,
            width,
            height,
        } => (
            format!("<{prefix}oneCellAnchor>"),
            marker(prefix, "from", *from)
                + &format!("<{prefix}ext cx=\"{width}\" cy=\"{height}\"/>"),
            "oneCellAnchor",
        ),
        Anchor::Absolute {
            x,
            y,
            width,
            height,
        } => (
            format!("<{prefix}absoluteAnchor>"),
            format!(
                "<{prefix}pos x=\"{x}\" y=\"{y}\"/><{prefix}ext cx=\"{width}\" cy=\"{height}\"/>"
            ),
            "absoluteAnchor",
        ),
    }
}

/// A new anchor's XML, for a chart or a picture whose relationship is `rel`.
fn new_anchor(prefixes: &(String, String), drawing: &Drawing, rel: &str) -> String {
    let (x, a) = (&prefixes.0, &prefixes.1);
    let name = escape_attr(&drawing.name);
    let descr = if drawing.description.is_empty() {
        String::new()
    } else {
        format!(" descr=\"{}\"", escape_attr(&drawing.description))
    };
    let (open, corners, tag) = anchor_frame(x, &drawing.anchor, None);
    let body = match drawing.kind {
        DrawingKind::Chart => format!(
            "<{x}graphicFrame macro=\"\"><{x}nvGraphicFramePr><{x}cNvPr id=\"{}\" name=\"{name}\"{descr}/><{x}cNvGraphicFramePr/></{x}nvGraphicFramePr><{x}xfrm><{a}off x=\"0\" y=\"0\"/><{a}ext cx=\"0\" cy=\"0\"/></{x}xfrm><{a}graphic><{a}graphicData uri=\"{C}\"><c:chart xmlns:c=\"{C}\" xmlns:r=\"{R}\" r:id=\"{rel}\"/></{a}graphicData></{a}graphic></{x}graphicFrame>",
            drawing.id
        ),
        _ => {
            let (width, height) = match drawing.anchor {
                Anchor::OneCell { width, height, .. } | Anchor::Absolute { width, height, .. } => (width, height),
                Anchor::TwoCell { .. } => (0, 0),
            };
            format!(
                "<{x}pic><{x}nvPicPr><{x}cNvPr id=\"{}\" name=\"{name}\"{descr}/><{x}cNvPicPr><{a}picLocks noChangeAspect=\"1\"/></{x}cNvPicPr></{x}nvPicPr><{x}blipFill><{a}blip xmlns:r=\"{R}\" r:embed=\"{rel}\"/><{a}stretch><{a}fillRect/></{a}stretch></{x}blipFill><{x}spPr><{a}xfrm><{a}off x=\"0\" y=\"0\"/><{a}ext cx=\"{width}\" cy=\"{height}\"/></{a}xfrm><{a}prstGeom prst=\"rect\"><{a}avLst/></{a}prstGeom></{x}spPr></{x}pic>",
                drawing.id
            )
        }
    };
    format!("{open}{corners}{body}<{x}clientData/></{x}{tag}>")
}

fn prefix_of(tag: &[u8]) -> String {
    let text = String::from_utf8_lossy(tag);
    let name = text
        .trim_start_matches('<')
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("");
    name.split_once(':')
        .map(|(p, _)| format!("{p}:"))
        .unwrap_or_default()
}

/// The prefix a root tag declares for a namespace, if it does.
fn declared(tag: &[u8], namespace: &str) -> Option<String> {
    let text = String::from_utf8_lossy(tag);
    for (at, _) in text.match_indices("xmlns:") {
        let rest = &text[at + 6..];
        let (prefix, value) = rest.split_once('=')?;
        let value = value.trim_start_matches(['"', '\'']);
        if value.starts_with(namespace) && value[namespace.len()..].starts_with(['"', '\'']) {
            return Some(format!("{prefix}:"));
        }
    }
    None
}

fn is_anchor(element: &Element) -> bool {
    matches!(
        element.local(),
        b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" | b"AlternateContent"
    )
}

fn set_number(element: &mut Element, name: &[u8], value: i64) {
    if let Some(child) = element.elements_mut().find(|c| c.local() == name) {
        let text = value.to_string();
        if child.text().trim() != text {
            child.set_text(&text);
        }
    }
}

fn set_marker(element: &mut Element, name: &[u8], point: AnchorPoint) {
    if let Some(corner) = element.elements_mut().find(|c| c.local() == name) {
        set_number(corner, b"col", i64::from(point.col.saturating_sub(1)));
        set_number(corner, b"colOff", point.col_off);
        set_number(corner, b"row", i64::from(point.row.saturating_sub(1)));
        set_number(corner, b"rowOff", point.row_off);
    }
}

fn set_size(element: &mut Element, name: &[u8], (a, b): (&str, &str), (x, y): (i64, i64)) {
    if let Some(child) = element.elements_mut().find(|c| c.local() == name) {
        let (x, y) = (x.to_string(), y.to_string());
        if child.attr_text(a.as_bytes()).as_deref() != Some(&x) {
            child.set_attr(a, Some(&x));
        }
        if child.attr_text(b.as_bytes()).as_deref() != Some(&y) {
            child.set_attr(b, Some(&y));
        }
    }
}

/// Puts the model's corners and size into an anchor the file had, or into each anchor of an
/// `mc:AlternateContent` block.
fn place_anchor(element: &mut Element, anchor: &Anchor) {
    if element.local() == b"AlternateContent" {
        for branch in element.elements_mut() {
            for inner in branch.elements_mut() {
                if is_anchor(inner) {
                    place_anchor(inner, anchor);
                }
            }
        }
        return;
    }
    match (element.local(), anchor) {
        (b"twoCellAnchor", Anchor::TwoCell { from, to }) => {
            set_marker(element, b"from", *from);
            set_marker(element, b"to", *to);
        }
        (
            b"oneCellAnchor",
            Anchor::OneCell {
                from,
                width,
                height,
            },
        ) => {
            set_marker(element, b"from", *from);
            set_size(element, b"ext", ("cx", "cy"), (*width, *height));
        }
        (
            b"absoluteAnchor",
            Anchor::Absolute {
                x,
                y,
                width,
                height,
            },
        ) => {
            set_size(element, b"pos", ("x", "y"), (*x, *y));
            set_size(element, b"ext", ("cx", "cy"), (*width, *height));
        }
        _ => {}
    }
}

/// Relationship ids an XML text uses, as `r:id`, `r:embed`, `r:link` and the like.
fn used_ids(xml: &[u8]) -> HashSet<String> {
    let text = String::from_utf8_lossy(xml);
    let mut out = HashSet::new();
    for (at, _) in text.match_indices("=\"") {
        let rest = &text[at + 2..];
        if let Some(end) = rest.find('"') {
            out.insert(rest[..end].to_string());
        }
    }
    out
}

/// A part and what hangs off it through its relationships, for taking out together.
fn with_children(package: &crate::Package, part: &str, out: &mut Vec<String>) {
    if out.iter().any(|p| p.eq_ignore_ascii_case(part)) {
        return;
    }
    out.push(part.to_string());
    let rels = rels_path(part);
    if let Some(xml) = package.read(&rels) {
        out.push(rels.clone());
        for (_, (kind, target)) in relationships_in(&xml, part) {
            // Chart styles and colours belong to their chart; anything else may be shared.
            if kind.ends_with("/chartStyle")
                || kind.ends_with("/chartColorStyle")
                || kind.ends_with("/chartUserShapes")
            {
                with_children(package, &target, out);
            }
        }
    }
}

pub struct DrawingsInput<'a> {
    pub package: &'a crate::Package,
    pub book: &'a Book,
    pub sheets: &'a std::collections::BTreeSet<u32>,
    pub media: &'a BTreeMap<String, Vec<u8>>,
}

fn io_damaged(error: std::io::Error) -> String {
    error.to_string()
}

/// Writes what changed. New parts go into `changes`, changed ones into `overlay`, and new sheet
/// elements into `elements`.
pub fn save_drawings(
    input: &DrawingsInput<'_>,
    overlay: &mut BTreeMap<String, Vec<u8>>,
    changes: &mut ZipChanges,
    elements: &mut BTreeMap<u32, Vec<NewElement>>,
) -> Result<DrawingsSave, String> {
    let DrawingsInput {
        package,
        book,
        sheets,
        media,
    } = *input;
    let mut save = DrawingsSave::default();
    let mut taken: HashSet<String> = package
        .parts()
        .iter()
        .map(|part| normalize(&part.name).to_ascii_lowercase())
        .chain(
            media
                .keys()
                .map(|name| normalize(name).to_ascii_lowercase()),
        )
        .collect();

    // Charts the app changed, that the file has.
    for part in &book.charts {
        let (Some(name), ChartBody::Classic(chart), true) = (&part.part, &part.body, part.changed)
        else {
            continue;
        };
        let Some(xml) = overlay.get(name).cloned().or_else(|| package.read(name)) else {
            continue;
        };
        let synced = sync_chart(&xml, chart).map_err(io_damaged)?;
        if synced != xml {
            overlay.insert(name.clone(), synced);
        }
    }

    let mut media_used: HashSet<String> = HashSet::new();
    for &sheet in sheets {
        let Some(info) = book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
            continue;
        };
        let Some(sheet_part) = info.part.as_ref() else {
            continue;
        };
        if info.drawing_part.is_none() && info.drawings.is_empty() {
            continue;
        }
        // The drawing part: the file's, or a new one.
        let (drawing_part, original) = match &info.drawing_part {
            Some(part) => (part.clone(), package.read(part)),
            None => (fresh(&mut taken, "xl/drawings/drawing", "xml"), None),
        };
        let original = original.unwrap_or_else(|| {
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<xdr:wsDr xmlns:xdr=\"{XDR}\" xmlns:a=\"{A}\"></xdr:wsDr>").into_bytes()
        });
        let mut tree = parse_tree(&original).map_err(io_damaged)?;
        let root_at = tree
            .iter()
            .position(|node| matches!(node, Node::Element(e) if e.local() == b"wsDr"))
            .ok_or("a drawing part isn't a drawing")?;
        let rels_name = rels_path(&drawing_part);
        let rels_had = overlay
            .get(&rels_name)
            .cloned()
            .or_else(|| package.read(&rels_name));
        let mut rels = rels_had.clone().unwrap_or_else(|| EMPTY_RELS.to_vec());
        let rel_list = relationships_in(&rels, &drawing_part);
        let Node::Element(root) = &mut tree[root_at] else {
            unreachable!()
        };
        let x = prefix_of(&root.tag);
        let a = match declared(&root.tag, A) {
            Some(prefix) => prefix,
            None => {
                root.set_attr("xmlns:a", Some(A));
                "a:".to_string()
            }
        };
        if root.empty {
            let tag = String::from_utf8_lossy(&root.tag)
                .trim_end_matches("/>")
                .trim_end()
                .to_string()
                + ">";
            root.end = format!("</{x}wsDr>").into_bytes();
            root.tag = tag.into_bytes();
            root.empty = false;
        }
        // The file's anchors, by their place.
        let mut units: Vec<Option<Node>> = Vec::new();
        let mut others = Vec::new();
        for node in std::mem::take(&mut root.children) {
            match &node {
                Node::Element(element) if is_anchor(element) => units.push(Some(node)),
                Node::Element(_) => others.push(node),
                Node::Text(_) => {}
            }
        }
        let had = units.len();
        let mut children: Vec<Node> = Vec::new();
        let mut sources = Vec::new();
        for drawing in &info.drawings {
            match drawing
                .source
                .and_then(|at| units.get_mut(at as usize))
                .and_then(Option::take)
            {
                Some(Node::Element(mut element)) => {
                    place_anchor(&mut element, &drawing.anchor);
                    children.push(Node::Element(element));
                }
                _ => {
                    // New: a chart part or a picture to link.
                    let rel = match (drawing.kind, drawing.chart, &drawing.image) {
                        (DrawingKind::Chart, Some(index), _) => {
                            let Some(part) = book.charts.get(index as usize) else {
                                continue;
                            };
                            let target = match &part.part {
                                Some(existing) => existing.clone(),
                                None => match save.chart_parts.iter().find(|(i, _)| *i == index) {
                                    Some((_, name)) => name.clone(),
                                    None => {
                                        let ChartBody::Classic(chart) = &part.body else {
                                            continue;
                                        };
                                        let name = fresh(&mut taken, "xl/charts/chart", "xml");
                                        let xml = write_chart(chart);
                                        changes
                                            .add
                                            .push((name.clone(), pack(&xml).map_err(io_damaged)?));
                                        save.added.push((name.clone(), CHART_TYPE.to_string()));
                                        save.chart_parts.push((index, name.clone()));
                                        name
                                    }
                                },
                            };
                            let (updated, id) = add_relationship_with_id(
                                &rels,
                                CHART_REL,
                                &relative(&drawing_part, &target),
                            );
                            rels = updated;
                            id
                        }
                        (_, _, Some(image)) => {
                            if let Some(bytes) = media.get(image) {
                                if media_used.insert(image.clone())
                                    && !package
                                        .parts()
                                        .iter()
                                        .any(|p| p.name.eq_ignore_ascii_case(image))
                                {
                                    changes
                                        .add
                                        .push((image.clone(), pack(bytes).map_err(io_damaged)?));
                                    save.added
                                        .push((image.clone(), media_type(image).to_string()));
                                }
                            }
                            let (updated, id) = add_relationship_with_id(
                                &rels,
                                IMAGE_REL,
                                &relative(&drawing_part, image),
                            );
                            rels = updated;
                            id
                        }
                        _ => continue,
                    };
                    let xml = new_anchor(&(x.clone(), a.clone()), drawing, &rel);
                    children.extend(parse_tree(xml.as_bytes()).map_err(io_damaged)?);
                }
            }
            sources.push((children.len() - 1) as u32);
        }
        save.sources.push((sheet, sources));
        // Anchors the model no longer has: their charts and pictures' links go, and charts only
        // they used go with them.
        let mut kept_xml = Vec::new();
        write_tree(&children, &mut kept_xml);
        let still = used_ids(&kept_xml);
        let mut gone_ids = Vec::new();
        for unit in units.iter().flatten() {
            let mut raw = Vec::new();
            write_tree(std::slice::from_ref(unit), &mut raw);
            for id in used_ids(&raw) {
                if let Some((kind, target)) = rel_list.get(&id) {
                    if still.contains(&id) || gone_ids.contains(&id) {
                        continue;
                    }
                    gone_ids.push(id.clone());
                    let chart_used_elsewhere = book
                        .charts
                        .iter()
                        .any(|c| c.part.as_deref() == Some(target.as_str()))
                        && book.sheets.iter().enumerate().any(|(s, other)| {
                            !other.deleted
                                && other.drawings.iter().any(|d| {
                                    d.chart
                                        .and_then(|i| book.charts.get(i as usize))
                                        .and_then(|c| c.part.as_deref())
                                        == Some(target.as_str())
                                        && (s as u32 != sheet || d.source.is_some())
                                })
                        });
                    if (kind.ends_with("/chart") || kind.ends_with("/chartEx"))
                        && !chart_used_elsewhere
                    {
                        let mut parts = Vec::new();
                        with_children(package, target, &mut parts);
                        for part in parts {
                            if !save.removed.iter().any(|p| p.eq_ignore_ascii_case(&part)) {
                                changes.remove.push(normalize(&part));
                                overlay.remove(&part);
                                save.removed.push(part);
                            }
                        }
                    }
                }
            }
        }
        if !gone_ids.is_empty() {
            rels = remove_relationship_ids(&rels, &gone_ids);
        }
        let Node::Element(root) = &mut tree[root_at] else {
            unreachable!()
        };
        root.children = children;
        root.children.extend(others);
        let mut xml = Vec::with_capacity(original.len() + 512);
        write_tree(&tree, &mut xml);

        if info.drawing_part.is_some() {
            if xml != original || had != info.drawings.len() {
                overlay.insert(drawing_part.clone(), xml);
            }
        } else {
            changes
                .add
                .push((drawing_part.clone(), pack(&xml).map_err(io_damaged)?));
            save.added
                .push((drawing_part.clone(), DRAWING_TYPE.to_string()));
            save.drawing_parts.push((sheet, drawing_part.clone()));
            // The sheet links to it.
            let sheet_rels_name = rels_path(sheet_part);
            let sheet_rels_had = overlay
                .get(&sheet_rels_name)
                .cloned()
                .or_else(|| package.read(&sheet_rels_name));
            let sheet_rels = sheet_rels_had
                .clone()
                .unwrap_or_else(|| EMPTY_RELS.to_vec());
            let (updated, id) = add_relationship_with_id(
                &sheet_rels,
                DRAWING_REL,
                &relative(sheet_part, &drawing_part),
            );
            if sheet_rels_had.is_some() {
                overlay.insert(sheet_rels_name, updated);
            } else {
                changes
                    .add
                    .push((sheet_rels_name, pack(&updated).map_err(io_damaged)?));
            }
            elements.entry(sheet).or_default().push(NewElement {
                name: "drawing".to_string(),
                xml: format!("<drawing xmlns:r=\"{R}\" r:id=\"{}\"/>", escape_attr(&id)),
                replace: true,
            });
        }
        if rels_had.as_deref() != Some(&rels[..]) && rels != EMPTY_RELS {
            if rels_had.is_some() {
                overlay.insert(rels_name, rels);
            } else {
                changes
                    .add
                    .push((rels_name, pack(&rels).map_err(io_damaged)?));
            }
        }
    }
    Ok(save)
}
