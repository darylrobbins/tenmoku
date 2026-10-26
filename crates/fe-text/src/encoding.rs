//! Reading text in any encoding a file might use, and writing it back the same way.
//!
//! [`Text::from_reader_detecting`] picks the encoding from a BOM, then from
//! the zero bytes of a BOM-less UTF-16 file, then from whether the start of the file is valid UTF-8, and otherwise from a
//! `chardetng` guess; [`Text::from_reader_with`] reads in an encoding
//! given instead. [`Text::write_encoded`] writes text back in a
//! [`FileEncoding`], failing rather than substituting a char the encoding
//! can't represent.

use std::collections::HashMap;
use std::io::{self, Read, Write};

use encoding_rs::{DecoderResult, Encoder, EncoderResult, Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::{LineEnding, Text, decode_lossy};

/// How many bytes from the start of a file decide its encoding.
const SNIFF_LEN: usize = 64 * 1024;
/// How much of the start of a file is checked for BOM-less UTF-16, as VS Code does.
const ZERO_BYTE_SNIFF_LEN: usize = 512;

/// The encoding a file was read in, and whether it started with a BOM.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileEncoding {
    pub encoding: &'static Encoding,
    /// Written back by [`Text::write_encoded`] for UTF-8 and UTF-16; other
    /// encodings have no BOM and ignore it.
    pub bom: bool,
}

impl FileEncoding {
    /// UTF-8 without a BOM.
    pub const UTF8: Self = Self {
        encoding: UTF_8,
        bom: false,
    };

    /// The encoding's WHATWG name for display: `"UTF-8"`, `"UTF-16LE"`,
    /// `"windows-1252"`, `"Shift_JIS"`, …
    pub fn name(self) -> &'static str {
        self.encoding.name()
    }

    /// For the status bar: `"UTF-8 with BOM"` for UTF-8 with a BOM, otherwise
    /// [`FileEncoding::name`].
    pub fn label(self) -> String {
        if self.encoding == UTF_8 && self.bom {
            "UTF-8 with BOM".to_owned()
        } else {
            self.name().to_owned()
        }
    }
}

/// What [`Text::from_reader_detecting`] or [`Text::from_reader_with`] read.
#[derive(Clone, Debug)]
pub struct DecodedFile {
    pub text: Text,
    pub line_ending: LineEnding,
    pub encoding: FileEncoding,
    /// Whether any input was malformed in `encoding` and decoded to U+FFFD.
    /// A U+FFFD the file legitimately contains doesn't count.
    pub replaced: bool,
}

impl Default for FileEncoding {
    fn default() -> Self {
        Self::UTF8
    }
}

/// Why [`Text::write_encoded`] stopped.
#[derive(Debug)]
pub enum EncodeError {
    /// `ch`, at byte offset `offset` in the text, has no representation in the
    /// encoding. Output written before it is incomplete.
    Unmappable {
        ch: char,
        offset: usize,
    },
    Io(io::Error),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::Unmappable { ch, offset } => write!(
                f,
                "{ch:?} (U+{:04X}) at byte {offset} can't be represented in the encoding",
                u32::from(*ch)
            ),
            EncodeError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EncodeError::Unmappable { .. } => None,
            EncodeError::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for EncodeError {
    fn from(error: io::Error) -> Self {
        EncodeError::Io(error)
    }
}

impl FileEncoding {
    /// The encoding [`Text::from_reader_detecting`] would read `reader` in,
    /// found the same way: by its BOM, then the zero bytes in its first 512 bytes
    /// (UTF-16 without a BOM), then the validity of its first 64 KiB
    /// as UTF-8, then `chardetng`. Reads at most those 64 KiB and a byte.
    pub fn detect(reader: impl Read) -> io::Result<FileEncoding> {
        let mut head = Vec::with_capacity(SNIFF_LEN + 1);
        reader.take(SNIFF_LEN as u64 + 1).read_to_end(&mut head)?;
        Ok(detect_head(&head).0)
    }
}

impl FileEncoding {
    /// Decodes `bytes`, the start of a line, as [`Text::from_reader_detecting`]
    /// would (each malformed sequence one U+FFFD), and calls `unit` with each
    /// decoded unit's source byte range and text, in order and with no gaps.
    /// A unit is the bytes of one char (or a few, where they decode to
    /// several), or of one malformed sequence, which is a U+FFFD. Bytes the
    /// decoder reads past a malformed sequence and decodes again are the
    /// units that follow it, not part of it.
    ///
    /// `complete` says `bytes` end where the stream does, so a char cut short
    /// is a malformed sequence, as the editor's decoding makes it. Otherwise
    /// the bytes of one unit still pending at the end are not reported, so
    /// `bytes` can run on into the line break that ends the line in the
    /// editor's decoding, which settles what comes before it.
    /// `unit` stops the walk by returning `Ok(true)`, or its error.
    pub fn for_each_unit<E>(
        self,
        bytes: &[u8],
        complete: bool,
        mut unit: impl FnMut(std::ops::Range<usize>, &str) -> Result<bool, E>,
    ) -> Result<(), E> {
        let iso_2022_jp = self.encoding == encoding_rs::ISO_2022_JP;
        let mut decoder = self.encoding.new_decoder_without_bom_handling();
        let mut scratch = String::new();
        // Where the bytes not yet in a unit start: fed, or pending in `decoder`.
        let mut unit_start = 0;
        // Whether `decoder` holds a byte to decode again, which an empty
        // input makes it decode before the next byte.
        let mut replay = false;
        // A byte at a time, then once more, empty, to end the stream.
        let mut at = 0;
        'bytes: while at <= bytes.len() {
            let last = at == bytes.len();
            if last && !complete {
                break;
            }
            let mut rest = &bytes[at..(at + 1).min(bytes.len())];
            loop {
                let replaying = std::mem::take(&mut replay);
                let input = if replaying { &[][..] } else { rest };
                scratch.clear();
                scratch.reserve(
                    decoder
                        .max_utf8_buffer_length_without_replacement(input.len())
                        .expect("a byte's worth of UTF-8 fits in usize"),
                );
                let (result, read) =
                    decoder.decode_to_string_without_replacement(input, &mut scratch, last);
                if !replaying {
                    rest = &rest[read..];
                }
                let end = if replaying {
                    // The decoded-again byte is one, as an escape's is.
                    unit_start + 1
                } else {
                    at + (usize::from(!last) - rest.len())
                };
                match result {
                    DecoderResult::InputEmpty | DecoderResult::OutputFull => {
                        if !scratch.is_empty() {
                            if unit(unit_start..end, &scratch)? {
                                return Ok(());
                            }
                            unit_start = end;
                        }
                        // A decoder that got the last call has finished, and
                        // can't be used again, whether or not it replayed.
                        if result == DecoderResult::InputEmpty && (!replaying || last) {
                            break;
                        }
                    }
                    DecoderResult::Malformed(bad, extra) => {
                        let bad_start = end - usize::from(extra) - usize::from(bad);
                        let bad_end = bad_start + usize::from(bad);
                        if !scratch.is_empty() && unit(unit_start..bad_start, &scratch)? {
                            return Ok(());
                        }
                        // Bytes that decoded to nothing, such as an escape
                        // sequence, go with the U+FFFD that follows them.
                        let from = if scratch.is_empty() {
                            unit_start
                        } else {
                            bad_start
                        };
                        if unit(from..bad_end, "\u{FFFD}")? {
                            return Ok(());
                        }
                        unit_start = bad_end;
                        if extra > 0 {
                            if iso_2022_jp {
                                // Its escapes don't decode again, and the one
                                // byte that does is decoded in the state it
                                // was read in.
                                replay = true;
                            } else {
                                // The bytes after it decode as if the stream
                                // began at them.
                                decoder = self.encoding.new_decoder_without_bom_handling();
                                at = bad_end;
                                continue 'bytes;
                            }
                        }
                    }
                }
            }
            at += 1;
        }
        Ok(())
    }
}

/// The encoding of a file starting with `head` (up to 64 KiB and a byte past
/// it, or the whole file if shorter), and the length of its BOM.
fn detect_head(head: &[u8]) -> (FileEncoding, usize) {
    let at_eof = head.len() <= SNIFF_LEN;
    let sniffed = &head[..head.len().min(SNIFF_LEN)];
    if let Some((encoding, bom_len)) = Encoding::for_bom(head) {
        (
            FileEncoding {
                encoding,
                bom: true,
            },
            bom_len,
        )
    } else if let Some(encoding) = utf16_by_zero_bytes(head) {
        (
            FileEncoding {
                encoding,
                bom: false,
            },
            0,
        )
    } else if starts_with_utf8(sniffed, at_eof) {
        (FileEncoding::UTF8, 0)
    } else {
        let mut detector = chardetng::EncodingDetector::new();
        detector.feed(sniffed, at_eof);
        (
            FileEncoding {
                encoding: detector.guess(None, false),
                bom: false,
            },
            0,
        )
    }
}

/// UTF-16LE if the first 512 bytes hold a zero byte and exactly the odd-indexed
/// bytes are zero, UTF-16BE if exactly the even-indexed ones are, else `None`.
fn utf16_by_zero_bytes(head: &[u8]) -> Option<&'static Encoding> {
    let window = &head[..head.len().min(ZERO_BYTE_SNIFF_LEN)];
    if !window.contains(&0) {
        return None;
    }
    let zero_at = |parity: usize| {
        window
            .iter()
            .enumerate()
            .all(|(i, &b)| (b == 0) == (i % 2 == parity))
    };
    if zero_at(1) {
        Some(UTF_16LE)
    } else if zero_at(0) {
        Some(UTF_16BE)
    } else {
        None
    }
}

impl Text {
    /// Like [`Text::from_reader`], but detects the file's encoding first:
    ///
    /// 1. a UTF-8, UTF-16LE or UTF-16BE BOM picks that encoding and is dropped;
    /// 2. otherwise, if the first 512 bytes have zeros at exactly the odd (or
    ///    exactly the even) indices, the file is BOM-less UTF-16LE (or BE);
    /// 3. otherwise, if the first 64 KiB are valid UTF-8 (a sequence cut off at
    ///    64 KiB doesn't count against it), the file is UTF-8, and invalid bytes
    ///    later on become U+FFFD exactly as with [`Text::from_reader`];
    /// 4. otherwise `chardetng` guesses from those 64 KiB.
    ///
    /// Only those first 64 KiB (and a byte) are held besides the text being built.
    /// [`DecodedFile::replaced`] reports whether any input was malformed.
    pub fn from_reader_detecting(mut reader: impl Read) -> io::Result<DecodedFile> {
        // One byte past the window tells a sequence cut at 64 KiB from one
        // cut by the end of the file.
        let mut head = Vec::with_capacity(SNIFF_LEN + 1);
        (&mut reader)
            .take(SNIFF_LEN as u64 + 1)
            .read_to_end(&mut head)?;
        let (file_encoding, bom_len) = detect_head(&head);
        head.drain(..bom_len);
        Self::decode_in(head, reader, file_encoding)
    }

    /// Like [`Text::from_reader_detecting`], but decodes in `encoding`'s
    /// [`Encoding`] whatever the file holds, as the same stream, malformed
    /// input becoming U+FFFD and reported in [`DecodedFile::replaced`].
    ///
    /// A BOM at the start is dropped only if it's that encoding's own (a
    /// UTF-8 or UTF-16 one); [`DecodedFile::encoding`]'s `bom` then says
    /// whether there was one, whatever `encoding.bom` asked for: it
    /// describes the file. Another encoding's BOM decodes as text.
    pub fn from_reader_with(
        mut reader: impl Read,
        encoding: FileEncoding,
    ) -> io::Result<DecodedFile> {
        // The longest BOM, UTF-8's.
        let mut head = Vec::with_capacity(3);
        (&mut reader).take(3).read_to_end(&mut head)?;
        let bom = match Encoding::for_bom(&head) {
            Some((found, bom_len)) if found == encoding.encoding => {
                head.drain(..bom_len);
                true
            }
            _ => false,
        };
        let file_encoding = FileEncoding {
            encoding: encoding.encoding,
            bom,
        };
        Self::decode_in(head, reader, file_encoding)
    }

    /// Decodes `head`, the start of the file with any BOM dropped, then
    /// the rest of `reader`, in `file_encoding`.
    fn decode_in(
        head: Vec<u8>,
        reader: impl Read,
        file_encoding: FileEncoding,
    ) -> io::Result<DecodedFile> {
        let mut replaced = false;
        let (text, line_ending) = if file_encoding.encoding == UTF_8 {
            Self::decode_stream(head, reader, |bytes, eof, out| {
                decode_lossy(bytes, eof, out, &mut replaced)
            })?
        } else {
            // The BOM, if any, is already gone.
            let mut decoder = file_encoding.encoding.new_decoder_without_bom_handling();
            Self::decode_stream(head, reader, |bytes, eof, out| {
                let needed = decoder
                    .max_utf8_buffer_length(bytes.len())
                    .expect("a read buffer's worth of UTF-8 fits in usize");
                out.reserve(needed);
                let (result, read, replaced_here) = decoder.decode_to_string(bytes, out, eof);
                debug_assert_eq!(result, encoding_rs::CoderResult::InputEmpty);
                replaced |= replaced_here;
                read
            })?
        };
        Ok(DecodedFile {
            text,
            line_ending,
            encoding: file_encoding,
            replaced,
        })
    }

    /// Writes the text in `encoding`, with `line_ending` for each line break as
    /// [`Text::write_to`] does, preceded by a BOM if `encoding.bom` is set
    /// (UTF-8 and UTF-16 only).
    ///
    /// A char the encoding can't represent stops the write with
    /// [`EncodeError::Unmappable`]; nothing is ever substituted for it. Output
    /// before that point has already been written, so write to a temporary
    /// file and keep it only on success.
    pub fn write_encoded(
        &self,
        line_ending: LineEnding,
        encoding: FileEncoding,
        mut out: impl Write,
    ) -> Result<(), EncodeError> {
        let mut sink = Sink::new(encoding.encoding);
        if encoding.bom {
            match sink {
                Sink::Utf8 => out.write_all(b"\xEF\xBB\xBF")?,
                Sink::Utf16 { big_endian: false } => out.write_all(b"\xFF\xFE")?,
                Sink::Utf16 { big_endian: true } => out.write_all(b"\xFE\xFF")?,
                Sink::Legacy { .. } => {}
            }
        }
        let mut offset = 0;
        for chunk in self.chunks() {
            let mut lines = chunk.split('\n');
            if let Some(first) = lines.next() {
                sink.write(first, offset, false, &mut out)?;
                offset += first.len();
            }
            for line in lines {
                sink.write(line_ending.as_str(), offset, false, &mut out)?;
                offset += 1;
                sink.write(line, offset, false, &mut out)?;
                offset += line.len();
            }
        }
        sink.write("", offset, true, &mut out)
    }
}

/// Whether `sniffed` is valid UTF-8, forgiving a sequence cut off at its end
/// unless the file ends there too.
fn starts_with_utf8(sniffed: &[u8], at_eof: bool) -> bool {
    match std::str::from_utf8(sniffed) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none() && !at_eof,
    }
}

/// Encodes UTF-8 text into a file's encoding, a piece at a time.
enum Sink {
    Utf8,
    Utf16 {
        big_endian: bool,
    },
    /// encoding_rs encodes everything else. (Its encoders write UTF-16 as UTF-8,
    /// as the web does, hence the variant above.)
    Legacy {
        encoding: &'static Encoding,
        encoder: Encoder,
        buf: Vec<u8>,
        /// Whether each char seen so far decodes back to itself.
        round_trips: HashMap<char, bool>,
    },
}

impl Sink {
    fn new(encoding: &'static Encoding) -> Self {
        if encoding == UTF_8 {
            Sink::Utf8
        } else if encoding == UTF_16LE || encoding == UTF_16BE {
            Sink::Utf16 {
                big_endian: encoding == UTF_16BE,
            }
        } else {
            Sink::Legacy {
                encoding,
                encoder: encoding.new_encoder(),
                buf: Vec::new(),
                round_trips: HashMap::new(),
            }
        }
    }

    /// Writes `text`, which starts at byte `offset` in the text being saved.
    /// `last` ends the stream (ISO-2022-JP returns to ASCII).
    fn write(
        &mut self,
        text: &str,
        offset: usize,
        last: bool,
        out: &mut impl Write,
    ) -> Result<(), EncodeError> {
        match self {
            Sink::Utf8 => out.write_all(text.as_bytes())?,
            Sink::Utf16 { big_endian } => {
                let mut bytes = Vec::with_capacity(text.len() * 2);
                for unit in text.encode_utf16() {
                    let pair = if *big_endian {
                        unit.to_be_bytes()
                    } else {
                        unit.to_le_bytes()
                    };
                    bytes.extend_from_slice(&pair);
                }
                out.write_all(&bytes)?;
            }
            Sink::Legacy {
                encoding,
                encoder,
                buf,
                round_trips,
            } => {
                // Some chars encode without an error yet decode as another
                // char: '¥' becomes '\' in Shift_JIS and EUC-JP, half-width
                // katakana become full-width in ISO-2022-JP. Those are
                // unmappable too.
                let skip_ascii = encoding.is_ascii_compatible();
                for (ix, ch) in text.char_indices() {
                    if skip_ascii && ch.is_ascii() {
                        continue;
                    }
                    let lossless = *round_trips
                        .entry(ch)
                        .or_insert_with(|| char_round_trips(encoding, ch));
                    if !lossless {
                        return Err(EncodeError::Unmappable {
                            ch,
                            offset: offset + ix,
                        });
                    }
                }

                buf.clear();
                let needed = encoder
                    .max_buffer_length_from_utf8_without_replacement(text.len())
                    .expect("a rope chunk's worth of output fits in usize");
                buf.reserve(needed);
                let (result, read) =
                    encoder.encode_from_utf8_to_vec_without_replacement(text, buf, last);
                out.write_all(buf)?;
                match result {
                    EncoderResult::InputEmpty => {}
                    EncoderResult::Unmappable(_) => {
                        // Every char was checked above, so this shouldn't
                        // happen. The reported char can be a stand-in
                        // (ISO-2022-JP reports U+FFFD for ESC), so name the
                        // input char `read` ends with instead.
                        let ch = text[..read].chars().next_back().expect("read a char");
                        return Err(EncodeError::Unmappable {
                            ch,
                            offset: offset + read - ch.len_utf8(),
                        });
                    }
                    EncoderResult::OutputFull => {
                        unreachable!("buffer was sized for the worst case")
                    }
                }
            }
        }
        Ok(())
    }
}

/// Whether `ch` encodes in `encoding` and decodes back to itself.
fn char_round_trips(encoding: &'static Encoding, ch: char) -> bool {
    let mut utf8 = [0; 4];
    let ch = &*ch.encode_utf8(&mut utf8);
    let (bytes, _, unmappable) = encoding.encode(ch);
    !unmappable && encoding.decode_without_bom_handling(&bytes).0 == ch
}

#[cfg(test)]
mod unit_tests {
    use std::ops::Range;

    use encoding_rs::{Encoding, GB18030, GBK, ISO_2022_JP, SHIFT_JIS};

    use super::FileEncoding;

    /// The units of `bytes`, with a check that they run on from 0 with no gaps
    /// and make the text the editor decodes the bytes to.
    fn units(
        encoding: &'static Encoding,
        bytes: &[u8],
        complete: bool,
    ) -> Vec<(Range<usize>, String)> {
        let mut units: Vec<(Range<usize>, String)> = Vec::new();
        FileEncoding {
            encoding,
            bom: false,
        }
        .for_each_unit(bytes, complete, |range, text| {
            units.push((range, text.to_owned()));
            Ok::<_, ()>(false)
        })
        .unwrap();
        let mut at = 0;
        for (range, _) in &units {
            assert_eq!(range.start, at, "{units:?}");
            assert!(range.start < range.end, "{units:?}");
            at = range.end;
        }
        if complete {
            assert_eq!(at, bytes.len(), "{units:?}");
            let text: String = units.iter().map(|(_, text)| &text[..]).collect();
            assert_eq!(text, encoding.decode_without_bom_handling(bytes).0);
        }
        units
    }

    fn unit(range: Range<usize>, text: &str) -> (Range<usize>, String) {
        (range, text.to_owned())
    }

    #[test]
    fn bytes_read_again_after_a_malformed_char_are_units_of_their_own() {
        for encoding in [GBK, GB18030] {
            // \x81\x30 is cut short by the space: U+FFFD for the \x81, and
            // the "0" and the space are read again.
            assert_eq!(
                units(encoding, b"\x81\x30 needle", true)[..4],
                [
                    unit(0..1, "\u{FFFD}"),
                    unit(1..2, "0"),
                    unit(2..3, " "),
                    unit(3..4, "n")
                ]
            );
            // \x81\x30\x81 is cut short by a space, and two bytes are read again,
            // the first a char and the second malformed with the space.
            assert_eq!(
                units(encoding, b"\x81\x30\x81 x", true),
                [
                    unit(0..1, "\u{FFFD}"),
                    unit(1..2, "0"),
                    unit(2..3, "\u{FFFD}"),
                    unit(3..4, " "),
                    unit(4..5, "x"),
                ]
            );
        }
    }

    #[test]
    fn a_line_break_settles_a_malformed_char_before_it() {
        // Through the "\n", as the editor decodes the line.
        assert_eq!(
            units(GBK, b"needle\x81\x30\n", false)[6..],
            [unit(6..7, "\u{FFFD}"), unit(7..8, "0"), unit(8..9, "\n")]
        );
        // But at the end of the file, \x81\x30 are one malformed sequence.
        assert_eq!(
            units(GBK, b"needle\x81\x30", true)[6..],
            [unit(6..8, "\u{FFFD}")]
        );
        // Without the end of the line or the file, the pending char is not reported.
        assert_eq!(units(GBK, b"needle\x81\x30", false).len(), 6);
    }

    #[test]
    fn a_malformed_sequence_with_no_bytes_to_read_again_is_just_those_bytes() {
        assert_eq!(
            units(SHIFT_JIS, b"\x82 a", true),
            [unit(0..1, "\u{FFFD}"), unit(1..2, " "), unit(2..3, "a")]
        );
    }

    #[test]
    fn an_escape_cut_short_leaves_the_byte_after_it_a_unit() {
        // ESC "$" starts an escape sequence that "X" cuts short: U+FFFD for
        // the ESC, then "$" is read again, in ASCII, and "X".
        assert_eq!(
            units(ISO_2022_JP, b"\x1B$X needle", true)[..4],
            [
                unit(0..1, "\u{FFFD}"),
                unit(1..2, "$"),
                unit(2..3, "X"),
                unit(3..4, " ")
            ]
        );
        // The same in JIS X 0208, where "$" is a lead byte "X" completes: the
        // ESC is U+FFFD, and "$X" is one char.
        assert_eq!(
            units(ISO_2022_JP, b"\x1B$B\x30\x21\x1B$X\x21y", true),
            [
                unit(0..5, "\u{4E9C}"),
                unit(5..6, "\u{FFFD}"),
                unit(6..8, "\u{3078}"),
                unit(8..10, "\u{2606}"),
            ]
        );
    }

    #[test]
    fn an_escape_cut_short_by_the_end_of_the_stream_is_walked_to_its_end() {
        assert_eq!(
            units(ISO_2022_JP, b"\x1B$", true),
            [unit(0..1, "\u{FFFD}"), unit(1..2, "$")]
        );
        assert_eq!(
            units(ISO_2022_JP, b"a\x1B(", true),
            [unit(0..1, "a"), unit(1..2, "\u{FFFD}"), unit(2..3, "(")]
        );
    }

    #[test]
    fn a_char_cut_short_by_an_escape_does_not_swallow_it() {
        // A lead byte cut short by an escape sequence back to ASCII.
        let units = units(ISO_2022_JP, b"\x1B$B$\x1B(Bneedle", true);
        assert_eq!(units[0], unit(0..4, "\u{FFFD}"));
        assert_eq!(units[1], unit(4..8, "n"));
    }
}
