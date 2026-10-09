//! Encoding detection on read and lossless encoding on write.

use std::io::Read;

use encoding_rs::{
    EUC_JP, Encoding, ISO_2022_JP, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252,
};
use fe_text::{EncodeError, FileEncoding, LineEnding, Text};

/// Hands out at most `step` bytes per read, so sequences split across reads.
struct Trickle<'a> {
    bytes: &'a [u8],
    step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(buf.len()).min(self.bytes.len());
        buf[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}

fn detect(bytes: &[u8]) -> (Text, LineEnding, FileEncoding) {
    Text::from_reader_detecting(bytes).unwrap()
}

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut bytes = if big_endian {
        vec![0xFE, 0xFF]
    } else {
        vec![0xFF, 0xFE]
    };
    for unit in text.encode_utf16() {
        let pair = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        bytes.extend_from_slice(&pair);
    }
    bytes
}

fn legacy(encoding: &'static encoding_rs::Encoding, text: &str) -> Vec<u8> {
    let (bytes, _, unmappable) = encoding.encode(text);
    assert!(!unmappable, "fixture must be representable");
    bytes.into_owned()
}

const SAMPLE: &str = "héllo wörld → 日本語 😀\nsecond line\n";

#[test]
fn boms_select_the_encoding() {
    let mut utf8 = b"\xEF\xBB\xBF".to_vec();
    utf8.extend_from_slice(SAMPLE.as_bytes());
    let cases = [
        (utf8, UTF_8),
        (utf16(SAMPLE, false), UTF_16LE),
        (utf16(SAMPLE, true), UTF_16BE),
    ];
    for (bytes, encoding) in cases {
        let (text, line_ending, file_encoding) = detect(&bytes);
        assert_eq!(text.to_string(), SAMPLE, "{}", encoding.name());
        assert_eq!(line_ending, LineEnding::Lf);
        assert_eq!(
            file_encoding,
            FileEncoding {
                encoding,
                bom: true
            }
        );
    }
}

#[test]
fn unmarked_files_are_detected() {
    let french = "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\n\
                  Café naïve: élève, garçon, où, être, fenêtre, hôtel, Noël.\n";
    assert!(french.contains("café") && french.contains("naïve"));
    let (text, _, file_encoding) = detect(&legacy(WINDOWS_1252, french));
    assert_eq!(text.to_string(), french);
    assert_eq!(file_encoding.encoding, WINDOWS_1252);
    assert!(!file_encoding.bom);
    assert_eq!(file_encoding.name(), "windows-1252");

    let japanese = "日本語のテキストです。これはシフトJISで書かれたファイルです。\n\
                    東京は日本の首都です。今日はいい天気ですね。\n";
    let (text, _, file_encoding) = detect(&legacy(SHIFT_JIS, japanese));
    assert_eq!(text.to_string(), japanese);
    assert_eq!(file_encoding.encoding, SHIFT_JIS);
    assert!(!file_encoding.bom);

    // 'é' takes bytes 65535 and 65536: the sniffed 64 KiB end mid-char.
    let mut straddling = "a".repeat(64 * 1024 - 1);
    straddling.push_str("é and more\n");
    let (text, _, file_encoding) = detect(straddling.as_bytes());
    assert_eq!(file_encoding, FileEncoding::UTF8);
    assert_eq!(text.to_string(), straddling);

    // A file of exactly 64 KiB cut mid-sequence by its end isn't forgiven.
    let mut exact = "a".repeat(64 * 1024 - 1).into_bytes();
    exact.push(0xE9);
    let (text, _, file_encoding) = detect(&exact);
    assert_eq!(file_encoding.encoding, WINDOWS_1252);
    assert!(text.to_string().ends_with("aé"));

    // Invalid UTF-8 only after the first 64 KiB: lossy UTF-8, as from_reader.
    let mut late_invalid = "b".repeat(64 * 1024 + 10).into_bytes();
    late_invalid.extend_from_slice(b"caf\xE9 \xFF end\n");
    let (text, line_ending, file_encoding) = detect(&late_invalid);
    assert_eq!(file_encoding, FileEncoding::UTF8);
    assert_eq!((text.to_string(), line_ending), {
        let (plain, line_ending) = Text::from_reader(&late_invalid[..]).unwrap();
        (plain.to_string(), line_ending)
    });
    assert!(text.to_string().ends_with("caf\u{FFFD} \u{FFFD} end\n"));
}

/// `text` repeated until its UTF-8 passes 150 KB, so every encoding of it runs
/// past the 64 KiB sniffed before decoding starts.
fn big(text: &str) -> String {
    text.repeat(150_000 / text.len() + 1)
}

const SNIFFED: usize = 64 * 1024 + 1;

#[test]
fn round_trip_is_byte_identical() {
    let crlf = big("first line\r\nsecond → line\r\n\r\nlast\r\n");
    let sample = big(SAMPLE);
    let mut utf8_bom = b"\xEF\xBB\xBF".to_vec();
    utf8_bom.extend_from_slice(sample.as_bytes());
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("UTF-8", sample.as_bytes().to_vec()),
        ("UTF-8 BOM", utf8_bom),
        ("UTF-16LE BOM", utf16(&sample, false)),
        ("UTF-16BE BOM", utf16(&sample.replace('\n', "\r\n"), true)),
        (
            "windows-1252",
            legacy(
                WINDOWS_1252,
                &big(
                    "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\r\n\
                     Ça coûte 5 € — «voilà».\r\n",
                ),
            ),
        ),
        (
            "Shift_JIS",
            legacy(
                SHIFT_JIS,
                &big(
                    "日本語のテキストです。これはシフトJISで書かれたファイルです。\r\n\
                     東京は日本の首都です。ｶﾀｶﾅ も。\r\n",
                ),
            ),
        ),
        ("CRLF", crlf.as_bytes().to_vec()),
    ];
    for (name, bytes) in files {
        assert!(
            bytes.len() > SNIFFED + 16 * 1024,
            "{name} must outrun the sniff"
        );
        // 3-byte reads split UTF-16 units and surrogate pairs, Shift_JIS pairs
        // and CRLFs between decoder calls once the sniffed bytes are used up.
        for step in [usize::MAX, 3] {
            let reader = Trickle {
                bytes: &bytes,
                step,
            };
            let (text, line_ending, file_encoding) = Text::from_reader_detecting(reader).unwrap();
            assert!(
                !text.to_string().contains('\u{FFFD}'),
                "{name}, step {step}"
            );
            let mut out = Vec::new();
            text.write_encoded(line_ending, file_encoding, &mut out)
                .unwrap();
            assert!(
                out == bytes,
                "{name} ({}), step {step}",
                file_encoding.name()
            );
        }
    }
    let (_, line_ending, _) = detect(crlf.as_bytes());
    assert_eq!(line_ending, LineEnding::CrLf);
}

#[test]
fn truncated_sequences_at_eof_become_replacement_chars() {
    let sample = big(SAMPLE);
    let mut odd_byte = utf16(&sample, false);
    odd_byte.push(b'x');
    let mut lone_surrogate = utf16(&sample, true);
    lone_surrogate.extend_from_slice(&[0xD8, 0x3D]);
    let japanese = big("東京は日本の首都です。今日はいい天気ですね。\n");
    let mut lead_byte = legacy(SHIFT_JIS, &japanese);
    lead_byte.push(0x93);
    let cases = [
        (odd_byte, UTF_16LE, &sample),
        (lone_surrogate, UTF_16BE, &sample),
        (lead_byte, SHIFT_JIS, &japanese),
    ];
    for (bytes, encoding, expected) in cases {
        for step in [usize::MAX, 3] {
            let reader = Trickle {
                bytes: &bytes,
                step,
            };
            let (text, _, file_encoding) = Text::from_reader_detecting(reader).unwrap();
            assert_eq!(file_encoding.encoding, encoding);
            assert!(
                text.to_string() == format!("{expected}\u{FFFD}"),
                "{}, step {step}",
                encoding.name()
            );
        }
    }
}

#[test]
fn unmappable_chars_are_errors() {
    let text = Text::new("a → b");
    let mut out = Vec::new();
    let encoding = FileEncoding {
        encoding: WINDOWS_1252,
        bom: false,
    };
    match text.write_encoded(LineEnding::Lf, encoding, &mut out) {
        Err(EncodeError::Unmappable { ch, offset }) => {
            assert_eq!(ch, '→');
            assert_eq!(offset, 2);
        }
        other => panic!("expected Unmappable, got {other:?}"),
    }
    assert!(!out.contains(&b'?'), "{out:?}");
    assert!(!out.windows(2).any(|pair| pair == b"&#"), "{out:?}");

    // The offset is into the text, not the encoded output, across lines and chunks.
    let long = format!("{}\nx€y\n→", "é".repeat(5000));
    let text = Text::new(&long);
    let mut out = Vec::new();
    match text.write_encoded(LineEnding::CrLf, encoding, &mut out) {
        Err(EncodeError::Unmappable { ch, offset }) => {
            assert_eq!(ch, '→');
            assert_eq!(offset, long.find('→').unwrap());
        }
        other => panic!("expected Unmappable, got {other:?}"),
    }
    assert!(!out.contains(&b'?') && !out.windows(2).any(|pair| pair == b"&#"));

    // Chars that encode without an error but decode as another char.
    let lossy = [
        (SHIFT_JIS, '¥'),
        (SHIFT_JIS, '‾'),
        (EUC_JP, '¥'),
        (EUC_JP, '‾'),
        (ISO_2022_JP, 'ｶ'),
    ];
    for (encoding, ch) in lossy {
        assert_unmappable(encoding, &format!("ab日{ch}c"), ch, "ab日".len());
    }

    // ISO-2022-JP can't write SO, SI or ESC, and reports U+FFFD for them.
    let long_prefix = "日本".repeat(3000);
    for ch in ['\u{0E}', '\u{0F}', '\u{1B}'] {
        for prefix in ["", "ab日", &long_prefix] {
            assert_unmappable(ISO_2022_JP, &format!("{prefix}{ch}z"), ch, prefix.len());
        }
    }

    // What ISO-2022-JP can write still round-trips.
    let fine = "日本 ¥ abc\n";
    let mut out = Vec::new();
    let encoding = FileEncoding {
        encoding: ISO_2022_JP,
        bom: false,
    };
    Text::new(fine)
        .write_encoded(LineEnding::Lf, encoding, &mut out)
        .unwrap();
    assert_eq!(ISO_2022_JP.decode_without_bom_handling(&out).0, fine);
}

fn assert_unmappable(encoding: &'static Encoding, text: &str, ch: char, offset: usize) {
    let file_encoding = FileEncoding {
        encoding,
        bom: false,
    };
    let mut out = Vec::new();
    match Text::new(text).write_encoded(LineEnding::Lf, file_encoding, &mut out) {
        Err(EncodeError::Unmappable {
            ch: found,
            offset: found_offset,
        }) => assert_eq!(
            (found, found_offset),
            (ch, offset),
            "{} in {}",
            ch.escape_unicode(),
            encoding.name()
        ),
        other => panic!(
            "{} in {}: expected Unmappable, got {other:?}",
            ch.escape_unicode(),
            encoding.name()
        ),
    }
}
