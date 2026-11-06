//! Reads every chart part in the test workbooks and counts what the reader made of them.
//! `cargo run -p cgs-bench --release --bin chartscan -- tests/workbooks [out.json]`

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use cgs_model::chart::{ChartFill, GroupKind};
use cgs_model::ChartBody;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("xlsx" | "xlsm" | "xltx" | "xltm")
        ) {
            out.push(path);
        }
    }
}

fn kind_name(kind: &GroupKind) -> String {
    let mut name = kind.tag().to_string();
    if let GroupKind::Bar {
        horizontal,
        grouping,
        ..
    } = kind
    {
        name.push_str(if *horizontal { ":bar" } else { ":col" });
        name.push(':');
        name.push_str(grouping.code());
    }
    if let GroupKind::Line { grouping, .. } | GroupKind::Area { grouping, .. } = kind {
        name.push(':');
        name.push_str(grouping.code());
    }
    name
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).map_or("tests/workbooks", String::as_str));
    let mut list = Vec::new();
    files(&root, &mut list);
    list.sort();
    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    let mut extended: BTreeMap<String, u32> = BTreeMap::new();
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    let mut panics = Vec::new();
    let mut unreadable = Vec::new();
    for path in &list {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        let Ok(mut archive) = zip::ZipArchive::new(file) else {
            continue;
        };
        let names: Vec<String> = archive
            .file_names()
            .filter_map(|name| name.ok().map(|name| name.into_owned()))
            .collect();
        for name in names {
            let lower = name.to_ascii_lowercase();
            let is_ex = lower.contains("xl/charts/chartex");
            if !(lower.starts_with("xl/charts/chart") && lower.ends_with(".xml")) {
                continue;
            }
            let mut xml = Vec::new();
            if archive
                .by_name(&name)
                .map(|mut entry| entry.read_to_end(&mut xml))
                .is_err()
            {
                continue;
            }
            *counts.entry("parts").or_default() += 1;
            let result = std::panic::catch_unwind(|| cgs_xlsx::chart::read_chart(&xml, is_ex));
            match result {
                Err(_) => panics.push(format!("{}::{name}", path.display())),
                Ok(ChartBody::Unreadable) => unreadable.push(format!("{}::{name}", path.display())),
                Ok(ChartBody::Extended(kind)) => *extended.entry(kind).or_default() += 1,
                Ok(ChartBody::Classic(chart)) => {
                    *counts.entry("classic").or_default() += 1;
                    if chart.editable() {
                        *counts.entry("editable").or_default() += 1;
                    }
                    if chart.groups.is_empty() {
                        *counts.entry("no groups").or_default() += 1;
                    }
                    if chart.groups.len() > 1 {
                        *counts.entry("combination").or_default() += 1;
                    }
                    if chart.three_d {
                        *counts.entry("3-D view").or_default() += 1;
                    }
                    if chart.pivot {
                        *counts.entry("pivot").or_default() += 1;
                    }
                    if chart.title.is_some() {
                        *counts.entry("titled").or_default() += 1;
                    }
                    if chart.legend.is_some() {
                        *counts.entry("legend").or_default() += 1;
                    }
                    for group in &chart.groups {
                        *kinds.entry(kind_name(&group.kind)).or_default() += 1;
                        for axis in &group.axes {
                            if chart.axis(*axis).is_none() {
                                *counts.entry("group axis missing").or_default() += 1;
                            }
                        }
                        for series in &group.series {
                            *counts.entry("series").or_default() += 1;
                            match &series.values {
                                Some(values) if values.formula.is_some() => {
                                    *counts.entry("series values from cells").or_default() += 1
                                }
                                Some(_) => *counts.entry("series values literal").or_default() += 1,
                                None => *counts.entry("series without values").or_default() += 1,
                            }
                            if matches!(series.shape.fill, Some(ChartFill::Solid(_))) {
                                *counts.entry("series solid fill").or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("files: {}", list.len());
    println!("counts: {counts:#?}");
    println!("kinds: {kinds:#?}");
    println!("extended: {extended:#?}");
    println!(
        "unreadable ({}): {:#?}",
        unreadable.len(),
        unreadable.iter().take(20).collect::<Vec<_>>()
    );
    println!("panics ({}): {:#?}", panics.len(), panics);
}
