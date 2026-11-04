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
    add_override, add_relationship, append_strings, count_strings, new_strings_part,
    read_table_part, remove_overrides, remove_relationships, set_full_calc_on_load,
    set_refresh_on_load, set_table_columns, StylesXml,
};
use crate::patch_sheet::{patch_into, patch_sheet, CellChange, CellPatch, NewElement, StringTable};
use crate::scan::{attr_value, utf8_part, Kind, Scanner};
use crate::workbook::read_relationships;
use crate::xml::{parse_rect, rels_path};
use crate::zipdir::{normalize, pack, rewrite, Directory, Packed, Packer, ZipChanges};

const STRINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const STRINGS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const STYLES_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const STYLES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

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
}

impl WorkbookEdits {
    pub fn is_empty(&self) -> bool {
        self.cells.values().all(BTreeMap::is_empty)
            && self.elements.values().all(Vec::is_empty)
            && !self.styles.as_ref().is_some_and(|styles| styles.changed)
            && !self.refresh_pivots
    }

    /// Whether a cell's contents or result changed, rather than only how it looks.
    pub fn changes_cells(&self) -> bool {
        self.cells
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
    changes: &mut ZipChanges,
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
            let Some(xml) = package.read(&target) else {
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
                    CellChange::Keep | CellChange::Result(_) => continue,
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
            changes
                .replace
                .insert(normalize(&target), pack_part(&updated)?);
            report.changed_parts.push(target);
        }
    }
    Ok(())
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
    if !edits.is_empty() {
        let main =
            main_part(package).ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?;
        let mut edits = edits.clone();
        let cells_change = edits.changes_cells();
        sync_table_headers(package, book, &mut edits, &mut changes, &mut report)?;

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

        for (&sheet, patches) in &edits.cells {
            let elements = edits.elements.get(&sheet).map_or(&[][..], Vec::as_slice);
            let part = book
                .sheets
                .get(sheet as usize)
                .and_then(|sheet| sheet.part.clone())
                .ok_or_else(|| SaveError::Damaged(format!("sheet {sheet} has no part")))?;
            if package.part(&part).is_none() {
                return Err(SaveError::Damaged(format!("sheet {sheet} has no part")));
            }
            let ascii_only = package
                .stream(&part)
                .map(declared_other_encoding)
                .transpose()
                .map_err(SaveError::read)?
                .unwrap_or(false);
            let open = || {
                package
                    .stream(&part)
                    .ok_or_else(|| io::Error::other("a part went missing"))
            };
            let (packed, outcome) = patch_sheet(open, patches, elements, &mut strings, ascii_only)
                .map_err(part_error)?;
            report.cells_written += outcome.cells_written;
            report.results_left_to_excel += outcome.results_skipped;
            if let Some(packed) = packed {
                changes.replace.insert(normalize(&part), packed);
                report.changed_parts.push(part);
            }
        }
        // A sheet that gets new elements and no cells.
        for (&sheet, elements) in &edits.elements {
            if elements.is_empty() || edits.cells.contains_key(&sheet) {
                continue;
            }
            let part = book
                .sheets
                .get(sheet as usize)
                .and_then(|sheet| sheet.part.clone())
                .ok_or_else(|| SaveError::Damaged(format!("sheet {sheet} has no part")))?;
            let open = || {
                package
                    .stream(&part)
                    .ok_or_else(|| io::Error::other("a part went missing"))
            };
            let none = BTreeMap::new();
            let (packed, _) =
                patch_sheet(open, &none, elements, &mut strings, false).map_err(part_error)?;
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

        let mut main_xml = None;
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
            let xml = package
                .read(&main)
                .ok_or_else(|| SaveError::Damaged("no workbook part".to_string()))?;
            main_xml = Some(set_full_calc_on_load(&xml));
            report.recalculates_on_open = true;
        }
        if let Some(xml) = main_xml {
            changes.replace.insert(normalize(&main), pack_part(&xml)?);
            report.changed_parts.push(main.clone());
        }
        if edits.refresh_pivots {
            for part in package.parts() {
                let lower = part.name.to_ascii_lowercase().replace('\\', "/");
                if lower.contains("/pivotcache/pivotcachedefinition") && lower.ends_with(".xml") {
                    let xml = package
                        .read(&part.name)
                        .ok_or_else(|| SaveError::Damaged(format!("no {}", part.name)))?;
                    let edited = set_refresh_on_load(&xml);
                    changes
                        .replace
                        .insert(normalize(&part.name), pack_part(&edited)?);
                    report.changed_parts.push(part.name.clone());
                    report.pivots_refresh_on_open = true;
                }
            }
        }
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
    let (out, _) = patch_into(|| Ok(source), Vec::new, patches, elements, strings, false)?;
    Ok(out)
}
