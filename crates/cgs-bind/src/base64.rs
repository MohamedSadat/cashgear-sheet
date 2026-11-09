//! Row versions travel as base64 of SQL Server's 8-byte `rowversion`. Kept as bytes here, so a
//! record costs 8 bytes for its version rather than a string.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn sextet(byte: u8) -> Option<u32> {
    ALPHABET.iter().position(|&c| c == byte).map(|i| i as u32)
}

/// Exactly 8 bytes in padded base64 (`AAAAAAAAB9E=`), or `None`.
pub fn decode_version(text: &str) -> Option<[u8; 8]> {
    let bytes = text.as_bytes();
    if bytes.len() != 12 || bytes[11] != b'=' || bytes[10] == b'=' {
        return None;
    }
    let mut out = [0u8; 9];
    for (group, chunk) in bytes.chunks(4).enumerate() {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' { 0 } else { sextet(c)? };
            n |= v << (18 - 6 * i);
        }
        out[group * 3] = (n >> 16) as u8;
        out[group * 3 + 1] = (n >> 8) as u8;
        out[group * 3 + 2] = n as u8;
    }
    // The last group carries two bytes; its third must be the padding's zero.
    if out[8] != 0 {
        return None;
    }
    let mut version = [0u8; 8];
    version.copy_from_slice(&out[..8]);
    // A canonical encoding only, so the same version always reads back as the same text.
    (encode_version(&version) == text).then_some(version)
}

/// The padded base64 of a version, as the ERP sends it.
pub fn encode_version(version: &[u8; 8]) -> String {
    let mut out = String::with_capacity(12);
    let mut padded = [0u8; 9];
    padded[..8].copy_from_slice(version);
    for chunk in padded.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        for i in 0..4 {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out.truncate(11);
    out.push('=');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_round_trip() {
        for text in [
            "AAAAAAAAB9E=",
            "AAAAAAAACQ0=",
            "//////////8=",
            "AQIDBAUGBwg=",
        ] {
            let version = decode_version(text).unwrap();
            assert_eq!(encode_version(&version), text);
        }
        assert_eq!(
            decode_version("AAAAAAAAB9E=").unwrap(),
            [0, 0, 0, 0, 0, 0, 7, 0xd1]
        );
    }

    #[test]
    fn anything_but_eight_bytes_is_refused() {
        for text in [
            "",
            "AAAA",
            "AAAAAAAAB9E",
            "AAAAAAAAB9==",
            "AAAAAAAAAAAAAAAA",
            "AAAAAAAA*9E=",
            "AAAAAAAAB9F=",
        ] {
            assert_eq!(decode_version(text), None, "{text}");
        }
    }
}
