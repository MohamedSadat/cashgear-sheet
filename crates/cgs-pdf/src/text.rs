//! Fonts and text: each face's fallback chain, shaping with rustybuzz in bidirectional runs,
//! and the subsets embedded as CID fonts with a ToUnicode map, so text can be searched and copied.

use std::collections::BTreeMap;

use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Filter, Name, Pdf, Rect, Ref, Str};

use rustybuzz::ttf_parser::GlyphId;
use rustybuzz::{Direction, Face, UnicodeBuffer};
use unicode_bidi::{BidiInfo, Level};

/// A font file, as the document lists it.
pub struct FontFile<'a> {
    pub data: &'a [u8],
    pub index: u32,
    pub name: String,
    face: Face<'a>,
    /// What each glyph used stands for, by its id in the whole font.
    used: BTreeMap<u16, String>,
    /// The subset's glyph ids, given out as glyphs are first used.
    pub remapper: subsetter::GlyphRemapper,
    pub units: f32,
    pub embeddable: bool,
}

impl<'a> FontFile<'a> {
    pub fn new(data: &'a [u8], index: u32, name: String) -> Option<Self> {
        let face = Face::from_slice(data, index)?;
        let units = face.units_per_em() as f32;
        // OS/2 `fsType`: bit 1 is a restricted licence, bit 9 allows only bitmaps.
        let embeddable = face.tables().os2.is_none_or(|os2| {
            let permissions = os2.permissions();
            !matches!(
                permissions,
                Some(rustybuzz::ttf_parser::Permissions::Restricted)
            )
        });
        Some(Self {
            data,
            index,
            name,
            face,
            used: BTreeMap::new(),
            remapper: subsetter::GlyphRemapper::new(),
            units,
            embeddable,
        })
    }

    fn has(&self, ch: char) -> bool {
        self.face.glyph_index(ch).is_some()
    }

    pub fn ascent(&self) -> f32 {
        f32::from(self.face.ascender()) / self.units
    }

    pub fn descent(&self) -> f32 {
        -f32::from(self.face.descender()) / self.units
    }

    fn advance(&self, glyph: u16) -> f32 {
        f32::from(self.face.glyph_hor_advance(GlyphId(glyph)).unwrap_or(0)) / self.units
    }
}

/// A shaped glyph: which file, its id in the subset, and how it moves the pen, in ems.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub file: usize,
    pub id: u16,
    /// The width the font gives it, which the PDF's widths say.
    pub natural: f32,
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

fn is_joiner(ch: char) -> bool {
    matches!(ch, '\u{200c}' | '\u{200d}' | '\u{fe0f}' | '\u{fe0e}')
        || ('\u{0300}'..='\u{036f}').contains(&ch)
        || ('\u{064b}'..='\u{065f}').contains(&ch)
        || ch == '\u{0670}'
}

/// Shapes `text` with the first file of `chain` that has each character, in the paragraph
/// direction given, and returns its glyphs left to right.
pub fn shape(files: &mut [FontFile<'_>], chain: &[usize], text: &str, rtl: bool) -> Vec<Glyph> {
    if text.is_empty() || chain.is_empty() {
        return Vec::new();
    }
    let level = if rtl { Level::rtl() } else { Level::ltr() };
    let bidi = BidiInfo::new(text, Some(level));
    let mut out = Vec::new();
    for paragraph in &bidi.paragraphs {
        let line = paragraph.range.clone();
        let (levels, runs) = bidi.visual_runs(paragraph, line);
        for run in runs {
            let run_rtl = levels[run.start].is_rtl();
            let piece = &text[run.clone()];
            // Split by which file has the characters; marks and joiners stay with what they join.
            let mut chunks: Vec<(usize, usize, usize)> = Vec::new();
            for (at, ch) in piece.char_indices() {
                let file = if is_joiner(ch) || ch.is_whitespace() {
                    chunks.last().map(|c| c.0)
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    chain
                        .iter()
                        .copied()
                        .find(|&f| files[f].has(ch))
                        .unwrap_or(chain[0])
                });
                match chunks.last_mut() {
                    Some(last) if last.0 == file => last.2 = at + ch.len_utf8(),
                    _ => chunks.push((file, at, at + ch.len_utf8())),
                }
            }
            // A space before anything was assigned took the first file; that's fine.
            let mut shaped: Vec<Vec<Glyph>> = chunks
                .iter()
                .map(|&(file, start, end)| shape_chunk(files, file, &piece[start..end], run_rtl))
                .collect();
            if run_rtl {
                shaped.reverse();
            }
            out.extend(shaped.into_iter().flatten());
        }
    }
    out
}

fn shape_chunk(files: &mut [FontFile<'_>], file: usize, text: &str, rtl: bool) -> Vec<Glyph> {
    let font = &files[file];
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    buffer.guess_segment_properties();
    let shaped = rustybuzz::shape(&font.face, &[], buffer);
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    // Each cluster's text, so a ligature copies as its letters.
    let mut starts: Vec<usize> = infos.iter().map(|i| i.cluster as usize).collect();
    starts.sort_unstable();
    starts.dedup();
    let cluster_text = |cluster: usize| -> String {
        let next = starts
            .iter()
            .copied()
            .find(|&s| s > cluster)
            .unwrap_or(text.len());
        text.get(cluster..next).unwrap_or("").to_string()
    };
    let units = font.units;
    let mut raw = Vec::with_capacity(infos.len());
    let mut seen_cluster: Option<u32> = None;
    for (info, pos) in infos.iter().zip(positions) {
        let old = info.glyph_id as u16;
        let first_of_cluster = seen_cluster != Some(info.cluster);
        seen_cluster = Some(info.cluster);
        let text = if first_of_cluster {
            cluster_text(info.cluster as usize)
        } else {
            String::new()
        };
        raw.push((
            old,
            font.advance(old),
            pos.x_advance,
            pos.x_offset,
            pos.y_offset,
            text,
        ));
    }
    let font = &mut files[file];
    let mut glyphs = Vec::with_capacity(raw.len());
    for (old, natural, advance, x_offset, y_offset, text) in raw {
        let entry = font.used.entry(old).or_default();
        if entry.is_empty() && !text.is_empty() {
            *entry = text.clone();
        }
        glyphs.push(Glyph {
            file,
            id: font.remapper.remap(old),
            natural,
            advance: advance as f32 / units,
            x_offset: x_offset as f32 / units,
            y_offset: y_offset as f32 / units,
        });
    }
    glyphs
}

/// The width of shaped glyphs, in ems.
pub fn width(glyphs: &[Glyph]) -> f32 {
    glyphs.iter().map(|g| g.advance).sum()
}

fn base_name(name: &str, index: usize) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    // A subset's name starts with six capitals, as the PDF reference asks.
    let tag: String = (0..6)
        .map(|i| (b'A' + ((index * 7 + i * 3) % 26) as u8) as char)
        .collect();
    format!("{tag}+{}", if clean.is_empty() { "Font" } else { &clean })
}

/// Writes every used file as a Type 0 font with a subset of its glyphs.
pub fn embed(
    pdf: &mut Pdf,
    next: &mut impl FnMut() -> Ref,
    files: &[FontFile<'_>],
    refs: &[Ref],
) -> Result<(), String> {
    for (index, file) in files.iter().enumerate() {
        if file.used.is_empty() {
            continue;
        }
        let mapper = &file.remapper;
        let subset = subsetter::subset(file.data, file.index, mapper)
            .map_err(|error| format!("The font {} couldn't be subset: {error}", file.name))?;
        let is_cff = subset.len() >= 4 && &subset[..4] == b"OTTO";
        let type0 = refs[index];
        let cid = next();
        let descriptor = next();
        let stream = next();
        let cmap_ref = next();
        let name = base_name(&file.name, index);
        pdf.type0_font(type0)
            .base_font(Name(name.as_bytes()))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(cid)
            .to_unicode(cmap_ref);
        let info = SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        };
        {
            let mut cid_font = pdf.cid_font(cid);
            cid_font
                .subtype(if is_cff {
                    CidFontType::Type0
                } else {
                    CidFontType::Type2
                })
                .base_font(Name(name.as_bytes()))
                .system_info(info)
                .font_descriptor(descriptor)
                .default_width(0.0);
            if !is_cff {
                cid_font.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            let mut widths = cid_font.widths();
            for old in mapper.remapped_gids() {
                let new = mapper.get(old).unwrap_or(0);
                widths.consecutive(new, [file.advance(old) * 1000.0]);
            }
        }
        let face = &file.face;
        let bbox = face.global_bounding_box();
        let scale = 1000.0 / file.units;
        let mut flags = FontFlags::NON_SYMBOLIC;
        if face.is_italic() {
            flags |= FontFlags::ITALIC;
        }
        if face.is_monospaced() {
            flags |= FontFlags::FIXED_PITCH;
        }
        let mut descriptor_dict = pdf.font_descriptor(descriptor);
        descriptor_dict
            .name(Name(name.as_bytes()))
            .flags(flags)
            .bbox(Rect::new(
                f32::from(bbox.x_min) * scale,
                f32::from(bbox.y_min) * scale,
                f32::from(bbox.x_max) * scale,
                f32::from(bbox.y_max) * scale,
            ))
            .italic_angle(face.italic_angle())
            .ascent(f32::from(face.ascender()) * scale)
            .descent(f32::from(face.descender()) * scale)
            .cap_height(f32::from(face.capital_height().unwrap_or(face.ascender())) * scale)
            .stem_v(80.0);
        if is_cff {
            descriptor_dict.font_file3(stream);
        } else {
            descriptor_dict.font_file2(stream);
        }
        drop(descriptor_dict);
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&subset, 6);
        let mut font_stream = pdf.stream(stream, &packed);
        font_stream.filter(Filter::FlateDecode);
        if is_cff {
            font_stream.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
        drop(font_stream);
        let mut cmap = pdf_writer::types::UnicodeCmap::<u16>::new(Name(b"Custom"), info);
        for (&old, text) in &file.used {
            let Some(new) = mapper.get(old) else { continue };
            if text.is_empty() {
                continue;
            }
            cmap.pair_with_multiple(new, text.chars());
        }
        let data = cmap.finish();
        pdf.cmap(cmap_ref, data.as_slice());
    }
    Ok(())
}
