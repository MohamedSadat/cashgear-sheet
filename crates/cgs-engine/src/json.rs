//! The little JSON the shell needs, without a JSON library in the WASM.

/// Appends text as a JSON string.
pub fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_what_json_requires() {
        let mut out = String::new();
        push_string(&mut out, "a\"b\\c\nd\u{1}ورقة");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001ورقة\"");
    }
}
