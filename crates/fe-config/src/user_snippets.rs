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
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

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

/// The file of `language`'s snippets in `dir`: `<dir>/<language>.json`.
pub fn language_file(dir: &Path, language: &str) -> PathBuf {
    dir.join(format!("{language}.{LANGUAGE_EXTENSION}"))
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
    (!stem.is_empty() && !stem.starts_with('.')).then(|| Some(stem.to_owned()))
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
                message: "a snippet file is a JSON object of snippets by name".to_owned(),
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
                message: format!("not valid JSON: {error}"),
            });
            return out;
        }
    };
    let keys = top_level_keys(&json);
    let mut names: Vec<&String> = top.keys().collect();
    // In the file's order, which the map may not keep.
    names.sort_by_key(|name| keys.iter().position(|(key, _)| key == *name));
    for name in names {
        let entry = &top[name];
        let at = (keys.iter().rev())
            .find(|(key, _)| key == name)
            .map_or(0, |&(_, at)| at);
        match snippet(name, entry, file_language.as_deref(), path, body_error) {
            Ok(snippet) => out.snippets.push(snippet),
            Err(message) => {
                let (line, column) = line_column(&json, at);
                out.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    file: path.to_owned(),
                    key: Some(name.clone()),
                    line,
                    column,
                    message: format!("snippet {name:?} is dropped: {message}"),
                });
            }
        }
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
        return Err("it isn't an object".to_owned());
    };
    let prefixes = match entry.get("prefix") {
        None | Some(Value::Null) => Vec::new(),
        Some(prefix) => lines(prefix).ok_or("its prefix isn't a string or a list of strings")?,
    };
    let prefixes: Vec<String> = (prefixes.into_iter())
        .filter(|prefix| !prefix.trim().is_empty())
        .collect();
    if prefixes.is_empty() {
        return Err("it has no prefix".to_owned());
    }
    let body = match entry.get("body") {
        None | Some(Value::Null) => return Err("it has no body".to_owned()),
        Some(body) => lines(body)
            .ok_or("its body isn't a string or a list of strings")?
            .join("\n"),
    };
    if body.is_empty() {
        return Err("it has no body".to_owned());
    }
    if let Some(error) = body_error(&body) {
        return Err(format!("its body doesn't parse: {error}"));
    }
    let description = match entry.get("description") {
        None | Some(Value::Null) => None,
        Some(Value::String(description)) => {
            Some(description.clone()).filter(|description| !description.is_empty())
        }
        Some(_) => return Err("its description isn't a string".to_owned()),
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
            Some(_) => return Err("its scope isn't a string of languages".to_owned()),
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
                message: format!("skipped: larger than {MAX_FILE_LEN} bytes"),
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
                        message: "skipped: not UTF-8".to_owned(),
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
        message: format!("can't read it: {error}"),
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
    let mut lines = vec![match language {
        Some(language) => format!("Snippets for {language}. Each has a name, and:"),
        None => "Snippets for any language. Each has a name, and:".to_owned(),
    }];
    lines.extend(
        [
            "- prefix: what's typed to offer it in completions (one, or a list);",
            "- body: the text it inserts (one string, or a list of lines), where",
            "  $1, $2... are tab stops, $0 the final caret, ${1:label} a",
            "  placeholder and ${1|one,two|} a choice;",
            "- description: what the completion menu and Insert Snippet say.",
        ]
        .map(str::to_owned),
    );
    if language.is_none() {
        lines.push(
            "- scope: the languages it's offered in, by comma; every one if none.".to_owned(),
        );
    }
    lines.extend(["For example:", "", "\"Print to console\": {"].map(str::to_owned));
    if language.is_none() {
        lines.push("\t\"scope\": \"javascript,typescript\",".to_owned());
    }
    lines.extend(
        [
            "\t\"prefix\": \"log\",",
            "\t\"body\": [",
            "\t\t\"console.log('$1');\",",
            "\t\t\"$2\"",
            "\t],",
            "\t\"description\": \"Log output to console\"",
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

/// The 1-based line and column, in chars, of byte `at` of `text`.
fn line_column(text: &str, at: usize) -> (usize, usize) {
    let at = (0..=at.min(text.len()))
        .rev()
        .find(|&ix| text.is_char_boundary(ix))
        .unwrap_or(0);
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |ix| ix + 1);
    (line, before[line_start..].chars().count() + 1)
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
        assert_eq!(line_column(json, keys[1].1), (3, 3));
    }
}
