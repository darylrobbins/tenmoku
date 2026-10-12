//! [`ScipIndex`]: a decoded SCIP index (`index.scip`), with each file's
//! occurrences and each symbol's definition and reference sites, for exact
//! navigation with no language server running.
//!
//! SCIP is a protobuf format written by scip-typescript, scip-java,
//! scip-python, rust-analyzer's `scip` command and others. The decoded
//! message isn't kept: [`ScipIndex::from_bytes`] keeps only what navigation
//! asks of it.

use std::{
    collections::{BTreeMap, HashMap},
    fmt, fs, io,
    ops::Range,
    path::Path,
    sync::Arc,
};

use fe_text::Text;
use protobuf::{Enum, Message};
use scip::types::{
    Document, Index, Occurrence, PositionEncoding, SymbolInformation, SymbolRole,
    occurrence::Typed_range, symbol_information::Kind,
};

/// Why an index couldn't be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScipError {
    /// Reading the file failed.
    Io(io::ErrorKind),
    /// It isn't a SCIP index: truncated, or not protobuf at all.
    Decode(String),
}

impl fmt::Display for ScipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScipError::Io(kind) => write!(f, "reading the SCIP index: {kind}"),
            ScipError::Decode(why) => write!(f, "decoding the SCIP index: {why}"),
        }
    }
}

impl std::error::Error for ScipError {}

/// What a site's character offsets count from the start of their line.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScipEncoding {
    Utf8,
    /// Also what a document with no (or an unknown) position encoding is
    /// read as, as LSP does.
    Utf16,
    Utf32,
}

impl ScipEncoding {
    fn of(document: &Document) -> Self {
        match document.position_encoding.enum_value() {
            Ok(PositionEncoding::UTF8CodeUnitOffsetFromLineStart) => ScipEncoding::Utf8,
            Ok(PositionEncoding::UTF32CodeUnitOffsetFromLineStart) => ScipEncoding::Utf32,
            _ => ScipEncoding::Utf16,
        }
    }
}

/// Where an occurrence is: 0-based `(line, character)` positions in `rel`,
/// with characters counted in `encoding`'s code units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScipSite {
    /// The document's `relative_path`, relative to the project root.
    pub rel: Arc<Path>,
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub encoding: ScipEncoding,
}

impl ScipSite {
    /// The site's bytes in `text`, the file's text as the editor loads it.
    /// `None` when a line is past the end of `text`, an offset is past the
    /// end of its line or splits a char, or the end is before the start:
    /// the index was made from a different version of the file.
    pub fn byte_range(&self, text: &Text) -> Option<Range<usize>> {
        byte_range(text, self.start, self.end, self.encoding)
    }
}

/// The occurrence at an offset, from [`ScipIndex::occurrence_at`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScipOccurrence {
    pub symbol: Arc<str>,
    pub site: ScipSite,
    /// The site's bytes in the text it was found in.
    pub range: Range<usize>,
    /// Whether its roles have the Definition bit.
    pub definition: bool,
    /// Which of the index's documents for this file it came from, by their
    /// order in the index: 0 unless the indexer split the file across
    /// several. A local symbol is scoped to its document, so this is what
    /// [`ScipIndex::local_definitions`] takes.
    pub scope: u32,
}

/// What the index's `SymbolInformation` says of a symbol, for workspace
/// symbols.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScipSymbolInfo {
    /// Empty when the indexer gave none.
    pub display_name: String,
    /// `UnspecifiedKind` when the indexer gave none or one this version of
    /// SCIP doesn't know.
    pub kind: Kind,
}

/// A decoded SCIP index.
///
/// Global symbols' sites are kept across documents; a symbol starting
/// `local ` is only meaningful in its own document, so its sites are kept
/// there ([`ScipIndex::local_definitions`]) and never mixed with another
/// document's local of the same name, even another document for the same
/// file. [`ScipIndex::definitions_of`] answers for either kind.
#[derive(Debug, Default)]
pub struct ScipIndex {
    documents: BTreeMap<Arc<Path>, ScipDocument>,
    symbols: HashMap<Arc<str>, Sites>,
    info: HashMap<Arc<str>, ScipSymbolInfo>,
    /// Every document's language, as the indexer spelt it, each once.
    languages: Vec<String>,
}

#[derive(Debug)]
struct ScipDocument {
    language: String,
    /// Sorted by start, then end.
    occurrences: Vec<Occ>,
    /// The most lines any occurrence spans, which bounds how far back from
    /// a line [`ScipIndex::occurrence_at`] looks.
    max_span: u32,
    /// By scope: one map for each of the index's documents for this file.
    locals: Vec<HashMap<Arc<str>, Sites>>,
}

#[derive(Debug)]
struct Occ {
    start: (u32, u32),
    end: (u32, u32),
    encoding: ScipEncoding,
    symbol: Arc<str>,
    definition: bool,
    scope: u32,
}

#[derive(Debug, Default)]
struct Sites {
    definitions: Vec<ScipSite>,
    references: Vec<ScipSite>,
}

impl ScipIndex {
    /// Reads and decodes the index at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ScipError> {
        let bytes = fs::read(path).map_err(|error| ScipError::Io(error.kind()))?;
        Self::from_bytes(&bytes)
    }

    /// Decodes an encoded `scip.Index` message.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ScipError> {
        let index =
            Index::parse_from_bytes(bytes).map_err(|error| ScipError::Decode(error.to_string()))?;
        check_framing(bytes).map_err(ScipError::Decode)?;
        Ok(Self::from_index(index))
    }

    fn from_index(index: Index) -> Self {
        // An indexer may split one file across several documents: they're
        // merged under its path, each occurrence keeping its document's
        // encoding and its locals their document's scope.
        let mut by_path: BTreeMap<&str, Vec<&Document>> = BTreeMap::new();
        for document in &index.documents {
            by_path
                .entry(document.relative_path.as_str())
                .or_default()
                .push(document);
        }
        let mut this = Self::default();
        for document in &index.documents {
            if !this.languages.contains(&document.language) {
                this.languages.push(document.language.clone());
            }
        }
        for (rel, documents) in by_path {
            this.add_document(Path::new(rel).into(), &documents);
        }
        for info in index
            .documents
            .iter()
            .flat_map(|document| &document.symbols)
            .chain(&index.external_symbols)
        {
            this.add_info(info);
        }
        this
    }

    fn add_document(&mut self, rel: Arc<Path>, documents: &[&Document]) {
        let mut doc = ScipDocument {
            language: documents[0].language.clone(),
            occurrences: Vec::new(),
            max_span: 0,
            locals: Vec::new(),
        };
        for (scope, document) in (0u32..).zip(documents) {
            let encoding = ScipEncoding::of(document);
            doc.locals.push(HashMap::new());
            for occurrence in &document.occurrences {
                if occurrence.symbol.is_empty() {
                    continue;
                }
                let Some((start, end)) = range(occurrence) else {
                    continue;
                };
                let symbol = if is_local(&occurrence.symbol) {
                    intern(&mut doc.locals[scope as usize], &occurrence.symbol)
                } else {
                    intern(&mut self.symbols, &occurrence.symbol)
                };
                doc.max_span = doc.max_span.max(end.0 - start.0);
                doc.occurrences.push(Occ {
                    start,
                    end,
                    encoding,
                    symbol,
                    definition: occurrence.symbol_roles & SymbolRole::Definition.value() != 0,
                    scope,
                });
            }
        }
        doc.occurrences
            .sort_by_key(|occ| (occ.start, occ.end, occ.definition));
        // Sites go in by document, then by position.
        for occ in &doc.occurrences {
            let map = if is_local(&occ.symbol) {
                &mut doc.locals[occ.scope as usize]
            } else {
                &mut self.symbols
            };
            let Some(sites) = map.get_mut(&occ.symbol) else {
                continue;
            };
            let site = ScipSite {
                rel: rel.clone(),
                start: occ.start,
                end: occ.end,
                encoding: occ.encoding,
            };
            if occ.definition {
                sites.definitions.push(site);
            } else {
                sites.references.push(site);
            }
        }
        self.documents.insert(rel, doc);
    }

    /// Keeps a global symbol's display name and kind. A symbol described
    /// twice keeps the first description, with its gaps filled from later
    /// ones.
    fn add_info(&mut self, info: &SymbolInformation) {
        if info.symbol.is_empty() || is_local(&info.symbol) {
            return;
        }
        let kind = info.kind.enum_value_or_default();
        if let Some(known) = self.info.get_mut(info.symbol.as_str()) {
            if known.display_name.is_empty() {
                known.display_name.clone_from(&info.display_name);
            }
            if known.kind == Kind::UnspecifiedKind {
                known.kind = kind;
            }
            return;
        }
        let symbol = match self.symbols.get_key_value(info.symbol.as_str()) {
            Some((symbol, _)) => symbol.clone(),
            None => info.symbol.as_str().into(),
        };
        self.info.insert(
            symbol,
            ScipSymbolInfo {
                display_name: info.display_name.clone(),
                kind,
            },
        );
    }

    /// The documents' relative paths, sorted.
    pub fn documents(&self) -> impl Iterator<Item = &Path> {
        self.documents.keys().map(|rel| &**rel)
    }

    /// Every language the indexer gave a document, as it spelt it, each once:
    /// those of each of a file's documents, where it split one.
    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.languages.iter().map(String::as_str)
    }

    /// The language the indexer gave `rel`, as it spelt it (`None` for a
    /// file not in the index; empty when it gave none): its first
    /// document's, where it split the file across several.
    pub fn language(&self, rel: &Path) -> Option<&str> {
        self.documents.get(rel).map(|doc| doc.language.as_str())
    }

    /// The occurrence in `rel` whose bytes in `text` contain `offset` or end
    /// at it: the innermost if several do, and of two the same length (one
    /// ending where the next starts), the one it isn't at the end of.
    /// `text` is the file's text, which the index's positions are mapped
    /// into.
    pub fn occurrence_at(&self, rel: &Path, text: &Text, offset: usize) -> Option<ScipOccurrence> {
        let doc = self.documents.get(rel)?;
        let row = text.offset_to_point(offset).row;
        let first_row = row.saturating_sub(doc.max_span);
        let lo = doc
            .occurrences
            .partition_point(|occ| occ.start.0 < first_row);
        let hi = doc.occurrences.partition_point(|occ| occ.start.0 <= row);
        let mut best: Option<(&Occ, Range<usize>)> = None;
        for occ in &doc.occurrences[lo..hi] {
            if occ.end.0 < row {
                continue;
            }
            let Some(range) = byte_range(text, occ.start, occ.end, occ.encoding) else {
                continue;
            };
            if !(range.start <= offset && offset <= range.end) {
                continue;
            }
            // The shortest; then one containing it over one ending at it;
            // then the later (they're sorted, so a tie keeps the later).
            let key = |range: &Range<usize>| (range.len(), offset == range.end);
            if best
                .as_ref()
                .is_none_or(|(_, best)| key(&range) <= key(best))
            {
                best = Some((occ, range));
            }
        }
        let (occ, range) = best?;
        Some(ScipOccurrence {
            symbol: occ.symbol.clone(),
            site: ScipSite {
                rel: self.documents.get_key_value(rel)?.0.clone(),
                start: occ.start,
                end: occ.end,
                encoding: occ.encoding,
            },
            range,
            definition: occ.definition,
            scope: occ.scope,
        })
    }

    /// Every occurrence in `site`'s file spanning exactly `site`, each with
    /// its bytes in `text`, the file's text: the several symbols an indexer
    /// gave one span, such as an import alias's own symbol beside the symbol
    /// it names. Empty when `site` isn't in `text`. They may come from
    /// several of the index's documents for the file: each has its
    /// [`ScipOccurrence::scope`], and only those of one scope were indexed
    /// together.
    pub fn occurrences_on(&self, site: &ScipSite, text: &Text) -> Vec<ScipOccurrence> {
        let Some(doc) = self.documents.get(&site.rel) else {
            return Vec::new();
        };
        let Some(range) = site.byte_range(text) else {
            return Vec::new();
        };
        let span = (site.start, site.end);
        let from = doc
            .occurrences
            .partition_point(|occ| (occ.start, occ.end) < span);
        doc.occurrences[from..]
            .iter()
            .take_while(|occ| (occ.start, occ.end) == span)
            .filter(|occ| occ.encoding == site.encoding)
            .map(|occ| ScipOccurrence {
                symbol: occ.symbol.clone(),
                site: site.clone(),
                range: range.clone(),
                definition: occ.definition,
                scope: occ.scope,
            })
            .collect()
    }

    /// Where `occurrence`'s symbol is defined: across the index for a
    /// global symbol, in its own document for a local one.
    pub fn definitions_of(&self, occurrence: &ScipOccurrence) -> &[ScipSite] {
        if is_local(&occurrence.symbol) {
            let site = &occurrence.site;
            self.local_definitions(&site.rel, occurrence.scope, &occurrence.symbol)
        } else {
            self.definitions(&occurrence.symbol)
        }
    }

    /// Where `occurrence`'s symbol occurs without the Definition role: across
    /// the index for a global symbol, in its own document for a local one.
    pub fn references_of(&self, occurrence: &ScipOccurrence) -> &[ScipSite] {
        if is_local(&occurrence.symbol) {
            let site = &occurrence.site;
            self.local_references(&site.rel, occurrence.scope, &occurrence.symbol)
        } else {
            self.references(&occurrence.symbol)
        }
    }

    /// Where the global `symbol` is defined, by document and position.
    /// Empty for a local symbol: see [`ScipIndex::local_definitions`].
    pub fn definitions(&self, symbol: &str) -> &[ScipSite] {
        self.symbols
            .get(symbol)
            .map_or(&[], |sites| &sites.definitions)
    }

    /// Where the global `symbol` occurs without the Definition role, by
    /// document and position.
    pub fn references(&self, symbol: &str) -> &[ScipSite] {
        self.symbols
            .get(symbol)
            .map_or(&[], |sites| &sites.references)
    }

    /// Where the local `symbol` (`local 0`, say) is defined in `rel`'s
    /// document `scope` ([`ScipOccurrence::scope`]).
    pub fn local_definitions(&self, rel: &Path, scope: u32, symbol: &str) -> &[ScipSite] {
        self.local(rel, scope, symbol)
            .map_or(&[], |sites| &sites.definitions)
    }

    /// Where the local `symbol` occurs in `rel`'s document `scope` without
    /// the Definition role.
    pub fn local_references(&self, rel: &Path, scope: u32, symbol: &str) -> &[ScipSite] {
        self.local(rel, scope, symbol)
            .map_or(&[], |sites| &sites.references)
    }

    fn local(&self, rel: &Path, scope: u32, symbol: &str) -> Option<&Sites> {
        self.documents
            .get(rel)?
            .locals
            .get(scope as usize)?
            .get(symbol)
    }

    /// The display name and kind the index gives the global `symbol`.
    pub fn symbol_info(&self, symbol: &str) -> Option<&ScipSymbolInfo> {
        self.info.get(symbol)
    }

    /// Whether the index has any global symbol: one occurring in a
    /// document, or one it only describes (an external symbol, say).
    pub fn has_global_symbols(&self) -> bool {
        !self.symbols.is_empty() || !self.info.is_empty()
    }

    /// Every global symbol the index describes, in no particular order.
    pub fn symbols(&self) -> impl Iterator<Item = (&str, &ScipSymbolInfo)> {
        self.info.iter().map(|(symbol, info)| (&**symbol, info))
    }
}

/// Checks that each of the message's fields fits in `bytes`, walking
/// into groups (which SCIP never writes, but an unknown field may be one).
///
/// protobuf takes a nested message that the input ends inside of as ending
/// there, so a cut through the last document would decode as that document
/// with its tail missing. A cut file always cuts its last field at the top
/// level (or in a group there), whose length then runs past the end.
fn check_framing(mut bytes: &[u8]) -> Result<(), String> {
    fn varint(bytes: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = bytes.split_first()?;
            *bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    let truncated = || "truncated".to_owned();
    // The field numbers of the groups the walk is in, innermost last.
    let mut groups = Vec::new();
    while !bytes.is_empty() {
        let tag = varint(&mut bytes).ok_or_else(truncated)?;
        let len = match tag & 7 {
            0 => {
                varint(&mut bytes).ok_or_else(truncated)?;
                0
            }
            1 => 8,
            2 => varint(&mut bytes).ok_or_else(truncated)?,
            3 => {
                groups.push(tag >> 3);
                0
            }
            4 if groups.last() == Some(&(tag >> 3)) => {
                groups.pop();
                0
            }
            5 => 4,
            _ => return Err(format!("bad field tag {tag}")),
        };
        let len = usize::try_from(len).map_err(|_| truncated())?;
        bytes = bytes.get(len..).ok_or_else(truncated)?;
    }
    if groups.is_empty() {
        Ok(())
    } else {
        Err(truncated())
    }
}

fn is_local(symbol: &str) -> bool {
    symbol.starts_with("local ")
}

/// `symbol` as the key `map` has for it, added if it's new.
fn intern(map: &mut HashMap<Arc<str>, Sites>, symbol: &str) -> Arc<str> {
    if let Some((symbol, _)) = map.get_key_value(symbol) {
        return symbol.clone();
    }
    let symbol: Arc<str> = symbol.into();
    map.insert(symbol.clone(), Sites::default());
    symbol
}

/// An occurrence's `(start, end)`: from `range`, `[line, start, end]` or
/// `[start_line, start, end_line, end]`, or from the typed range newer
/// indexers may write instead. `None` for any other length, a negative
/// value, or an end before the start.
fn range(occurrence: &Occurrence) -> Option<((u32, u32), (u32, u32))> {
    let at = |ix: usize| u32::try_from(occurrence.range[ix]).ok();
    let (start, end) = match occurrence.range.len() {
        3 => ((at(0)?, at(1)?), (at(0)?, at(2)?)),
        4 => ((at(0)?, at(1)?), (at(2)?, at(3)?)),
        0 => {
            let at = |value: i32| u32::try_from(value).ok();
            match occurrence.typed_range.as_ref()? {
                Typed_range::SingleLineRange(range) => (
                    (at(range.line)?, at(range.start_character)?),
                    (at(range.line)?, at(range.end_character)?),
                ),
                Typed_range::MultiLineRange(range) => (
                    (at(range.start_line)?, at(range.start_character)?),
                    (at(range.end_line)?, at(range.end_character)?),
                ),
                _ => return None,
            }
        }
        _ => return None,
    };
    (start <= end).then_some((start, end))
}

fn byte_range(
    text: &Text,
    start: (u32, u32),
    end: (u32, u32),
    encoding: ScipEncoding,
) -> Option<Range<usize>> {
    let start = to_offset(text, start, encoding)?;
    let end = to_offset(text, end, encoding)?;
    (start <= end).then_some(start..end)
}

/// The byte offset of `(line, column)`, `column` counting `encoding`'s
/// code units from the line's start.
fn to_offset(text: &Text, (line, column): (u32, u32), encoding: ScipEncoding) -> Option<usize> {
    if line >= text.row_count() {
        return None;
    }
    let line_start = text.row_start(line);
    let line_end = line_start + text.row_len(line) as usize;
    if encoding == ScipEncoding::Utf8 {
        let offset = line_start + column as usize;
        return (offset <= line_end && text.is_char_boundary(offset)).then_some(offset);
    }
    let target = u64::from(column);
    let mut units = 0u64;
    let mut offset = line_start;
    for chunk in text.chunks_in_range(line_start..line_end) {
        for ch in chunk.chars() {
            if units == target {
                return Some(offset);
            }
            units += match encoding {
                ScipEncoding::Utf16 => ch.len_utf16() as u64,
                _ => 1,
            };
            if units > target {
                return None;
            }
            offset += ch.len_utf8();
        }
    }
    (units == target).then_some(offset)
}
