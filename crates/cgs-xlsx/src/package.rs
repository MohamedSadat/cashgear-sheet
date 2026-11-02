//! The zip package: finding parts and reading them without loading the file into memory.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::rc::Rc;

use flate2::{Decompress, FlushDecompress, Status};
use zip::{CompressionMethod, ZipArchive};

use crate::ReadError;

/// Random access to the file's bytes: a `File` in the browser, memory or a file natively.
pub trait ByteSource {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Fills `out` from `offset`, and returns how many bytes it read.
    fn read_at(&self, offset: u64, out: &mut [u8]) -> io::Result<usize>;
}

impl ByteSource for Vec<u8> {
    fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    fn read_at(&self, offset: u64, out: &mut [u8]) -> io::Result<usize> {
        let start = (offset as usize).min(self.as_slice().len());
        let count = out.len().min(self.as_slice().len() - start);
        out[..count].copy_from_slice(&self[start..start + count]);
        Ok(count)
    }
}

/// Lets the zip crate read the central directory through a `ByteSource`.
struct SourceReader {
    source: Rc<dyn ByteSource>,
    at: u64,
}

impl Read for SourceReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = self.source.read_at(self.at, out)?;
        self.at += count as u64;
        Ok(count)
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let len = self.source.len() as i128;
        let next = match to {
            SeekFrom::Start(at) => i128::from(at),
            SeekFrom::End(by) => len + i128::from(by),
            SeekFrom::Current(by) => i128::from(self.at) + i128::from(by),
        };
        if next < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before 0"));
        }
        self.at = next as u64;
        Ok(self.at)
    }
}

#[derive(Debug, Clone)]
pub struct PartInfo {
    pub name: String,
    pub offset: u64,
    pub compressed: u64,
    pub size: u64,
    pub deflated: bool,
}

/// The package's parts, found by name whatever the case or slash direction a writer used.
pub struct Package {
    source: Rc<dyn ByteSource>,
    parts: Vec<PartInfo>,
    by_name: HashMap<String, usize>,
}

fn normalize(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

/// Does the file start as an old binary Office file? Both `.xls` and password-protected
/// `.xlsx` files do; a protected one names its `EncryptedPackage` in the first sectors.
fn classify_compound_file(source: &dyn ByteSource) -> Option<ReadError> {
    let mut head = vec![0u8; 512 * 1024];
    let count = source.read_at(0, &mut head).ok()?;
    head.truncate(count);
    if !head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return None;
    }
    let marker: Vec<u8> = "EncryptedPackage"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    if head.windows(marker.len()).any(|window| window == marker) {
        Some(ReadError::Encrypted)
    } else {
        Some(ReadError::OldFormat)
    }
}

impl Package {
    pub fn open(source: Rc<dyn ByteSource>) -> Result<Self, ReadError> {
        if let Some(error) = classify_compound_file(source.as_ref()) {
            return Err(error);
        }
        let reader = SourceReader {
            source: Rc::clone(&source),
            at: 0,
        };
        let mut archive =
            ZipArchive::new(reader).map_err(|error| ReadError::NotAPackage(error.to_string()))?;
        let mut parts = Vec::with_capacity(archive.len());
        let mut by_name = HashMap::with_capacity(archive.len());
        for index in 0..archive.len() {
            let Ok(file) = archive.by_index_raw(index) else {
                continue;
            };
            if file.encrypted() {
                return Err(ReadError::Encrypted);
            }
            let Ok(name) = file.name().map(|name| name.to_string()) else {
                continue;
            };
            let deflated = match file.compression() {
                CompressionMethod::Deflated => true,
                CompressionMethod::Stored => false,
                // A compression we can't read: the part counts as missing.
                _ => continue,
            };
            let Some(offset) = file.data_start() else {
                continue;
            };
            let info = PartInfo {
                name: name.clone(),
                offset,
                compressed: file.compressed_size(),
                size: file.size(),
                deflated,
            };
            by_name.insert(normalize(&name), parts.len());
            parts.push(info);
        }
        Ok(Self {
            source,
            parts,
            by_name,
        })
    }

    pub fn part(&self, name: &str) -> Option<&PartInfo> {
        self.by_name
            .get(&normalize(name))
            .map(|&at| &self.parts[at])
    }

    pub fn parts(&self) -> &[PartInfo] {
        &self.parts
    }

    /// Streams a part. Holds no borrow, so a paused sheet can keep it between slices.
    pub fn stream(&self, name: &str) -> Option<PartStream> {
        Some(PartStream::new(Rc::clone(&self.source), self.part(name)?))
    }

    /// Reads a whole part, for the small ones.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let mut stream = self.stream(name)?;
        let mut out = Vec::with_capacity(self.part(name)?.size.min(16 * 1024 * 1024) as usize);
        stream.read_to_end(&mut out).ok()?;
        Some(out)
    }
}

const COMPRESSED_CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    /// Not looked at yet.
    Unknown,
    Utf8,
    Utf16 {
        big_endian: bool,
    },
}

/// A part's bytes, inflated on the way out. XML in UTF-16 comes out as UTF-8.
pub struct PartStream {
    source: Rc<dyn ByteSource>,
    next: u64,
    end: u64,
    start: u64,
    inflate: Option<Decompress>,
    input: Vec<u8>,
    input_at: usize,
    finished: bool,
    encoding: Encoding,
    /// UTF-8 ready to hand out, and a trailing byte or surrogate of UTF-16 not yet decoded.
    decoded: Vec<u8>,
    decoded_at: usize,
    carry: Vec<u8>,
}

impl PartStream {
    fn new(source: Rc<dyn ByteSource>, part: &PartInfo) -> Self {
        Self {
            source,
            next: part.offset,
            end: part.offset + part.compressed,
            start: part.offset,
            inflate: part.deflated.then(|| Decompress::new(false)),
            input: Vec::new(),
            input_at: 0,
            finished: false,
            encoding: Encoding::Unknown,
            decoded: Vec::new(),
            decoded_at: 0,
            carry: Vec::new(),
        }
    }

    /// Compressed bytes taken from the file so far.
    pub fn consumed(&self) -> u64 {
        self.next - self.start
    }

    fn refill(&mut self) -> io::Result<()> {
        let want = ((self.end - self.next) as usize).min(COMPRESSED_CHUNK);
        self.input.resize(want, 0);
        let count = if want == 0 {
            0
        } else {
            self.source.read_at(self.next, &mut self.input)?
        };
        self.input.truncate(count);
        self.input_at = 0;
        self.next += count as u64;
        Ok(())
    }
}

/// Finds UTF-16 from a byte order mark, or from `<` with a zero beside it.
fn sniff(bytes: &[u8]) -> (Encoding, usize) {
    match bytes {
        [0xFE, 0xFF, ..] => (Encoding::Utf16 { big_endian: true }, 2),
        [0xFF, 0xFE, ..] => (Encoding::Utf16 { big_endian: false }, 2),
        [0x00, b'<', ..] => (Encoding::Utf16 { big_endian: true }, 0),
        [b'<', 0x00, ..] => (Encoding::Utf16 { big_endian: false }, 0),
        _ => (Encoding::Utf8, 0),
    }
}

impl PartStream {
    /// Decodes `carry` plus `raw`, keeping any part of a character for next time.
    fn decode_utf16(&mut self, raw: &[u8], big_endian: bool) {
        self.carry.extend_from_slice(raw);
        let unit = |pair: &[u8]| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        };
        let mut usable = self.carry.len() & !1;
        // A high surrogate at the end waits for its partner.
        if usable >= 2 && (0xD800..0xDC00).contains(&unit(&self.carry[usable - 2..usable])) {
            usable -= 2;
        }
        let (pairs, _) = self.carry[..usable].as_chunks::<2>();
        let units = pairs.iter().map(|pair| unit(pair));
        let text: String = char::decode_utf16(units)
            .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
        self.decoded.clear();
        self.decoded_at = 0;
        self.decoded.extend_from_slice(text.as_bytes());
        self.carry.drain(..usable);
    }
}

impl Read for PartStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.decoded_at < self.decoded.len() {
                let count = (self.decoded.len() - self.decoded_at).min(out.len());
                out[..count]
                    .copy_from_slice(&self.decoded[self.decoded_at..self.decoded_at + count]);
                self.decoded_at += count;
                return Ok(count);
            }
            if self.encoding == Encoding::Utf8 {
                return self.read_raw(out);
            }
            let mut raw = vec![0u8; 64 * 1024];
            let count = self.read_raw(&mut raw)?;
            raw.truncate(count);
            let mut skip = 0;
            if self.encoding == Encoding::Unknown {
                let (encoding, bom) = sniff(&raw);
                self.encoding = encoding;
                skip = bom;
            }
            match self.encoding {
                Encoding::Utf16 { big_endian } => {
                    if count == 0 {
                        return Ok(0);
                    }
                    self.decode_utf16(&raw[skip..], big_endian);
                }
                _ => {
                    self.decoded = raw;
                    self.decoded_at = 0;
                    if count == 0 {
                        return Ok(0);
                    }
                }
            }
        }
    }
}

impl PartStream {
    fn read_raw(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished {
            return Ok(0);
        }
        loop {
            if self.input_at >= self.input.len() {
                self.refill()?;
            }
            let input = &self.input[self.input_at..];
            let Some(inflate) = self.inflate.as_mut() else {
                let count = input.len().min(out.len());
                out[..count].copy_from_slice(&input[..count]);
                self.input_at += count;
                if count == 0 {
                    self.finished = true;
                }
                return Ok(count);
            };
            let before_in = inflate.total_in();
            let before_out = inflate.total_out();
            let flush = if input.is_empty() {
                FlushDecompress::Finish
            } else {
                FlushDecompress::None
            };
            let status = inflate
                .decompress(input, out, flush)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            self.input_at += (inflate.total_in() - before_in) as usize;
            let produced = (inflate.total_out() - before_out) as usize;
            if status == Status::StreamEnd {
                self.finished = true;
                return Ok(produced);
            }
            if produced > 0 {
                return Ok(produced);
            }
            if input.is_empty() {
                // The compressed data ran out before its end marker.
                self.finished = true;
                return Ok(0);
            }
        }
    }
}
