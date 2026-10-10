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

/// Sets the boolean at `key`, a path of table names then the key's name
/// (`["vim", "enabled"]`), in the settings file at `path`.
///
/// An existing key keeps its place and its comments: only its value's text
/// is replaced. A missing key is added at the end of its table (or
/// replaces its commented-out default), and a missing table at the end of
/// the file. Every byte outside the change is
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
/// untouched.
///
/// The new text goes to a temporary file in the same directory, renamed
/// over the old, or linked in place of a missing one, so a crash never
/// leaves the file cut short. A symlink is written through to its target
/// and stays a link. A file changed since it was read (saved, created,
/// removed, given other permissions, or linked elsewhere) isn't written
/// over: the setting is made again to what's there now, up to 8 times
/// before giving up. That's checked just before the rename, so only a
/// change in the instant between would be undone.
///
/// The file is read as it is: another program writing it in place, rather
/// than through a temporary file, could be read half written.
pub fn write_setting(path: &Path, key: &[&str], value: bool, template: &str) -> io::Result<()> {
    assert!(!key.is_empty(), "a setting has a name");
    // Another round each time the file changes under us.
    for _ in 0..8 {
        let target = resolve(path)?;
        let read = match fs::read_to_string(&target) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(dir) = target.parent() {
                    fs::create_dir_all(dir)?;
                }
                None
            }
            Err(error) => return Err(error),
        };
        let edited =
            edit(read.as_deref().unwrap_or(""), key, value, template).map_err(|error| {
                let error = format!("{}: {error}", target.display());
                io::Error::new(io::ErrorKind::InvalidData, error)
            })?;
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

/// `source`, or `template` if it's empty, with `key` set to `value`.
fn edit(source: &str, key: &[&str], value: bool, template: &str) -> Result<String, String> {
    // toml_edit drops a byte order mark: set aside, and put back.
    let (mark, source) = match source.strip_prefix('\u{feff}') {
        Some(source) => ("\u{feff}", source),
        None => ("", source),
    };
    let source = if source.is_empty() { template } else { source };
    let parsed = Document::parse(source).map_err(|error| format!("isn't valid TOML: {error}"))?;
    if let Some(span) = existing(parsed.as_table(), key)? {
        let (before, after) = (&source[..span.start], &source[span.end..]);
        return Ok(format!("{mark}{before}{value}{after}"));
    }
    let (range, text) = insertion(source, parsed.as_table(), key, value)?;
    let (before, after) = (&source[..range.start], &source[range.end..]);
    Ok(format!("{mark}{before}{text}{after}"))
}

/// Where the value at `key` is in the text `root` was parsed from; `None`
/// if the key, or a table on its path, is missing.
fn existing(root: &Table, key: &[&str]) -> Result<Option<Range<usize>>, String> {
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
    match table.get(name) {
        Some(Item::Value(value)) => Ok(value.span()),
        Some(Item::None) | None => Ok(None),
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
    value: bool,
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
            // A table of its own, after everything.
            let mut prefix = String::new();
            if !source.ends_with('\n') {
                prefix.push_str(eol);
            }
            if !source.ends_with("\n\n") && !source.ends_with("\n\r\n") {
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
/// of its own, if a comment in it is the setting's default (`# name =
/// false`): that comment's text, made the setting.
fn commented(
    source: &str,
    root: &Table,
    table: &Table,
    name: &str,
    value: bool,
) -> Option<(Range<usize>, String)> {
    let header = table.span()?;
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
        let Some(word) = ["true", "false"].into_iter().find(|w| rest.starts_with(w)) else {
            continue;
        };
        let after = rest[word.len()..].trim();
        if !after.is_empty() && !after.starts_with('#') {
            continue;
        }
        let hash = line_start + line.len() - line.trim_start().len();
        let word_end = line_start + line.len() - rest.len() + word.len();
        return Some((hash..word_end, format!("{name} = {value}")));
    }
    None
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
