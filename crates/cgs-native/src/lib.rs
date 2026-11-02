//! Desktop-only native side: identifying the host, serving byte ranges of chosen files, and
//! the recent files list. Phase 9 adds the file association.
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Largest single read; the webview asks again for the rest.
pub const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECENT: usize = 10;

/// Identifies the native host to the UI. Returned by the `native_info` Tauri command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeInfo {
    pub product: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

pub fn info() -> NativeInfo {
    NativeInfo {
        product: "CashGear Sheet",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// A file the webview can read, under an id that says nothing about its path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NativeFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// A file on the recent list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecentFile {
    pub id: String,
    pub name: String,
    pub detail: String,
}

/// Files the webview may read. Only what the user chose gets in, so the protocol can't reach
/// anything else on the disk.
#[derive(Debug, Default)]
pub struct FileRegistry {
    files: Mutex<HashMap<String, PathBuf>>,
    next: Mutex<u64>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

impl FileRegistry {
    /// Makes a file readable, and returns how the webview can ask for it.
    pub fn register(&self, path: &Path) -> Result<NativeFile, String> {
        let size = fs::metadata(path)
            .map_err(|error| format!("{}: {error}", path.display()))?
            .len();
        let id = {
            let mut next = self.next.lock().map_err(|error| error.to_string())?;
            *next += 1;
            format!("file-{next}")
        };
        self.files
            .lock()
            .map_err(|error| error.to_string())?
            .insert(id.clone(), path.to_path_buf());
        Ok(NativeFile {
            id,
            name: file_name(path),
            path: path.to_string_lossy().into_owned(),
            size,
        })
    }

    /// Reads a range of a registered file, capped at `MAX_READ_BYTES`: (bytes, whole size).
    pub fn read_range(&self, id: &str, start: u64, end: u64) -> Result<(Vec<u8>, u64), ReadRange> {
        let path = self
            .files
            .lock()
            .map_err(|_| ReadRange::NotFound)?
            .get(id)
            .cloned()
            .ok_or(ReadRange::NotFound)?;
        let mut file = fs::File::open(&path).map_err(|_| ReadRange::NotFound)?;
        let size = file.metadata().map_err(|_| ReadRange::NotFound)?.len();
        if start >= size || end < start {
            return Err(ReadRange::Unsatisfiable(size));
        }
        let last = end.min(size - 1).min(start + MAX_READ_BYTES - 1);
        let mut body = vec![0; (last - start + 1) as usize];
        file.seek(SeekFrom::Start(start))
            .map_err(|_| ReadRange::NotFound)?;
        file.read_exact(&mut body)
            .map_err(|_| ReadRange::NotFound)?;
        Ok((body, size))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadRange {
    NotFound,
    Unsatisfiable(u64),
}

/// Parses `bytes=a-b` or `bytes=a-`. No header means the whole file.
pub fn parse_range(header: Option<&str>) -> Option<(u64, u64)> {
    let Some(header) = header else {
        return Some((0, u64::MAX));
    };
    let (start, end) = header.trim().strip_prefix("bytes=")?.split_once('-')?;
    let start = start.parse().ok()?;
    let end = if end.is_empty() {
        u64::MAX
    } else {
        end.parse().ok()?
    };
    Some((start, end))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentList {
    paths: Vec<PathBuf>,
}

/// The recent list, kept in a small JSON file in the app's data folder.
#[derive(Debug)]
pub struct RecentStore {
    file: PathBuf,
}

impl RecentStore {
    pub fn new(folder: &Path) -> Self {
        Self {
            file: folder.join("recent-files.json"),
        }
    }

    fn read(&self) -> RecentList {
        fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, list: &RecentList) -> Result<(), String> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(list).map_err(|error| error.to_string())?;
        // Through a temporary file, so a crash can't leave half a list.
        let temporary = self.file.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.file).map_err(|error| error.to_string())
    }

    /// The list, newest first, without files that have gone. An entry's id is its path.
    pub fn list(&self) -> Vec<RecentFile> {
        self.read()
            .paths
            .iter()
            .filter(|path| path.is_file())
            .map(|path| RecentFile {
                id: path.to_string_lossy().into_owned(),
                name: file_name(path),
                detail: path.to_string_lossy().into_owned(),
            })
            .collect()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.read().paths.iter().any(|known| known == path)
    }

    pub fn remember(&self, path: &Path) -> Result<(), String> {
        let mut list = self.read();
        list.paths.retain(|known| known != path);
        list.paths.insert(0, path.to_path_buf());
        list.paths.truncate(MAX_RECENT);
        self.write(&list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cgs-native-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn serves_ranges_of_registered_files_only() {
        let dir = temp_dir("ranges");
        let path = dir.join("book.xlsx");
        fs::write(&path, b"0123456789").unwrap();
        let registry = FileRegistry::default();
        let file = registry.register(&path).unwrap();
        assert_eq!(file.name, "book.xlsx");
        assert_eq!(file.size, 10);
        assert_eq!(
            registry.read_range(&file.id, 2, 4).unwrap(),
            (b"234".to_vec(), 10)
        );
        assert_eq!(
            registry.read_range(&file.id, 8, u64::MAX).unwrap(),
            (b"89".to_vec(), 10)
        );
        assert_eq!(
            registry.read_range(&file.id, 10, 12),
            Err(ReadRange::Unsatisfiable(10))
        );
        assert_eq!(
            registry.read_range("file-99", 0, 1),
            Err(ReadRange::NotFound)
        );
    }

    #[test]
    fn parses_range_headers() {
        assert_eq!(parse_range(None), Some((0, u64::MAX)));
        assert_eq!(parse_range(Some("bytes=5-9")), Some((5, 9)));
        assert_eq!(parse_range(Some("bytes=5-")), Some((5, u64::MAX)));
        assert_eq!(parse_range(Some("items=5-9")), None);
        assert_eq!(parse_range(Some("bytes=x-9")), None);
    }

    #[test]
    fn the_recent_list_keeps_ten_newest_first_and_drops_what_has_gone() {
        let dir = temp_dir("recent");
        let store = RecentStore::new(&dir);
        let paths: Vec<PathBuf> = (0..12).map(|n| dir.join(format!("{n}.xlsx"))).collect();
        for path in &paths {
            fs::write(path, b"x").unwrap();
            store.remember(path).unwrap();
        }
        let list = store.list();
        assert_eq!(list.len(), 10);
        assert_eq!(list[0].name, "11.xlsx");
        store.remember(&paths[5]).unwrap();
        assert_eq!(store.list()[0].name, "5.xlsx");
        assert_eq!(store.list().len(), 10);
        fs::remove_file(&paths[5]).unwrap();
        assert_eq!(store.list()[0].name, "11.xlsx");
        assert!(store.contains(&paths[11]));
        assert!(!store.contains(&dir.join("other.xlsx")));
    }

    #[test]
    fn reports_this_build() {
        let info = info();
        assert_eq!(info.product, "CashGear Sheet");
        assert_eq!(info.os, std::env::consts::OS);
    }
}
