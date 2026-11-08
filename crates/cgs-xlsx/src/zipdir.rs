//! The zip container, as a save sees it. Entries nobody changed are copied as the bytes they
//! are, local header and data descriptor included, so a raw copy can't lose anything the zip
//! crate doesn't model. Output only ever moves forward, so it can go straight to a stream.

use std::collections::HashMap;
use std::io::{self, Write};

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};

use crate::pace::{block_on, Pace};
use crate::package::ByteSource;
use crate::save::SaveError;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const END64: u32 = 0x0606_4b50;
const LOCATOR64: u32 = 0x0706_4b50;
const END_SIZE: usize = 22;
const CHUNK: usize = 1024 * 1024;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut raw = [0; 8];
    raw.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(raw)
}

fn read_exact(source: &dyn ByteSource, mut offset: u64, out: &mut [u8]) -> Result<(), SaveError> {
    let mut done = 0;
    while done < out.len() {
        let count = source
            .read_at(offset, &mut out[done..])
            .map_err(|error| SaveError::SourceUnreadable(error.to_string()))?;
        if count == 0 {
            return Err(SaveError::SourceUnreadable(
                "the file is shorter than it was".to_string(),
            ));
        }
        done += count;
        offset += count as u64;
    }
    Ok(())
}

/// One entry of the central directory, with its record kept as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    record: Vec<u8>,
    /// Where the local header starts in the file.
    pub local: u64,
    pub compressed: u64,
    pub size: u64,
    pub crc: u32,
}

/// A zip's central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub entries: Vec<Entry>,
    comment: Vec<u8>,
    /// Where the directory starts, which is where the last entry's data ends.
    data_end: u64,
}

/// The zip64 values of a central record's extra field, in the order the format gives them.
fn zip64_extra(record: &[u8]) -> Option<(usize, usize)> {
    let names = usize::from(u16_at(record, 28));
    let extras = usize::from(u16_at(record, 30));
    let mut at = 46 + names;
    let end = (at + extras).min(record.len());
    while at + 4 <= end {
        let id = u16_at(record, at);
        let length = usize::from(u16_at(record, at + 2));
        if id == 1 && at + 4 + length <= end {
            return Some((at + 4, length));
        }
        at += 4 + length;
    }
    None
}

fn parse_entry(record: Vec<u8>, shift: u64) -> Entry {
    let names = usize::from(u16_at(&record, 28));
    let name = String::from_utf8_lossy(&record[46..46 + names]).into_owned();
    let mut compressed = u64::from(u32_at(&record, 20));
    let mut size = u64::from(u32_at(&record, 24));
    let mut local = u64::from(u32_at(&record, 42));
    if let Some((mut at, length)) = zip64_extra(&record) {
        let end = at + length;
        for field in [&mut size, &mut compressed, &mut local] {
            if *field == u64::from(u32::MAX) && at + 8 <= end {
                *field = u64_at(&record, at);
                at += 8;
            }
        }
    }
    Entry {
        name,
        crc: u32_at(&record, 16),
        record,
        local: local + shift,
        compressed,
        size,
    }
}

impl Directory {
    pub fn read(source: &dyn ByteSource) -> Result<Self, SaveError> {
        let damaged = |what: &str| SaveError::Damaged(what.to_string());
        let length = source.len();
        let tail_length = length.min(66_000) as usize;
        let tail_start = length - tail_length as u64;
        let mut tail = vec![0; tail_length];
        read_exact(source, tail_start, &mut tail)?;
        let end_at = (0..=tail.len().saturating_sub(END_SIZE))
            .rev()
            .find(|&at| {
                u32_at(&tail, at) == END
                    && at + END_SIZE + usize::from(u16_at(&tail, at + 20)) <= tail.len()
            })
            .ok_or_else(|| damaged("no zip directory"))?;
        let end = &tail[end_at..];
        let comment = end[END_SIZE..END_SIZE + usize::from(u16_at(end, 20))].to_vec();
        let end_position = tail_start + end_at as u64;
        let mut count = u64::from(u16_at(end, 10));
        let mut directory_size = u64::from(u32_at(end, 12));
        let mut directory_at = u64::from(u32_at(end, 16));
        let mut directory_end = end_position;
        if end_position >= 20 {
            let mut locator = [0; 20];
            read_exact(source, end_position - 20, &mut locator)?;
            if u32_at(&locator, 0) == LOCATOR64 {
                let at = u64_at(&locator, 8);
                let mut wide = [0; 56];
                read_exact(source, at, &mut wide)?;
                if u32_at(&wide, 0) == END64 {
                    count = u64_at(&wide, 32);
                    directory_size = u64_at(&wide, 40);
                    directory_at = u64_at(&wide, 48);
                    directory_end = at;
                }
            }
        }
        // A file with something in front of its zip has offsets short by that much.
        let start = directory_end
            .checked_sub(directory_size)
            .ok_or_else(|| damaged("a zip directory larger than the file"))?;
        let shift = start
            .checked_sub(directory_at)
            .ok_or_else(|| damaged("a zip directory in the wrong place"))?;
        let mut raw = vec![0; directory_size as usize];
        read_exact(source, start, &mut raw)?;
        let mut entries = Vec::with_capacity(count.min(1 << 20) as usize);
        let mut at = 0;
        while at + 46 <= raw.len() {
            if u32_at(&raw, at) != CENTRAL {
                return Err(damaged("a damaged zip directory"));
            }
            let total = 46
                + usize::from(u16_at(&raw, at + 28))
                + usize::from(u16_at(&raw, at + 30))
                + usize::from(u16_at(&raw, at + 32));
            let record = raw
                .get(at..at + total)
                .ok_or_else(|| damaged("a damaged zip directory"))?;
            entries.push(parse_entry(record.to_vec(), shift));
            at += total;
        }
        Ok(Self {
            entries,
            comment,
            data_end: start,
        })
    }
}

/// A changed part, deflated and ready to go in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub data: Vec<u8>,
    pub crc: u32,
    pub size: u64,
}

/// Deflates a part while it is written.
pub struct Packer {
    encoder: DeflateEncoder<Vec<u8>>,
    crc: Crc,
    size: u64,
}

impl Default for Packer {
    fn default() -> Self {
        Self {
            encoder: DeflateEncoder::new(Vec::new(), Compression::default()),
            crc: Crc::new(),
            size: 0,
        }
    }
}

impl Packer {
    pub fn finish(self) -> io::Result<Packed> {
        Ok(Packed {
            data: self.encoder.finish()?,
            crc: self.crc.sum(),
            size: self.size,
        })
    }
}

impl Write for Packer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.encoder.write(bytes)?;
        self.crc.update(&bytes[..count]);
        self.size += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.encoder.flush()
    }
}

pub fn pack(bytes: &[u8]) -> io::Result<Packed> {
    let mut packer = Packer::default();
    packer.write_all(bytes)?;
    packer.finish()
}

/// What a save does to the package, by part name in lower case with forward slashes.
#[derive(Debug, Default)]
pub struct ZipChanges {
    pub replace: HashMap<String, Packed>,
    pub remove: Vec<String>,
    /// New parts, by their real name, in the order they go in.
    pub add: Vec<(String, Packed)>,
}

pub fn normalize(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

/// The order entries sit in the file, each with where it ends.
pub fn physical_order(directory: &Directory) -> Vec<(usize, u64)> {
    let mut order: Vec<usize> = (0..directory.entries.len()).collect();
    order.sort_by_key(|&index| directory.entries[index].local);
    let mut spans = Vec::with_capacity(order.len());
    for (place, &index) in order.iter().enumerate() {
        let end = order
            .get(place + 1)
            .map_or(directory.data_end, |&next| directory.entries[next].local);
        spans.push((index, end));
    }
    spans
}

/// Forward-only output that knows how far it has got.
struct Counting<'a> {
    out: &'a mut dyn Write,
    at: u64,
}

impl Counting<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), SaveError> {
        self.out
            .write_all(bytes)
            .map_err(|error| SaveError::Write(error.to_string()))?;
        self.at += bytes.len() as u64;
        Ok(())
    }
}

fn write_local(
    out: &mut Counting<'_>,
    name: &[u8],
    flags: u16,
    time: (u16, u16),
    part: &Packed,
) -> Result<(), SaveError> {
    let wide = part.data.len() as u64 >= u64::from(u32::MAX) || part.size >= u64::from(u32::MAX);
    let mut header = Vec::with_capacity(30 + name.len() + 20);
    header.extend_from_slice(&LOCAL.to_le_bytes());
    header.extend_from_slice(&(if wide { 45u16 } else { 20u16 }).to_le_bytes());
    header.extend_from_slice(&flags.to_le_bytes());
    header.extend_from_slice(&8u16.to_le_bytes());
    header.extend_from_slice(&time.0.to_le_bytes());
    header.extend_from_slice(&time.1.to_le_bytes());
    header.extend_from_slice(&part.crc.to_le_bytes());
    let clamp = |value: u64| value.min(u64::from(u32::MAX)) as u32;
    let (packed, size) = if wide {
        (u32::MAX, u32::MAX)
    } else {
        (clamp(part.data.len() as u64), clamp(part.size))
    };
    header.extend_from_slice(&packed.to_le_bytes());
    header.extend_from_slice(&size.to_le_bytes());
    header.extend_from_slice(&(name.len() as u16).to_le_bytes());
    header.extend_from_slice(&(if wide { 20u16 } else { 0u16 }).to_le_bytes());
    header.extend_from_slice(name);
    if wide {
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes());
        header.extend_from_slice(&part.size.to_le_bytes());
        header.extend_from_slice(&(part.data.len() as u64).to_le_bytes());
    }
    out.write(&header)?;
    out.write(&part.data)
}

/// A central record for a part we wrote, keeping what it can from the one it replaces.
fn new_record(
    old: Option<&[u8]>,
    name: &[u8],
    flags: u16,
    time: (u16, u16),
    part: &Packed,
    local: u64,
) -> Vec<u8> {
    let packed = part.data.len() as u64;
    let mut wide = Vec::new();
    if part.size >= u64::from(u32::MAX) {
        wide.extend_from_slice(&part.size.to_le_bytes());
    }
    if packed >= u64::from(u32::MAX) {
        wide.extend_from_slice(&packed.to_le_bytes());
    }
    if local >= u64::from(u32::MAX) {
        wide.extend_from_slice(&local.to_le_bytes());
    }
    let clamp = |value: u64| value.min(u64::from(u32::MAX)) as u32;
    let mut record = Vec::with_capacity(46 + name.len() + wide.len() + 4);
    record.extend_from_slice(&CENTRAL.to_le_bytes());
    let made_by = old.map_or(20, |old| u16_at(old, 4));
    let need = if wide.is_empty() { 20u16 } else { 45u16 };
    record.extend_from_slice(&made_by.to_le_bytes());
    record.extend_from_slice(&need.to_le_bytes());
    record.extend_from_slice(&flags.to_le_bytes());
    record.extend_from_slice(&8u16.to_le_bytes());
    record.extend_from_slice(&time.0.to_le_bytes());
    record.extend_from_slice(&time.1.to_le_bytes());
    record.extend_from_slice(&part.crc.to_le_bytes());
    record.extend_from_slice(&clamp(packed).to_le_bytes());
    record.extend_from_slice(&clamp(part.size).to_le_bytes());
    record.extend_from_slice(&(name.len() as u16).to_le_bytes());
    let extra = if wide.is_empty() { 0 } else { wide.len() + 4 };
    record.extend_from_slice(&(extra as u16).to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    record.extend_from_slice(&old.map_or(0, |old| u16_at(old, 36)).to_le_bytes());
    record.extend_from_slice(&old.map_or(0, |old| u32_at(old, 38)).to_le_bytes());
    record.extend_from_slice(&clamp(local).to_le_bytes());
    record.extend_from_slice(name);
    if !wide.is_empty() {
        record.extend_from_slice(&1u16.to_le_bytes());
        record.extend_from_slice(&(wide.len() as u16).to_le_bytes());
        record.extend_from_slice(&wide);
    }
    record
}

/// An unchanged entry's record at its new offset. Only the offset can differ from the source.
fn moved_record(entry: &Entry, local: u64) -> Vec<u8> {
    let mut record = entry.record.clone();
    if u32_at(&record, 42) == u32::MAX {
        if let Some((start, length)) = zip64_extra(&record) {
            // The offset comes after any size fields, so it is the extra's last value here.
            let mut at = start;
            for wide in [
                u32_at(&record, 24) == u32::MAX,
                u32_at(&record, 20) == u32::MAX,
            ] {
                if wide {
                    at += 8;
                }
            }
            if at + 8 <= start + length {
                record[at..at + 8].copy_from_slice(&local.to_le_bytes());
                return record;
            }
        }
    }
    if local < u64::from(u32::MAX) {
        record[42..46].copy_from_slice(&(local as u32).to_le_bytes());
        return record;
    }
    // Moved past 4 GB, so it needs a place for the offset.
    let names = usize::from(u16_at(&record, 28));
    let extras = usize::from(u16_at(&record, 30));
    let insert = 46 + names + extras;
    let mut added = Vec::with_capacity(12);
    added.extend_from_slice(&1u16.to_le_bytes());
    added.extend_from_slice(&8u16.to_le_bytes());
    added.extend_from_slice(&local.to_le_bytes());
    record.splice(insert..insert, added);
    record[30..32].copy_from_slice(&((extras + 12) as u16).to_le_bytes());
    record[42..46].copy_from_slice(&u32::MAX.to_le_bytes());
    record
}

/// Copies an entry's bytes across, after checking they look like what the directory says.
async fn copy_span(
    source: &dyn ByteSource,
    entry: &Entry,
    end: u64,
    out: &mut Counting<'_>,
    buffer: &mut [u8],
    pace: &Pace,
) -> Result<(), SaveError> {
    let mut head = [0; 30];
    read_exact(source, entry.local, &mut head)?;
    if u32_at(&head, 0) != LOCAL {
        return Err(SaveError::SourceUnreadable(
            "a part isn't where the file's directory says".to_string(),
        ));
    }
    let header = 30 + u64::from(u16_at(&head, 26)) + u64::from(u16_at(&head, 28));
    if entry.local + header + entry.compressed > end {
        return Err(SaveError::Damaged("zip entries overlap".to_string()));
    }
    let mut at = entry.local;
    while at < end {
        let count = ((end - at) as usize).min(buffer.len());
        read_exact(source, at, &mut buffer[..count])?;
        out.write(&buffer[..count])?;
        at += count as u64;
        pace.advance(count as u64);
        pace.point_now().await;
    }
    Ok(())
}

/// Writes the source's package with these changes. Entries stay in the order they had, both in
/// the file and in its directory. With no changes this writes the entries and directory as they
/// were, so the output differs only in dropping anything in front of the zip.
pub fn rewrite(
    source: &dyn ByteSource,
    directory: &Directory,
    changes: &ZipChanges,
    out: &mut dyn Write,
) -> Result<(), SaveError> {
    block_on(rewrite_paced(
        source,
        directory,
        changes,
        out,
        &Pace::unlimited(),
    ))
}

/// `rewrite`, stopping between chunks and entries at `pace`'s points.
pub async fn rewrite_paced(
    source: &dyn ByteSource,
    directory: &Directory,
    changes: &ZipChanges,
    out: &mut dyn Write,
    pace: &Pace,
) -> Result<(), SaveError> {
    let mut out = Counting { out, at: 0 };
    let mut buffer = vec![0; CHUNK];
    let mut locals = vec![None; directory.entries.len()];
    let mut seen = HashMap::new();
    let mut records: Vec<Option<Vec<u8>>> = vec![None; directory.entries.len()];
    for (index, end) in physical_order(directory) {
        let entry = &directory.entries[index];
        let key = normalize(&entry.name);
        if changes.remove.contains(&key) {
            continue;
        }
        let local = out.at;
        if let Some(part) = changes.replace.get(&key) {
            // A file with this part twice keeps one copy, as readers can see only one.
            if seen.insert(key, ()).is_some() {
                continue;
            }
            let names = usize::from(u16_at(&entry.record, 28));
            let name = &entry.record[46..46 + names];
            let flags = u16_at(&entry.record, 8) & 0x0800;
            let time = (u16_at(&entry.record, 12), u16_at(&entry.record, 14));
            write_local(&mut out, name, flags, time, part)?;
            pace.advance(entry.compressed);
            pace.point_now().await;
            records[index] = Some(new_record(
                Some(&entry.record),
                name,
                flags,
                time,
                part,
                local,
            ));
        } else {
            copy_span(source, entry, end, &mut out, &mut buffer, pace).await?;
            records[index] = Some(moved_record(entry, local));
        }
        locals[index] = Some(local);
    }
    let mut added = Vec::with_capacity(changes.add.len());
    for (name, part) in &changes.add {
        let local = out.at;
        let flags = if name.is_ascii() { 0 } else { 0x0800 };
        // 1 January 1980, as Excel stamps its parts.
        let time = (0, 0x0021);
        write_local(&mut out, name.as_bytes(), flags, time, part)?;
        pace.point_now().await;
        added.push(new_record(None, name.as_bytes(), flags, time, part, local));
    }
    let directory_at = out.at;
    let mut count = 0u64;
    for record in records.iter().flatten().chain(added.iter()) {
        out.write(record)?;
        count += 1;
    }
    let directory_size = out.at - directory_at;
    let wide = count >= 0xFFFF
        || directory_at >= u64::from(u32::MAX)
        || directory_size >= u64::from(u32::MAX);
    if wide {
        let wide_at = out.at;
        let mut record = Vec::with_capacity(76);
        record.extend_from_slice(&END64.to_le_bytes());
        record.extend_from_slice(&44u64.to_le_bytes());
        record.extend_from_slice(&45u16.to_le_bytes());
        record.extend_from_slice(&45u16.to_le_bytes());
        record.extend_from_slice(&[0; 8]);
        record.extend_from_slice(&count.to_le_bytes());
        record.extend_from_slice(&count.to_le_bytes());
        record.extend_from_slice(&directory_size.to_le_bytes());
        record.extend_from_slice(&directory_at.to_le_bytes());
        record.extend_from_slice(&LOCATOR64.to_le_bytes());
        record.extend_from_slice(&[0; 4]);
        record.extend_from_slice(&wide_at.to_le_bytes());
        record.extend_from_slice(&1u32.to_le_bytes());
        out.write(&record)?;
    }
    let clamp16 = |value: u64| value.min(0xFFFF) as u16;
    let clamp32 = |value: u64| value.min(u64::from(u32::MAX)) as u32;
    let mut end = Vec::with_capacity(END_SIZE + directory.comment.len());
    end.extend_from_slice(&END.to_le_bytes());
    end.extend_from_slice(&[0; 4]);
    end.extend_from_slice(&clamp16(count).to_le_bytes());
    end.extend_from_slice(&clamp16(count).to_le_bytes());
    end.extend_from_slice(&clamp32(directory_size).to_le_bytes());
    end.extend_from_slice(&clamp32(directory_at).to_le_bytes());
    end.extend_from_slice(&(directory.comment.len() as u16).to_le_bytes());
    end.extend_from_slice(&directory.comment);
    out.write(&end)
}

/// A stored zip of these parts, which is what a new blank workbook is.
pub fn write_new(parts: &[(&str, &[u8])]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut counting = Counting {
        out: &mut out,
        at: 0,
    };
    let mut records = Vec::new();
    let to_io = |error: SaveError| io::Error::other(error.to_string());
    for (name, bytes) in parts {
        let part = pack(bytes)?;
        let local = counting.at;
        write_local(&mut counting, name.as_bytes(), 0, (0, 0x0021), &part).map_err(to_io)?;
        records.push(new_record(
            None,
            name.as_bytes(),
            0,
            (0, 0x0021),
            &part,
            local,
        ));
    }
    let directory_at = counting.at;
    for record in &records {
        counting.write(record).map_err(to_io)?;
    }
    let directory_size = counting.at - directory_at;
    let mut end = Vec::with_capacity(END_SIZE);
    end.extend_from_slice(&END.to_le_bytes());
    end.extend_from_slice(&[0; 4]);
    end.extend_from_slice(&(records.len() as u16).to_le_bytes());
    end.extend_from_slice(&(records.len() as u16).to_le_bytes());
    end.extend_from_slice(&(directory_size as u32).to_le_bytes());
    end.extend_from_slice(&(directory_at as u32).to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    counting.write(&end).map_err(to_io)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::zip_of;
    use std::io::Read;
    use std::rc::Rc;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    fn parts_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut file = archive.by_index(index).unwrap();
                let mut body = Vec::new();
                file.read_to_end(&mut body).unwrap();
                (file.name().unwrap().to_string(), body)
            })
            .collect()
    }

    fn rewritten(source: &[u8], changes: &ZipChanges) -> Vec<u8> {
        let source = source.to_vec();
        let directory = Directory::read(&source).unwrap();
        let mut out = Vec::new();
        rewrite(&source, &directory, changes, &mut out).unwrap();
        out
    }

    #[test]
    fn no_changes_gives_back_the_same_bytes() {
        let source = zip_of(&[("a.xml", "<a/>"), ("b/c.bin", "binary")]);
        assert_eq!(rewritten(&source, &ZipChanges::default()), source);
    }

    #[test]
    fn keeps_a_comment_and_stored_entries_and_descriptors() {
        let mut out = std::io::Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut out);
            writer.set_comment("kept").unwrap();
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            writer.start_file("stored.txt", stored).unwrap();
            writer.write_all(b"plain").unwrap();
            let wide = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .large_file(true);
            writer.start_file("wide.txt", wide).unwrap();
            writer.write_all(b"zip64 headers on a small file").unwrap();
            writer.finish().unwrap();
        }
        let source = out.into_inner();
        assert_eq!(rewritten(&source, &ZipChanges::default()), source);
    }

    #[test]
    fn replaces_removes_and_adds_and_copies_the_rest_exactly() {
        let source = zip_of(&[
            ("one.xml", "<one/>"),
            ("gone.xml", "<x/>"),
            ("two.xml", "<two/>"),
            ("three.bin", "333"),
        ]);
        let mut changes = ZipChanges::default();
        changes
            .replace
            .insert("two.xml".to_string(), pack(b"<two>new</two>").unwrap());
        changes.remove.push("gone.xml".to_string());
        changes
            .add
            .push(("new/part.xml".to_string(), pack("<ن/>".as_bytes()).unwrap()));
        let out = rewritten(&source, &changes);
        assert_eq!(
            parts_of(&out),
            [
                ("one.xml".to_string(), b"<one/>".to_vec()),
                ("two.xml".to_string(), b"<two>new</two>".to_vec()),
                ("three.bin".to_string(), b"333".to_vec()),
                ("new/part.xml".to_string(), "<ن/>".as_bytes().to_vec()),
            ]
        );
        // The untouched entries' whole spans match: header, data and all.
        let before = Directory::read(&source).unwrap();
        let after = Directory::read(&out).unwrap();
        let span = |bytes: &[u8], directory: &Directory, name: &str| {
            let at = directory
                .entries
                .iter()
                .position(|e| e.name == name)
                .unwrap();
            let entry = &directory.entries[at];
            let end = physical_order(directory)
                .into_iter()
                .find(|&(index, _)| index == at)
                .unwrap()
                .1;
            bytes[entry.local as usize..end as usize].to_vec()
        };
        for name in ["one.xml", "three.bin"] {
            assert_eq!(span(&source, &before, name), span(&out, &after, name));
        }
        let package = crate::Package::open(Rc::new(out)).unwrap();
        assert_eq!(package.read("two.xml").unwrap(), b"<two>new</two>");
    }

    #[test]
    fn a_physical_order_unlike_the_directory_order_survives() {
        let mut source = zip_of(&[("first.xml", "<a/>"), ("second.xml", "<b/>")]);
        // Swap the two directory records, which is legal and leaves the data where it is.
        let directory = Directory::read(&source).unwrap();
        let start = directory.data_end as usize;
        let one = directory.entries[0].record.clone();
        let two = directory.entries[1].record.clone();
        source[start..start + two.len()].copy_from_slice(&two);
        source[start + two.len()..start + two.len() + one.len()].copy_from_slice(&one);
        assert_eq!(rewritten(&source, &ZipChanges::default()), source);
    }

    #[test]
    fn overlapping_entries_are_refused_rather_than_guessed_at() {
        let mut source = zip_of(&[
            ("first.xml", "<a>some text to compress</a>"),
            ("second.xml", "<b/>"),
        ]);
        let directory = Directory::read(&source).unwrap();
        // Point the second entry's offset into the first entry's data.
        let record_at = directory.data_end as usize + directory.entries[0].record.len();
        let inside = (directory.entries[0].local + 31) as u32;
        source[record_at + 42..record_at + 46].copy_from_slice(&inside.to_le_bytes());
        let directory = Directory::read(&source).unwrap();
        let error = rewrite(&source, &directory, &ZipChanges::default(), &mut Vec::new());
        assert!(error.is_err());
    }

    #[test]
    fn a_new_zip_opens_with_the_zip_crate_and_in_excels_format() {
        let bytes = write_new(&[("a.xml", b"<a/>"), ("b.xml", b"<b/>")]).unwrap();
        assert_eq!(parts_of(&bytes).len(), 2);
    }
}
