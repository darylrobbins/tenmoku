//! The spelling core, with no GPUI: [`tokenize`] finds the words in some
//! text, [`check_text`] asks a [`Speller`] about them past the user's
//! [`Dictionary`], and [`suggest`] fits a speller's suggestions to a word.
//!
//! The speller is a seam: the first release's is macOS's, and [`NoSpeller`]
//! is everywhere else's. A word the tokenizer gets wrong is a false squiggle
//! in someone's code, so anything that doesn't read as a word is left out.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    ops::Range,
    path::{Path, PathBuf},
};

/// The user's dictionary's name, in the config directory.
pub const DICTIONARY_FILE: &str = "dictionary.txt";

/// The most suggestions [`suggest`] gives.
pub const MAX_SUGGESTIONS: usize = 8;

/// What knows how words are spelled.
pub trait Speller: Send + Sync {
    /// Whether `word` is misspelled in `language`: `"auto"` for the system's
    /// languages, or a BCP-47 tag.
    fn is_misspelled(&self, word: &str, language: &str) -> bool;

    /// What `word` might have been meant to be, the likeliest first.
    fn suggest(&self, word: &str, language: &str) -> Vec<String>;
}

/// Finds every word correct: off macOS, and until a speller is wired in.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSpeller;

impl Speller for NoSpeller {
    fn is_misspelled(&self, _word: &str, _language: &str) -> bool {
        false
    }

    fn suggest(&self, _word: &str, _language: &str) -> Vec<String> {
        Vec::new()
    }
}

/// A word [`tokenize`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word<'a> {
    /// Where it is in the text, in bytes.
    pub range: Range<usize>,
    pub text: &'a str,
}

/// The config directory [`Dictionary::user`] reads from.
#[cfg(not(test))]
fn user_config_dir() -> Option<PathBuf> {
    fe_config::paths::config_dir()
}

#[cfg(test)]
thread_local! {
    /// The config directory a test named on this thread, if any.
    static TEST_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Under test, the directory a test named on this thread, else none.
#[cfg(test)]
fn user_config_dir() -> Option<PathBuf> {
    TEST_CONFIG_DIR.with(|dir| dir.borrow().clone())
}

/// Names the config directory [`Dictionary::user`] reads from on this
/// thread, under test; `None` for none.
#[cfg(test)]
pub(crate) fn set_test_config_dir(dir: Option<PathBuf>) {
    TEST_CONFIG_DIR.with(|cell| *cell.borrow_mut() = dir);
}

/// A word [`check_text`] found misspelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misspelling {
    /// Where it is in the text, in bytes.
    pub range: Range<usize>,
    pub word: String,
}

/// The words the user has accepted: those in their dictionary file, which
/// stay, and those ignored for this session, which don't. Both ignore case,
/// by Unicode's rules: `Straße` covers `STRASSE`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dictionary {
    /// `None` keeps added words for the session only.
    path: Option<PathBuf>,
    words: HashSet<String>,
    ignored: HashSet<String>,
}

impl Dictionary {
    /// `<config dir>/`[`DICTIONARY_FILE`]; with no config directory, an
    /// empty dictionary that writes nothing. Reads the file, blocking.
    /// Under test the config directory is only one a test names
    /// ([`set_test_config_dir`]), never the environment's, so no test
    /// reads or writes the developer's own dictionary.
    pub fn user() -> Self {
        match user_config_dir() {
            Some(dir) => Self::load(dir.join(DICTIONARY_FILE)),
            None => Self::default(),
        }
    }

    /// The dictionary in the file at `path`: one word per line, with blank
    /// lines and `#` comments ignored. A file that's missing or can't be read
    /// is an empty dictionary. Reads the file, blocking.
    pub fn load(path: PathBuf) -> Self {
        Self {
            words: read_words(&path),
            path: Some(path),
            ignored: HashSet::new(),
        }
    }

    /// The file added words go to, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether `word` was added or ignored, in any case.
    pub fn contains(&self, word: &str) -> bool {
        let key = key_of(word);
        self.words.contains(&key) || self.ignored.contains(&key)
    }

    /// Adds `word` for good, as a lowercase line at the end of the file,
    /// which is created with its directory if need be, unless the file has
    /// it already. It counts for this session even if the write fails.
    /// Something that can't be a line of the file, being empty, holding
    /// whitespace or reading as a comment, is no word and adds nothing.
    /// Blocks on the file and on its lock.
    pub fn add_word(&mut self, word: &str) -> io::Result<()> {
        let line = clean(word).to_lowercase();
        if line.is_empty() || line.starts_with('#') || line.contains(char::is_whitespace) {
            return Ok(());
        }
        self.words.insert(key_of(&line));
        let Some(path) = &self.path else {
            return Ok(());
        };
        self.words.extend(append_new_word(path, &line)?);
        Ok(())
    }

    /// Accepts `word` until the app quits. Nothing is written.
    pub fn ignore_word(&mut self, word: &str) {
        self.ignored.insert(key_of(word));
    }
}

/// `word` without space around it, with a typographic apostrophe as a plain
/// one, so `don’t` is `don't`.
fn clean(word: &str) -> String {
    word.trim().replace('’', "'")
}

/// What `word` is looked up by: the same for any case of it, as Unicode's
/// default case folding has it. Lowercase alone won't do, since `ß` is `SS`
/// in capitals and `σ` is `ς` at a word's end, so each letter goes through
/// capitals too, lowering first for `ẞ`. All that goes wrong that way is `ı`,
/// whose capital is `I`: it's a letter of its own, not a case of `i`.
fn key_of(word: &str) -> String {
    let mut key = String::new();
    for c in clean(word).chars() {
        if c == 'ı' {
            key.push(c);
            continue;
        }
        for upper in c.to_lowercase().flat_map(char::to_uppercase) {
            key.extend(upper.to_lowercase());
        }
    }
    key
}

/// The keys of the words in a dictionary file's `text`.
fn parse_words(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(key_of)
        .collect()
}

/// The keys of the words in the file at `path`; none if it can't be read.
fn read_words(path: &Path) -> HashSet<String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_words(&text),
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                log::warn!("can't read the dictionary {}: {error}", path.display());
            }
            HashSet::new()
        }
    }
}

/// Appends `line` to the file at `path` unless the file has its word, and
/// returns the keys of the words the file had. The file says what's written,
/// not a copy of it in memory, since another window or a hand edit may have
/// changed it; the lock, held until the file closes, keeps another writer
/// from coming between the check and the write. The line is one of its own
/// even if the file's last line has no newline.
fn append_new_word(path: &Path, line: &str) -> io::Result<HashSet<String>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;
    file.lock()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let words = match std::str::from_utf8(&bytes) {
        Ok(text) => parse_words(text),
        Err(error) => {
            log::warn!("can't read the dictionary {}: {error}", path.display());
            HashSet::new()
        }
    };
    if !words.contains(&key_of(line)) {
        let mut text = String::with_capacity(line.len() + 2);
        if bytes.last().is_some_and(|&last| last != b'\n') {
            text.push('\n');
        }
        text.push_str(line);
        text.push('\n');
        file.write_all(text.as_bytes())?;
    }
    Ok(words)
}

/// The words of `text` a speller could judge, in order. An identifier gives
/// its parts: `parseHttpResponse_body` is parse, Http, Response and body,
/// `HTTPServer` HTTP and Server. An apostrophe inside a word stays in it.
///
/// Left out are URLs, email addresses, file paths (anything with a `/`, so
/// `and/or` too, or a `\\` that isn't a string escape) and dotted or `::`
/// names; hex literals and words with digits; words under 3 letters; a word
/// (not a part of a camelCase one) that's all capitals with 2 to 5 letters;
/// and identifiers after a `$` or `@` sigil.
pub fn tokenize(text: &str) -> Vec<Word<'_>> {
    let mut words = Vec::new();
    let mut at = 0;
    while let Some(skip) = text[at..].find(|c| !ends_chunk(c)) {
        let start = at + skip;
        let rest = &text[start..];
        let end = rest.find(ends_chunk).unwrap_or(rest.len());
        if rest[..end].contains("://") {
            // A URL's brackets and commas are its own.
            at = start + rest.find(ends_url).unwrap_or(rest.len());
            continue;
        }
        if !is_address(&rest[..end]) {
            chunk_words(text, start..start + end, &mut words);
        }
        at = start + end;
    }
    words
}

/// What separates the chunks of text that are looked at whole, to tell if
/// they're addresses rather than words.
fn ends_chunk(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '*')
        || ends_url(c)
}

/// What can't be in a URL. It may run past a chunk's end, not this.
fn ends_url(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '`' | '<' | '>')
}

/// Whether `chunk`, from [`tokenize`], says where something is rather than
/// saying something in words: a file path, a dotted or `::` name, or an
/// email address. A `don\\'t` is left out with them, since no word's text
/// could hold it and its halves aren't words.
fn is_address(chunk: &str) -> bool {
    // Quotes around it aren't part of it.
    let chunk = chunk.trim_matches(is_apostrophe);
    if is_windows_path(chunk) {
        return true;
    }
    // A dotfile, which an ellipsis isn't.
    if let Some(name) = chunk.strip_prefix('.')
        && name.starts_with(char::is_alphanumeric)
    {
        return true;
    }
    // A sentence's punctuation is no part of it, nor is a `//comment`'s marker.
    let body = chunk.trim_matches(|c| is_apostrophe(c) || matches!(c, '.' | ':' | '!' | '?'));
    let body = body
        .strip_prefix("//")
        .map_or(body, |comment| comment.trim_start_matches('/'));
    body.contains('/')
        || body.contains("::")
        || body.contains("\\'")
        || body.contains("\\’")
        || joins_words(body, '.')
        || joins_words(body, '@')
        || has_path_backslash(body)
}

/// Whether `text` starts as only a Windows path does: `C:\`, `.\`, `..\`,
/// `~\` or `\\`.
fn is_windows_path(text: &str) -> bool {
    let mut chars = text.chars();
    let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next() == Some(':')
        && chars.next() == Some('\\');
    drive
        || ["\\\\", ".\\", "..\\", "~\\"]
            .iter()
            .any(|prefix| text.starts_with(prefix))
}

/// Whether a `\` in `text` can't be a string's escape, so it separates a
/// path's names: `assets\wrod`, `assets\_wrod`, or a string's `assets\\wrod`.
/// Where every one could be an escape nothing tells `src\new` from
/// `first\nsecond`, and it's read as the escape.
fn has_path_backslash(text: &str) -> bool {
    text.match_indices('\\').any(|(at, _)| {
        let mut after = text[at + 1..].chars();
        match after.next() {
            Some('\\') => after.next().is_some(),
            Some(c) => !is_string_escape(c),
            None => false,
        }
    })
}

/// Whether `\c` is an escape strings have.
fn is_string_escape(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            'a' | 'b' | 'e' | 'f' | 'n' | 'r' | 't' | 'u' | 'U' | 'v' | 'x'
        )
}

/// Whether `separator` is in `text` with a letter or digit on either side.
fn joins_words(text: &str, separator: char) -> bool {
    text.match_indices(separator).any(|(at, found)| {
        let before = text[..at].chars().next_back();
        let after = text[at + found.len()..].chars().next();
        before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric)
    })
}

/// Adds the words of the chunk at `range` of `text`. `\n` and its like, which
/// a string has, don't run into the word after them.
fn chunk_words<'a>(text: &'a str, range: Range<usize>, words: &mut Vec<Word<'a>>) {
    let end = range.end;
    let mut chars = text[range.clone()].char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        if c == '\\' {
            chars.next_if(|(_, next)| next.is_alphanumeric());
            continue;
        }
        if !c.is_alphanumeric() && c != '_' {
            continue;
        }
        let start = range.start + offset;
        let mut stop = end;
        let mut previous = c;
        while let Some(&(next_offset, next)) = chars.peek() {
            let in_word = next.is_alphanumeric()
                || next == '_'
                || is_combining_mark(next)
                || (is_apostrophe(next)
                    && (previous.is_alphabetic() || is_combining_mark(previous))
                    && text[range.start + next_offset + next.len_utf8()..end]
                        .starts_with(char::is_alphabetic));
            if !in_word {
                stop = range.start + next_offset;
                break;
            }
            previous = next;
            chars.next();
        }
        identifier_words(text, start..stop, words);
    }
}

/// Adds the words of the identifier at `range` of `text`.
fn identifier_words<'a>(text: &'a str, range: Range<usize>, words: &mut Vec<Word<'a>>) {
    let identifier = &text[range.clone()];
    if after_sigil(&text[..range.start])
        || identifier.starts_with("0x")
        || identifier.starts_with("0X")
    {
        return;
    }
    let mut start = range.start;
    for segment in identifier.split('_') {
        let segment_range = start..start + segment.len();
        start = segment_range.end + 1;
        if !is_acronym(segment) {
            camel_words(text, segment_range, words);
        }
    }
}

/// Whether what comes before an identifier is a `$` or `@` sigil, which may
/// have opened a bracket first, as `${name}` does.
fn after_sigil(before: &str) -> bool {
    let before = before.strip_suffix(['{', '(']).unwrap_or(before);
    before.ends_with(['$', '@'])
}

/// Adds the parts of the camelCase word at `range` of `text`, which holds
/// no `_`, that a speller could judge.
fn camel_words<'a>(text: &'a str, range: Range<usize>, words: &mut Vec<Word<'a>>) {
    // Marks belong to the letter before them, so they're passed over.
    let letters: Vec<(usize, char)> = text[range.clone()]
        .char_indices()
        .filter(|&(_, c)| !is_combining_mark(c))
        .map(|(offset, c)| (range.start + offset, c))
        .collect();
    let mut start = range.start;
    for ix in 1..=letters.len() {
        if ix < letters.len() && !starts_part(&letters, ix) {
            continue;
        }
        let end = letters.get(ix).map_or(range.end, |&(offset, _)| offset);
        let part = &text[start..end];
        if is_checkable_part(part) {
            words.push(Word {
                range: start..end,
                text: part,
            });
        }
        start = end;
    }
}

/// Whether a new part of a camelCase word starts at `letters[ix]`: a capital
/// after a lowercase letter or digit, or the last capital of a run if a
/// lowercase letter follows, unless that's only a plural's `s`.
fn starts_part(letters: &[(usize, char)], ix: usize) -> bool {
    let at = |ix: usize| letters.get(ix).map(|&(_, c)| c);
    let (Some(previous), Some(current)) = (at(ix - 1), at(ix)) else {
        return false;
    };
    if !current.is_uppercase() {
        return false;
    }
    if previous.is_lowercase() || previous.is_numeric() {
        return true;
    }
    let plural = at(ix + 1) == Some('s') && at(ix + 2).is_none_or(char::is_uppercase);
    previous.is_uppercase() && at(ix + 1).is_some_and(char::is_lowercase) && !plural
}

/// Whether a part of a camelCase word is one a speller could judge. `HTTP`
/// in `HTTPServer` is, but `URLs` in `parseURLs` is an acronym's plural,
/// which a dictionary won't have.
fn is_checkable_part(part: &str) -> bool {
    let letters = part.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3
        && !part.chars().any(char::is_numeric)
        && (strip_plural(part) == part || !is_acronym(part))
}

/// `word` without the `s` or `'s` of a plural or possessive.
fn strip_plural(word: &str) -> &str {
    word.strip_suffix("'s")
        .or_else(|| word.strip_suffix("’s"))
        .or_else(|| word.strip_suffix('s'))
        .unwrap_or(word)
}

/// Whether `word` is all capitals with 2 to 5 letters, perhaps a plural or
/// possessive of them: `URL`, `URLs`, `DON'T`.
fn is_acronym(word: &str) -> bool {
    let mut letters = 0;
    for c in strip_plural(word).chars() {
        if c.is_alphabetic() {
            if !c.is_uppercase() {
                return false;
            }
            letters += 1;
        } else if !is_apostrophe(c) && !is_combining_mark(c) {
            return false;
        }
    }
    (2..=5).contains(&letters)
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

/// Whether `c` is a combining diacritic, as in a decomposed `é`.
fn is_combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}')
}

/// The words of `text` that are misspelled in `language`, in order: those
/// not in `dictionary` that `speller` finds wrong. It's asked once for each
/// distinct word, with case counted, since `paris` may be wrong and `Paris`
/// not.
pub fn check_text(
    speller: &dyn Speller,
    dictionary: &Dictionary,
    text: &str,
    language: &str,
) -> Vec<Misspelling> {
    let mut verdicts: HashMap<&str, bool> = HashMap::new();
    tokenize(text)
        .into_iter()
        .filter(|word| {
            *verdicts.entry(word.text).or_insert_with(|| {
                !dictionary.contains(word.text) && speller.is_misspelled(word.text, language)
            })
        })
        .map(|word| Misspelling {
            word: word.text.to_owned(),
            range: word.range,
        })
        .collect()
}

/// `speller`'s suggestions for `word` in `language`, in `word`'s case
/// (`Teh` gives `The`, `TEH` gives `THE`), each once, at most
/// [`MAX_SUGGESTIONS`] of them.
pub fn suggest(speller: &dyn Speller, word: &str, language: &str) -> Vec<String> {
    let case = Case::of(word);
    let mut seen = HashSet::new();
    speller
        .suggest(word, language)
        .iter()
        .map(|suggestion| case.apply(suggestion))
        .filter(|suggestion| seen.insert(suggestion.clone()))
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// The case of a word, which its suggestions take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    /// `TEH`: every letter a capital, with at least two letters.
    Upper,
    /// `Teh`: only the first. A lone capital is this too.
    Title,
    /// Anything else, whose suggestions are left as the speller wrote them.
    AsGiven,
}

impl Case {
    fn of(word: &str) -> Self {
        let mut letters = word.chars().filter(|c| c.is_alphabetic());
        if !letters.next().is_some_and(char::is_uppercase) {
            return Self::AsGiven;
        }
        let rest: Vec<char> = letters.collect();
        if rest.iter().all(|c| c.is_lowercase()) {
            Self::Title
        } else if rest.iter().all(|c| c.is_uppercase()) {
            Self::Upper
        } else {
            Self::AsGiven
        }
    }

    fn apply(self, suggestion: &str) -> String {
        match self {
            Self::Upper => suggestion.to_uppercase(),
            Self::Title => {
                let mut chars = suggestion.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
            Self::AsGiven => suggestion.to_owned(),
        }
    }
}
