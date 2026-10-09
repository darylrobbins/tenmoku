//! Reading text in any encoding a file might use, and writing it back the same way.
//!
//! [`Text::from_reader_detecting`] picks the encoding from a BOM, then from
//! whether the start of the file is valid UTF-8, and otherwise from a
//! `chardetng` guess. [`Text::write_encoded`] writes text back in a
//! [`FileEncoding`], failing rather than substituting a char the encoding
//! can't represent.

use std::collections::HashMap;
use std::io::{self, Read, Write};

use encoding_rs::{Encoder, EncoderResult, Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::{LineEnding, Text, decode_lossy};

/// How many bytes from the start of a file decide its encoding.
const SNIFF_LEN: usize = 64 * 1024;

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

/// What [`Text::from_reader_detecting`] read.
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

impl Text {
    /// Like [`Text::from_reader`], but detects the file's encoding first:
    ///
    /// 1. a UTF-8, UTF-16LE or UTF-16BE BOM picks that encoding and is dropped;
    /// 2. otherwise, if the first 64 KiB are valid UTF-8 (a sequence cut off at
    ///    64 KiB doesn't count against it), the file is UTF-8, and invalid bytes
    ///    later on become U+FFFD exactly as with [`Text::from_reader`];
    /// 3. otherwise `chardetng` guesses from those 64 KiB.
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
        let at_eof = head.len() <= SNIFF_LEN;
        let sniffed = &head[..head.len().min(SNIFF_LEN)];

        let file_encoding = if let Some((encoding, bom_len)) = Encoding::for_bom(&head) {
            head.drain(..bom_len);
            FileEncoding {
                encoding,
                bom: true,
            }
        } else if starts_with_utf8(sniffed, at_eof) {
            FileEncoding::UTF8
        } else {
            let mut detector = chardetng::EncodingDetector::new();
            detector.feed(sniffed, at_eof);
            FileEncoding {
                encoding: detector.guess(None, false),
                bom: false,
            }
        };

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
