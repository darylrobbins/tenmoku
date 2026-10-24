// SPDX-License-Identifier: LicenseRef-Proprietary

//! Writing one setting into a `settings.toml`, keeping everything else in
//! it as it was: comments, blank lines, key order and formatting.

use std::{
    fs,
    io::{self, Write as _},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use toml_edit::{Document, Item, Table, TableLike, Value};

/// A value [`write_settings`] sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingValue<'a> {
    Bool(bool),
    /// Written as a basic string, escaped: `"Dark \"Modern\""`.
    Str(&'a str),
    Int(i64),
    /// Written with a fractional part or an exponent, `13.0` or `1e300`,
    /// so it reads back as a float. One that isn't finite can't be written.
    Float(f64),
    /// Written as an array of basic strings: `["a", "b"]`.
    StrList(&'a [&'a str]),
}

impl SettingValue<'_> {
    /// The value as TOML.
    fn toml(self) -> String {
        match self {
            SettingValue::Bool(value) => value.to_string(),
            SettingValue::Str(text) => basic_string(text),
            SettingValue::Int(value) => value.to_string(),
            // Debug always writes a point or an exponent, as TOML's floats
            // need: `13.0`, `1e300`.
            SettingValue::Float(value) => format!("{value:?}"),
            SettingValue::StrList(items) => {
                let items: Vec<String> = items.iter().map(|item| basic_string(item)).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

/// What [`write`] does at a key.
#[derive(Clone, Copy)]
enum Change<'a> {
    Set(SettingValue<'a>),
    /// Removes the key.
    Remove,
    /// Removes the key, and the tables that leaves empty.
    Reset,
}

/// Why [`edit`] couldn't make its changes.
#[derive(Debug, PartialEq)]
enum EditError {
    /// The file isn't TOML, or a key's path isn't tables.
    Invalid(String),
    /// Removing the key would take a comment with it.
    DropsComment(String),
}

impl From<String> for EditError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::DropsComment(message) => f.write_str(message),
        }
    }
}

/// Sets the boolean at `key`, a path of table names then the key's name
/// (`["vim", "enabled"]`), in the settings file at `path`. See
/// [`write_settings`].
pub fn write_setting(path: &Path, key: &[&str], value: bool, template: &str) -> io::Result<()> {
    write_settings(path, &[(key, Some(SettingValue::Bool(value)))], template)
}

/// Sets the string at `key` (`["theme", "name"]`) in the settings file at
/// `path`, written as a basic string with its quotes and backslashes
/// escaped. See [`write_settings`].
pub fn write_setting_str(path: &Path, key: &[&str], value: &str, template: &str) -> io::Result<()> {
    write_settings(path, &[(key, Some(SettingValue::Str(value)))], template)
}

/// Makes each of `changes` in turn, in one write, to the settings file at
/// `path`: sets the value at its key, a path of table names then the key's
/// name (`["vim", "enabled"]`), or, for `None`, removes the key if it's
/// there.
///
/// An existing key keeps its place and its comments: only its value's text
/// is replaced. A missing key is added at the end of its table (or
/// replaces its commented-out default, one of the same type), and a missing
/// table at the end of the file. A key removed takes its line, the comment
/// after it included, where nothing else on the line is a setting; one
/// inside braces with its `key = value` and a comma, the comments around it
/// kept. Any other (a dotted key in braces, say) is removed by `toml_edit`,
/// unless that would take a comment with it: then nothing is written, and
/// the error is [`io::ErrorKind::Unsupported`], to edit the file by hand.
/// Every byte outside the change is
/// kept, line endings, a byte order mark and a missing final newline
/// included, and what's added uses the file's line endings. A missing file,
/// and its directory, is created from `template` (the app's commented-out
/// defaults) first, as
/// is an empty one (a byte order mark aside), which has nothing to keep.
/// Where a missing key goes is taken from the file as parsed, so nothing
/// else in it has to be of any form: a table made of dotted keys gets the
/// key beside them, an inline table inside its braces, and a table whose
/// only mention of the key is its commented-out default gets the key in
/// that comment's place. A file that doesn't parse as TOML, or where a name
/// on the path isn't a table (`vim = 3`), is an error and is left
/// untouched. Changes that leave the file as it was (a missing or empty
/// one as the template) write nothing.
///
/// The new text goes to a temporary file in the same directory, renamed
/// over the old, or linked in place of a missing one, so a crash never
/// leaves the file cut short. A symlink is written through to its target
/// and stays a link. A file changed since it was read (saved, created,
/// removed, given other permissions, or linked elsewhere) isn't written
/// over: the changes are made again to what's there now, up to 8 times
/// before giving up. That's checked just before the rename, so only a
/// change in the instant between would be undone.
///
/// The file is read as it is: another program writing it in place, rather
/// than through a temporary file, could be read half written.
pub fn write_settings(
    path: &Path,
    changes: &[(&[&str], Option<SettingValue<'_>>)],
    template: &str,
) -> io::Result<()> {
    let changes: Vec<(&[&str], Change<'_>)> = changes
        .iter()
        .map(|&(key, value)| (key, value.map_or(Change::Remove, Change::Set)))
        .collect();
    write(path, &changes, template)
}

/// Removes each of `keys` (`["editor", "font_size"]`), in one write, from
/// the settings file at `path`, as [`write_settings`] removes a key, and
/// then each table that leaves empty: an inline table's key with it, or a
/// `[header]` with nothing after it but blank lines, if no comment is in
/// it, on its line or on the line before it. A table that still holds other keys or
/// a comment stays. A missing key, or file, writes nothing. A key whose
/// removal would drop a comment is refused, as [`write_settings`] refuses
/// it, and nothing is written.
pub fn reset_settings(path: &Path, keys: &[&[&str]], template: &str) -> io::Result<()> {
    let changes: Vec<(&[&str], Change<'_>)> =
        keys.iter().map(|&key| (key, Change::Reset)).collect();
    write(path, &changes, template)
}

/// [`write_settings`], with each change one of [`Change`].
fn write(path: &Path, changes: &[(&[&str], Change<'_>)], template: &str) -> io::Result<()> {
    for (key, change) in changes {
        assert!(!key.is_empty(), "a setting has a name");
        if let Change::Set(SettingValue::Float(value)) = change
            && !value.is_finite()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: {value} isn't a number TOML can hold", key.join(".")),
            ));
        }
    }
    // Another round each time the file changes under us.
    for _ in 0..8 {
        let target = resolve(path)?;
        let read = match fs::read_to_string(&target) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let edited = edit(read.as_deref().unwrap_or(""), changes, template).map_err(|error| {
            let kind = match error {
                EditError::Invalid(_) => io::ErrorKind::InvalidData,
                EditError::DropsComment(_) => io::ErrorKind::Unsupported,
            };
            io::Error::new(kind, format!("{}: {error}", target.display()))
        })?;
        // Nothing changed, a missing file's template included: nothing to
        // write.
        if edit(read.as_deref().unwrap_or(""), &[], template).as_ref() == Ok(&edited) {
            return Ok(());
        }
        if read.is_none()
            && let Some(dir) = target.parent()
        {
            fs::create_dir_all(dir)?;
        }
        let still = || resolve(path).is_ok_and(|now| now == target);
        if put(&target, edited.as_bytes(), read.as_deref(), still)? {
            return Ok(());
        }
    }
    Err(io::Error::other(format!(
        "{} keeps changing",
        path.display()
    )))
}

/// `source`, or `template` if it's empty, with `changes` made.
fn edit(
    source: &str,
    changes: &[(&[&str], Change<'_>)],
    template: &str,
) -> Result<String, EditError> {
    // toml_edit drops a byte order mark: set aside, and put back.
    let (mark, source) = match source.strip_prefix('\u{feff}') {
        Some(source) => ("\u{feff}", source),
        None => ("", source),
    };
    let mut text = if source.is_empty() { template } else { source }.to_owned();
    for &(key, change) in changes {
        text = match change {
            Change::Set(value) => set(&text, key, &value.toml())?,
            Change::Remove => remove(&text, key)?,
            Change::Reset => reset(&text, key)?,
        };
    }
    Ok(format!("{mark}{text}"))
}

/// `source` with `key` set to `value`, TOML text.
fn set(source: &str, key: &[&str], value: &str) -> Result<String, String> {
    let parsed = Document::parse(source).map_err(|error| format!("isn't valid TOML: {error}"))?;
    if let Some(span) = existing(parsed.as_table(), key)? {
        let (before, after) = (&source[..span.start], &source[span.end..]);
        return Ok(format!("{before}{value}{after}"));
    }
    let (range, text) = insertion(source, parsed.as_table(), key, value)?;
    let (before, after) = (&source[..range.start], &source[range.end..]);
    Ok(format!("{before}{text}{after}"))
}

/// `source` without `key`, or as it is if it has none: without the line
/// it's on, if that's all it takes, else as `toml_edit` removes it.
fn remove(source: &str, key: &[&str]) -> Result<String, EditError> {
    let parsed = Document::parse(source).map_err(|error| format!("isn't valid TOML: {error}"))?;
    let Some((found, value)) = lookup(parsed.as_table(), key)? else {
        return Ok(source.to_owned());
    };
    let mut document: toml_edit::DocumentMut = source
        .parse()
        .map_err(|error| format!("isn't valid TOML: {error}"))?;
    let (name, tables) = key.split_last().expect("a setting has a name");
    let mut table: &mut dyn TableLike = document.as_table_mut();
    for part in tables {
        table = table
            .get_mut(part)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| format!("{} isn't a table", tables.join(".")))?;
    }
    table.remove(name);
    let removed = document.to_string();
    if let (Some(key_span), Some(value_span)) = (found.span(), value.span())
        && let Some(line) = line_of(source, key_span.start, value_span.end)
    {
        let cut = format!("{}{}", &source[..line.start], &source[line.end..]);
        // Braces, or another key on the line, would be cut too.
        let same = |text: &str| text.parse::<toml::Table>().ok();
        if same(&cut).is_some_and(|cut| Some(cut) == same(&removed)) {
            return Ok(cut);
        }
    }
    // In braces, cut just the key, its value and a comma: toml_edit would
    // take the comments before the key with it.
    if let Some(cut) = inline_cut(source, parsed.as_table(), key)
        && cut.parse::<toml::Table>().ok() == removed.parse::<toml::Table>().ok()
    {
        return Ok(cut);
    }
    // toml_edit takes the comments before a key it removes: a dotted key in
    // braces, say, which neither cut can take.
    if comments(&removed).len() < comments(source).len() {
        return Err(EditError::DropsComment(format!(
            "removing {} would drop a comment beside it; edit the file by hand",
            key.join(".")
        )));
    }
    Ok(removed)
}

/// `source` without `key`, a key of an inline table in `root` (parsed from
/// it): without its `key = value` and the comma after it, or else the one
/// before it, the space beside them and every comment kept. `None` if it
/// isn't in an inline table.
fn inline_cut(source: &str, root: &Table, key: &[&str]) -> Option<String> {
    let (name, tables) = key.split_last()?;
    let (parent, tables) = tables.split_last()?;
    let mut table: &dyn TableLike = root;
    for part in tables {
        table = table.get(part)?.as_table_like()?;
    }
    let item = table.get(parent)?;
    let Item::Value(Value::InlineTable(inline)) = item else {
        return None;
    };
    let braces = item.span()?;
    let (found, value) = inline.get_key_value(name)?;
    let (start, end) = (found.span()?.start, value.span()?.end);
    let comments = comments(source);
    let bytes = source.as_bytes();
    // The first byte that isn't space or in a comment after `at`, or before
    // it.
    let next = |mut at: usize, forward: bool| -> Option<usize> {
        loop {
            if !forward {
                at = at.checked_sub(1)?;
            }
            if let Some(comment) = comments.iter().find(|comment| comment.contains(&at)) {
                at = if forward { comment.end } else { comment.start };
                continue;
            }
            if !bytes.get(at)?.is_ascii_whitespace() {
                return Some(at);
            }
            if forward {
                at += 1;
            }
        }
    };
    let blank = |range: Range<usize>| source[range].chars().all(|c| c == ' ' || c == '\t');
    let mut cuts = Vec::new();
    match (next(end, true), next(start, false)) {
        (Some(comma), _) if bytes[comma] == b',' => {
            if source[end..comma].trim().is_empty() {
                // The space after the comma, on its line, goes too.
                let after = source[comma + 1..]
                    .find(|c| c != ' ' && c != '\t')
                    .map_or(source.len(), |at| comma + 1 + at);
                cuts.push(start..after);
            } else {
                cuts.extend([start..end, comma..comma + 1]);
            }
        }
        (_, Some(comma)) if bytes[comma] == b',' => {
            if source[comma + 1..start].trim().is_empty() {
                cuts.push(comma..end);
            } else {
                cuts.extend([comma..comma + 1, start..end]);
            }
        }
        // Its only key: `{ key = value }` is `{}`.
        _ if blank(braces.start + 1..start) && blank(end..braces.end - 1) => {
            cuts.push(braces.start + 1..braces.end - 1);
        }
        _ => cuts.push(start..end),
    }
    cuts.sort_by_key(|cut| cut.start);
    let mut text = String::with_capacity(source.len());
    let mut at = 0;
    for cut in cuts {
        text.push_str(&source[at..cut.start]);
        at = cut.end;
    }
    text.push_str(&source[at..]);
    Some(text)
}

/// Where each comment in `source` is, `#` to the end of its line: a `#`
/// outside any string.
fn comments(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'#' => {
                let end = source[at..].find('\n').map_or(bytes.len(), |end| at + end);
                found.push(at..end);
                at = end;
            }
            quote @ (b'"' | b'\'') => {
                let triple = bytes[at..].starts_with(&[quote; 3]);
                at += if triple { 3 } else { 1 };
                while at < bytes.len() {
                    if quote == b'"' && bytes[at] == b'\\' {
                        at += 2;
                    } else if triple && bytes[at..].starts_with(&[quote; 3]) {
                        // A closing quote or two may be the string's own.
                        while bytes.get(at) == Some(&quote) {
                            at += 1;
                        }
                        break;
                    } else if !triple && (bytes[at] == quote || bytes[at] == b'\n') {
                        at += 1;
                        break;
                    } else {
                        at += 1;
                    }
                }
            }
            _ => at += 1,
        }
    }
    found
}

/// Whether a comment starts in `range` of `source`.
fn comment_in(comments: &[Range<usize>], range: Range<usize>) -> bool {
    comments
        .iter()
        .any(|comment| range.contains(&comment.start))
}

/// Whether the line before the one starting at `line_start` is only a
/// comment.
fn comment_line_before(source: &str, comments: &[Range<usize>], line_start: usize) -> bool {
    if line_start == 0 {
        return false;
    }
    let before = source[..line_start - 1].rfind('\n').map_or(0, |at| at + 1);
    let line = &source[before..line_start];
    let first = before + line.len() - line.trim_start().len();
    comments.iter().any(|comment| comment.start == first)
}

/// `source` without `key`, as [`remove`] takes it, and without each table
/// on its path that leaves empty, innermost first. As it is if it has no
/// `key`.
fn reset(source: &str, key: &[&str]) -> Result<String, EditError> {
    let mut text = remove(source, key)?;
    if text == source {
        return Ok(text);
    }
    for depth in (1..key.len()).rev() {
        match prune(&text, &key[..depth])? {
            Some(pruned) => text = pruned,
            None => break,
        }
    }
    Ok(text)
}

/// `source` without the table at `key` if it's empty: an inline table's
/// key, unless a comment is in it, after it or just before its line, or a
/// `[header]` with only blank lines under it and no comment on
/// or just before its line. `source` as it is if there's no such table, an
/// implicit or dotted one gone with its last key; `None` if the table
/// stays.
fn prune(source: &str, key: &[&str]) -> Result<Option<String>, EditError> {
    let parsed = Document::parse(source).map_err(|error| format!("isn't valid TOML: {error}"))?;
    let (name, tables) = key.split_last().expect("a table has a name");
    let mut table: &dyn TableLike = parsed.as_table();
    for (depth, part) in tables.iter().enumerate() {
        let Some(item) = table.get(part) else {
            return Ok(Some(source.to_owned()));
        };
        table = item
            .as_table_like()
            .ok_or_else(|| format!("{} isn't a table", tables[..=depth].join(".")))?;
    }
    let Some(item) = table.get(name) else {
        return Ok(Some(source.to_owned()));
    };
    let inner = item
        .as_table_like()
        .ok_or_else(|| format!("{} isn't a table", key.join(".")))?;
    if inner.iter().next().is_some() {
        return Ok(None);
    }
    match item {
        Item::Value(Value::InlineTable(_)) => match item.span() {
            Some(braces) if !commented_around(source, braces.clone()) => {
                remove(source, key).map(Some)
            }
            _ => Ok(None),
        },
        Item::Table(inner) if !inner.is_implicit() && !inner.is_dotted() => {
            Ok(header_cut(source, parsed.as_table(), inner))
        }
        _ => Ok(None),
    }
}

/// Whether a comment is inside `braces`, an empty inline table, after it
/// on its line, or on the line before the one it starts on.
fn commented_around(source: &str, braces: Range<usize>) -> bool {
    let comments = comments(source);
    let line_start = source[..braces.start].rfind('\n').map_or(0, |at| at + 1);
    let line_end = source[braces.end..]
        .find('\n')
        .map_or(source.len(), |at| braces.end + at);
    comment_in(&comments, braces.start..line_end)
        || comment_line_before(source, &comments, line_start)
}

/// `source` without `table`'s `[header]` and the blank lines under it,
/// those before it too if it's last, if nothing else is under it and no
/// comment is on its line or the line before.
fn header_cut(source: &str, root: &Table, table: &Table) -> Option<String> {
    let header = table.span()?;
    let line_start = source[..header.end].rfind('\n').map_or(0, |at| at + 1);
    let line_end = source[header.end..]
        .find('\n')
        .map_or(source.len(), |at| header.end + at + 1);
    let comments = comments(source);
    if comment_in(&comments, header.start..line_end)
        || comment_line_before(source, &comments, line_start)
    {
        return None;
    }
    let mut starts = Vec::new();
    headers(root, &mut starts);
    let end = starts
        .into_iter()
        .filter(|&at| at > header.start)
        .min()
        .unwrap_or(source.len());
    if !source[line_end.min(end)..end].trim().is_empty() {
        return None;
    }
    // Last in the file: the blank lines before it go too.
    let mut start = line_start;
    if end == source.len() {
        while start > 0 {
            let before = source[..start - 1].rfind('\n').map_or(0, |at| at + 1);
            if !source[before..start].trim().is_empty() {
                break;
            }
            start = before;
        }
    }
    Some(format!("{}{}", &source[..start], &source[end..]))
}

/// The lines a `key = value` from `start` to `end` is on, newline
/// included, if nothing but space, a comma or a comment follows it.
fn line_of(source: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let line_start = source[..start].rfind('\n').map_or(0, |at| at + 1);
    let line_end = source[end..]
        .find('\n')
        .map_or(source.len(), |at| end + at + 1);
    let after = source[end..line_end].trim();
    // The comma after a key in an inline table written over several lines.
    let after = after.strip_prefix(',').map_or(after, str::trim_start);
    (after.is_empty() || after.starts_with('#')).then_some(line_start..line_end)
}

/// `text` as a TOML basic string: quoted, with quotes, backslashes and
/// control characters escaped.
fn basic_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Where the value at `key` is in the text `root` was parsed from; `None`
/// if the key, or a table on its path, is missing.
fn existing(root: &Table, key: &[&str]) -> Result<Option<Range<usize>>, String> {
    Ok(lookup(root, key)?.and_then(|(_, value)| value.span()))
}

/// The key and value at `key` in `root`; `None` if the key, or a table on
/// its path, is missing.
fn lookup<'a>(
    root: &'a Table,
    key: &[&str],
) -> Result<Option<(&'a toml_edit::Key, &'a Value)>, String> {
    let (name, tables) = key.split_last().expect("a setting has a name");
    let mut table: &dyn TableLike = root;
    for (depth, part) in tables.iter().enumerate() {
        let Some(item) = table.get(part) else {
            return Ok(None);
        };
        table = item
            .as_table_like()
            .ok_or_else(|| format!("{} isn't a table", tables[..=depth].join(".")))?;
    }
    match table.get_key_value(name) {
        Some((found, Item::Value(value))) => Ok(Some((found, value))),
        Some((_, Item::None)) | None => Ok(None),
        Some(_) => Err(format!("{} is a table", key.join("."))),
    }
}

/// `path` with every symlink at its end followed: the file a write goes
/// to, which may not exist yet.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_owned();
    // As many links as macOS follows (MAXSYMLINKS).
    for _ in 0..32 {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&path)?;
                path = match path.parent() {
                    Some(dir) => dir.join(link),
                    None => link,
                };
            }
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "too many levels of symbolic links at {}",
        path.display()
    )))
}

/// What the table holding a setting is, in the text it was parsed from.
enum Kind<'a> {
    /// The top of the file, before any header.
    Root,
    /// Under a `[header]`.
    Header(&'a Table),
    /// Named only on the way to a deeper header (`[a.b]` makes `a`).
    Implicit,
    /// Made by dotted keys (`a.b = 1`).
    Dotted(&'a Table),
    /// Inside `{ ... }` at that span, which `tables[relative..]` leads on
    /// from through dotted keys (`{ a.b = 1 }`) if the table is one.
    Inline {
        table: &'a dyn TableLike,
        braces: Range<usize>,
        relative: usize,
        dotted: bool,
    },
}

/// The text to put, and the range of `source` it replaces, to add `key` set
/// to `value`, which `root` (parsed from `source`) lacks.
fn insertion(
    source: &str,
    root: &Table,
    key: &[&str],
    value: &str,
) -> Result<(Range<usize>, String), String> {
    let (name, tables) = key.split_last().expect("a setting has a name");
    let eol = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut kind = Kind::Root;
    let mut table: &dyn TableLike = root;
    // Where the tables below the last header start in `tables`.
    let mut relative = 0;
    // How many of `tables` exist.
    let mut present = 0;
    for (depth, part) in tables.iter().enumerate() {
        let Some(item) = table.get(part) else {
            break;
        };
        present = depth + 1;
        table = item
            .as_table_like()
            .ok_or_else(|| format!("{} isn't a table", tables[..=depth].join(".")))?;
        kind = match item {
            Item::Table(inner) if inner.is_dotted() => {
                if !matches!(kind, Kind::Dotted(_)) {
                    relative = depth;
                }
                Kind::Dotted(inner)
            }
            Item::Table(inner) if inner.is_implicit() => Kind::Implicit,
            Item::Table(inner) => {
                relative = depth + 1;
                Kind::Header(inner)
            }
            Item::Value(Value::InlineTable(inner)) if inner.is_dotted() => {
                // Only inside braces.
                let Kind::Inline {
                    braces, relative, ..
                } = kind
                else {
                    return Err("dotted keys outside of any table".into());
                };
                Kind::Inline {
                    table,
                    braces,
                    relative,
                    dotted: true,
                }
            }
            _ => Kind::Inline {
                table,
                braces: item.span().ok_or("a table without a place")?,
                relative: depth + 1,
                dotted: false,
            },
        };
    }
    let missing = present < tables.len();
    let line = |at: usize, text: &str| after_line(source, at, text, eol);
    let setting = format!("{} = {value}", quoted(name));
    match kind {
        Kind::Inline {
            table: inner,
            braces,
            relative,
            dotted,
        } => {
            let nested = if dotted {
                // Beside the keys it's made of, written the way they are.
                let path = tables[relative..].iter().chain([name]);
                let path = path.map(|part| quoted(part)).collect::<Vec<_>>().join(".");
                format!("{path} = {value}")
            } else {
                tables[present..]
                    .iter()
                    .rev()
                    .fold(setting, |inside, table| {
                        format!("{} = {{ {inside} }}", quoted(table))
                    })
            };
            Ok(match last_end(inner) {
                Some(end) => (end..end, format!(", {nested}")),
                None => (braces.start + 1..braces.start + 1, format!(" {nested} ")),
            })
        }
        Kind::Dotted(inner) => {
            let end = last_end(inner).ok_or("a dotted table without keys")?;
            let path = tables[relative..].iter().chain([name]);
            let path = path.map(|part| quoted(part)).collect::<Vec<_>>().join(".");
            Ok(line(end, &format!("{path} = {value}")))
        }
        Kind::Root if !missing => Ok(match last_end(root) {
            Some(end) => line(end, &setting),
            None => (0..0, format!("{setting}{eol}")),
        }),
        Kind::Header(inner) if !missing => {
            let header = inner.span().ok_or("a header without a place")?;
            let end = last_end(inner);
            if end.is_none()
                && let Some(found) = commented(source, root, inner, name, value)
            {
                return Ok(found);
            }
            Ok(line(end.unwrap_or(header.end), &setting))
        }
        _ => {
            // A table of its own, after everything, set apart from what's
            // before it, if anything is.
            let mut prefix = String::new();
            if !source.is_empty() && !source.ends_with('\n') {
                prefix.push_str(eol);
            }
            if !source.is_empty() && !source.ends_with("\n\n") && !source.ends_with("\n\r\n") {
                prefix.push_str(eol);
            }
            let header = tables
                .iter()
                .map(|part| quoted(part))
                .collect::<Vec<_>>()
                .join(".");
            let text = format!("{prefix}[{header}]{eol}{setting}{eol}");
            Ok((source.len()..source.len(), text))
        }
    }
}

/// `part` as a key: bare if it can be, else quoted and escaped.
fn quoted(part: &str) -> String {
    toml_edit::Key::new(part).to_string()
}

/// `text` as a line after the one `at` is on, as a range and what to put.
fn after_line(source: &str, at: usize, text: &str, eol: &str) -> (Range<usize>, String) {
    match source[at..].find('\n') {
        Some(newline) => {
            let next = at + newline + 1;
            (next..next, format!("{text}{eol}"))
        }
        None => (source.len()..source.len(), format!("{eol}{text}")),
    }
}

/// Where the last value in `table` ends, those of the dotted tables in it
/// included.
fn last_end(table: &dyn TableLike) -> Option<usize> {
    table
        .iter()
        .filter_map(|(_, item)| match item {
            Item::Value(Value::InlineTable(inner)) if inner.is_dotted() => last_end(inner),
            Item::Value(value) => value.span().map(|span| span.end),
            Item::Table(inner) if inner.is_dotted() => last_end(inner),
            _ => None,
        })
        .max()
}

/// The place for `name` set to `value` in `table`, a `[header]` with no keys
/// of its own, if a comment in it is the setting's default, a value of the
/// same type (`# name = false`): that comment's text, made the setting.
fn commented(
    source: &str,
    root: &Table,
    table: &Table,
    name: &str,
    value: &str,
) -> Option<(Range<usize>, String)> {
    let header = table.span()?;
    let kind = value.parse::<Value>().ok()?.type_name();
    let name = quoted(name);
    let name = name.as_str();
    let start = header.end + source[header.end..].find('\n')? + 1;
    let mut starts = Vec::new();
    headers(root, &mut starts);
    let end = starts
        .into_iter()
        .filter(|&at| at > header.start)
        .min()
        .unwrap_or(source.len());
    let mut at = start;
    for line in source.get(start..end)?.split_inclusive('\n') {
        let line_start = at;
        at += line.len();
        let Some(comment) = line.trim_start().strip_prefix('#') else {
            continue;
        };
        let Some(rest) = comment
            .trim_start()
            .strip_prefix(name)
            .and_then(|rest| rest.trim_start().strip_prefix('='))
        else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(len) = default_len(rest, kind) else {
            continue;
        };
        let hash = line_start + line.len() - line.trim_start().len();
        let value_end = line_start + line.len() - rest.len() + len;
        return Some((hash..value_end, format!("{name} = {value}")));
    }
    None
}

/// How long the value of `kind` (a [`Value::type_name`]) that `rest`, the
/// rest of a commented-out `name = value`, starts with is, if nothing but
/// space or a comment follows it on the line.
fn default_len(rest: &str, kind: &str) -> Option<usize> {
    const KEY: &str = "x = ";
    let line = format!("{KEY}{rest}");
    let parsed = Document::parse(line.as_str()).ok()?;
    match parsed.as_table().get("x")? {
        Item::Value(value) if value.type_name() == kind => Some(value.span()?.end - KEY.len()),
        _ => None,
    }
}

/// Where each `[header]` in `table` starts.
fn headers(table: &Table, starts: &mut Vec<usize>) {
    if table.position().is_some()
        && let Some(span) = table.span()
    {
        starts.push(span.start);
    }
    for (_, item) in table {
        match item {
            Item::Table(inner) => headers(inner, starts),
            Item::ArrayOfTables(array) => array.iter().for_each(|inner| headers(inner, starts)),
            _ => {}
        }
    }
}

/// Writes `contents` to `path` through a temporary file beside it, if
/// `still` holds once that's synced: renamed over the file if it still
/// holds `read` and has the permissions it had, which it keeps; or, if it
/// was missing (`read` is `None`), linked in its place if it still is.
/// `Ok(false)` if not, with nothing written.
fn put(
    path: &Path,
    contents: &[u8],
    read: Option<&str>,
    still: impl FnOnce() -> bool,
) -> io::Result<bool> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let permissions = || {
        fs::metadata(path)
            .ok()
            .map(|metadata| metadata.permissions())
    };
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let kept = read.and_then(|_| permissions());
        if let Some(kept) = &kept {
            file.set_permissions(kept.clone())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        #[cfg(test)]
        SYNCED.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook(path);
            }
        });
        // Checked last, after the slow sync, so a change in between is rare.
        if !still() {
            return Ok(false);
        }
        let Some(read) = read else {
            return match fs::hard_link(&temporary, path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
                linked => linked.map(|()| true),
            };
        };
        if fs::read(path).ok().as_deref() != Some(read.as_bytes()) || permissions() != kept {
            return Ok(false);
        }
        fs::rename(&temporary, path).map(|()| true)
    })();
    // Unless it was renamed into place.
    if read.is_none() || !matches!(written, Ok(true)) {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
pub(crate) type Hook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
thread_local! {
    /// Runs once a new text is synced, before it's put in place, for a test
    /// to change the file then.
    pub(crate) static SYNCED: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
}
