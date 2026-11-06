//! Saves a workbook with everything it doesn't change intact. Sheets, strings and the small parts
//! are patched in memory first, then the package is written with each untouched entry copied as
//! bytes. Nothing reaches the output until every patch has worked.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};

use cgs_calc::CalcValue;
use cgs_model::Book;
use flate2::read::DeflateDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::package::{ByteSource, Package, PartStream};
use crate::patch_parts::{
    add_external_relationship, add_override, add_relationship, add_relationship_with_id,
    append_strings, count_strings, new_strings_part, read_table_part, remove_overrides,
    remove_relationship_ids, remove_relationships, set_full_calc_on_load, set_refresh_on_load,
    set_table_columns, StylesXml,
};
use crate::patch_sheet::{
    patch_into, patch_sheet, ArrayKind, CellChange, CellPatch, NewElement, SheetExtras, StringTable,
};
use crate::scan::{attr_value, escape_attr, utf8_part, Kind, Scanner};
use crate::structure::{
    all_own_edits, book_edits, own_edits, parse_tree, rewrite_chart, rewrite_comments,
    rewrite_drawing, rewrite_pivot_cache, rewrite_pivot_table, rewrite_sheet, rewrite_table,
    rewrite_vml, write_tree, BookOp, Element, Masters, Node, SheetEdits,
};
use crate::workbook::read_relationships;
use crate::xml::{parse_rect, parse_u32, rels_path, unescape_attr};
use crate::zipdir::{normalize, pack, rewrite, Directory, Packed, Packer, ZipChanges};

const STRINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const STRINGS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const STYLES_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const STYLES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const WORKSHEET_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const METADATA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml";
const METADATA_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata";

/// Why a save didn't happen. The file on disk hasn't been touched in any of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The file we are copying from can't be read, or isn't what it was.
    SourceUnreadable(String),
    /// The file has something we can't copy safely.
    Damaged(String),
    /// The output couldn't be written.
    Write(String),
    /// What came out isn't what should have.
    Verify(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceUnreadable(detail) => {
                write!(f, "The original file can no longer be read, so this workbook can't be saved safely: {detail}")
            }
            Self::Damaged(detail) => {
                write!(
                    f,
                    "This workbook can't be saved, because the file is damaged: {detail}"
                )
            }
            Self::Write(detail) => write!(f, "The file couldn't be written: {detail}"),
            Self::Verify(detail) => {
                write!(
                    f,
                    "The saved file failed its check, so it wasn't used: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveError {
    fn read(error: impl std::fmt::Display) -> Self {
        Self::SourceUnreadable(error.to_string())
    }
}

/// Everything to change, keyed by sheet index. A book with nothing here saves as it was.
#[derive(Debug, Default, Clone)]
pub struct WorkbookEdits {
    pub cells: BTreeMap<u32, BTreeMap<(u32, u32), CellPatch>>,
    pub elements: BTreeMap<u32, Vec<NewElement>>,
    /// The whole styles part, if styles were added.
    pub styles: Option<StylesXml>,
    /// Ask Excel to rebuild every pivot table when it opens the file.
    pub refresh_pivots: bool,
    /// Changes to the workbook's shape, in order. `cells` is in final coordinates.
    pub ops: Vec<BookOp>,
    /// Sheets whose merges differ from what the file says, whatever the shape changes did.
    pub merges: std::collections::BTreeSet<u32>,
    /// The calculation mode to save, if it was changed: manual or not.
    pub calc_mode: Option<bool>,
    /// What changes in a sheet apart from its cells' contents, in final coordinates. The engine
    /// fills this in from the lists below when it saves.
    pub extras: BTreeMap<u32, SheetExtras>,
    /// Areas whose cells may have new styles, which the sheet itself holds.
    pub style_areas: BTreeMap<u32, Vec<cgs_model::Rect>>,
    /// Rows whose records changed, as sorted spans.
    pub row_lines: BTreeMap<u32, Vec<(u32, u32)>>,
    /// Columns whose records changed, as sorted spans.
    pub col_lines: BTreeMap<u32, Vec<(u32, u32)>>,
    /// Sheets whose panes changed.
    pub view_freeze: std::collections::BTreeSet<u32>,
    /// Sheets whose gridlines or headings may have changed.
    pub view_flags: std::collections::BTreeSet<u32>,
    /// Sheets whose default row height may have changed.
    pub default_rows: std::collections::BTreeSet<u32>,
    /// Some table was made, removed, or changed how it looks, so the model's tables are saved.
    pub tables_changed: bool,
    /// Sheets whose own autofilter changed.
    pub filters: std::collections::BTreeSet<u32>,
    /// Sheets whose conditional formats changed.
    pub cf_sheets: std::collections::BTreeSet<u32>,
    /// Sheets whose data validation changed.
    pub dv_sheets: std::collections::BTreeSet<u32>,
    /// The workbook's defined names changed, apart from what moving rows does to them.
    pub names_changed: bool,
    /// Sheets whose notes changed.
    pub notes_sheets: std::collections::BTreeSet<u32>,
    /// Sheets whose hyperlinks changed.
    pub link_sheets: std::collections::BTreeSet<u32>,
}

impl WorkbookEdits {
    pub fn is_empty(&self) -> bool {
        self.cells.values().all(BTreeMap::is_empty)
            && self.elements.values().all(Vec::is_empty)
            && !self.styles.as_ref().is_some_and(|styles| styles.changed)
            && !self.refresh_pivots
            && self.ops.is_empty()
            && self.merges.is_empty()
            && self.calc_mode.is_none()
            && self.extras.values().all(SheetExtras::is_empty)
            && !self.tables_changed
            && self.filters.is_empty()
            && self.cf_sheets.is_empty()
            && self.dv_sheets.is_empty()
            && !self.names_changed
            && self.notes_sheets.is_empty()
            && self.link_sheets.is_empty()
    }

    /// Whether a cell's contents or result changed, rather than only how it looks.
    pub fn changes_cells(&self) -> bool {
        !self.ops.is_empty()
            || self
                .cells
                .values()
                .flat_map(BTreeMap::values)
                .any(|patch| !matches!(patch.change, CellChange::Keep))
    }
}

/// What kind of content a save kept without being able to edit it, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kept {
    Macros,
    Charts,
    Drawings,
    Images,
    PivotTables,
    Comments,
    ExternalLinks,
    Connections,
    Controls,
    EmbeddedObjects,
    Slicers,
    CustomXml,
    Other,
}

impl Kept {
    pub fn label(self) -> &'static str {
        match self {
            Self::Macros => "macros",
            Self::Charts => "charts",
            Self::Drawings => "drawings",
            Self::Images => "images",
            Self::PivotTables => "pivot tables",
            Self::Comments => "comments",
            Self::ExternalLinks => "links to other workbooks",
            Self::Connections => "data connections",
            Self::Controls => "form controls",
            Self::EmbeddedObjects => "embedded objects",
            Self::Slicers => "slicers and timelines",
            Self::CustomXml => "custom XML",
            Self::Other => "other content",
        }
    }
}

/// Which report line a part this app can't edit belongs under. Parts it can edit aren't listed.
fn classify(name: &str) -> Option<Kept> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".rels") || lower == "[content_types].xml" || lower.starts_with("docprops/")
    {
        return None;
    }
    let inside = |folder: &str| lower.contains(folder);
    Some(
        if lower.ends_with("vbaproject.bin") || lower.ends_with("vbadata.xml") {
            Kept::Macros
        } else if inside("/charts/") || inside("/chartsheets/") {
            Kept::Charts
        } else if inside("/pivottables/") || inside("/pivotcache/") {
            Kept::PivotTables
        } else if inside("/media/") {
            Kept::Images
        } else if inside("/drawings/") {
            Kept::Drawings
        } else if inside("/comments") || inside("/threadedcomments/") || inside("/persons/") {
            Kept::Comments
        } else if inside("/externallinks/") {
            Kept::ExternalLinks
        } else if inside("/connections") || inside("/querytables/") {
            Kept::Connections
        } else if inside("/ctrlprops/") || inside("/activex/") {
            Kept::Controls
        } else if inside("/embeddings/") {
            Kept::EmbeddedObjects
        } else if inside("/slicers/") || inside("/slicercaches/") || inside("/timeline") {
            Kept::Slicers
        } else if lower.starts_with("customxml/") {
            Kept::CustomXml
        } else if lower.starts_with("xl/worksheets/")
            || lower.starts_with("xl/tables/")
            || lower.starts_with("xl/theme/")
            || lower == "xl/workbook.xml"
            || lower == "xl/styles.xml"
            || lower == "xl/sharedstrings.xml"
            || lower == "xl/calcchain.xml"
        {
            return None;
        } else {
            Kept::Other
        },
    )
}

/// What a save did, for the save report and for the check that follows it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SaveReport {
    /// Parts copied as the bytes they were.
    pub kept_parts: usize,
    pub changed_parts: Vec<String>,
    pub removed_parts: Vec<String>,
    pub added_parts: Vec<String>,
    pub cells_written: u64,
    pub strings_added: usize,
    pub recalculates_on_open: bool,
    pub pivots_refresh_on_open: bool,
    /// New results the file can't hold, which Excel works out when it opens the file.
    pub results_left_to_excel: u64,
    /// Table headers rewritten so that the table part and its cells agree.
    pub table_headers_fixed: u64,
    /// What the workbook holds that this app keeps without editing, with how many parts.
    pub kept_content: Vec<(Kept, usize)>,
    /// Extensions in sheets that a change to rows or columns can't reach, so they may now point
    /// at the wrong cells, by their identifiers.
    pub left_extensions: Vec<String>,
}

/// What a saved file must hold: a part's name to the sum and length of each entry with that name.
/// A file can name a part twice, and it is copied as it is.
pub type SavedParts = HashMap<String, Vec<(u32, u64)>>;

/// A saved part's expected state, kept until the file has been checked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SavePlan {
    pub report: SaveReport,
    expected: SavedParts,
    changed: Vec<String>,
    /// Sum of the source's entries that were copied, in their directory.
    copied: Vec<(String, u32, u64)>,
    /// New sheets' parts, by the sheet's index, and which sheet each `<sheet>` element now holds.
    pub new_parts: Vec<(u32, String)>,
    /// New tables' parts, by their sheet and name.
    pub table_parts: Vec<(u32, String, String)>,
    pub file_sheets: Vec<u32>,
}

/// A failure reading a part while patching it. Its bytes not making sense says the file is damaged,
/// and anything else says it can't be read.
fn part_error(error: io::Error) -> SaveError {
    if error.kind() == io::ErrorKind::InvalidData {
        SaveError::Damaged(error.to_string())
    } else {
        SaveError::read(error)
    }
}

fn zip_error(error: io::Error) -> SaveError {
    SaveError::Damaged(error.to_string())
}

/// Packs a small part, which `Package::read` has decoded if it was in UTF-16.
fn pack_part(xml: &[u8]) -> Result<Packed, SaveError> {
    pack(&utf8_part(xml.to_vec())).map_err(zip_error)
}

/// The workbook part's name, and its relationships part's.
fn main_part(package: &Package) -> Option<String> {
    let rels = read_relationships(package, "");
    rels.into_values()
        .find(|(kind, _)| kind.ends_with("/officeDocument"))
        .map(|(_, target)| target)
        .filter(|target| package.part(target).is_some())
        .or_else(|| {
            package
                .parts()
                .iter()
                .map(|part| part.name.replace('\\', "/"))
                .find(|name| name.eq_ignore_ascii_case("xl/workbook.xml"))
        })
}

/// A part's encoding, from its first bytes: whether anything we add must be plain ASCII.
fn declared_other_encoding(stream: PartStream) -> io::Result<bool> {
    let mut scanner = Scanner::new(stream);
    while let Some(kind) = scanner.next()? {
        match kind {
            Kind::Other if scanner.token().starts_with(b"<?xml") => {
                return Ok(attr_value(&pi_as_tag(scanner.token()), b"encoding")
                    .is_some_and(|name| !is_utf(name)));
            }
            Kind::Start | Kind::Empty => return Ok(false),
            _ => {}
        }
    }
    Ok(false)
}

/// `<?xml a="b"?>` as `<xml a="b">`, which the tag tools can read.
fn pi_as_tag(token: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(token.len());
    out.push(b'<');
    out.extend_from_slice(&token[2..token.len() - 2]);
    out.push(b'>');
    out
}

fn is_utf(name: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(b"utf-8")
        || lower.starts_with(b"utf8")
        || lower.starts_with(b"utf-16")
        || lower.starts_with(b"utf16")
}

/// A name for a new part beside the workbook part. A file can hold a part nothing links to, which
/// isn't ours to replace, so the name has to be free.
fn free_part_name(package: &Package, main: &str, base: &str) -> String {
    let folder = main.rsplit_once('/').map_or("", |(folder, _)| folder);
    let path = |file: String| {
        if folder.is_empty() {
            file
        } else {
            format!("{folder}/{file}")
        }
    };
    let mut name = path(format!("{base}.xml"));
    let mut number = 1;
    while package.part(&name).is_some() {
        name = path(format!("{base}{number}.xml"));
        number += 1;
    }
    name
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, file)| file)
}

/// What saving the model's tables does to the package.
#[derive(Debug, Default)]
struct TableSave {
    added: Vec<String>,
    removed: Vec<String>,
    parts: Vec<(u32, String, String)>,
}

/// Tables the model made, changed or dropped: new parts for new ones, patches for changed ones,
/// and parts taken out for ones that are cells again, with each sheet's relationships and
/// `<tableParts>` to match. Tables that only moved with rows and columns are left to the shape pass.
fn save_tables(
    package: &Package,
    book: &Book,
    edits: &mut WorkbookEdits,
    overlay: &mut BTreeMap<String, Vec<u8>>,
    already_removed: &[String],
    changes: &mut ZipChanges,
    report: &mut SaveReport,
) -> Result<TableSave, SaveError> {
    let mut save = TableSave::default();
    let mut taken: HashSet<String> = package
        .parts()
        .iter()
        .map(|part| normalize(&part.name))
        .collect();
    for (index, info) in book.sheets.iter().enumerate() {
        let sheet = index as u32;
        let Some(part) = info.part.as_ref().filter(|_| !info.deleted) else {
            continue;
        };
        let rels_name = rels_path(part);
        let rels_had = overlay
            .get(&rels_name)
            .cloned()
            .or_else(|| package.read(&rels_name));
        let mut rels = rels_had.clone().unwrap_or_else(|| {
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec()
        });
        let file_tables: Vec<(String, String)> = crate::workbook::relationships_in(&rels, part)
            .into_iter()
            .filter(|(_, (kind, _))| kind.ends_with("/table"))
            .map(|(id, (_, target))| (id, target))
            .collect();
        let mine: Vec<&cgs_model::Table> = book
            .tables
            .iter()
            .filter(|table| table.sheet == sheet)
            .collect();
        let mut sheet_changed = false;
        let mut gone_ids = Vec::new();
        for (id, target) in &file_tables {
            let kept = mine
                .iter()
                .any(|table| table.part.as_deref().map(normalize) == Some(normalize(target)));
            if kept || already_removed.contains(target) {
                continue;
            }
            gone_ids.push(id.clone());
            changes.remove.push(normalize(target));
            report.removed_parts.push(target.clone());
            save.removed.push(target.clone());
            let table_rels = rels_path(target);
            if package.part(&table_rels).is_some() {
                changes.remove.push(normalize(&table_rels));
                report.removed_parts.push(table_rels);
            }
            overlay.remove(target);
            sheet_changed = true;
        }
        if !gone_ids.is_empty() {
            rels = remove_relationship_ids(&rels, &gone_ids);
        }
        for table in &mine {
            match &table.part {
                Some(target) if table.edited => {
                    let Some(xml) = overlay
                        .get(target)
                        .cloned()
                        .or_else(|| package.read(target))
                    else {
                        continue;
                    };
                    let patched =
                        crate::tables::patch_table_part(&xml, table).map_err(part_error)?;
                    if patched != xml {
                        overlay.insert(target.clone(), patched);
                    }
                }
                Some(_) => {}
                None => {
                    let mut number = 1;
                    let name = loop {
                        let name = format!("xl/tables/table{number}.xml");
                        if !taken.contains(&normalize(&name)) {
                            break name;
                        }
                        number += 1;
                    };
                    taken.insert(normalize(&name));
                    let xml = crate::tables::new_table_part(table);
                    changes
                        .add
                        .push((name.clone(), pack(xml.as_bytes()).map_err(zip_error)?));
                    report.added_parts.push(name.clone());
                    let target = relative_target(part, &name);
                    let (updated, _) =
                        add_relationship_with_id(&rels, crate::tables::TABLE_REL, &target);
                    rels = updated;
                    save.added.push(name.clone());
                    save.parts.push((sheet, table.name.clone(), name));
                    sheet_changed = true;
                }
            }
        }
        if !sheet_changed {
            continue;
        }
        if rels_had.is_some() {
            overlay.insert(rels_name, rels.clone());
        } else {
            changes
                .add
                .push((rels_name.clone(), pack(&rels).map_err(zip_error)?));
            report.added_parts.push(rels_name);
        }
        let mut ids: Vec<String> = crate::workbook::relationships_in(&rels, part)
            .into_iter()
            .filter(|(_, (kind, _))| kind.ends_with("/table"))
            .map(|(id, _)| id)
            .collect();
        ids.sort();
        let xml = if ids.is_empty() {
            String::new()
        } else {
            let mut xml = format!(
                "<tableParts count=\"{}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
                ids.len()
            );
            for id in &ids {
                xml.push_str(&format!("<tablePart r:id=\"{}\"/>", escape_attr(id)));
            }
            xml.push_str("</tableParts>");
            xml
        };
        edits.elements.entry(sheet).or_default().push(NewElement {
            name: "tableParts".to_string(),
            xml,
            replace: true,
        });
    }
    Ok(save)
}

#[derive(Default)]
struct NotesSave {
    /// New parts, with their content types.
    added: Vec<(String, &'static str)>,
    removed: Vec<String>,
}

fn legacy_drawing(id: &str) -> NewElement {
    NewElement {
        name: "legacyDrawing".to_string(),
        xml: format!("<legacyDrawing xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"{}\"/>", escape_attr(id)),
        replace: true,
    }
}

/// A new part's name, `stem` and the first number free.
fn fresh_part(taken: &mut HashSet<String>, stem: &str, ext: &str) -> String {
    let mut number = 1;
    loop {
        let name = format!("{stem}{number}.{ext}");
        if taken.insert(normalize(&name)) {
            break name;
        }
        number += 1;
    }
}

/// Notes and hyperlinks as the model has them: each changed sheet's comments part and VML drawing
/// made, replaced or taken out, its hyperlinks' relationships made again, and its `legacyDrawing`
/// and `hyperlinks` to match. A VML drawing's other shapes, such as form controls, stay.
fn save_notes(
    package: &Package,
    book: &Book,
    edits: &mut WorkbookEdits,
    overlay: &mut BTreeMap<String, Vec<u8>>,
    changes: &mut ZipChanges,
    report: &mut SaveReport,
) -> Result<NotesSave, SaveError> {
    let mut save = NotesSave::default();
    let mut taken: HashSet<String> = package
        .parts()
        .iter()
        .map(|part| normalize(&part.name))
        .collect();
    let sheets: std::collections::BTreeSet<u32> = edits
        .notes_sheets
        .iter()
        .chain(edits.link_sheets.iter())
        .copied()
        .collect();
    for sheet in sheets {
        let Some(info) = book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
            continue;
        };
        let Some(part) = info.part.as_ref() else {
            continue;
        };
        let rels_name = rels_path(part);
        let rels_had = overlay
            .get(&rels_name)
            .cloned()
            .or_else(|| package.read(&rels_name));
        let mut rels = rels_had.clone().unwrap_or_else(|| {
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec()
        });
        let list = crate::workbook::relationships_in(&rels, part);
        let mut elements = Vec::new();
        if edits.notes_sheets.contains(&sheet) {
            let find = |end: &str| {
                list.iter()
                    .find(|(_, (kind, _))| kind.ends_with(end))
                    .map(|(id, (_, target))| (id.clone(), target.clone()))
            };
            let comments = find("/comments");
            let vml = find("/vmlDrawing");
            let mut gone = Vec::new();
            let mut drop_part = |target: &str, overlay: &mut BTreeMap<String, Vec<u8>>| {
                changes.remove.push(normalize(target));
                report.removed_parts.push(target.to_string());
                save.removed.push(target.to_string());
                overlay.remove(target);
            };
            if info.notes.is_empty() {
                if let Some((id, target)) = &comments {
                    drop_part(target, overlay);
                    gone.push(id.clone());
                }
            }
            let existing = vml.as_ref().and_then(|(_, target)| {
                overlay
                    .get(target)
                    .cloned()
                    .or_else(|| package.read(target))
            });
            let drawing = crate::notes::vml_xml(&info.notes, existing.as_deref());
            let has_shapes = drawing.contains("<v:shape ") || drawing.contains("<v:shape>");
            match (&vml, has_shapes) {
                (Some((id, target)), false) => {
                    drop_part(target, overlay);
                    gone.push(id.clone());
                    elements.push(NewElement {
                        name: "legacyDrawing".to_string(),
                        xml: String::new(),
                        replace: true,
                    });
                }
                (Some((id, target)), true) => {
                    overlay.insert(target.clone(), drawing.into_bytes());
                    elements.push(legacy_drawing(id));
                }
                (None, true) => {
                    let name = fresh_part(&mut taken, "xl/drawings/vmlDrawing", "vml");
                    changes
                        .add
                        .push((name.clone(), pack(drawing.as_bytes()).map_err(zip_error)?));
                    report.added_parts.push(name.clone());
                    save.added.push((name.clone(), crate::notes::VML_TYPE));
                    let (updated, id) = add_relationship_with_id(
                        &rels,
                        crate::notes::VML_REL,
                        &relative_target(part, &name),
                    );
                    rels = updated;
                    elements.push(legacy_drawing(&id));
                }
                (None, false) => {}
            }
            if !info.notes.is_empty() {
                let xml = crate::notes::comments_xml(&info.notes).into_bytes();
                match &comments {
                    Some((_, target)) => {
                        overlay.insert(target.clone(), xml);
                    }
                    None => {
                        let name = fresh_part(&mut taken, "xl/comments", "xml");
                        changes
                            .add
                            .push((name.clone(), pack(&xml).map_err(zip_error)?));
                        report.added_parts.push(name.clone());
                        save.added.push((name.clone(), crate::notes::COMMENTS_TYPE));
                        let (updated, _) = add_relationship_with_id(
                            &rels,
                            crate::notes::COMMENTS_REL,
                            &relative_target(part, &name),
                        );
                        rels = updated;
                    }
                }
            }
            if !gone.is_empty() {
                rels = remove_relationship_ids(&rels, &gone);
            }
        }
        if edits.link_sheets.contains(&sheet) {
            rels = remove_relationships(&rels, "/hyperlink");
            let mut links = Vec::new();
            for link in &info.links {
                let id = match &link.target {
                    Some(target) => {
                        let (updated, id) =
                            add_external_relationship(&rels, crate::notes::LINK_REL, target);
                        rels = updated;
                        Some(id)
                    }
                    None => None,
                };
                links.push((link.clone(), id));
            }
            elements.push(NewElement {
                name: "hyperlinks".to_string(),
                xml: crate::notes::hyperlinks_xml(&links),
                replace: true,
            });
        }
        if rels_had.as_deref() != Some(&rels[..]) {
            if rels_had.is_some() {
                overlay.insert(rels_name, rels);
            } else {
                changes
                    .add
                    .push((rels_name.clone(), pack(&rels).map_err(zip_error)?));
                report.added_parts.push(rels_name);
            }
        }
        edits.elements.entry(sheet).or_default().extend(elements);
    }
    Ok(save)
}

/// A part's path as a relationship from `from` names it.
fn relative_target(from: &str, to: &str) -> String {
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

/// Text a table header holds, which Excel requires to be a string that matches the table part.
fn header_text(value: &CalcValue) -> String {
    match value {
        CalcValue::Empty | CalcValue::Error(_) => String::new(),
        CalcValue::Number(number) => crate::patch_sheet::number_text(*number),
        CalcValue::Bool(true) => "TRUE".to_string(),
        CalcValue::Bool(false) => "FALSE".to_string(),
        CalcValue::Text(text) => text.clone(),
    }
}

/// Keeps table parts and their header cells agreeing, as Excel repairs a file where they don't.
/// A header that is empty, or matches another, gets a name of Excel's form.
fn sync_table_headers(
    package: &Package,
    book: &Book,
    edits: &mut WorkbookEdits,
    overlay: &mut BTreeMap<String, Vec<u8>>,
    report: &mut SaveReport,
) -> Result<(), SaveError> {
    for (&sheet, patches) in &mut edits.cells {
        let Some(part) = book
            .sheets
            .get(sheet as usize)
            .and_then(|sheet| sheet.part.clone())
        else {
            continue;
        };
        for (kind, target) in read_relationships(package, &part).into_values() {
            if !kind.ends_with("/table") {
                continue;
            }
            // A table the model changed is written from the model, whose names follow its cells.
            if edits.tables_changed
                && book.tables.iter().any(|table| {
                    table.edited && table.part.as_deref().map(normalize) == Some(normalize(&target))
                })
            {
                continue;
            }
            let Some(xml) = overlay
                .get(&target)
                .cloned()
                .or_else(|| package.read(&target))
            else {
                continue;
            };
            let Some(table) = read_table_part(&xml) else {
                continue;
            };
            let Some(area) = parse_rect(&table.reference) else {
                continue;
            };
            if table.header_rows == 0 {
                continue;
            }
            let row = area.top;
            let mut names: Vec<(usize, String)> = Vec::new();
            let mut current = table.columns.clone();
            for (index, column) in (area.left..=area.right).enumerate() {
                let Some(patch) = patches.get(&(row, column)) else {
                    continue;
                };
                let wanted = match &patch.change {
                    CellChange::Keep
                    | CellChange::Result(_)
                    | CellChange::ArrayResult { .. }
                    | CellChange::Spilled(_) => continue,
                    CellChange::Clear => String::new(),
                    CellChange::Value(value) => header_text(value),
                    CellChange::Formula { value, .. } => header_text(value),
                };
                if index < current.len() {
                    current[index] = wanted.clone();
                    names.push((index, wanted));
                }
            }
            if names.is_empty() {
                continue;
            }
            for (index, name) in &mut names {
                let clash = |candidate: &str, current: &[String], at: usize| {
                    current
                        .iter()
                        .enumerate()
                        .any(|(other, text)| other != at && text.eq_ignore_ascii_case(candidate))
                };
                if name.is_empty() || clash(name, &current, *index) {
                    let base = if name.is_empty() {
                        "Column".to_string()
                    } else {
                        name.clone()
                    };
                    let mut number = *index + 1;
                    let mut candidate = format!("{base}{number}");
                    while clash(&candidate, &current, *index) {
                        number += 1;
                        candidate = format!("{base}{number}");
                    }
                    *name = candidate;
                    current[*index] = name.clone();
                }
                let column = area.left + *index as u32;
                if let Some(patch) = patches.get_mut(&(row, column)) {
                    patch.change = CellChange::Value(CalcValue::Text(name.clone()));
                }
                report.table_headers_fixed += 1;
            }
            let updated = set_table_columns(&xml, &names);
            // Formulas the part holds for its columns follow the new names, or Excel refuses it.
            let renames: Vec<(String, String)> = names
                .iter()
                .filter_map(|(index, name)| {
                    let old = table.columns.get(*index)?;
                    (old != name).then(|| (old.clone(), name.clone()))
                })
                .collect();
            let updated = if renames.is_empty() {
                updated
            } else {
                crate::patch_parts::rename_in_table_formulas(
                    &updated,
                    &table.display_name,
                    &renames,
                )
            };
            overlay.insert(target, updated);
        }
    }
    let _ = report;
    Ok(())
}

/// A part's own relationships, as (id, type, target).
fn parts_linked_from(package: &Package, part: &str) -> Vec<String> {
    read_relationships(package, part)
        .into_values()
        .map(|(_, target)| target)
        .collect()
}

/// Every part reachable from the package's root, without going through `skip`.
fn reachable(package: &Package, skip: &HashSet<String>) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut waiting = parts_linked_from(package, "");
    while let Some(part) = waiting.pop() {
        let key = normalize(&part);
        if skip.contains(&key) || !seen.insert(key) {
            continue;
        }
        if package.part(&part).is_some() {
            waiting.extend(parts_linked_from(package, &part));
        }
    }
    seen
}

/// A worksheet for a new sheet, or a sheet that lost its part.
fn blank_sheet() -> Vec<u8> {
    b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><sheetData/></worksheet>".to_vec()
}

/// Puts `<sheets>` and everything that counts sheets by position in `workbook.xml` right.
fn rewrite_workbook_part(
    xml: &[u8],
    book: &Book,
    ops: &[BookOp],
    new_ids: &HashMap<u32, String>,
) -> Result<(Vec<u8>, Vec<String>), SaveError> {
    let edits = book_edits(ops);
    let in_file = book.sheets_in_file();
    let order = book.tab_order();
    // Each sheet's new position, by its old one.
    let moved: Vec<Option<u32>> = in_file
        .iter()
        .map(|sheet| {
            order
                .iter()
                .position(|other| other == sheet)
                .map(|at| at as u32)
        })
        .collect();
    let mut gone_ids = Vec::new();
    let mut nodes = parse_tree(xml).map_err(part_error)?;
    let mut failed = false;
    for node in &mut nodes {
        let Node::Element(root) = node else {
            continue;
        };
        let root_tag = String::from_utf8_lossy(&root.tag).into_owned();
        for child in root.elements_mut() {
            match child.local() {
                b"sheets" => {
                    let prefix = crate::scan::prefix(&child.tag);
                    let mut known: Vec<Option<Element>> = Vec::new();
                    let mut id_attr = "r:id".to_string();
                    let mut next_id = 0;
                    for inner in std::mem::take(&mut child.children) {
                        if let Node::Element(sheet) = inner {
                            next_id = next_id
                                .max(sheet.attr(b"sheetId").and_then(parse_u32).unwrap_or(0));
                            for name in sheet_id_attrs(&sheet.tag) {
                                id_attr = name;
                            }
                            known.push(Some(sheet));
                        }
                    }
                    for (position, &stable) in in_file.iter().enumerate() {
                        let deleted = !order.contains(&stable);
                        if deleted {
                            if let Some(id) = known
                                .get(position)
                                .and_then(Option::as_ref)
                                .and_then(|sheet| sheet.attr(id_attr.as_bytes()))
                            {
                                gone_ids.push(unescape_attr(id));
                            }
                        }
                    }
                    for &stable in &order {
                        let info = &book.sheets[stable as usize];
                        let found = in_file.iter().position(|&sheet| sheet == stable);
                        let element = match found.and_then(|at| known.get_mut(at)?.take()) {
                            Some(mut sheet) => {
                                let name = sheet.attr_text(b"name");
                                if name.as_deref() != Some(info.name.as_str()) {
                                    let escaped = escape_attr(&info.name);
                                    sheet.set_attr("name", Some(&escaped));
                                }
                                sheet
                            }
                            None => {
                                next_id += 1;
                                let id = new_ids.get(&stable).cloned().unwrap_or_default();
                                // Some writers declare the relationship prefix on each sheet and
                                // not at the top, which leaves a new sheet needing its own.
                                let declared = id_attr
                                    .split_once(':')
                                    .map(|(name, _)| format!("xmlns:{name}="))
                                    .filter(|needed| !root_tag.contains(needed))
                                    .map(|needed| format!(" {needed}\"{RELATIONSHIP_NAMESPACE}\""))
                                    .unwrap_or_default();
                                Element {
                                    tag: format!(
                                        "<{prefix}sheet name=\"{}\" sheetId=\"{next_id}\" {id_attr}=\"{id}\"{declared}/>",
                                        escape_attr(&info.name)
                                    )
                                    .into_bytes(),
                                    empty: true,
                                    children: Vec::new(),
                                    end: Vec::new(),
                                }
                            }
                        };
                        child.children.push(Node::Element(element));
                    }
                }
                b"definedNames" => {
                    let before = child.elements().count();
                    child.children.retain_mut(|inner| {
                        let Node::Element(name) = inner else {
                            return true;
                        };
                        if let Some(old) = name.attr(b"localSheetId").and_then(parse_u32) {
                            match moved.get(old as usize).copied().flatten() {
                                Some(next) => {
                                    if next != old {
                                        let text = next.to_string();
                                        name.set_attr("localSheetId", Some(&text));
                                    }
                                }
                                None => return false,
                            }
                        }
                        let text = name.text();
                        if let std::borrow::Cow::Owned(next) =
                            cgs_refs::apply_edits(&text, "", &edits)
                        {
                            name.set_text(&next);
                        }
                        true
                    });
                    if before > 0 && child.elements().count() == 0 {
                        failed = true;
                    }
                }
                b"bookViews" => {
                    for view in child.elements_mut() {
                        for attr in ["activeTab", "firstSheet"] {
                            let Some(old) = view.attr(attr.as_bytes()).and_then(parse_u32) else {
                                continue;
                            };
                            let next = moved.get(old as usize).copied().flatten().unwrap_or(0);
                            if next != old {
                                let text = next.to_string();
                                view.set_attr(attr, Some(&text));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if failed {
            // A workbook can't have an empty list of names.
            root.children.retain(|inner| {
                !matches!(inner, Node::Element(element) if element.local() == b"definedNames")
            });
        }
    }
    let mut out = Vec::with_capacity(xml.len());
    write_tree(&nodes, &mut out);
    Ok((out, gone_ids))
}

/// `definedNames` brought into step with the model's names: changed ones get their new text, gone
/// ones go and new ones are added, and the rest stay as they were with whatever else they say.
fn patch_defined_names(xml: &[u8], book: &Book, reordered: bool) -> io::Result<Vec<u8>> {
    let order = if reordered {
        book.tab_order()
    } else {
        book.sheets_in_file()
    };
    let local_id = |sheet: u32| {
        order
            .iter()
            .position(|&other| other == sheet)
            .map(|at| at as u32)
    };
    let mut nodes = parse_tree(xml)?;
    for node in &mut nodes {
        let Node::Element(root) = node else { continue };
        if root.local() != b"workbook" {
            continue;
        }
        let prefix = crate::scan::prefix(&root.tag);
        let mut handled: Vec<usize> = Vec::new();
        let at = root
            .children
            .iter()
            .position(|child| matches!(child, Node::Element(e) if e.local() == b"definedNames"));
        let mut list = match at {
            Some(at) => match root.children.remove(at) {
                Node::Element(element) => element,
                Node::Text(_) => unreachable!(),
            },
            None => Element {
                tag: format!("<{prefix}definedNames>").into_bytes(),
                empty: false,
                children: Vec::new(),
                end: format!("</{prefix}definedNames>").into_bytes(),
            },
        };
        list.children.retain_mut(|child| {
            let Node::Element(element) = child else {
                return true;
            };
            let name = element.attr_text(b"name").unwrap_or_default();
            let sheet = element
                .attr(b"localSheetId")
                .and_then(crate::xml::parse_u32)
                .and_then(|id| order.get(id as usize).copied());
            let found = book
                .names
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(&name) && n.local_sheet == sheet);
            match found {
                Some(index) => {
                    handled.push(index);
                    let wanted = &book.names[index].formula;
                    if element.text() != *wanted {
                        element.set_text(wanted);
                    }
                    if book.names[index].name != name {
                        let escaped = escape_attr(&book.names[index].name);
                        element.set_attr("name", Some(&escaped));
                    }
                    true
                }
                None => false,
            }
        });
        for (index, name) in book.names.iter().enumerate() {
            if handled.contains(&index) {
                continue;
            }
            let local = name
                .local_sheet
                .and_then(local_id)
                .map(|id| format!(" localSheetId=\"{id}\""))
                .unwrap_or_default();
            let mut text = Vec::new();
            crate::scan::escape_text(&name.formula, &mut text);
            let tag = format!(
                "<{prefix}definedName name=\"{}\"{local}>",
                escape_attr(&name.name)
            );
            list.children.push(Node::Element(Element {
                tag: tag.into_bytes(),
                empty: false,
                children: vec![Node::Text(text)],
                end: format!("</{prefix}definedName>").into_bytes(),
            }));
        }
        if list.elements().count() == 0 {
            continue;
        }
        if list.empty {
            list.empty = false;
        }
        // After sheets and the external references, before calcPr.
        let place = root
            .children
            .iter()
            .position(|child| {
                matches!(child, Node::Element(e) if matches!(e.local(), b"calcPr" | b"oleSize" | b"customWorkbookViews" | b"pivotCaches" | b"smartTagPr" | b"smartTagTypes" | b"webPublishing" | b"fileRecoveryPr" | b"webPublishObjects" | b"extLst"))
            })
            .unwrap_or(root.children.len());
        root.children.insert(place, Node::Element(list));
    }
    let mut out = Vec::with_capacity(xml.len() + 64);
    write_tree(&nodes, &mut out);
    Ok(out)
}

const RELATIONSHIP_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The attribute names on a `<sheet>` tag that hold its relationship: `r:id`, whatever the prefix.
fn sheet_id_attrs(tag: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(tag);
    text.split_ascii_whitespace()
        .filter_map(|part| part.split_once('=').map(|(name, _)| name.to_string()))
        .filter(|name| name.ends_with(":id"))
        .collect()
}

/// Everything about a save that depends on the workbook's shape having changed.
#[derive(Default)]
struct Structure {
    /// Worksheets as they stand after the shape changes, which cell patches then apply to.
    sheets: HashMap<String, Packed>,
    /// New sheets' parts, by index.
    new_parts: BTreeMap<u32, String>,
    new_ids: HashMap<u32, String>,
    /// Parts to take out, and their relationships.
    removed: Vec<String>,
}

/// Rewrites every part that refers to cells whose place has changed.
fn apply_structure(
    package: &Package,
    book: &Book,
    main: &str,
    edits: &WorkbookEdits,
    overlay: &mut BTreeMap<String, Vec<u8>>,
    report: &mut SaveReport,
) -> Result<Structure, SaveError> {
    let mut done = Structure::default();
    let ops = &edits.ops;
    let list = book_edits(ops);
    // Names as the file has them: what each edit's list calls a sheet to begin with.
    let mut file_names: HashMap<u32, String> = HashMap::new();
    for (index, info) in book.sheets.iter().enumerate() {
        file_names.insert(index as u32, info.name.clone());
    }
    for op in ops.iter().rev() {
        if let BookOp::Rename { sheet, old, .. } = op {
            file_names.insert(*sheet, old.clone());
        }
    }
    let any_axis = ops.iter().any(|op| matches!(op, BookOp::Axis { .. }));
    let mut deleted_parts = HashSet::new();
    for (index, info) in book.sheets.iter().enumerate() {
        let Some(part) = info.part.as_ref() else {
            continue;
        };
        if info.deleted {
            deleted_parts.insert(normalize(part));
            continue;
        }
        let own = own_edits(ops, index as u32);
        let all_own = all_own_edits(ops, index as u32);
        let home = file_names
            .get(&(index as u32))
            .cloned()
            .unwrap_or_else(|| info.name.clone());
        // Tables whose whole area has gone take their parts with them.
        let mut gone_tables = Vec::new();
        for (id, (kind, target)) in read_relationships(package, part) {
            if !kind.ends_with("/table") {
                continue;
            }
            let Some(table) = package.read(&target).and_then(|xml| read_table_part(&xml)) else {
                continue;
            };
            let Some(area) = parse_rect(&table.reference) else {
                continue;
            };
            let bounds = (area.top, area.left, area.bottom, area.right);
            if crate::structure::moved_rect(bounds, &own).is_none() {
                gone_tables.push(id);
                done.removed.push(target.clone());
                let rels = rels_path(&target);
                if package.part(&rels).is_some() {
                    done.removed.push(rels);
                }
            }
        }
        if !gone_tables.is_empty() {
            let rels_name = rels_path(part);
            if let Some(rels) = package.read(&rels_name) {
                overlay.insert(rels_name, remove_relationship_ids(&rels, &gone_tables));
            }
        }
        let sheet_edits = SheetEdits {
            own: &own,
            all_own: &all_own,
            book: &list,
            home: &home,
            gone_tables: &gone_tables,
        };
        if info.kind == cgs_model::SheetKind::Worksheet {
            let mut masters = Masters::default();
            let mut packed = None;
            for pass in 0..2 {
                let stream = package
                    .stream(part)
                    .ok_or_else(|| SaveError::Damaged(format!("no {part}")))?;
                let mut packer = Packer::default();
                let result = rewrite_sheet(stream, &mut packer, &sheet_edits, &mut masters)
                    .map_err(part_error)?;
                for uri in &result.left_extensions {
                    if !report.left_extensions.contains(uri) {
                        report.left_extensions.push(uri.clone());
                    }
                }
                // A follower with no master anywhere stays as the file had it, on the second pass.
                if !result.again || pass == 1 {
                    if result.changed {
                        packed = Some(packer.finish().map_err(zip_error)?);
                    }
                    break;
                }
            }
            if let Some(packed) = packed {
                done.sheets.insert(part.clone(), packed);
            }
        }
        // What hangs off this sheet and holds cell addresses of its own.
        for (kind, target) in read_relationships(package, part).into_values() {
            let Some(xml) = package.read(&target) else {
                continue;
            };
            if done.removed.contains(&target) {
                continue;
            }
            let new = if kind.ends_with("/table") {
                let mut made = 0;
                rewrite_table(&xml, &sheet_edits, &mut |_| {
                    made += 1;
                    format!("New Column {made}")
                })
                .map_err(part_error)?
            } else if kind.ends_with("/comments") || kind.ends_with("/threadedComment") {
                rewrite_comments(&xml, &sheet_edits).map_err(part_error)?
            } else if kind.ends_with("/vmlDrawing") {
                rewrite_vml(&xml, &sheet_edits)
            } else if kind.ends_with("/drawing") {
                rewrite_drawing(&xml, &sheet_edits).map_err(part_error)?
            } else if kind.ends_with("/pivotTable") {
                rewrite_pivot_table(&xml, &sheet_edits).map_err(part_error)?
            } else {
                None
            };
            if let Some(new) = new {
                overlay.insert(target, new);
            }
        }
    }
    // Parts that read cells on any sheet.
    let mut has_pivots = false;
    for part in package.parts() {
        let lower = part.name.to_ascii_lowercase().replace('\\', "/");
        if !lower.ends_with(".xml") {
            continue;
        }
        let new = if lower.contains("/charts/chart") {
            package
                .read(&part.name)
                .map(|xml| rewrite_chart(&xml, &list))
                .transpose()
                .map_err(part_error)?
                .flatten()
        } else if lower.contains("/pivotcache/pivotcachedefinition") {
            has_pivots = true;
            package
                .read(&part.name)
                .map(|xml| rewrite_pivot_cache(&xml, &list))
                .transpose()
                .map_err(part_error)?
                .flatten()
        } else {
            None
        };
        if let Some(new) = new {
            overlay.insert(part.name.clone(), new);
        }
    }
    if has_pivots && any_axis {
        report.pivots_refresh_on_open = true;
    }
    // Sheets that have gone, and what only they used.
    if !deleted_parts.is_empty() {
        let before = reachable(package, &HashSet::new());
        let after = reachable(package, &deleted_parts);
        for part in package.parts() {
            let key = normalize(&part.name);
            if before.contains(&key) && !after.contains(&key) {
                done.removed.push(part.name.clone());
                let rels = rels_path(&part.name);
                if package.part(&rels).is_some() {
                    done.removed.push(rels);
                }
            }
        }
    }
    // New sheets get parts beside the workbook part.
    let folder = main.rsplit_once('/').map_or("", |(folder, _)| folder);
    let mut number = 1;
    for (index, info) in book.sheets.iter().enumerate() {
        if info.part.is_some() || info.deleted || info.kind != cgs_model::SheetKind::Worksheet {
            continue;
        }
        let name = loop {
            let candidate = if folder.is_empty() {
                format!("worksheets/sheet{number}.xml")
            } else {
                format!("{folder}/worksheets/sheet{number}.xml")
            };
            number += 1;
            if package.part(&candidate).is_none() {
                break candidate;
            }
        };
        done.new_parts.insert(index as u32, name);
    }
    Ok(done)
}

/// Finds or makes the cell metadata that marks a formula as a dynamic array, and returns the value
/// for a cell's `cm`, counted from 1. Excel shows a formula without it as `@` when it opens.
fn dynamic_array_metadata(existing: Option<&[u8]>) -> (Vec<u8>, u32) {
    const NEW: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<metadata xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:xda=\"http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray\"><metadataTypes count=\"1\"><metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/></metadataTypes><futureMetadata name=\"XLDAPR\" count=\"1\"><bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk></futureMetadata><cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata></metadata>";
    let Some(xml) = existing else {
        return (NEW.as_bytes().to_vec(), 1);
    };
    let text = String::from_utf8_lossy(xml);
    // Which metadata type this is, counted from 1, and which of its entries is dynamic.
    let mut type_index = None;
    for (index, part) in text.split("<metadataType ").skip(1).enumerate() {
        if part.starts_with("name=\"XLDAPR\"") || part.contains(" name=\"XLDAPR\"") {
            type_index = Some(index as u32 + 1);
        }
    }
    let Some(type_index) = type_index else {
        return append_dynamic_metadata(&text);
    };
    let Some(cells) = text.find("<cellMetadata") else {
        return append_dynamic_metadata(&text);
    };
    for (index, block) in text[cells..].split("<bk>").skip(1).enumerate() {
        if block.contains(&format!("<rc t=\"{type_index}\"")) {
            return (xml.to_vec(), index as u32 + 1);
        }
    }
    append_dynamic_metadata(&text)
}

/// Adds what a dynamic array needs to metadata that doesn't have it.
fn append_dynamic_metadata(text: &str) -> (Vec<u8>, u32) {
    const TYPE: &str = "<metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/>";
    const FUTURE: &str = "<futureMetadata name=\"XLDAPR\" count=\"1\"><bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties xmlns:xda=\"http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray\" fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk></futureMetadata>";
    let mut out = text.to_string();
    let types = out.matches("<metadataType ").count() as u32;
    let bump = |out: &mut String, element: &str, by: u32| {
        let at = out.find(&format!("<{element} "))?;
        let end = at + out[at..].find('>')?;
        let tag = out[at..end].to_string();
        let start = tag.find("count=\"")? + 7;
        let stop = start + tag[start..].find('"')?;
        let count: u32 = tag[start..stop].parse().ok()?;
        let new = format!("{}{}{}", &tag[..start], count + by, &tag[stop..]);
        out.replace_range(at..end, &new);
        Some(count)
    };
    bump(&mut out, "metadataTypes", 1);
    if let Some(at) = out.find("</metadataTypes>") {
        out.insert_str(at, TYPE);
    }
    // After the last `futureMetadata`, or the types.
    let place = out
        .rfind("</futureMetadata>")
        .map(|at| at + "</futureMetadata>".len())
        .or_else(|| {
            out.find("</metadataTypes>")
                .map(|at| at + "</metadataTypes>".len())
        })
        .unwrap_or(out.len());
    out.insert_str(place, FUTURE);
    let block = format!("<bk><rc t=\"{}\" v=\"0\"/></bk>", types + 1);
    let index = match bump(&mut out, "cellMetadata", 1) {
        Some(count) => {
            if let Some(at) = out.find("</cellMetadata>") {
                out.insert_str(at, &block);
            }
            count + 1
        }
        None => {
            let place = out
                .rfind("</futureMetadata>")
                .map_or(out.len(), |at| at + "</futureMetadata>".len());
            out.insert_str(
                place,
                &format!("<cellMetadata count=\"1\">{block}</cellMetadata>"),
            );
            1
        }
    };
    (out.into_bytes(), index)
}

/// A part that a structural pass packed and nothing else changed.
fn pack_inflated(packed: &Packed) -> Result<Packed, SaveError> {
    Ok(packed.clone())
}

/// Plans and writes a save of `package` with `edits` to `out`.
pub fn save_workbook(
    package: &Package,
    book: &Book,
    edits: &WorkbookEdits,
    out: &mut dyn Write,
) -> Result<SavePlan, SaveError> {
    let directory = Directory::read(package.source().as_ref())?;
    let mut changes = ZipChanges::default();
    let mut report = SaveReport::default();
    let mut file_sheets_after = None;
    let mut new_parts_after = Vec::new();
    let mut table_parts_after = Vec::new();
    if !edits.is_empty() {
        let main =
            main_part(package).ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?;
        let mut edits = edits.clone();
        let cells_change = edits.changes_cells();
        // Small parts as they stand so far, so that each step builds on the last.
        let mut overlay: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let structure = if edits.ops.is_empty() {
            Structure::default()
        } else {
            apply_structure(package, book, &main, &edits, &mut overlay, &mut report)?
        };
        sync_table_headers(package, book, &mut edits, &mut overlay, &mut report)?;
        let table_save = if edits.tables_changed {
            save_tables(
                package,
                book,
                &mut edits,
                &mut overlay,
                &structure.removed,
                &mut changes,
                &mut report,
            )?
        } else {
            TableSave::default()
        };
        let notes_save = if edits.notes_sheets.is_empty() && edits.link_sheets.is_empty() {
            NotesSave::default()
        } else {
            save_notes(
                package,
                book,
                &mut edits,
                &mut overlay,
                &mut changes,
                &mut report,
            )?
        };

        // Formulas that spill need Excel's mark for a dynamic array, once for the workbook.
        let wants_dynamic = edits
            .cells
            .values()
            .flat_map(BTreeMap::values)
            .any(|patch| {
                matches!(
                    patch.change,
                    CellChange::Formula {
                        array: ArrayKind::Dynamic(..),
                        ..
                    }
                )
            });

        // Shared strings: new cells add to them, and a missing part gets made.
        let workbook_rels = read_relationships(package, &main);
        let strings_part = workbook_rels
            .values()
            .find(|(kind, _)| kind.ends_with("/sharedStrings"))
            .map(|(_, target)| target.clone())
            .filter(|target| package.part(target).is_some());
        let mut strings = StringTable::new(
            match strings_part.as_deref().and_then(|p| package.stream(p)) {
                Some(stream) => count_strings(stream).map_err(SaveError::read)?,
                None => 0,
            },
        );

        // Dynamic arrays: the metadata their cells point at.
        let mut dynamic_part = None;
        if wants_dynamic {
            let found = workbook_rels
                .values()
                .find(|(kind, _)| kind.ends_with("/sheetMetadata"))
                .map(|(_, target)| target.clone())
                .filter(|target| package.part(target).is_some());
            let existing = found.as_deref().and_then(|part| package.read(part));
            let (xml, cm) = dynamic_array_metadata(existing.as_deref());
            for patch in edits.cells.values_mut().flat_map(BTreeMap::values_mut) {
                if let CellChange::Formula {
                    array: ArrayKind::Dynamic(_, index),
                    ..
                } = &mut patch.change
                {
                    *index = cm;
                }
            }
            dynamic_part = Some((
                found,
                (existing.as_deref() != Some(&xml[..])).then_some(xml),
            ));
        }

        // A sheet with changes apart from its cells goes through the same steps as one with cells.
        let extras = std::mem::take(&mut edits.extras);
        for (&sheet, extra) in &extras {
            if !extra.is_empty() {
                edits.cells.entry(sheet).or_default();
            }
        }
        let none_extra = SheetExtras::default();
        // Which parts already went out, as new ones or with the shape changes.
        let mut written: HashSet<u32> = HashSet::new();
        for (&sheet, patches) in &edits.cells {
            let info = book
                .sheets
                .get(sheet as usize)
                .ok_or_else(|| SaveError::Damaged(format!("no sheet {sheet}")))?;
            if info.deleted {
                continue;
            }
            written.insert(sheet);
            let elements = edits.elements.get(&sheet).map_or(&[][..], Vec::as_slice);
            let extra = extras.get(&sheet).unwrap_or(&none_extra);
            let Some(part) = info.part.clone() else {
                let name = structure
                    .new_parts
                    .get(&sheet)
                    .ok_or_else(|| SaveError::Damaged(format!("sheet {sheet} has no part")))?;
                let source = blank_sheet();
                let (packer, outcome) = patch_into(
                    || Ok(&source[..]),
                    Packer::default,
                    patches,
                    elements,
                    extra,
                    &mut strings,
                    false,
                )
                .map_err(part_error)?;
                report.cells_written += outcome.cells_written;
                report.results_left_to_excel += outcome.results_skipped;
                changes
                    .add
                    .push((name.clone(), packer.finish().map_err(zip_error)?));
                report.added_parts.push(name.clone());
                continue;
            };
            if package.part(&part).is_none() {
                return Err(SaveError::Damaged(format!("sheet {sheet} has no part")));
            }
            let ascii_only = package
                .stream(&part)
                .map(declared_other_encoding)
                .transpose()
                .map_err(SaveError::read)?
                .unwrap_or(false);
            if let Some(rewritten) = structure.sheets.get(&part) {
                let open = || Ok::<_, io::Error>(DeflateDecoder::new(&rewritten.data[..]));
                let (packed, outcome) =
                    patch_sheet(open, patches, elements, extra, &mut strings, ascii_only)
                        .map_err(part_error)?;
                report.cells_written += outcome.cells_written;
                report.results_left_to_excel += outcome.results_skipped;
                let packed = match packed {
                    Some(packed) => packed,
                    None => pack_inflated(rewritten)?,
                };
                changes.replace.insert(normalize(&part), packed);
                report.changed_parts.push(part);
                continue;
            }
            let open = || {
                package
                    .stream(&part)
                    .ok_or_else(|| io::Error::other("a part went missing"))
            };
            let (packed, outcome) =
                patch_sheet(open, patches, elements, extra, &mut strings, ascii_only)
                    .map_err(part_error)?;
            report.cells_written += outcome.cells_written;
            report.results_left_to_excel += outcome.results_skipped;
            if let Some(packed) = packed {
                changes.replace.insert(normalize(&part), packed);
                report.changed_parts.push(part);
            }
        }
        // Sheets whose only change is the shape, and new sheets that are still empty.
        for (index, info) in book.sheets.iter().enumerate() {
            let sheet = index as u32;
            if written.contains(&sheet) || info.deleted {
                continue;
            }
            if let Some(part) = info.part.as_ref() {
                if let Some(rewritten) = structure.sheets.get(part) {
                    changes
                        .replace
                        .insert(normalize(part), pack_inflated(rewritten)?);
                    report.changed_parts.push(part.clone());
                }
            } else if let Some(name) = structure.new_parts.get(&sheet) {
                changes
                    .add
                    .push((name.clone(), pack(&blank_sheet()).map_err(zip_error)?));
                report.added_parts.push(name.clone());
            }
        }
        // A sheet that gets new elements and no cells.
        for (&sheet, elements) in &edits.elements {
            if elements.is_empty() || edits.cells.contains_key(&sheet) {
                continue;
            }
            let info = book
                .sheets
                .get(sheet as usize)
                .ok_or_else(|| SaveError::Damaged(format!("no sheet {sheet}")))?;
            if info.deleted {
                continue;
            }
            written.insert(sheet);
            let none = BTreeMap::new();
            let Some(part) = info.part.clone() else {
                if let Some(name) = structure.new_parts.get(&sheet) {
                    let source = blank_sheet();
                    let (packer, _) = patch_into(
                        || Ok(&source[..]),
                        Packer::default,
                        &none,
                        elements,
                        &none_extra,
                        &mut strings,
                        false,
                    )
                    .map_err(part_error)?;
                    changes
                        .add
                        .push((name.clone(), packer.finish().map_err(zip_error)?));
                    report.added_parts.push(name.clone());
                }
                continue;
            };
            let packed = if let Some(rewritten) = structure.sheets.get(&part) {
                let open = || Ok::<_, io::Error>(DeflateDecoder::new(&rewritten.data[..]));
                patch_sheet(open, &none, elements, &none_extra, &mut strings, false)
                    .map_err(part_error)?
                    .0
            } else {
                let open = || {
                    package
                        .stream(&part)
                        .ok_or_else(|| io::Error::other("a part went missing"))
                };
                patch_sheet(open, &none, elements, &none_extra, &mut strings, false)
                    .map_err(part_error)?
                    .0
            };
            if let Some(packed) = packed {
                changes.replace.insert(normalize(&part), packed);
                report.changed_parts.push(part);
            }
        }

        // A package with no content types isn't one Excel would open, but we do, and it can be saved
        // as what it is.
        let had_types = package.part("[Content_Types].xml").is_some();
        let mut content_types = package.read("[Content_Types].xml").unwrap_or_default();
        let mut types_changed = false;
        let main_rels_name = rels_path(&main);
        let mut main_rels = package.read(&main_rels_name);
        let mut rels_changed = false;

        report.strings_added = strings.added.len();
        if !strings.added.is_empty() {
            match &strings_part {
                Some(part) => {
                    let ascii_only = package
                        .stream(part)
                        .map(declared_other_encoding)
                        .transpose()
                        .map_err(SaveError::read)?
                        .unwrap_or(false);
                    let stream = package
                        .stream(part)
                        .ok_or_else(|| SaveError::Damaged(format!("no {part}")))?;
                    let mut packer = Packer::default();
                    append_strings(stream, &strings, ascii_only, &mut packer)
                        .map_err(part_error)?;
                    changes
                        .replace
                        .insert(normalize(part), packer.finish().map_err(zip_error)?);
                    report.changed_parts.push(part.clone());
                }
                None => {
                    let name = free_part_name(package, &main, "sharedStrings");
                    changes.add.push((
                        name.clone(),
                        pack(&new_strings_part(&strings)).map_err(zip_error)?,
                    ));
                    content_types = add_override(&content_types, &name, STRINGS_TYPE);
                    types_changed = true;
                    if let Some(rels) = &mut main_rels {
                        *rels = add_relationship(rels, STRINGS_REL, file_name(&name));
                        rels_changed = true;
                    }
                    report.added_parts.push(name);
                }
            }
        }

        // Styles: the part as `StylesXml` holds it.
        if let Some(styles) = edits.styles.as_ref().filter(|styles| styles.changed) {
            let found = workbook_rels
                .values()
                .find(|(kind, _)| kind.ends_with("/styles"))
                .map(|(_, target)| target.clone())
                .filter(|target| package.part(target).is_some());
            let packed = pack_part(&styles.bytes)?;
            match found {
                Some(part) => {
                    changes.replace.insert(normalize(&part), packed);
                    report.changed_parts.push(part);
                }
                None => {
                    let name = free_part_name(package, &main, "styles");
                    changes.add.push((name.clone(), packed));
                    content_types = add_override(&content_types, &name, STYLES_TYPE);
                    types_changed = true;
                    if let Some(rels) = &mut main_rels {
                        *rels = add_relationship(rels, STYLES_REL, file_name(&name));
                        rels_changed = true;
                    }
                    report.added_parts.push(name);
                }
            }
        }

        // Tables made and taken out.
        for name in &table_save.added {
            content_types = add_override(&content_types, name, crate::tables::TABLE_TYPE);
            types_changed = true;
        }
        if !table_save.removed.is_empty() {
            content_types = remove_overrides(&content_types, &table_save.removed);
            types_changed = true;
        }
        for (name, kind) in &notes_save.added {
            content_types = add_override(&content_types, name, kind);
            types_changed = true;
        }
        if !notes_save.removed.is_empty() {
            content_types = remove_overrides(&content_types, &notes_save.removed);
            types_changed = true;
        }
        table_parts_after = table_save.parts;
        // Parts taken out, and new sheets linked in.
        if !structure.removed.is_empty() {
            for part in &structure.removed {
                changes.remove.push(normalize(part));
                report.removed_parts.push(part.clone());
            }
            content_types = remove_overrides(&content_types, &structure.removed);
            types_changed = true;
        }
        let mut new_ids = structure.new_ids.clone();
        for (&sheet, name) in &structure.new_parts {
            content_types = add_override(&content_types, name, WORKSHEET_TYPE);
            types_changed = true;
            if let Some(rels) = &mut main_rels {
                let folder = main.rsplit_once('/').map_or("", |(folder, _)| folder);
                let target = name
                    .strip_prefix(folder)
                    .map_or(name.as_str(), |rest| rest.trim_start_matches('/'));
                let (updated, id) = add_relationship_with_id(rels, WORKSHEET_REL, target);
                *rels = updated;
                rels_changed = true;
                new_ids.insert(sheet, id);
            }
        }

        match dynamic_part {
            Some((Some(part), Some(xml))) => {
                overlay.insert(part, xml);
            }
            Some((None, Some(xml))) => {
                let name = free_part_name(package, &main, "metadata");
                changes
                    .add
                    .push((name.clone(), pack(&xml).map_err(zip_error)?));
                content_types = add_override(&content_types, &name, METADATA_TYPE);
                types_changed = true;
                if let Some(rels) = &mut main_rels {
                    *rels = add_relationship(rels, METADATA_REL, file_name(&name));
                    rels_changed = true;
                }
                report.added_parts.push(name);
            }
            _ => {}
        }

        let mut main_xml = None;
        if let Some(manual) = edits.calc_mode {
            let xml = package
                .read(&main)
                .ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?;
            main_xml = Some(crate::patch_parts::set_calc_mode(&xml, manual));
        }
        if !edits.ops.is_empty() {
            let xml = match main_xml.take() {
                Some(xml) => xml,
                None => package
                    .read(&main)
                    .ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?,
            };
            let (xml, gone_ids) = rewrite_workbook_part(&xml, book, &edits.ops, &new_ids)?;
            if let (false, Some(rels)) = (gone_ids.is_empty(), &mut main_rels) {
                *rels = remove_relationship_ids(rels, &gone_ids);
                rels_changed = true;
            }
            main_xml = Some(xml);
        }
        if cells_change {
            // Excel rebuilds the calculation chain, and an out-of-date one makes it repair the file.
            let chain = workbook_rels
                .values()
                .find(|(kind, _)| kind.ends_with("/calcChain"))
                .map(|(_, target)| target.clone());
            let chain = chain.or_else(|| {
                package
                    .parts()
                    .iter()
                    .map(|part| part.name.replace('\\', "/"))
                    .find(|name| name.eq_ignore_ascii_case("xl/calcChain.xml"))
            });
            if let Some(chain) = chain {
                if package.part(&chain).is_some() {
                    changes.remove.push(normalize(&chain));
                    report.removed_parts.push(chain.clone());
                }
                content_types = remove_overrides(&content_types, &[chain]);
                types_changed = true;
                if let Some(rels) = &mut main_rels {
                    *rels = remove_relationships(rels, "/calcChain");
                    rels_changed = true;
                }
            }
            let xml = match main_xml.take() {
                Some(xml) => xml,
                None => package
                    .read(&main)
                    .ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?,
            };
            main_xml = Some(set_full_calc_on_load(&xml));
            report.recalculates_on_open = true;
        }
        if edits.names_changed {
            let xml = match main_xml.take() {
                Some(xml) => xml,
                None => package
                    .read(&main)
                    .ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?,
            };
            main_xml =
                Some(patch_defined_names(&xml, book, !edits.ops.is_empty()).map_err(part_error)?);
        }
        if let Some(xml) = main_xml {
            changes.replace.insert(normalize(&main), pack_part(&xml)?);
            report.changed_parts.push(main.clone());
        }
        if edits.refresh_pivots || report.pivots_refresh_on_open {
            for part in package.parts() {
                let lower = part.name.to_ascii_lowercase().replace('\\', "/");
                if lower.contains("/pivotcache/pivotcachedefinition") && lower.ends_with(".xml") {
                    let xml = overlay
                        .get(&part.name)
                        .cloned()
                        .or_else(|| package.read(&part.name))
                        .ok_or_else(|| SaveError::Damaged(format!("no {}", part.name)))?;
                    overlay.insert(part.name.clone(), set_refresh_on_load(&xml));
                    report.pivots_refresh_on_open = true;
                }
            }
        }
        for (part, xml) in &overlay {
            if structure.removed.contains(part) {
                continue;
            }
            changes.replace.insert(normalize(part), pack_part(xml)?);
            report.changed_parts.push(part.clone());
        }
        file_sheets_after = (!edits.ops.is_empty()).then(|| book.tab_order());
        new_parts_after = structure.new_parts.into_iter().collect();
        if types_changed && had_types {
            changes
                .replace
                .insert(normalize("[Content_Types].xml"), pack_part(&content_types)?);
            report.changed_parts.push("[Content_Types].xml".to_string());
        }
        if let (true, Some(rels)) = (rels_changed, &main_rels) {
            changes
                .replace
                .insert(normalize(&main_rels_name), pack_part(rels)?);
            report.changed_parts.push(main_rels_name.clone());
        }
    }

    rewrite(package.source().as_ref(), &directory, &changes, out)?;

    let mut kept: BTreeMap<Kept, usize> = BTreeMap::new();
    let mut plan = SavePlan::default();
    let changed: HashSet<&String> = changes.replace.keys().collect();
    for entry in &directory.entries {
        let key = normalize(&entry.name);
        if changes.remove.contains(&key) {
            continue;
        }
        if let Some(part) = changes.replace.get(&key) {
            plan.expected
                .insert(key.clone(), vec![(part.crc, part.size)]);
            plan.changed.push(key);
        } else {
            plan.expected
                .entry(key)
                .or_default()
                .push((entry.crc, entry.size));
            plan.copied
                .push((entry.name.clone(), entry.crc, entry.size));
            report.kept_parts += 1;
        }
        if !changed.contains(&normalize(&entry.name)) {
            if let Some(what) = classify(&entry.name) {
                *kept.entry(what).or_default() += 1;
            }
        }
    }
    for (name, part) in &changes.add {
        let key = normalize(name);
        plan.expected
            .insert(key.clone(), vec![(part.crc, part.size)]);
        plan.changed.push(key);
    }
    report.kept_content = kept.into_iter().collect();
    plan.report = report;
    plan.new_parts = new_parts_after;
    plan.table_parts = table_parts_after;
    plan.file_sheets = file_sheets_after.unwrap_or_default();
    Ok(plan)
}

/// Checks a saved file against its plan, before it replaces anything. Every part's sum and length
/// must be as planned, and each changed part must inflate to well-formed XML with that sum.
pub fn verify_saved(saved: &dyn ByteSource, plan: &SavePlan) -> Result<(), SaveError> {
    let directory = verify_directory(saved, plan)?;
    for entry in &directory.entries {
        let key = normalize(&entry.name);
        if !plan.changed.contains(&key) {
            continue;
        }
        check_changed_part(saved, entry)
            .map_err(|error| SaveError::Verify(format!("{}: {error}", entry.name)))?;
    }
    Ok(())
}

/// Checks that a file holds exactly the parts a save planned, by their sums and lengths.
pub fn verify_directory(saved: &dyn ByteSource, plan: &SavePlan) -> Result<Directory, SaveError> {
    let wrong = |what: String| SaveError::Verify(what);
    let directory = Directory::read(saved).map_err(|error| wrong(error.to_string()))?;
    let mut found = HashSet::new();
    for entry in &directory.entries {
        let key = normalize(&entry.name);
        let Some(expected) = plan.expected.get(&key) else {
            return Err(wrong(format!("{} shouldn't be there", entry.name)));
        };
        if !expected.contains(&(entry.crc, entry.size)) {
            return Err(wrong(format!("{} isn't what was written", entry.name)));
        }
        found.insert(key);
    }
    if found.len() != plan.expected.len() {
        return Err(wrong("a part is missing".to_string()));
    }
    Ok(directory)
}

/// Lets `DeflateDecoder` read one stretch of a `ByteSource`.
struct Span<'a> {
    source: &'a dyn ByteSource,
    at: u64,
    end: u64,
}

impl Read for Span<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.end {
            return Ok(0);
        }
        let room = ((self.end - self.at) as usize).min(out.len());
        let count = self.source.read_at(self.at, &mut out[..room])?;
        self.at += count as u64;
        Ok(count)
    }
}

/// Reads a part back: its sum must match, and XML must parse from start to end.
fn check_changed_part(saved: &dyn ByteSource, entry: &crate::zipdir::Entry) -> Result<(), String> {
    let mut head = [0; 30];
    let count = saved
        .read_at(entry.local, &mut head)
        .map_err(|error| error.to_string())?;
    if count < 30 || head[..4] != [0x50, 0x4b, 0x03, 0x04] {
        return Err("no header".to_string());
    }
    let names = u64::from(u16::from_le_bytes([head[26], head[27]]));
    let extras = u64::from(u16::from_le_bytes([head[28], head[29]]));
    let start = entry.local + 30 + names + extras;
    let inflater = DeflateDecoder::new(Span {
        source: saved,
        at: start,
        end: start + entry.compressed,
    });
    let mut checker = Summed {
        inner: inflater,
        crc: flate2::Crc::new(),
    };
    if entry.name.to_ascii_lowercase().ends_with(".bin") {
        io::copy(&mut checker, &mut io::sink()).map_err(|error| error.to_string())?;
    } else {
        check_xml(&mut checker)?;
        io::copy(&mut checker, &mut io::sink()).map_err(|error| error.to_string())?;
    }
    if checker.crc.sum() != entry.crc {
        return Err("its contents don't match its sum".to_string());
    }
    Ok(())
}

struct Summed<R> {
    inner: R,
    crc: flate2::Crc,
}

impl<R: Read> Read for Summed<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(out)?;
        self.crc.update(&out[..count]);
        Ok(count)
    }
}

/// Parses XML through to its end, with tags matched, which `quick-xml` checks for us.
fn check_xml(reader: &mut dyn Read) -> Result<(), String> {
    let mut xml = Reader::from_reader(io::BufReader::with_capacity(128 * 1024, reader));
    let mut buf = Vec::new();
    let mut depth = 0i64;
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => depth -= 1,
            Ok(_) => {}
            Err(error) => return Err(error.to_string()),
        }
        buf.clear();
    }
    if depth == 0 {
        Ok(())
    } else {
        Err("its elements aren't all closed".to_string())
    }
}

/// A copy of a part as `patch_into` would make it, for tests that read the XML.
pub fn patched_xml(
    source: &[u8],
    patches: &BTreeMap<(u32, u32), CellPatch>,
    elements: &[NewElement],
    strings: &mut StringTable,
) -> io::Result<Vec<u8>> {
    let (out, _) = patch_into(
        || Ok(source),
        Vec::new,
        patches,
        elements,
        &SheetExtras::default(),
        strings,
        false,
    )?;
    Ok(out)
}
