//! Installed fonts, for PDFs: which file holds a family in a style. Windows lists its fonts in the
//! registry by display name, such as `Calibri Bold (TrueType)`, and a collection's faces are told
//! apart by reading the file's own names.

use std::path::{Path, PathBuf};

/// A font file and, in a collection, which face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFile {
    pub path: PathBuf,
    pub index: u32,
}

/// One registry entry: its display name and file.
#[derive(Debug, Clone)]
pub struct Listed {
    pub name: String,
    pub path: PathBuf,
}

fn style_of(words: &str) -> (bool, bool) {
    let lower = words.to_ascii_lowercase();
    let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
    let italic = lower.contains("italic") || lower.contains("oblique");
    (bold, italic)
}

/// The families and style a display name lists: `Cambria & Cambria Math (TrueType)` is two.
fn parse_listed(name: &str) -> Vec<(String, bool, bool)> {
    let name = name.rsplit_once(" (").map_or(name, |(head, _)| head).trim();
    name.split(" & ")
        .map(|part| {
            let part = part.trim();
            let mut family = part.to_string();
            let mut style = String::new();
            for word in [
                "Bold Italic",
                "Bold Oblique",
                "Italic",
                "Oblique",
                "Bold",
                "Regular",
            ] {
                if let Some(head) = part.strip_suffix(word) {
                    if head.ends_with(' ') {
                        family = head.trim_end().to_string();
                        style = word.to_string();
                        break;
                    }
                }
            }
            let (bold, italic) = style_of(&style);
            (family, bold, italic)
        })
        .collect()
}

/// The face in a collection whose family and style match, read from its own names.
fn face_in(data: &[u8], family: &str, bold: bool, italic: bool) -> Option<u32> {
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut fallback = None;
    for index in 0..count {
        let Ok(face) = ttf_parser::Face::parse(data, index) else {
            continue;
        };
        let names: Vec<String> = face
            .names()
            .into_iter()
            .filter(|n| {
                n.name_id == ttf_parser::name_id::FAMILY
                    || n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY
            })
            .filter_map(|n| n.to_string())
            .collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(family)) {
            continue;
        }
        if face.is_bold() == bold && face.is_italic() == italic {
            return Some(index);
        }
        fallback.get_or_insert(index);
    }
    fallback
}

/// Finds a family in a style among listed fonts, closest style first.
pub fn find_in(list: &[Listed], family: &str, bold: bool, italic: bool) -> Option<FontFile> {
    let mut best: Option<(u32, &Listed)> = None;
    for entry in list {
        for (name, b, i) in parse_listed(&entry.name) {
            if !name.eq_ignore_ascii_case(family) {
                continue;
            }
            let score = u32::from(b != bold) * 2 + u32::from(i != italic);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, entry));
            }
        }
    }
    let (_, entry) = best?;
    let is_collection = entry
        .path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ttc") || e.eq_ignore_ascii_case("otc"));
    let index = if is_collection {
        let data = std::fs::read(&entry.path).ok()?;
        face_in(&data, family, bold, italic).unwrap_or(0)
    } else {
        0
    };
    Some(FontFile {
        path: entry.path.clone(),
        index,
    })
}

/// Where a listed file is: relative names are in the system's fonts folder.
fn resolve(file: &str, fonts: &Path) -> PathBuf {
    let path = PathBuf::from(file);
    if path.is_absolute() {
        path
    } else {
        fonts.join(path)
    }
}

/// The fonts Windows has, for everyone and for this user.
#[cfg(windows)]
pub fn installed() -> Vec<Listed> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let windows =
        std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let fonts = windows.join("Fonts");
    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(key) =
            RegKey::predef(root).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts")
        else {
            continue;
        };
        for (name, value) in key.enum_values().flatten() {
            let file = value.to_string();
            let file = file.trim_matches('\u{0}').trim();
            if file.is_empty() {
                continue;
            }
            out.push(Listed {
                name,
                path: resolve(file, &fonts),
            });
        }
    }
    out
}

#[cfg(not(windows))]
pub fn installed() -> Vec<Listed> {
    let _ = resolve;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_display_names() {
        assert_eq!(
            parse_listed("Calibri Bold Italic (TrueType)"),
            vec![("Calibri".to_string(), true, true)]
        );
        assert_eq!(
            parse_listed("Cambria & Cambria Math (TrueType)"),
            vec![
                ("Cambria".to_string(), false, false),
                ("Cambria Math".to_string(), false, false)
            ]
        );
        assert_eq!(
            parse_listed("Segoe UI Semibold (TrueType)"),
            vec![("Segoe UI Semibold".to_string(), false, false)]
        );
    }

    #[test]
    fn finds_the_closest_style() {
        let list = vec![
            Listed {
                name: "Arial (TrueType)".into(),
                path: "arial.ttf".into(),
            },
            Listed {
                name: "Arial Bold (TrueType)".into(),
                path: "arialbd.ttf".into(),
            },
        ];
        assert_eq!(
            find_in(&list, "arial", true, false).unwrap().path,
            PathBuf::from("arialbd.ttf")
        );
        // No bold italic: bold is closer than italic.
        assert_eq!(
            find_in(&list, "Arial", true, true).unwrap().path,
            PathBuf::from("arialbd.ttf")
        );
        assert!(find_in(&list, "Calibri", false, false).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn this_pc_lists_its_fonts() {
        let list = installed();
        // Every Windows has Arial.
        let arial = find_in(&list, "Arial", false, false).expect("Arial is installed");
        assert!(arial.path.exists(), "{}", arial.path.display());
    }
}
