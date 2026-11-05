//! `sharedStrings.xml`, read in slices and held as one buffer, so a large table costs
//! its text plus 4 bytes a string rather than an allocation each.

use std::collections::HashMap;
use std::io::BufReader;

use cgs_model::{RichRun, RunFont, Underline};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::package::PartStream;
use crate::styles::parse_color;
use crate::xml::{attr, attr_f64, push_text, unescape_x, val_flag};

#[derive(Debug, Default)]
pub struct SharedStrings {
    text: String,
    /// Where each string ends in `text`.
    ends: Vec<u32>,
    rich: HashMap<u32, Vec<RichRun>>,
}

impl SharedStrings {
    pub fn get(&self, index: usize) -> Option<&str> {
        let end = *self.ends.get(index)? as usize;
        let start = if index == 0 {
            0
        } else {
            self.ends[index - 1] as usize
        };
        self.text.get(start..end)
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Whether this string has more than one look.
    pub fn is_rich(&self, index: u32) -> bool {
        self.rich.contains_key(&index)
    }

    /// A rich string's runs.
    pub fn runs(&self, index: u32) -> Option<&[RichRun]> {
        self.rich.get(&index).map(Vec::as_slice)
    }

    pub fn rich_strings(&self) -> HashMap<u32, Vec<RichRun>> {
        self.rich.clone()
    }

    fn push(&mut self, value: &str) {
        self.text.push_str(&unescape_x(value));
        self.ends.push(self.text.len() as u32);
    }
}

/// Reads the table a little at a time.
pub struct StringsLoader {
    reader: Reader<BufReader<PartStream>>,
    buf: Vec<u8>,
    current: String,
    in_text: bool,
    in_phonetic: bool,
    strings: SharedStrings,
    done: bool,
    /// The runs of the string being read, with where each starts in `current`.
    runs: Vec<RichRun>,
    in_run_props: bool,
    run_font: Option<RunFont>,
}

fn run_font_child(element: &BytesStart<'_>, font: &mut RunFont) {
    match element.local_name().as_ref() {
        b"rFont" | b"name" => font.name = attr(element, b"val"),
        b"sz" => font.size = attr_f64(element, b"val").map(|size| size.clamp(1.0, 409.0) as f32),
        b"b" => font.bold = val_flag(element),
        b"i" => font.italic = val_flag(element),
        b"strike" => font.strike = val_flag(element),
        b"u" => {
            font.underline = Some(match attr(element, b"val").as_deref() {
                Some("none") => Underline::None,
                Some("double") => Underline::Double,
                Some("singleAccounting") => Underline::SingleAccounting,
                Some("doubleAccounting") => Underline::DoubleAccounting,
                _ => Underline::Single,
            });
        }
        b"color" => font.color = Some(parse_color(element)),
        _ => {}
    }
}

impl StringsLoader {
    pub fn new(stream: PartStream) -> Self {
        Self {
            reader: Reader::from_reader(BufReader::with_capacity(128 * 1024, stream)),
            buf: Vec::with_capacity(4096),
            current: String::new(),
            in_text: false,
            in_phonetic: false,
            strings: SharedStrings::default(),
            done: false,
            runs: Vec::new(),
            in_run_props: false,
            run_font: None,
        }
    }

    /// Compressed bytes taken from the file so far.
    pub fn consumed(&self) -> u64 {
        self.reader.get_ref().get_ref().consumed()
    }

    /// Reads up to `events` events, and returns whether the table is complete. A table
    /// damaged part way keeps what it read, and cells past its end show as empty.
    pub fn step(&mut self, events: u32) -> bool {
        if self.done {
            return true;
        }
        for _ in 0..events {
            let event = self.reader.read_event_into(&mut self.buf);
            match event {
                Ok(Event::Eof) | Err(_) => {
                    self.done = true;
                    self.buf.clear();
                    return true;
                }
                Ok(Event::Start(ref element)) => match element.local_name().as_ref() {
                    b"si" => {
                        self.current.clear();
                        self.runs.clear();
                    }
                    b"rPh" => self.in_phonetic = true,
                    b"t" => self.in_text = !self.in_phonetic,
                    b"r" if !self.in_phonetic => {
                        self.runs.push(RichRun {
                            start: self.current.len() as u32,
                            font: None,
                        });
                    }
                    b"rPr" => {
                        self.in_run_props = true;
                        self.run_font = Some(RunFont::default());
                    }
                    _ if self.in_run_props => {
                        if let Some(font) = self.run_font.as_mut() {
                            run_font_child(element, font);
                        }
                    }
                    _ => {}
                },
                Ok(Event::Empty(ref element)) if element.local_name().as_ref() == b"si" => {
                    self.strings.push("");
                }
                Ok(Event::Empty(ref element)) if self.in_run_props => {
                    if let Some(font) = self.run_font.as_mut() {
                        run_font_child(element, font);
                    }
                }
                Ok(Event::End(ref element)) => match element.local_name().as_ref() {
                    b"si" => {
                        self.finish_runs();
                        self.strings.push(&self.current);
                        self.current.clear();
                    }
                    b"rPr" => {
                        self.in_run_props = false;
                        if let Some(run) = self.runs.last_mut() {
                            run.font = self.run_font.take();
                        }
                    }
                    b"rPh" => self.in_phonetic = false,
                    b"t" => self.in_text = false,
                    _ => {}
                },
                Ok(ref other) => {
                    if self.in_text {
                        push_text(other, &mut self.current);
                    }
                }
            }
            self.buf.clear();
        }
        false
    }

    /// Keeps the string's runs if it has any worth keeping: more than one look.
    fn finish_runs(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        if runs.is_empty() || (runs.len() == 1 && runs[0].font.is_none()) {
            return;
        }
        // Positions were counted in the file's text, which still holds escapes.
        if self.current.contains("_x") {
            for run in &mut runs {
                run.start = unescape_x(&self.current[..run.start as usize]).len() as u32;
            }
        }
        self.strings
            .rich
            .insert(self.strings.ends.len() as u32, runs);
    }

    pub fn finish(self) -> SharedStrings {
        self.strings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::Package;
    use crate::test_support::zip_of;
    use std::rc::Rc;

    #[test]
    fn keeps_where_each_run_starts_and_what_it_says() {
        let xml = r#"<sst><si><r><t>plain </t></r><r><rPr><b/><sz val="14"/><color rgb="FFFF0000"/><rFont val="Arial"/></rPr><t>bold</t></r></si><si><t>one</t></si><si><r><t>same</t></r></si></sst>"#;
        let package = Package::open(Rc::new(zip_of(&[("xl/sharedStrings.xml", xml)]))).unwrap();
        let mut loader = StringsLoader::new(package.stream("xl/sharedStrings.xml").unwrap());
        while !loader.step(64) {}
        let strings = loader.finish();
        assert_eq!(strings.get(0), Some("plain bold"));
        assert!(strings.is_rich(0));
        assert!(!strings.is_rich(1));
        assert!(!strings.is_rich(2));
        let runs = &strings.rich_strings()[&0];
        assert_eq!(runs[0].start, 0);
        assert!(runs[0].font.is_none());
        assert_eq!(runs[1].start, 6);
        let font = runs[1].font.as_ref().unwrap();
        assert!(font.bold);
        assert_eq!(font.size, Some(14.0));
        assert_eq!(font.name.as_deref(), Some("Arial"));
        assert_eq!(font.color, Some(cgs_model::Color::rgb(0xFF0000)));
    }

    #[test]
    fn reads_plain_rich_empty_and_phonetic_strings() {
        let xml = r#"<sst><si><t>plain &amp; simple</t></si>
          <si><r><rPr><b/></rPr><t>rich </t></r><r><t xml:space="preserve">text</t></r></si>
          <si/>
          <si><t/></si>
          <si><t>kanji</t><rPh sb="0" eb="1"><t>phonetic</t></rPh></si>
          <si><t>a_x000D_b</t></si>
          <si><t>عربي</t></si></sst>"#;
        let bytes = zip_of(&[("xl/sharedStrings.xml", xml)]);
        let package = Package::open(Rc::new(bytes)).unwrap();
        let mut loader = StringsLoader::new(package.stream("xl/sharedStrings.xml").unwrap());
        while !loader.step(3) {}
        let strings = loader.finish();
        assert_eq!(strings.len(), 7);
        assert_eq!(strings.get(0), Some("plain & simple"));
        assert_eq!(strings.get(1), Some("rich text"));
        assert_eq!(strings.get(2), Some(""));
        assert_eq!(strings.get(3), Some(""));
        assert_eq!(strings.get(4), Some("kanji"));
        assert_eq!(strings.get(5), Some("a\rb"));
        assert_eq!(strings.get(6), Some("عربي"));
        assert_eq!(strings.get(7), None);
    }
}
