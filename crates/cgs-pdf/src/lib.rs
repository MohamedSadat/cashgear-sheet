//! Vector PDF from a recording of the grid's own drawing. The UI draws each page with the same
//! code that draws the screen, into a context that records paths, text and images in page
//! points, and this writes them out: text shaped and embedded as font subsets, so it can be
//! searched and copied, Arabic included.

mod text;

use std::collections::HashMap;

use pdf_writer::types::{FunctionShadingType, LineCapStyle, LineJoinStyle, PaintType, TilingType};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use serde::Deserialize;

pub use text::FontFile;

/// A font file inside the blob that comes with the document.
#[derive(Debug, Clone, Deserialize)]
pub struct FileRef {
    pub offset: usize,
    pub length: usize,
    #[serde(default)]
    pub index: u32,
    pub name: String,
}

/// A face as the drawing names it: its files in the order characters fall back through.
#[derive(Debug, Clone, Deserialize)]
pub struct FaceSpec {
    pub chain: Vec<usize>,
}

/// A picture inside the blob: RGBA pixels, or a JPEG as it is.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageSpec {
    pub offset: usize,
    pub length: usize,
    pub width: u32,
    pub height: u32,
    /// `rgba` or `jpeg`.
    pub kind: String,
}

/// What a path is filled with.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Paint {
    /// `#rrggbbaa`.
    Color(String),
    /// A linear gradient from `(x0, y0)` to `(x1, y1)`, in page points.
    Linear {
        linear: [f32; 4],
        stops: Vec<(f32, String)>,
    },
    /// A picture repeated, its top left at `x`, `y` and each copy `w` by `h` points.
    Tiles {
        image: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
}

/// One drawing step. Paths are in page points, y down from the top left; text and pictures
/// carry the matrix that takes their own space there.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "k", rename_all = "camelCase")]
pub enum Op {
    Save,
    Restore,
    /// `d` is flat: 0 move x y, 1 line x y, 2 curve x1 y1 x2 y2 x y, 3 close, 4 rect x y w h.
    Path {
        d: Vec<f32>,
        #[serde(default)]
        fill: Option<Paint>,
        #[serde(default)]
        stroke: Option<String>,
        #[serde(default = "one")]
        lw: f32,
        #[serde(default)]
        dash: Vec<f32>,
        #[serde(default)]
        evenodd: bool,
        /// `butt`, `round` or `square`.
        #[serde(default)]
        cap: Option<String>,
        #[serde(default)]
        join: Option<String>,
    },
    Clip {
        d: Vec<f32>,
        #[serde(default)]
        evenodd: bool,
    },
    Text {
        t: String,
        x: f32,
        y: f32,
        m: [f32; 6],
        face: usize,
        size: f32,
        fill: String,
        #[serde(default)]
        rtl: bool,
        #[serde(default)]
        align: Option<String>,
        #[serde(default)]
        base: Option<String>,
    },
    Image {
        image: usize,
        /// Takes the picture's unit square, top left at 0, 0, to the page.
        m: [f32; 6],
    },
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageSpec {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub title: String,
    pub pages: Vec<PageSpec>,
    #[serde(default)]
    pub files: Vec<FileRef>,
    #[serde(default)]
    pub faces: Vec<FaceSpec>,
    #[serde(default)]
    pub images: Vec<ImageSpec>,
}

/// `#rrggbbaa` or `#rrggbb` as 0 to 1, with alpha.
fn color(text: &str) -> ([f32; 3], f32) {
    let hex = text.trim_start_matches('#');
    let part = |at: usize| {
        hex.get(at..at + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .map_or(0.0, |v| f32::from(v) / 255.0)
    };
    let alpha = if hex.len() >= 8 { part(6) } else { 1.0 };
    ([part(0), part(2), part(4)], alpha)
}

fn multiply(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    // a then b, as canvas matrices: b × a.
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn path(content: &mut Content, d: &[f32]) {
    let mut at = 0usize;
    let get = |i: usize| d.get(i).copied().unwrap_or(0.0);
    while at < d.len() {
        match d[at] as i32 {
            0 => {
                content.move_to(get(at + 1), get(at + 2));
                at += 3;
            }
            1 => {
                content.line_to(get(at + 1), get(at + 2));
                at += 3;
            }
            2 => {
                content.cubic_to(
                    get(at + 1),
                    get(at + 2),
                    get(at + 3),
                    get(at + 4),
                    get(at + 5),
                    get(at + 6),
                );
                at += 7;
            }
            3 => {
                content.close_path();
                at += 1;
            }
            4 => {
                content.rect(get(at + 1), get(at + 2), get(at + 3), get(at + 4));
                at += 5;
            }
            _ => break,
        }
    }
}

/// Resources a page uses, named as its content names them.
#[derive(Default)]
struct Resources {
    alphas: Vec<(u32, Ref)>,
    patterns: Vec<(String, Ref)>,
    images: Vec<(usize, Ref)>,
    fonts: Vec<usize>,
}

struct Writer<'a> {
    pdf: Pdf,
    next: i32,
    blob: &'a [u8],
    doc: &'a Document,
    files: Vec<FontFile<'a>>,
    font_refs: Vec<Ref>,
    image_refs: HashMap<usize, Ref>,
    alpha_refs: HashMap<u32, Ref>,
}

impl<'a> Writer<'a> {
    fn alloc(&mut self) -> Ref {
        self.next += 1;
        Ref::new(self.next)
    }

    fn alpha(&mut self, resources: &mut Resources, alpha: f32) -> Option<String> {
        if alpha >= 0.999 {
            return None;
        }
        let key = (alpha * 1000.0).round() as u32;
        let id = match self.alpha_refs.get(&key) {
            Some(&id) => id,
            None => {
                let id = self.alloc();
                self.pdf
                    .ext_graphics(id)
                    .non_stroking_alpha(alpha)
                    .stroking_alpha(alpha);
                self.alpha_refs.insert(key, id);
                id
            }
        };
        if !resources.alphas.iter().any(|(k, _)| *k == key) {
            resources.alphas.push((key, id));
        }
        Some(format!("A{key}"))
    }

    fn image(&mut self, index: usize) -> Result<Ref, String> {
        if let Some(&id) = self.image_refs.get(&index) {
            return Ok(id);
        }
        let spec = self
            .doc
            .images
            .get(index)
            .ok_or_else(|| format!("There is no picture {index}."))?
            .clone();
        let bytes = self
            .blob
            .get(spec.offset..spec.offset + spec.length)
            .ok_or("A picture's bytes are missing.")?;
        let id = self.alloc();
        if spec.kind == "jpeg" {
            let mut image = self.pdf.image_xobject(id, bytes);
            image.filter(Filter::DctDecode);
            image.width(spec.width as i32);
            image.height(spec.height as i32);
            image.color_space().device_rgb();
            image.bits_per_component(8);
        } else {
            let pixels = (spec.width * spec.height) as usize;
            if bytes.len() < pixels * 4 {
                return Err("A picture has fewer pixels than it says.".to_string());
            }
            let mut rgb = Vec::with_capacity(pixels * 3);
            let mut alpha = Vec::with_capacity(pixels);
            let mut any_alpha = false;
            for px in bytes[..pixels * 4].as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
                alpha.push(px[3]);
                any_alpha |= px[3] != 255;
            }
            let mask = if any_alpha {
                let mask_id = self.alloc();
                let packed = miniz_oxide::deflate::compress_to_vec_zlib(&alpha, 6);
                let mut mask = self.pdf.image_xobject(mask_id, &packed);
                mask.filter(Filter::FlateDecode);
                mask.width(spec.width as i32);
                mask.height(spec.height as i32);
                mask.color_space().device_gray();
                mask.bits_per_component(8);
                Some(mask_id)
            } else {
                None
            };
            let packed = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6);
            let mut image = self.pdf.image_xobject(id, &packed);
            image.filter(Filter::FlateDecode);
            image.width(spec.width as i32);
            image.height(spec.height as i32);
            image.color_space().device_rgb();
            image.bits_per_component(8);
            if let Some(mask) = mask {
                image.s_mask(mask);
            }
        }
        self.image_refs.insert(index, id);
        Ok(id)
    }

    fn paint(
        &mut self,
        content: &mut Content,
        resources: &mut Resources,
        paint: &Paint,
        page_height: f32,
    ) -> Result<(), String> {
        let flip = [1.0, 0.0, 0.0, -1.0, 0.0, page_height];
        match paint {
            Paint::Color(text) => {
                let ([r, g, b], a) = color(text);
                if let Some(name) = self.alpha(resources, a) {
                    content.set_parameters(Name(name.as_bytes()));
                }
                content.set_fill_rgb(r, g, b);
            }
            Paint::Linear { linear, stops } => {
                let function = self.alloc();
                let pattern = self.alloc();
                let colors: Vec<[f32; 3]> = stops.iter().map(|(_, c)| color(c).0).collect();
                if colors.len() <= 2 {
                    let first = colors.first().copied().unwrap_or([0.0; 3]);
                    let last = colors.last().copied().unwrap_or(first);
                    self.pdf
                        .exponential_function(function)
                        .domain([0.0, 1.0])
                        .c0(first)
                        .c1(last)
                        .n(1.0);
                } else {
                    let mut parts = Vec::new();
                    for pair in colors.windows(2) {
                        let id = self.alloc();
                        self.pdf
                            .exponential_function(id)
                            .domain([0.0, 1.0])
                            .c0(pair[0])
                            .c1(pair[1])
                            .n(1.0);
                        parts.push(id);
                    }
                    let bounds: Vec<f32> = stops[1..stops.len() - 1].iter().map(|s| s.0).collect();
                    let encode: Vec<f32> = parts.iter().flat_map(|_| [0.0, 1.0]).collect();
                    self.pdf
                        .stitching_function(function)
                        .domain([0.0, 1.0])
                        .functions(parts)
                        .bounds(bounds)
                        .encode(encode);
                }
                let (start, end) = (
                    stops.first().map_or(0.0, |s| s.0),
                    stops.last().map_or(1.0, |s| s.0),
                );
                let dx = linear[2] - linear[0];
                let dy = linear[3] - linear[1];
                let coords = [
                    linear[0] + dx * start,
                    linear[1] + dy * start,
                    linear[0] + dx * end,
                    linear[1] + dy * end,
                ];
                let mut pattern_dict = self.pdf.shading_pattern(pattern);
                pattern_dict.matrix(flip);
                let mut shading = pattern_dict.function_shading();
                shading
                    .shading_type(FunctionShadingType::Axial)
                    .coords(coords)
                    .function(function)
                    .extend([true, true]);
                shading.color_space().device_rgb();
                shading.finish();
                pattern_dict.finish();
                let name = format!("P{}", pattern.get());
                resources.patterns.push((name.clone(), pattern));
                content.set_fill_color_space(pdf_writer::types::ColorSpaceOperand::Pattern);
                content.set_fill_pattern(None, Name(name.as_bytes()));
            }
            Paint::Tiles { image, x, y, w, h } => {
                let image_id = self.image(*image)?;
                let pattern = self.alloc();
                let mut cell = Content::new();
                cell.save_state();
                cell.transform([*w, 0.0, 0.0, *h, 0.0, 0.0]);
                cell.x_object(Name(b"I"));
                cell.restore_state();
                let data = cell.finish();
                let mut tiling = self.pdf.tiling_pattern(pattern, &data);
                tiling
                    .tiling_type(TilingType::ConstantSpacing)
                    .paint_type(PaintType::Colored)
                    .bbox(Rect::new(0.0, 0.0, *w, *h))
                    .x_step(*w)
                    .y_step(*h)
                    .matrix([1.0, 0.0, 0.0, 1.0, *x, page_height - *y - *h]);
                tiling.resources().x_objects().pair(Name(b"I"), image_id);
                drop(tiling);
                let name = format!("P{}", pattern.get());
                resources.patterns.push((name.clone(), pattern));
                content.set_fill_color_space(pdf_writer::types::ColorSpaceOperand::Pattern);
                content.set_fill_pattern(None, Name(name.as_bytes()));
            }
        }
        Ok(())
    }
}

/// Writes the document. `blob` holds the font files and pictures it points into.
pub fn write_pdf(doc: &Document, blob: &[u8]) -> Result<Vec<u8>, String> {
    let mut files = Vec::with_capacity(doc.files.len());
    for file in &doc.files {
        let data = blob
            .get(file.offset..file.offset + file.length)
            .ok_or_else(|| format!("The font {} is missing its bytes.", file.name))?;
        let parsed = FontFile::new(data, file.index, file.name.clone())
            .ok_or_else(|| format!("The font {} can't be read.", file.name))?;
        files.push(parsed);
    }
    let mut writer = Writer {
        pdf: Pdf::new(),
        next: 0,
        blob,
        doc,
        files,
        font_refs: Vec::new(),
        image_refs: HashMap::new(),
        alpha_refs: HashMap::new(),
    };
    let catalog = writer.alloc();
    let tree = writer.alloc();
    let info = writer.alloc();
    writer.font_refs = (0..writer.files.len()).map(|_| writer.alloc()).collect();
    let mut page_refs = Vec::with_capacity(doc.pages.len());
    for page in &doc.pages {
        let page_ref = writer.alloc();
        let content_ref = writer.alloc();
        let mut resources = Resources::default();
        let mut content = Content::new();
        content.transform([1.0, 0.0, 0.0, -1.0, 0.0, page.height]);
        for op in &page.ops {
            draw(&mut writer, &mut content, &mut resources, op, page.height)?;
        }
        let data = content.finish();
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
        writer
            .pdf
            .stream(content_ref, &packed)
            .filter(Filter::FlateDecode);
        let mut page_dict = writer.pdf.page(page_ref);
        page_dict
            .media_box(Rect::new(0.0, 0.0, page.width, page.height))
            .parent(tree)
            .contents(content_ref);
        let mut res = page_dict.resources();
        if !resources.fonts.is_empty() {
            let mut fonts = res.fonts();
            for &file in &resources.fonts {
                let name = format!("F{file}");
                fonts.pair(Name(name.as_bytes()), writer.font_refs[file]);
            }
        }
        if !resources.alphas.is_empty() {
            let mut states = res.ext_g_states();
            for (key, id) in &resources.alphas {
                let name = format!("A{key}");
                states.pair(Name(name.as_bytes()), *id);
            }
        }
        if !resources.patterns.is_empty() {
            let mut patterns = res.patterns();
            for (name, id) in &resources.patterns {
                patterns.pair(Name(name.as_bytes()), *id);
            }
        }
        if !resources.images.is_empty() {
            let mut objects = res.x_objects();
            for (index, id) in &resources.images {
                let name = format!("I{index}");
                objects.pair(Name(name.as_bytes()), *id);
            }
        }
        res.finish();
        page_dict.finish();
        page_refs.push(page_ref);
    }
    writer
        .pdf
        .pages(tree)
        .kids(page_refs.iter().copied())
        .count(page_refs.len() as i32);
    writer.pdf.catalog(catalog).pages(tree);
    let mut info_dict = writer.pdf.document_info(info);
    if !doc.title.is_empty() {
        info_dict.title(TextStr(&doc.title));
    }
    info_dict.producer(TextStr("CashGear Sheet"));
    info_dict.finish();
    let Writer {
        mut pdf,
        mut next,
        files,
        font_refs,
        ..
    } = writer;
    let mut alloc = || {
        next += 1;
        Ref::new(next)
    };
    text::embed(&mut pdf, &mut alloc, &files, &font_refs)?;
    Ok(pdf.finish())
}

fn draw(
    writer: &mut Writer<'_>,
    content: &mut Content,
    resources: &mut Resources,
    op: &Op,
    page_height: f32,
) -> Result<(), String> {
    match op {
        Op::Save => {
            content.save_state();
        }
        Op::Restore => {
            content.restore_state();
        }
        Op::Clip { d, evenodd } => {
            path(content, d);
            if *evenodd {
                content.clip_even_odd();
            } else {
                content.clip_nonzero();
            }
            content.end_path();
        }
        Op::Path {
            d,
            fill,
            stroke,
            lw,
            dash,
            evenodd,
            cap,
            join,
        } => {
            if fill.is_none() && stroke.is_none() {
                return Ok(());
            }
            content.save_state();
            if let Some(fill) = fill {
                writer.paint(content, resources, fill, page_height)?;
            }
            if let Some(stroke) = stroke {
                let ([r, g, b], a) = color(stroke);
                if fill.is_none() {
                    if let Some(name) = writer.alpha(resources, a) {
                        content.set_parameters(Name(name.as_bytes()));
                    }
                }
                content.set_stroke_rgb(r, g, b);
                content.set_line_width(*lw);
                if !dash.is_empty() {
                    content.set_dash_pattern(dash.iter().copied(), 0.0);
                }
                content.set_line_cap(match cap.as_deref() {
                    Some("round") => LineCapStyle::RoundCap,
                    Some("square") => LineCapStyle::ProjectingSquareCap,
                    _ => LineCapStyle::ButtCap,
                });
                content.set_line_join(match join.as_deref() {
                    Some("round") => LineJoinStyle::RoundJoin,
                    Some("bevel") => LineJoinStyle::BevelJoin,
                    _ => LineJoinStyle::MiterJoin,
                });
            }
            path(content, d);
            match (fill.is_some(), stroke.is_some(), evenodd) {
                (true, true, false) => content.fill_nonzero_and_stroke(),
                (true, true, true) => content.fill_even_odd_and_stroke(),
                (true, false, false) => content.fill_nonzero(),
                (true, false, true) => content.fill_even_odd(),
                _ => content.stroke(),
            };
            content.restore_state();
        }
        Op::Image { image, m } => {
            let id = writer.image(*image)?;
            if !resources.images.iter().any(|(i, _)| i == image) {
                resources.images.push((*image, id));
            }
            content.save_state();
            content.transform(multiply([1.0, 0.0, 0.0, -1.0, 0.0, 1.0], *m));
            let name = format!("I{image}");
            content.x_object(Name(name.as_bytes()));
            content.restore_state();
        }
        Op::Text {
            t,
            x,
            y,
            m,
            face,
            size,
            fill,
            rtl,
            align,
            base,
        } => {
            let chain = writer
                .doc
                .faces
                .get(*face)
                .map(|f| f.chain.clone())
                .unwrap_or_default();
            let mut chain: Vec<usize> = chain
                .into_iter()
                .filter(|&f| f < writer.files.len())
                .collect();
            // A font whose licence forbids embedding is passed over, keeping the last resort.
            if let Some(&last) = chain.last() {
                chain.retain(|&f| writer.files[f].embeddable);
                if chain.is_empty() {
                    chain.push(last);
                }
            }
            if chain.is_empty() || t.is_empty() {
                return Ok(());
            }
            let glyphs = text::shape(&mut writer.files, &chain, t, *rtl);
            if glyphs.is_empty() {
                return Ok(());
            }
            let width = text::width(&glyphs) * size;
            let start = match (align.as_deref(), rtl) {
                (Some("right"), _) | (Some("end"), false) | (Some("start"), true) => x - width,
                (Some("center"), _) => x - width / 2.0,
                (None, true) => x - width,
                _ => *x,
            };
            let primary = &writer.files[chain[0]];
            let (ascent, descent) = (primary.ascent() * size, primary.descent() * size);
            let baseline = match base.as_deref() {
                Some("top") => y + ascent,
                Some("hanging") => y + ascent * 0.8,
                Some("middle") => y + (ascent - descent) / 2.0,
                Some("bottom") | Some("ideographic") => y - descent,
                _ => *y,
            };
            let ([r, g, b], a) = color(fill);
            content.save_state();
            if let Some(name) = writer.alpha(resources, a) {
                content.set_parameters(Name(name.as_bytes()));
            }
            content.transform(*m);
            content.set_fill_rgb(r, g, b);
            // The text as it reads, for copying in the right order.
            content
                .begin_marked_content_with_properties(Name(b"Span"))
                .properties()
                .actual_text(TextStr(t));
            // Where each glyph goes along the line, in ems from the start.
            let mut pen = 0.0f32;
            let placed: Vec<f32> = glyphs
                .iter()
                .map(|glyph| {
                    let at = pen + glyph.x_offset;
                    pen += glyph.advance;
                    at
                })
                .collect();
            let main: Vec<usize> = (0..glyphs.len()).filter(|&i| !glyphs[i].mark).collect();
            content.begin_text();
            let mut current: Option<usize> = None;
            let mut k = 0usize;
            while k < main.len() {
                let first = main[k];
                let file = glyphs[first].file;
                let rise = glyphs[first].y_offset;
                // One show for the glyphs that share a file and a height.
                let mut end = k;
                while end < main.len()
                    && glyphs[main[end]].file == file
                    && (glyphs[main[end]].y_offset - rise).abs() <= 1e-6
                {
                    end += 1;
                }
                if current != Some(file) {
                    let name = format!("F{file}");
                    content.set_font(Name(name.as_bytes()), *size);
                    current = Some(file);
                    if !resources.fonts.contains(&file) {
                        resources.fonts.push(file);
                    }
                }
                content.set_text_matrix([
                    1.0,
                    0.0,
                    0.0,
                    -1.0,
                    start + placed[first] * size,
                    baseline - rise * size,
                ]);
                let mut shown = content.show_positioned();
                let mut items = shown.items();
                let mut at = placed[first];
                for &i in &main[k..end] {
                    let gap = placed[i] - at;
                    if gap.abs() > 1e-5 {
                        items.adjust(-gap * 1000.0);
                    }
                    items.show(Str(&glyphs[i].id.to_be_bytes()));
                    at = placed[i] + glyphs[i].natural;
                }
                drop(items);
                drop(shown);
                k = end;
            }
            content.end_text();
            content.end_marked_content();
            // Marks as outlines, in the text's colour.
            let mut any = false;
            for (i, glyph) in glyphs.iter().enumerate().filter(|(_, g)| g.mark) {
                any |= text::outline(
                    &writer.files,
                    glyph,
                    start + placed[i] * size,
                    baseline - glyph.y_offset * size,
                    *size,
                    content,
                );
            }
            if any {
                content.fill_nonzero();
            }
            content.restore_state();
        }
    }
    Ok(())
}

/// The glyphs shaping makes of `text`, left to right, as (file, subset id), for tests.
#[doc(hidden)]
pub fn shape_for_tests(
    files: &mut [FontFile<'_>],
    chain: &[usize],
    text: &str,
    rtl: bool,
) -> Vec<(usize, u16)> {
    text::shape(files, chain, text, rtl)
        .iter()
        .map(|g| (g.file, g.id))
        .collect()
}
