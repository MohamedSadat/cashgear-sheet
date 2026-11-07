//! Fonts and text: each face's fallback chain, shaping with rustybuzz in bidirectional runs,
//! and the subsets embedded as CID fonts with a ToUnicode map, so text can be searched and copied.

use std::collections::{BTreeMap, HashMap};

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
    /// Codes in the PDF, one for each glyph with each text it stands for, so a glyph that
    /// several letters share, as Noto's Arabic skeletons are, copies as the right letter. Each is
    /// (subset glyph, glyph in the whole font, text); code 0 is `.notdef`.
    codes: Vec<(u16, u16, String)>,
    code_of: HashMap<(u16, String), u16>,
    /// CFF outlines take no map from codes to glyphs, so their codes are the glyphs.
    cff: bool,
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
        let cff = face.tables().cff.is_some();
        Some(Self {
            data,
            index,
            name,
            face,
            used: BTreeMap::new(),
            remapper: subsetter::GlyphRemapper::new(),
            codes: vec![(0, 0, String::new())],
            code_of: HashMap::new(),
            cff,
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

/// A shaped glyph: which file, its code in the PDF, and how it moves the pen, in ems.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub file: usize,
    pub id: u16,
    /// Its id in the whole font.
    pub old: u16,
    /// A mark with no text of its own, such as a dot Noto draws apart from its letter: drawn as
    /// an outline, so it neither copies as a stray character nor spaces the letters out.
    pub mark: bool,
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
    // A cluster's text goes on its widest glyph, the letter itself: Noto draws dots as marks that
    // many letters share, which must copy as nothing.
    let mut base: std::collections::HashMap<u32, (usize, i32)> = std::collections::HashMap::new();
    for (index, (info, pos)) in infos.iter().zip(positions).enumerate() {
        let entry = base.entry(info.cluster).or_insert((index, pos.x_advance));
        if pos.x_advance > entry.1 {
            *entry = (index, pos.x_advance);
        }
    }
    let mut raw = Vec::with_capacity(infos.len());
    for (index, (info, pos)) in infos.iter().zip(positions).enumerate() {
        let old = info.glyph_id as u16;
        let text = if base.get(&info.cluster).map(|b| b.0) == Some(index) {
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
        let mark_glyph = text.is_empty() && advance == 0;
        if !mark_glyph {
            let entry = font.used.entry(old).or_default();
            if entry.is_empty() && !text.is_empty() {
                *entry = text.clone();
            }
        }
        let mark = text.is_empty() && advance == 0;
        let new = if mark { 0 } else { font.remapper.remap(old) };
        let id = if mark {
            0
        } else if font.cff {
            new
        } else {
            let key = (new, text.clone());
            match font.code_of.get(&key) {
                Some(&code) => code,
                None => {
                    let code = u16::try_from(font.codes.len()).unwrap_or(u16::MAX);
                    font.codes.push((new, old, text.clone()));
                    font.code_of.insert(key, code);
                    code
                }
            }
        };
        glyphs.push(Glyph {
            file,
            id,
            old,
            mark,
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
        let is_cff = file.cff;
        let type0 = refs[index];
        let cid = next();
        let descriptor = next();
        let stream = next();
        let cmap_ref = next();
        let map_ref = next();
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
                cid_font.cid_to_gid_map_stream(map_ref);
            }
            let mut widths = cid_font.widths();
            if is_cff {
                for old in mapper.remapped_gids() {
                    let new = mapper.get(old).unwrap_or(0);
                    widths.consecutive(new, [file.advance(old) * 1000.0]);
                }
            } else {
                widths.consecutive(
                    0,
                    file.codes
                        .iter()
                        .map(|&(_, old, _)| file.advance(old) * 1000.0),
                );
            }
        }
        if !is_cff {
            // Each code's glyph in the subset, two bytes a code.
            let map: Vec<u8> = file
                .codes
                .iter()
                .flat_map(|&(new, _, _)| new.to_be_bytes())
                .collect();
            let packed = miniz_oxide::deflate::compress_to_vec_zlib(&map, 6);
            pdf.stream(map_ref, &packed).filter(Filter::FlateDecode);
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
        // A glyph with no text of its own, such as a dot drawn as a mark, maps to nothing, so a
        // reader doesn't guess a character for it.
        if is_cff {
            for (&old, text) in &file.used {
                let Some(new) = mapper.get(old) else { continue };
                cmap.pair_with_multiple(new, text.chars());
            }
        } else {
            for (code, (_, _, text)) in file.codes.iter().enumerate().skip(1) {
                cmap.pair_with_multiple(code as u16, text.chars());
            }
        }
        let data = cmap.finish();
        pdf.cmap(cmap_ref, data.as_slice());
    }
    Ok(())
}

/// Draws a glyph's outline as a path, its origin at `x`, `y` in a space whose y runs down.
pub fn outline(
    files: &[FontFile<'_>],
    glyph: &Glyph,
    x: f32,
    y: f32,
    size: f32,
    content: &mut pdf_writer::Content,
) -> bool {
    struct Pen<'c> {
        content: &'c mut pdf_writer::Content,
        x: f32,
        y: f32,
        scale: f32,
        last: (f32, f32),
    }
    impl Pen<'_> {
        fn at(&self, fx: f32, fy: f32) -> (f32, f32) {
            (self.x + fx * self.scale, self.y - fy * self.scale)
        }
    }
    impl rustybuzz::ttf_parser::OutlineBuilder for Pen<'_> {
        fn move_to(&mut self, fx: f32, fy: f32) {
            let (px, py) = self.at(fx, fy);
            self.content.move_to(px, py);
            self.last = (px, py);
        }
        fn line_to(&mut self, fx: f32, fy: f32) {
            let (px, py) = self.at(fx, fy);
            self.content.line_to(px, py);
            self.last = (px, py);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, fx: f32, fy: f32) {
            let (qx, qy) = self.at(x1, y1);
            let (px, py) = self.at(fx, fy);
            let (sx, sy) = self.last;
            self.content.cubic_to(
                sx + 2.0 / 3.0 * (qx - sx),
                sy + 2.0 / 3.0 * (qy - sy),
                px + 2.0 / 3.0 * (qx - px),
                py + 2.0 / 3.0 * (qy - py),
                px,
                py,
            );
            self.last = (px, py);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, fx: f32, fy: f32) {
            let (ax, ay) = self.at(x1, y1);
            let (bx, by) = self.at(x2, y2);
            let (px, py) = self.at(fx, fy);
            self.content.cubic_to(ax, ay, bx, by, px, py);
            self.last = (px, py);
        }
        fn close(&mut self) {
            self.content.close_path();
        }
    }
    let file = &files[glyph.file];
    let mut pen = Pen {
        content,
        x,
        y,
        scale: size / file.units,
        last: (x, y),
    };
    file.face
        .outline_glyph(GlyphId(glyph.old), &mut pen)
        .is_some()
}
