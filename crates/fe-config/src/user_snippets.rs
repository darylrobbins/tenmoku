//! The user's own snippets, in VS Code's snippet file format, from
//! `<config_dir>/snippets/`:
//!
//! - **`<language>.json`**, such as `rust.json`: every snippet in it is for
//!   that language, named by fastedit's language name (any entry's `scope`
//!   is ignored, as VS Code ignores it).
//! - **`*.code-snippets`**, such as [`GLOBAL_SNIPPETS_FILE`]: each snippet
//!   is for the languages its `scope` lists (`"rust,python"`), or for every
//!   language with none.
//!
//! A file is a JSON object of snippets by name, each
//! `{ "prefix": str | [str], "body": str | [str], "description": str,
//! "scope": str }`, with `//` and `/* */` comments and trailing commas
//! allowed, as VS Code's are. A body given as lines is joined with `\n`. A
//! snippet with no prefix, no body, a body that isn't text or doesn't parse
//! (the host's check, [`parse_snippet_file`]'s `body_error`), or that isn't
//! an object, is dropped with a warning naming the file and the snippet;
//! the rest load. A file that isn't a JSON object loads nothing, with an
//! error.
//!
//! [`DirStamp`] tells whether the directory's files changed since they
//! were read, so a host re-reads them, without a restart, when they have.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use kiln_l10n::{tr, verbatim};
use serde_json::{Map, Value};

use crate::{Diagnostic, Severity};

/// The directory of the user's snippet files, in the config directory.
pub const SNIPPETS_DIR: &str = "snippets";

/// The extension of a file of one language's snippets.
pub const LANGUAGE_EXTENSION: &str = "json";

/// The extension of a file whose snippets name their own languages.
pub const SCOPED_EXTENSION: &str = "code-snippets";

/// The file Configure User Snippets creates for snippets of any language.
pub const GLOBAL_SNIPPETS_FILE: &str = "global.code-snippets";

/// A snippet file larger than this is skipped, with an error.
pub const MAX_FILE_LEN: u64 = 4 << 20;

/// One of the user's snippets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserSnippet {
    /// Its key in the file: the name people see.
    pub name: String,
    /// What's typed to offer it: one or more, none empty.
    pub prefixes: Vec<String>,
    /// In the LSP snippet grammar, lines joined with `\n`.
    pub body: String,
    pub description: Option<String>,
    /// The languages it's offered in, as the file names them: every one if
    /// none.
    pub languages: Vec<String>,
    /// The file it's from.
    pub file: PathBuf,
}

impl UserSnippet {
    /// Whether it's offered in a buffer of `language` (`None`: plain text),
    /// by name, case aside.
    pub fn is_for(&self, language: Option<&str>) -> bool {
        self.languages.is_empty()
            || language.is_some_and(|language| {
                (self.languages.iter()).any(|name| name.eq_ignore_ascii_case(language))
            })
    }
}

/// The snippets read from one file or a directory of them, and the problems
/// found doing it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserSnippets {
    pub snippets: Vec<UserSnippet>,
    pub diagnostics: Vec<Diagnostic>,
}

/// `<config_dir>/snippets`.
pub fn snippets_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(SNIPPETS_DIR)
}

/// The file of `language`'s snippets in `dir`: `<dir>/<language>.json`,
/// always a file of `dir` itself. A language's name is any string (an
/// extension's languages name themselves), so what a file name can't
/// hold, or would read as another directory or a hidden file, is written
/// as `%XX`: a separator, `%`, a control character, one of `<>:"|?*`, and
/// a first `.`. [`file_language`] reads it back.
pub fn language_file(dir: &Path, language: &str) -> PathBuf {
    let mut name = String::with_capacity(language.len() + 5);
    for (at, c) in language.char_indices() {
        let escaped = match c {
            '.' => at == 0,
            '/' | '\\' | '%' | '<' | '>' | ':' | '"' | '|' | '?' | '*' => true,
            c => c.is_control(),
        };
        if escaped {
            let mut bytes = [0; 4];
            for byte in c.encode_utf8(&mut bytes).bytes() {
                name.push('%');
                name.push_str(&hex(byte));
            }
        } else {
            name.push(c);
        }
    }
    if name.is_empty() {
        // An empty name can't be told from `.json`, a hidden file.
        name.push_str("%00");
    }
    name.push('.');
    name.push_str(LANGUAGE_EXTENSION);
    dir.join(name)
}

/// `byte` as two uppercase hex digits.
fn hex(byte: u8) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    [
        DIGITS[usize::from(byte >> 4)],
        DIGITS[usize::from(byte & 15)],
    ]
    .iter()
    .map(|&digit| char::from(digit))
    .collect()
}

/// `stem` with each `%XX` [`language_file`] writes decoded; `None` if that
/// isn't UTF-8.
fn unescape(stem: &str) -> Option<String> {
    let bytes = stem.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let digits = bytes.get(at + 1..at + 3).and_then(|digits| {
            let digits = std::str::from_utf8(digits).ok()?;
            u8::from_str_radix(digits, 16).ok()
        });
        match (bytes[at], digits) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// What kind of snippet file `path` is, by its name: `Some(Some(language))`
/// for a language's, `Some(None)` for one whose snippets name their own,
/// `None` for neither.
pub fn file_language(path: &Path) -> Option<Option<String>> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case(SCOPED_EXTENSION) {
        return Some(None);
    }
    if !extension.eq_ignore_ascii_case(LANGUAGE_EXTENSION) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.is_empty() || stem.starts_with('.') {
        return None;
    }
    Some(Some(unescape(stem)?))
}

/// The snippets in `text`, the file at `path`, scoped by its name
/// ([`file_language`]): a snippet whose body `body_error` finds a problem
/// with is dropped with that problem. Problems are warnings, but for a file
/// that isn't a JSON object, which is an error and gives no snippets.
pub fn parse_snippet_file(
    path: &Path,
    text: &str,
    body_error: &dyn Fn(&str) -> Option<String>,
) -> UserSnippets {
    let mut out = UserSnippets::default();
    let file_language = file_language(path).unwrap_or(None);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let json = strip_jsonc(text);
    if json.trim().is_empty() {
        return out;
    }
    let top: Map<String, Value> = match serde_json::from_str::<Value>(&json) {
        Ok(Value::Object(top)) => top,
        Ok(_) => {
            out.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                file: path.to_owned(),
                key: None,
                line: 1,
                column: 1,
                message: tr!("config-snippets-not-object").to_string(),
            });
            return out;
        }
        Err(error) => {
            out.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                file: path.to_owned(),
                key: None,
                line: error.line().max(1),
                column: error.column().max(1),
                message: tr!("config-snippets-not-json", error = error.to_string()).to_string(),
            });
            return out;
        }
    };
    // Where each key is first written, which orders the snippets as the
    // file does (the map may not), and last, which is the entry that
    // counts, as JSON's last duplicate wins.
    let mut written: HashMap<&str, (usize, usize)> = HashMap::new();
    let keys = top_level_keys(&json);
    for (index, (key, at)) in keys.iter().enumerate() {
        let (_, last) = written.entry(key.as_str()).or_insert((index, *at));
        *last = *at;
    }
    let mut names: Vec<(&String, Option<(usize, usize)>)> = (top.keys())
        .map(|name| (name, written.get(name.as_str()).copied()))
        .collect();
    names.sort_by_key(|(_, written)| written.map(|(index, _)| index));
    let mut dropped: Vec<(usize, Diagnostic)> = Vec::new();
    for (name, written) in names {
        let entry = &top[name];
        match snippet(name, entry, file_language.as_deref(), path, body_error) {
            Ok(snippet) => out.snippets.push(snippet),
            Err(problem) => {
                let quoted = format!("{name:?}");
                let message = tr!("config-snippet-dropped", name = quoted, problem = problem);
                let at = written.map_or(0, |(_, at)| at);
                dropped.push((
                    at,
                    Diagnostic {
                        severity: Severity::Warning,
                        file: path.to_owned(),
                        key: Some(name.clone()),
                        line: 1,
                        column: 1,
                        message: message.to_string(),
                    },
                ));
            }
        }
    }
    let offsets: Vec<usize> = dropped.iter().map(|(at, _)| *at).collect();
    for ((_, mut diagnostic), (line, column)) in
        dropped.into_iter().zip(line_columns(&json, &offsets))
    {
        diagnostic.line = line;
        diagnostic.column = column;
        out.diagnostics.push(diagnostic);
    }
    out
}

/// The snippet `entry` is, under `name` in the file at `path`, or why it's
/// none.
fn snippet(
    name: &str,
    entry: &Value,
    file_language: Option<&str>,
    path: &Path,
    body_error: &dyn Fn(&str) -> Option<String>,
) -> Result<UserSnippet, String> {
    let Value::Object(entry) = entry else {
        return Err(tr!("config-snippet-not-object").to_string());
    };
    let prefixes = match entry.get("prefix") {
        None | Some(Value::Null) => Vec::new(),
        Some(prefix) => {
            lines(prefix).ok_or_else(|| tr!("config-snippet-prefix-not-text").to_string())?
        }
    };
    let prefixes: Vec<String> = (prefixes.into_iter())
        .filter(|prefix| !prefix.trim().is_empty())
        .collect();
    if prefixes.is_empty() {
        return Err(tr!("config-snippet-no-prefix").to_string());
    }
    let body = match entry.get("body") {
        None | Some(Value::Null) => return Err(tr!("config-snippet-no-body").to_string()),
        Some(body) => lines(body)
            .ok_or_else(|| tr!("config-snippet-body-not-text").to_string())?
            .join("\n"),
    };
    if body.is_empty() {
        return Err(tr!("config-snippet-no-body").to_string());
    }
    if let Some(error) = body_error(&body) {
        return Err(tr!("config-snippet-body-unparsed", error = error).to_string());
    }
    let description = match entry.get("description") {
        None | Some(Value::Null) => None,
        Some(Value::String(description)) => {
            Some(description.clone()).filter(|description| !description.is_empty())
        }
        Some(_) => return Err(tr!("config-snippet-description-not-text").to_string()),
    };
    let languages = match file_language {
        Some(language) => vec![language.to_owned()],
        None => match entry.get("scope") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(scope)) => (scope.split(','))
                .map(str::trim)
                .filter(|language| !language.is_empty())
                .map(str::to_owned)
                .collect(),
            Some(_) => return Err(tr!("config-snippet-scope-not-text").to_string()),
        },
    };
    Ok(UserSnippet {
        name: name.to_owned(),
        prefixes,
        body,
        description,
        languages,
        file: path.to_owned(),
    })
}

/// A string as one line, or a list of strings: `None` for anything else.
fn lines(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(line) => Some(vec![line.clone()]),
        Value::Array(lines) => (lines.iter())
            .map(|line| line.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

/// The snippets of every snippet file in `dir` ([`file_language`]), in the
/// order of their names: none, and no problem, if `dir` doesn't exist.
pub fn load_snippets(dir: &Path, body_error: &dyn Fn(&str) -> Option<String>) -> UserSnippets {
    let mut out = UserSnippets::default();
    let files = match snippet_files(dir) {
        Ok(files) => files,
        Err(error) => {
            out.diagnostics.push(io_error(dir, &error));
            return out;
        }
    };
    for (file, meta) in files {
        if meta.len() > MAX_FILE_LEN {
            out.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                file,
                key: None,
                line: 1,
                column: 1,
                message: tr!(
                    "config-snippets-too-large",
                    limit = MAX_FILE_LEN.to_string()
                )
                .to_string(),
            });
            continue;
        }
        let text = match fs::read(&file) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    out.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        file,
                        key: None,
                        line: 1,
                        column: 1,
                        message: tr!("config-snippets-not-utf8").to_string(),
                    });
                    continue;
                }
            },
            // Deleted since it was listed.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.diagnostics.push(io_error(&file, &error));
                continue;
            }
        };
        let parsed = parse_snippet_file(&file, &text, body_error);
        out.snippets.extend(parsed.snippets);
        out.diagnostics.extend(parsed.diagnostics);
    }
    out
}

fn io_error(path: &Path, error: &io::Error) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        file: path.to_owned(),
        key: None,
        line: 1,
        column: 1,
        message: tr!("config-snippets-unreadable", error = error.to_string()).to_string(),
    }
}

/// `dir`'s snippet files, symlinks followed, by name, with their metadata:
/// none if `dir` doesn't exist.
fn snippet_files(dir: &Path) -> io::Result<Vec<(PathBuf, fs::Metadata)>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if file_language(&path).is_none() {
            continue;
        }
        // A dangling link, or one deleted since it was listed.
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_file() {
            files.push((path, meta));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// The snippet files in a directory as they are on disk: equal stamps of
/// one directory mean nothing was created, deleted, written or replaced
/// there in between, as far as the file system's timestamps tell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirStamp(Vec<(PathBuf, FileStamp)>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    /// Device, inode and change time, which a file replaced by a rename
    /// changes.
    #[cfg(unix)]
    unix: (u64, u64, i64, i64),
}

impl FileStamp {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            unix: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
            },
        }
    }
}

impl DirStamp {
    /// `dir`'s snippet files now: none if it doesn't exist or can't be
    /// read.
    pub fn of(dir: &Path) -> Self {
        let files = snippet_files(dir).unwrap_or_default();
        Self(
            (files.into_iter())
                .map(|(path, meta)| (path, FileStamp::of(&meta)))
                .collect(),
        )
    }
}

/// What Configure User Snippets writes to a new snippet file: an empty
/// object with a commented example, for `language`'s file, or with `None`
/// the global one, whose example names its languages.
pub fn example_file(language: Option<&str>) -> String {
    let mut prose = match language {
        Some(language) => tr!("config-snippets-example-language", language = language),
        None => tr!("config-snippets-example-global"),
    }
    .to_string();
    prose.push('\n');
    prose.push_str(&tr!("config-snippets-example-fields"));
    if language.is_none() {
        prose.push('\n');
        prose.push_str(&tr!("config-snippets-example-scope"));
    }
    prose.push('\n');
    prose.push_str(&tr!("config-snippets-example-intro"));
    let prose = kiln_l10n::strip_isolates(&prose).into_owned();
    let mut lines: Vec<String> = prose.split('\n').map(str::to_owned).collect();
    // The example is code, as a snippet file holds it.
    lines.extend(["", verbatim!("\"Print to console\": {")].map(str::to_owned));
    if language.is_none() {
        lines.push(verbatim!("\t\"scope\": \"javascript,typescript\",").to_owned());
    }
    lines.extend(
        [
            verbatim!("\t\"prefix\": \"log\","),
            verbatim!("\t\"body\": ["),
            verbatim!("\t\t\"console.log('$1');\","),
            "\t\t\"$2\"",
            "\t],",
            verbatim!("\t\"description\": \"Log output to console\""),
            "}",
        ]
        .map(str::to_owned),
    );
    let mut text = "{\n".to_owned();
    for line in lines {
        text.push_str("\t//");
        if !line.is_empty() {
            text.push(' ');
            text.push_str(&line);
        }
        text.push('\n');
    }
    text.push_str("}\n");
    text
}

/// `text` with its `//` and `/* */` comments, and any comma before a
/// closing `}` or `]`, turned to spaces, keeping every line where it was.
/// Strings are kept whole.
fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    // A comma written with nothing but space and comments after it yet.
    let mut comma: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match b {
            b'"' => {
                comma = None;
                out.push(b);
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    out.push(c);
                    i += 1;
                    if c == b'\\' && i < bytes.len() {
                        out.push(bytes[i]);
                        i += 1;
                    } else if c == b'"' || c == b'\n' {
                        break;
                    }
                }
            }
            b'/' if next == Some(b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(blank(bytes[i]));
                    i += 1;
                }
            }
            b'/' if next == Some(b'*') => {
                out.extend_from_slice(b"  ");
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    out.push(blank(bytes[i]));
                    i += 1;
                }
                if i < bytes.len() {
                    out.extend_from_slice(b"  ");
                    i += 2;
                }
            }
            b',' => {
                comma = Some(out.len());
                out.push(b);
                i += 1;
            }
            b'}' | b']' => {
                if let Some(at) = comma.take() {
                    out[at] = b' ';
                }
                out.push(b);
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                out.push(b);
                i += 1;
            }
            _ => {
                comma = None;
                out.push(b);
                i += 1;
            }
        }
    }
    // Strings are copied whole, and only ASCII is written in place of
    // comments' bytes, so the text stays UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

/// A comment's byte as it's blanked: line breaks kept, the rest spaces.
fn blank(b: u8) -> u8 {
    match b {
        b'\n' | b'\r' => b,
        _ => b' ',
    }
}

/// The keys of `json`'s top-level object, each with the byte it starts
/// at, in order.
fn top_level_keys(json: &str) -> Vec<(String, usize)> {
    let bytes = json.as_bytes();
    let mut keys = Vec::new();
    let mut depth = 0usize;
    // At depth 1, after the `{` or a `,`: the next string is a key.
    let mut key_next = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let end = (i + 1).min(bytes.len());
                if depth == 1
                    && key_next
                    && let Ok(key) = serde_json::from_str::<String>(&json[start..end])
                {
                    keys.push((key, start));
                }
                key_next = false;
            }
            b'{' | b'[' => {
                depth += 1;
                key_next = depth == 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                key_next = false;
            }
            b',' => key_next = depth == 1,
            _ => {}
        }
        i += 1;
    }
    keys
}

/// The 1-based line and column, in chars, of each byte of `offsets` in
/// `text`, in one pass over it however many there are.
fn line_columns(text: &str, offsets: &[usize]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|&index| offsets[index]);
    let mut out = vec![(1, 1); offsets.len()];
    let (mut line, mut column) = (1, 1);
    let mut chars = text.char_indices().peekable();
    for index in order {
        let at = offsets[index].min(text.len());
        while let Some(&(ix, c)) = chars.peek() {
            if ix >= at {
                break;
            }
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            chars.next();
        }
        out[index] = (line, column);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas_are_blanked_in_place() {
        let text = "{ // a\n \"a\": \"//x,]\", /* b\n */ \"c\": [1, 2,],\n}";
        let json = strip_jsonc(text);
        assert_eq!(json.len(), text.len());
        assert_eq!(json.lines().count(), text.lines().count());
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["a"], "//x,]");
        assert_eq!(value["c"], serde_json::json!([1, 2]));
    }

    #[test]
    fn keys_are_found_where_they_are_written() {
        let json = "{\n  \"one\": {\"inner\": 1},\n  \"t\\\"wo\": [\"x\", {\"y\": 2}]\n}";
        let keys = top_level_keys(json);
        let names: Vec<_> = keys.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(names, ["one", "t\"wo"]);
        assert_eq!(line_columns(json, &[keys[1].1, 0]), [(3, 3), (1, 1)]);
    }

    #[test]
    fn a_large_file_keeps_its_order_and_locations() {
        // Tens of thousands of snippets, one invalid in every ten, on one
        // line and on a line each: linear, where scanning the keys for each
        // name and the text for each problem was quadratic.
        let count = 40_000;
        let entry = |i: usize| match i % 10 {
            0 => format!("\"s{i}\": {{\"prefix\": \"p{i}\"}}"),
            _ => format!("\"s{i}\": {{\"prefix\": \"p{i}\", \"body\": \"b\"}}"),
        };
        let entries: Vec<String> = (0..count).rev().map(entry).collect();
        for separator in [", ", ",\n"] {
            let text = format!("{{{}}}", entries.join(separator));
            let parsed = parse_snippet_file(Path::new("big.code-snippets"), &text, &|_| None);
            assert_eq!(parsed.snippets.len(), count - count / 10);
            assert_eq!(parsed.diagnostics.len(), count / 10);
            let names: Vec<&str> = parsed.snippets.iter().map(|s| s.name.as_str()).collect();
            let expected: Vec<String> = (0..count)
                .rev()
                .filter(|i| i % 10 != 0)
                .map(|i| format!("s{i}"))
                .collect();
            assert!(names.iter().eq(expected.iter()), "in the file's order");
            // Checking every location would scan the text for each.
            for diagnostic in parsed.diagnostics.iter().step_by(199) {
                let name = diagnostic.key.as_deref().unwrap();
                let at = text.find(&format!("\"{name}\"")).unwrap();
                let before = &text[..at];
                let line = before.matches('\n').count() + 1;
                let column = before.len() - before.rfind('\n').map_or(0, |ix| ix + 1) + 1;
                assert_eq!(
                    (diagnostic.line, diagnostic.column),
                    (line, column),
                    "{name}"
                );
            }
            let first: Vec<&str> = (parsed.diagnostics.iter().take(2))
                .map(|d| d.key.as_deref().unwrap())
                .collect();
            assert_eq!(
                first,
                [format!("s{}", count - 10), format!("s{}", count - 20)]
            );
        }
    }

    #[test]
    fn a_duplicate_key_is_located_at_the_entry_that_counts() {
        let text = "{\n\"a\": {\"prefix\": \"x\", \"body\": \"y\"},\n\"a\": {}\n}";
        let parsed = parse_snippet_file(Path::new("d.code-snippets"), text, &|_| None);
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            (parsed.diagnostics[0].line, parsed.diagnostics[0].column),
            (3, 1)
        );
    }

    #[test]
    fn language_files_stay_in_the_directory() {
        let dir = Path::new("/config/snippets");
        for language in [
            "../outside",
            "/etc/passwd",
            "..",
            ".",
            ".hidden",
            "a/b",
            "a\\b",
            "c:d",
            "100%",
            "%2E%2E",
            "tab\there",
        ] {
            let file = language_file(dir, language);
            assert_eq!(file.parent(), Some(dir), "{language:?}: {}", file.display());
            let name = file.file_name().unwrap().to_str().unwrap();
            assert!(!name.starts_with('.'), "{language:?}: {name}");
            assert!(!name.contains(['/', '\\']), "{language:?}: {name}");
            assert_eq!(
                file_language(&file),
                Some(Some(language.to_owned())),
                "{language:?} reads back from {name}"
            );
        }
        // An empty name is a file of its own, never `.json`.
        let empty = language_file(dir, "");
        assert_eq!(empty.parent(), Some(dir));
        assert_ne!(empty.file_name().unwrap(), ".json");
        for plain in ["rust", "c++", "objective-c", "F#", "Déjà vu"] {
            let file = language_file(dir, plain);
            assert_eq!(file, dir.join(format!("{plain}.json")));
            assert_eq!(file_language(&file), Some(Some(plain.to_owned())));
        }
    }
}
