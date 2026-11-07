//! Desktop-only native side: identifying the host, serving byte ranges of chosen files, saving
//! them atomically, working copies, the recent files list, and the workbook a double-click in
//! Explorer starts the app with.

pub mod fonts;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

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

/// What a file looked like when it was registered. A read can't lock the file, so it compares
/// this instead: reading other bytes at old offsets would silently damage a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

fn stamp_of(path: &Path) -> std::io::Result<Stamp> {
    let metadata = fs::metadata(path)?;
    Ok(Stamp {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

#[derive(Debug, Clone)]
struct Registered {
    path: PathBuf,
    stamp: Stamp,
}

/// Files the webview may read. Only what the user chose gets in, so the protocol can't reach
/// anything else on the disk.
#[derive(Debug, Default)]
pub struct FileRegistry {
    files: Mutex<HashMap<String, Registered>>,
    next: Mutex<u64>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

impl FileRegistry {
    /// Makes a file readable, and returns how the webview can ask for it.
    pub fn register(&self, path: &Path) -> Result<NativeFile, String> {
        let stamp = stamp_of(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let size = stamp.size;
        let id = {
            let mut next = self.next.lock().map_err(|error| error.to_string())?;
            *next += 1;
            format!("file-{next}")
        };
        self.files
            .lock()
            .map_err(|error| error.to_string())?
            .insert(
                id.clone(),
                Registered {
                    path: path.to_path_buf(),
                    stamp,
                },
            );
        Ok(NativeFile {
            id,
            name: file_name(path),
            path: path.to_string_lossy().into_owned(),
            size,
        })
    }

    /// Reads a range of a registered file, capped at `MAX_READ_BYTES`: (bytes, whole size).
    pub fn read_range(&self, id: &str, start: u64, end: u64) -> Result<(Vec<u8>, u64), ReadRange> {
        let entry = self
            .files
            .lock()
            .map_err(|_| ReadRange::NotFound)?
            .get(id)
            .cloned()
            .ok_or(ReadRange::NotFound)?;
        let mut file = fs::File::open(&entry.path).map_err(|_| ReadRange::NotFound)?;
        let metadata = file.metadata().map_err(|_| ReadRange::NotFound)?;
        let now = Stamp {
            size: metadata.len(),
            modified: metadata.modified().ok(),
        };
        if now != entry.stamp {
            return Err(ReadRange::Changed);
        }
        let size = now.size;
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

    /// Whether this path was opened in this run.
    pub fn is_registered(&self, path: &Path) -> bool {
        self.files
            .lock()
            .is_ok_and(|files| files.values().any(|entry| entry.path == path))
    }

    /// Forgets a file, so the webview can no longer read it.
    pub fn forget(&self, id: &str) -> Option<PathBuf> {
        let mut files = self.files.lock().ok()?;
        files.remove(id).map(|entry| entry.path)
    }

    /// Copies a file to `folder` and registers the copy, so that what happens to the original no
    /// longer matters. Refuses a file that isn't what was opened, before or after the copy.
    pub fn working_copy(&self, id: &str, folder: &Path) -> Result<NativeFile, String> {
        let entry = self
            .files
            .lock()
            .map_err(|error| error.to_string())?
            .get(id)
            .cloned()
            .ok_or_else(|| "That file is no longer open.".to_string())?;
        let changed = || "The file has changed since it was opened.".to_string();
        if stamp_of(&entry.path).ok() != Some(entry.stamp) {
            return Err(changed());
        }
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        let copy = folder.join(format!("{}-{id}.xlsx", std::process::id()));
        fs::copy(&entry.path, &copy).map_err(|error| error.to_string())?;
        if stamp_of(&entry.path).ok() != Some(entry.stamp) {
            let _ = fs::remove_file(&copy);
            return Err(changed());
        }
        self.register(&copy)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadRange {
    NotFound,
    Unsatisfiable(u64),
    /// The file on disk is no longer the one that was opened.
    Changed,
}

/// Deletes working copies left by a run that didn't get to clean up. Each starts with its
/// process id, so one still in use by another window of the app stays.
pub fn sweep_working_copies(folder: &Path, running: impl Fn(u32) -> bool) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let owner = name.split('-').next().and_then(|text| text.parse().ok());
        if owner.is_none_or(|owner: u32| !running(owner)) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Saves in progress. Bytes go to a new file beside the target, which replaces the target in one
/// step only when all of them have reached the disk, so a failed save leaves the original as it
/// was. Only paths the user chose can be written, for the same reason the registry exists.
#[derive(Debug, Default)]
pub struct SaveStore {
    allowed: Mutex<HashSet<PathBuf>>,
    open: Mutex<HashMap<String, OpenSave>>,
    next: Mutex<u64>,
}

#[derive(Debug)]
struct OpenSave {
    target: PathBuf,
    temporary: PathBuf,
    file: fs::File,
    written: u64,
}

impl SaveStore {
    /// Lets a path be saved to, because the user chose it.
    pub fn allow(&self, path: &Path) -> Result<(), String> {
        self.allowed
            .lock()
            .map_err(|error| error.to_string())?
            .insert(path.to_path_buf());
        Ok(())
    }

    /// Starts a save, and returns the token its bytes and its finish go under.
    pub fn begin(&self, target: &Path) -> Result<String, String> {
        if !self
            .allowed
            .lock()
            .map_err(|error| error.to_string())?
            .contains(target)
        {
            return Err("That file wasn't chosen for saving.".to_string());
        }
        if fs::metadata(target).is_ok_and(|metadata| metadata.permissions().readonly()) {
            return Err(
                "That file is read-only, so it can't be replaced. Save it under another name."
                    .to_string(),
            );
        }
        let number = {
            let mut next = self.next.lock().map_err(|error| error.to_string())?;
            *next += 1;
            *next
        };
        let name = target
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let temporary = target.with_file_name(format!(
            "~{name}.{}-{number}.cgs-saving",
            std::process::id()
        ));
        let file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
        let token = format!("save-{number}");
        self.open.lock().map_err(|error| error.to_string())?.insert(
            token.clone(),
            OpenSave {
                target: target.to_path_buf(),
                temporary,
                file,
                written: 0,
            },
        );
        Ok(token)
    }

    /// Adds the next bytes of a save.
    pub fn append(&self, token: &str, bytes: &[u8]) -> Result<(), String> {
        let mut open = self.open.lock().map_err(|error| error.to_string())?;
        let save = open
            .get_mut(token)
            .ok_or_else(|| "That save isn't in progress.".to_string())?;
        save.file
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
        save.written += bytes.len() as u64;
        Ok(())
    }

    /// Ends a save: its bytes reach the disk, and then the file takes the target's place.
    pub fn finish(&self, token: &str, size: u64) -> Result<PathBuf, String> {
        let save = self
            .open
            .lock()
            .map_err(|error| error.to_string())?
            .remove(token)
            .ok_or_else(|| "That save isn't in progress.".to_string())?;
        let OpenSave {
            target,
            temporary,
            file,
            written,
        } = save;
        let synced = if written == size {
            file.sync_all().map_err(|error| error.to_string())
        } else {
            Err("Part of the file didn't arrive.".to_string())
        };
        // Closed first, as Windows won't always move a file that is open.
        drop(file);
        let moved = synced.and_then(|()| {
            fs::rename(&temporary, &target).map_err(|error| {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    "Another program has the file open, so it can't be replaced. Close it, or save under another name.".to_string()
                } else {
                    error.to_string()
                }
            })
        });
        if let Err(message) = moved {
            let _ = fs::remove_file(&temporary);
            return Err(message);
        }
        Ok(target)
    }

    /// Gives up on a save and removes what it wrote.
    pub fn abort(&self, token: &str) -> Result<(), String> {
        let save = self
            .open
            .lock()
            .map_err(|error| error.to_string())?
            .remove(token);
        if let Some(save) = save {
            drop(save.file);
            let _ = fs::remove_file(&save.temporary);
        }
        Ok(())
    }
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

/// The file types the installer registers the app for, which it opens when started with one.
pub const OPENABLE_EXTENSIONS: [&str; 5] = ["xlsx", "xlsm", "xltx", "xltm", "csv"];

/// The workbook the app was started with: Explorer runs `cgs-desktop.exe "C:\Books\Sales.xlsx"`
/// for a double-click. The first argument that names an existing file of a type the app opens,
/// made absolute against `cwd`. Anything else, such as a flag, is no file to open.
pub fn launch_path(
    args: impl IntoIterator<Item = std::ffi::OsString>,
    cwd: &Path,
) -> Option<PathBuf> {
    args.into_iter().skip(1).find_map(|arg| {
        let path = PathBuf::from(arg);
        if path.to_string_lossy().starts_with('-') {
            return None;
        }
        let openable = path.extension().is_some_and(|ext| {
            OPENABLE_EXTENSIONS
                .iter()
                .any(|known| ext.to_string_lossy().eq_ignore_ascii_case(known))
        });
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        (openable && path.is_file()).then_some(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_double_click_names_the_workbook_to_open() {
        let dir = temp_dir("launch");
        let book = dir.join("Quarterly Sales.XLSX");
        fs::write(&book, b"PK").unwrap();
        fs::write(dir.join("notes.txt"), b"text").unwrap();
        let args = |list: &[&str]| -> Vec<std::ffi::OsString> {
            std::iter::once("cgs-desktop.exe")
                .chain(list.iter().copied())
                .map(Into::into)
                .collect()
        };
        let path = book.to_string_lossy().into_owned();
        assert_eq!(launch_path(args(&[&path]), &dir), Some(book.clone()));
        // Relative to where it was started, past flags and files it doesn't open.
        assert_eq!(
            launch_path(args(&["--flag", "notes.txt", "Quarterly Sales.XLSX"]), &dir),
            Some(book)
        );
        assert_eq!(launch_path(args(&["missing.xlsx"]), &dir), None);
        assert_eq!(launch_path(args(&[]), &dir), None);
        assert_eq!(launch_path(args(&["notes.txt"]), &dir), None);
    }

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
    fn a_file_that_changed_is_refused_rather_than_misread() {
        let dir = temp_dir("changed");
        let path = dir.join("book.xlsx");
        fs::write(&path, b"0123456789").unwrap();
        let registry = FileRegistry::default();
        let file = registry.register(&path).unwrap();
        fs::write(&path, b"0123456789AB").unwrap();
        assert_eq!(registry.read_range(&file.id, 0, 3), Err(ReadRange::Changed));
        assert!(registry
            .working_copy(&file.id, &dir.join("working"))
            .is_err());
    }

    #[test]
    fn a_working_copy_outlives_the_original() {
        let dir = temp_dir("working");
        let path = dir.join("book.xlsx");
        fs::write(&path, b"0123456789").unwrap();
        let registry = FileRegistry::default();
        let file = registry.register(&path).unwrap();
        let folder = dir.join("copies");
        let copy = registry.working_copy(&file.id, &folder).unwrap();
        fs::write(&path, b"something else").unwrap();
        assert_eq!(
            registry.read_range(&copy.id, 2, 4).unwrap(),
            (b"234".to_vec(), 10)
        );
        assert_eq!(registry.forget(&copy.id), Some(PathBuf::from(&copy.path)));
        assert_eq!(
            registry.read_range(&copy.id, 2, 4),
            Err(ReadRange::NotFound)
        );
    }

    #[test]
    fn sweeping_leaves_copies_that_a_running_app_owns() {
        let dir = temp_dir("sweep");
        fs::write(dir.join("100-file-1.xlsx"), b"x").unwrap();
        fs::write(dir.join("200-file-1.xlsx"), b"x").unwrap();
        fs::write(dir.join("stray.xlsx"), b"x").unwrap();
        sweep_working_copies(&dir, |id| id == 100);
        assert!(dir.join("100-file-1.xlsx").exists());
        assert!(!dir.join("200-file-1.xlsx").exists());
        assert!(!dir.join("stray.xlsx").exists());
    }

    #[test]
    fn saving_replaces_the_file_only_when_it_finishes() {
        let dir = temp_dir("save");
        let target = dir.join("book.xlsx");
        fs::write(&target, b"old").unwrap();
        let store = SaveStore::default();
        assert!(store.begin(&target).is_err(), "not chosen by the user");
        store.allow(&target).unwrap();

        let token = store.begin(&target).unwrap();
        store.append(&token, b"new ").unwrap();
        store.append(&token, b"bytes").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(store.finish(&token, 9).unwrap(), target);
        assert_eq!(fs::read(&target).unwrap(), b"new bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        // Bytes that went missing leave the original, and nothing beside it.
        let token = store.begin(&target).unwrap();
        store.append(&token, b"short").unwrap();
        assert!(store.finish(&token, 99).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        let token = store.begin(&target).unwrap();
        store.append(&token, b"given up").unwrap();
        store.abort(&token).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn a_read_only_file_is_not_replaced() {
        let dir = temp_dir("readonly");
        let target = dir.join("book.xlsx");
        fs::write(&target, b"old").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions.clone()).unwrap();
        let store = SaveStore::default();
        store.allow(&target).unwrap();
        assert!(store.begin(&target).is_err());
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&target, permissions).unwrap();
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
