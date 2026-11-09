//! Saving pivot tables and caches from the model: parts patched, made and taken out, with their
//! relationships and content types.

use std::collections::{BTreeMap, HashSet};

use cgs_model::pivot::{CacheValue, PivotCache};
use cgs_model::Book;

use crate::package::Package;
use crate::patch_parts::{
    add_override, add_relationship_with_id, remove_overrides, remove_relationship_ids,
};
use crate::pivot_write::{
    add_workbook_cache, new_cache_part, new_table_part, patch_cache_part, patch_table_part,
    remove_workbook_caches, CACHE_REL, CACHE_TYPE, TABLE_REL, TABLE_TYPE,
};
use crate::save::{SaveError, SaveReport};
use crate::workbook::{read_relationships, relationships_in};
use crate::xml::rels_path;
use crate::zipdir::{normalize, pack, ZipChanges};

const EMPTY_RELS: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";

/// What saving pivots changed in parts that the rest of the save also writes.
pub struct PivotSave<'a> {
    pub package: &'a Package,
    pub book: &'a Book,
    /// Each cache's values, listed or not, by cache index.
    pub values: &'a BTreeMap<u32, Vec<Vec<CacheValue>>>,
    pub main: &'a str,
    pub overlay: &'a mut BTreeMap<String, Vec<u8>>,
    pub changes: &'a mut ZipChanges,
    pub report: &'a mut SaveReport,
    pub content_types: &'a mut Vec<u8>,
    pub main_rels: &'a mut Option<Vec<u8>>,
    pub main_xml: &'a mut Option<Vec<u8>>,
    /// Parts of sheets made in this save.
    pub new_sheets: &'a BTreeMap<u32, String>,
}

/// What else the save must write.
#[derive(Default)]
pub struct PivotOutcome {
    pub types_changed: bool,
    pub rels_changed: bool,
}

fn zip_error(error: std::io::Error) -> SaveError {
    SaveError::Write(error.to_string())
}

fn part_error(error: std::io::Error) -> SaveError {
    SaveError::Damaged(error.to_string())
}

fn relative(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect();
    let from_dir = &from_dir[..from_dir.len().saturating_sub(1)];
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from_dir
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = Vec::new();
    for _ in common..from_dir.len() {
        out.push("..".to_string());
    }
    for part in &to_parts[common..] {
        out.push((*part).to_string());
    }
    out.join("/")
}

impl PivotSave<'_> {
    fn read(&self, part: &str) -> Option<Vec<u8>> {
        self.overlay
            .get(part)
            .cloned()
            .or_else(|| self.package.read(part))
    }

    fn free_name(&self, taken: &mut HashSet<String>, stem: &str) -> String {
        let mut n = 1;
        loop {
            let name = format!("{stem}{n}.xml");
            if !taken.contains(&normalize(&name)) {
                taken.insert(normalize(&name));
                return name;
            }
            n += 1;
        }
    }

    fn add_part(&mut self, name: &str, xml: &[u8], kind: &str) -> Result<(), SaveError> {
        self.changes
            .add
            .push((name.to_string(), pack(xml).map_err(zip_error)?));
        self.report.added_parts.push(name.to_string());
        *self.content_types = add_override(self.content_types, name, kind);
        Ok(())
    }

    fn remove_part(&mut self, name: &str) {
        if self.package.part(name).is_some() {
            self.changes.remove.push(normalize(name));
            self.report.removed_parts.push(name.to_string());
        }
        self.overlay.remove(name);
        let rels = rels_path(name);
        if self.package.part(&rels).is_some() {
            self.changes.remove.push(normalize(&rels));
            self.report.removed_parts.push(rels.clone());
        }
        self.overlay.remove(&rels);
        *self.content_types = remove_overrides(self.content_types, &[name.to_string()]);
    }

    /// Whether a cache's file records still fit its fields: the same fields, and every field that
    /// lists items now listed them before, so no record holds a value where an index must go.
    fn records_fit(&self, cache: &PivotCache, part: &str) -> bool {
        let Some(old) = self
            .package
            .read(part)
            .and_then(|xml| crate::pivot::read_cache_definition(&xml, self.book.date1904))
        else {
            return false;
        };
        old.fields.len() == cache.fields.len()
            && old.fields.iter().zip(&cache.fields).all(|(a, b)| {
                a.name == b.name && a.database == b.database && (a.listed || !b.listed)
            })
            && cache.fields.iter().zip(&old.fields).all(|(new, old)| {
                // Old indexes still point at the same values.
                old.shared.len() <= new.shared.len() || !new.listed
            })
    }

    pub fn run(mut self) -> Result<PivotOutcome, SaveError> {
        let mut outcome = PivotOutcome::default();
        let types_before = self.content_types.clone();
        let rels_before = self.main_rels.clone();
        let had_main = self.main_xml.is_some();
        let book = self.book;
        let package = self.package;
        let mut taken: HashSet<String> =
            package.parts().iter().map(|p| normalize(&p.name)).collect();
        let date1904 = book.date1904;
        // Caches: the file's patched, new ones made.
        let mut cache_parts: Vec<Option<String>> = Vec::with_capacity(book.pivot_caches.len());
        let mut workbook_xml = self.main_xml.take().or_else(|| package.read(self.main));
        let mut workbook_changed = false;
        for (index, cache) in book.pivot_caches.iter().enumerate() {
            let empty = Vec::new();
            let values = self.values.get(&(index as u32)).unwrap_or(&empty);
            match &cache.part {
                Some(part) if cache.changed => {
                    let Some(xml) = self.read(part) else {
                        cache_parts.push(Some(part.clone()));
                        continue;
                    };
                    let drop = match &cache.records_part {
                        Some(_) => !self.records_fit(cache, part),
                        None => false,
                    };
                    let patched = patch_cache_part(&xml, cache, values, drop, date1904)
                        .map_err(part_error)?;
                    self.overlay.insert(part.clone(), patched);
                    if drop {
                        if let Some(records) = &cache.records_part {
                            self.remove_part(records);
                            let rels_name = rels_path(part);
                            if let Some(rels) = self.read(&rels_name) {
                                let ids: Vec<String> = relationships_in(&rels, part)
                                    .into_iter()
                                    .filter(|(_, (kind, _))| kind.ends_with("/pivotCacheRecords"))
                                    .map(|(id, _)| id)
                                    .collect();
                                let rels = remove_relationship_ids(&rels, &ids);
                                self.overlay.insert(rels_name, rels);
                            }
                        }
                    }
                    self.report.pivots_refresh_on_open = true;
                    cache_parts.push(Some(part.clone()));
                }
                Some(part) => cache_parts.push(Some(part.clone())),
                None => {
                    // Only a cache a pivot table uses goes in.
                    if !book.pivot_tables.iter().any(|t| t.cache as usize == index) {
                        cache_parts.push(None);
                        continue;
                    }
                    let name = self.free_name(&mut taken, "xl/pivotCache/pivotCacheDefinition");
                    let xml = new_cache_part(cache, values, date1904);
                    self.add_part(&name, xml.as_bytes(), CACHE_TYPE)?;
                    let target = relative(self.main, &name);
                    let rels = self.main_rels.get_or_insert_with(|| EMPTY_RELS.to_vec());
                    let (updated, id) = add_relationship_with_id(rels, CACHE_REL, &target);
                    *rels = updated;
                    if let Some(xml) = workbook_xml.as_mut() {
                        *xml = add_workbook_cache(xml, cache.id, &id).map_err(part_error)?;
                        workbook_changed = true;
                    }
                    self.report.pivots_refresh_on_open = true;
                    cache_parts.push(Some(name));
                }
            }
        }
        // Tables taken out: their parts go, with the sheet's link to them.
        let kept: HashSet<String> = book
            .pivot_tables
            .iter()
            .filter_map(|t| t.part.as_deref().map(normalize))
            .collect();
        let mut gone_caches: Vec<u32> = Vec::new();
        for (index, info) in book.sheets.iter().enumerate() {
            let Some(sheet_part) = &info.part else {
                continue;
            };
            let rels_name = rels_path(sheet_part);
            let Some(rels) = self.read(&rels_name) else {
                continue;
            };
            let mut gone_ids = Vec::new();
            for (id, (kind, target)) in relationships_in(&rels, sheet_part) {
                if kind.ends_with("/pivotTable") && !kept.contains(&normalize(&target)) {
                    gone_ids.push(id);
                    self.remove_part(&target);
                }
            }
            let _ = index;
            if !gone_ids.is_empty() {
                let rels = remove_relationship_ids(&rels, &gone_ids);
                self.overlay.insert(rels_name, rels);
            }
        }
        // Caches whose tables all went, and that were new here, never went in; the file's own
        // stay, as Excel keeps them until it saves.
        for (index, cache) in book.pivot_caches.iter().enumerate() {
            if cache.part.is_none() && cache_parts[index].is_none() {
                gone_caches.push(cache.id);
            }
        }
        // Tables: the file's patched, new ones made and linked. A sheet's new relationships wait
        // in the overlay until every table on it is linked, so a second one adds to the first's.
        let mut fresh_rels: Vec<String> = Vec::new();
        for table in &book.pivot_tables {
            let cache_part = cache_parts.get(table.cache as usize).cloned().flatten();
            let Some(cache_part) = cache_part else {
                continue;
            };
            match &table.part {
                Some(part) if table.changed => {
                    let Some(xml) = self.read(part) else { continue };
                    let patched = patch_table_part(&xml, table).map_err(part_error)?;
                    self.overlay.insert(part.clone(), patched);
                }
                Some(_) => {}
                None => {
                    let Some(sheet_part) = book
                        .sheets
                        .get(table.sheet as usize)
                        .and_then(|s| s.part.clone())
                        .or_else(|| self.new_sheets.get(&table.sheet).cloned())
                    else {
                        continue;
                    };
                    let cache_id = book.pivot_caches[table.cache as usize].id;
                    let name = self.free_name(&mut taken, "xl/pivotTables/pivotTable");
                    let xml = new_table_part(table, cache_id);
                    self.add_part(&name, xml.as_bytes(), TABLE_TYPE)?;
                    let rels = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{CACHE_REL}\" Target=\"{}\"/></Relationships>",
                        relative(&name, &cache_part)
                    );
                    let rels_name = rels_path(&name);
                    self.changes
                        .add
                        .push((rels_name.clone(), pack(rels.as_bytes()).map_err(zip_error)?));
                    self.report.added_parts.push(rels_name);
                    let sheet_rels = rels_path(&sheet_part);
                    let had = self.read(&sheet_rels);
                    let base = had.clone().unwrap_or_else(|| EMPTY_RELS.to_vec());
                    let (updated, _) =
                        add_relationship_with_id(&base, TABLE_REL, &relative(&sheet_part, &name));
                    if had.is_some() || package.part(&sheet_rels).is_some() {
                        self.overlay.insert(sheet_rels, updated);
                    } else if self
                        .changes
                        .add
                        .iter()
                        .any(|(n, _)| normalize(n) == normalize(&sheet_rels))
                    {
                        // A new sheet's relationships, made earlier in this save.
                        self.overlay.insert(sheet_rels, updated);
                    } else {
                        self.overlay.insert(sheet_rels.clone(), updated);
                        fresh_rels.push(sheet_rels);
                    }
                }
            }
        }
        for name in fresh_rels {
            if let Some(bytes) = self.overlay.remove(&name) {
                self.changes
                    .add
                    .push((name.clone(), pack(&bytes).map_err(zip_error)?));
                self.report.added_parts.push(name);
            }
        }
        if !gone_caches.is_empty() {
            if let Some(xml) = workbook_xml.as_mut() {
                *xml = remove_workbook_caches(xml, &gone_caches).map_err(part_error)?;
                workbook_changed = true;
            }
        }
        if workbook_changed || had_main {
            *self.main_xml = workbook_xml;
        }
        let _ = read_relationships;
        outcome.types_changed = *self.content_types != types_before;
        outcome.rels_changed = *self.main_rels != rels_before;
        Ok(outcome)
    }
}
