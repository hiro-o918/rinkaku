//! Clipboard export (ADR 0048 sink B) via an OSC 52 terminal escape
//! sequence — see the ADR's Alternatives for why OSC 52 over a clipboard
//! crate or shelling out to `pbcopy`/`xclip`: no new dependency, works
//! over SSH, and degrades safely on a terminal that doesn't support it.

use rinkaku_tui::review::ports::ClipboardSink;
use std::io::Write;

/// [`ClipboardSink`] that writes an OSC 52 escape sequence to stdout.
/// Best-effort: a successful `Ok(())` here means the escape sequence was
/// written, not that the terminal actually populated the system
/// clipboard (the ADR's own Consequences note this can't be detected).
pub(crate) struct Osc52Clipboard;

impl ClipboardSink for Osc52Clipboard {
    fn copy(&self, text: &str) -> Result<(), String> {
        let encoded = base64_encode(text.as_bytes());
        let sequence = format!("\x1b]52;c;{encoded}\x07");
        let mut stdout = std::io::stdout();
        stdout
            .write_all(sequence.as_bytes())
            .and_then(|()| stdout.flush())
            .map_err(|err| format!("failed to write OSC 52 sequence: {err}"))
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 encoding (RFC 4648, with `=` padding) — implemented
/// directly rather than adding a dependency for one small, stable
/// algorithm (ADR 0048's own "no new dependency" rationale for OSC 52
/// applies equally here).
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();

        out.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(
            BASE64_ALPHABET[(((b0 & 0b0000_0011) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char,
        );
        match b1 {
            Some(b1) => {
                out.push(
                    BASE64_ALPHABET[(((b1 & 0b0000_1111) << 2) | (b2.unwrap_or(0) >> 6)) as usize]
                        as char,
                );
            }
            None => out.push('='),
        }
        match b2 {
            Some(b2) => out.push(BASE64_ALPHABET[(b2 & 0b0011_1111) as usize] as char),
            None => out.push('='),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_encode_empty_input_as_empty_string() {
        let actual = base64_encode(b"");

        assert_eq!("", actual);
    }

    #[test]
    fn should_encode_known_vector_with_no_padding() {
        // "Man" -> "TWFu" is the canonical RFC 4648 base64 example.
        let actual = base64_encode(b"Man");

        assert_eq!("TWFu", actual);
    }

    #[test]
    fn should_encode_known_vector_with_one_padding_character() {
        let actual = base64_encode(b"Ma");

        assert_eq!("TWE=", actual);
    }

    #[test]
    fn should_encode_known_vector_with_two_padding_characters() {
        let actual = base64_encode(b"M");

        assert_eq!("TQ==", actual);
    }

    #[test]
    fn should_encode_the_full_rfc_4648_test_vector() {
        let actual = base64_encode(b"pleasure.");

        assert_eq!("cGxlYXN1cmUu", actual);
    }
}
