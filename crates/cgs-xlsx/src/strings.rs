//! `sharedStrings.xml`, read in slices and held as one buffer, so a large table costs
//! its text plus 4 bytes a string rather than an allocation each.

use std::io::BufReader;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::package::PartStream;
use crate::xml::{push_text, unescape_x};

#[derive(Debug, Default)]
pub struct SharedStrings {
    text: String,
    /// Where each string ends in `text`.
    ends: Vec<u32>,
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
                    b"si" => self.current.clear(),
                    b"rPh" => self.in_phonetic = true,
                    b"t" => self.in_text = !self.in_phonetic,
                    _ => {}
                },
                Ok(Event::Empty(ref element)) if element.local_name().as_ref() == b"si" => {
                    self.strings.push("");
                }
                Ok(Event::End(ref element)) => match element.local_name().as_ref() {
                    b"si" => {
                        self.strings.push(&self.current);
                        self.current.clear();
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
