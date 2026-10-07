//! Writing one setting into a `settings.toml`, keeping everything else in
//! it as it was: comments, blank lines, key order and formatting.

use std::{
    fs,
    io::{self, Write as _},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use toml_edit::{Document, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::USER_SETTINGS_TEMPLATE;

/// Sets the boolean at `key`, a path of table names then the key's name
/// (`["vim", "enabled"]`), in the settings file at `path`.
///
/// An existing key keeps its place and its comments: only its value's text
/// is replaced. A missing key is added at the end of its table, and a
/// missing table at the end of the file. Every byte outside the change is
/// kept, line endings, a byte order mark and a missing final newline
/// included, and what's added uses the file's line endings. A missing file,
/// and its directory, is created from [`USER_SETTINGS_TEMPLATE`] first, as
/// is an empty one (a byte order mark aside), which has nothing to keep.
/// A file that doesn't parse as TOML, or where a name on the path isn't a
/// table (`vim = 3`), is an error and is left untouched. So is one the key
/// has to be added to, if toml_edit would write it back with more changed
/// than that: it gathers a dotted table's keys (`a.b = 1`, `c = 2`,
/// `a.d = 3`). Nothing else in the file is checked.
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
pub fn write_setting(path: &Path, key: &[&str], value: bool) -> io::Result<()> {
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
        let edited = edit(read.as_deref().unwrap_or(""), key, value).map_err(|error| {
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

/// `source` with `key` set to `value`.
fn edit(source: &str, key: &[&str], value: bool) -> Result<String, String> {
    // toml_edit drops a byte order mark: set aside, and put back.
    let (mark, source) = match source.strip_prefix('\u{feff}') {
        Some(source) => ("\u{feff}", source),
        None => ("", source),
    };
    let source = if source.is_empty() {
        USER_SETTINGS_TEMPLATE
    } else {
        source
    };
    let parsed = Document::parse(source).map_err(|error| format!("isn't valid TOML: {error}"))?;
    if let Some(span) = existing(parsed.as_table(), key)? {
        let (before, after) = (&source[..span.start], &source[span.end..]);
        return Ok(format!("{mark}{before}{value}{after}"));
    }
    let mut document = parsed.into_mut();
    let base = document.to_string();
    set(&mut document, &base, key, value)?;
    let edited = splice(source, &base, &document.to_string())
        .ok_or("can't take the setting without reformatting the rest of it")?;
    Ok(format!("{mark}{edited}"))
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

/// Sets `key` to `value` in `document`, parsed from `text`, creating the
/// tables on its path.
fn set(document: &mut DocumentMut, text: &str, key: &[&str], value: bool) -> Result<(), String> {
    let (name, tables) = key.split_last().expect("a setting has a name");
    // A new table goes after every other, taking the comments that end the
    // file before it, so the text before it is unchanged.
    let position = last_position(document.as_table()).map_or(0, |last| last + 1);
    let mut prefix = document.trailing().as_str().unwrap_or("").to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        prefix.push('\n');
    }
    if !text.is_empty() && !text.ends_with("\n\n") && !text.ends_with("\n\r\n") {
        prefix.push('\n');
    }
    let mut created = false;
    let mut table: &mut dyn TableLike = document.as_table_mut();
    let mut inline = false;
    for (depth, &part) in tables.iter().enumerate() {
        if table.get(part).is_none() {
            let new = if inline {
                Item::Value(Value::InlineTable(InlineTable::new()))
            } else {
                let mut new = Table::new();
                // Only a table with keys gets a header: the innermost.
                new.set_implicit(true);
                new.set_position(Some(position));
                new.decor_mut().set_prefix(prefix.as_str());
                created = true;
                Item::Table(new)
            };
            table.insert(part, new);
        }
        let item = table.get_mut(part).expect("just found or inserted");
        inline = matches!(item, Item::Value(Value::InlineTable(_)));
        table = item
            .as_table_like_mut()
            .ok_or_else(|| format!("{} isn't a table", tables[..=depth].join(".")))?;
    }
    match table.get_mut(name) {
        Some(Item::Value(old)) => {
            let decor = old.decor().clone();
            *old = Value::from(value);
            *old.decor_mut() = decor;
        }
        Some(_) => return Err(format!("{} is a table", key.join("."))),
        None => {
            table.insert(name, Item::Value(Value::from(value)));
        }
    }
    // The new table's header took them.
    if created {
        document.set_trailing("");
    }
    Ok(())
}

/// The highest position of any table in `table`, itself included.
fn last_position(table: &Table) -> Option<isize> {
    let nested = table.iter().filter_map(|(_, item)| match item {
        Item::Table(inner) => last_position(inner),
        Item::ArrayOfTables(array) => array.iter().filter_map(last_position).max(),
        _ => None,
    });
    nested.chain(table.position()).max()
}

/// `source` with the change from `base`, toml_edit's text for it, to
/// `edited` made to it in place: toml_edit writes `\n` after headers and
/// keys whatever the file used, and ends the file with one, so its text
/// can differ from `source` beyond the change. `None` if it differs in
/// more than that, as when toml_edit gathers a dotted table's keys.
fn splice(source: &str, base: &str, edited: &str) -> Option<String> {
    let (offsets, added_newline) = align(source, base)?;
    let common = |a: &[u8], b: &[u8]| a.iter().zip(b).take_while(|(a, b)| a == b).count();
    let mut start = common(base.as_bytes(), edited.as_bytes());
    while !base.is_char_boundary(start) {
        start -= 1;
    }
    let most = base.len().min(edited.len()) - start;
    let reversed = |text: &str| {
        text.as_bytes()[start..]
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>()
    };
    let mut end = common(&reversed(base), &reversed(edited)).min(most);
    while !base.is_char_boundary(base.len() - end) {
        end -= 1;
    }
    let mut inserted = edited[start..edited.len() - end].to_owned();
    // After the final newline toml_edit added, which the source lacks.
    if added_newline && start == base.len() && !inserted.is_empty() {
        inserted.insert(0, '\n');
    }
    if source.contains("\r\n") {
        inserted = crlf(&inserted);
    }
    let (from, to) = (offsets[start], offsets[base.len() - end]);
    Some(format!("{}{inserted}{}", &source[..from], &source[to..]))
}

/// Where each byte offset of `base` is in `source`, and where its end is,
/// if `base` is `source` with some `\r\n` written `\n` and perhaps a final
/// `\n` added; and whether it was.
fn align(source: &str, base: &str) -> Option<(Vec<usize>, bool)> {
    let source = source.as_bytes();
    let mut offsets = Vec::with_capacity(base.len() + 1);
    let mut at = 0;
    for &byte in base.as_bytes() {
        offsets.push(at);
        if source[at..].starts_with(b"\r\n") && byte == b'\n' {
            at += 2;
        } else if source.get(at) == Some(&byte) {
            at += 1;
        } else if !(byte == b'\n' && at == source.len()) {
            return None;
        }
    }
    offsets.push(at);
    let added_newline = !base.is_empty() && offsets[base.len() - 1] == source.len();
    (at == source.len()).then_some((offsets, added_newline))
}

/// `text` with each `\n` not already after a `\r` written `\r\n`.
fn crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = None;
    for ch in text.chars() {
        if ch == '\n' && previous != Some('\r') {
            out.push('\r');
        }
        out.push(ch);
        previous = Some(ch);
    }
    out
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
type Hook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
thread_local! {
    /// Runs once a new text is synced, before it's put in place, for a test
    /// to change the file then.
    static SYNCED: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt as _, symlink},
        path::Path,
    };

    use super::{SYNCED, write_setting};
    use crate::USER_SETTINGS_TEMPLATE;

    const OFF: &str = "[vim]\nenabled = false\n";
    const ON: &str = "[vim]\nenabled = true\n";

    /// Turns vim on in `file`, with `change` made to it once the first new
    /// text is synced, just before that would be put in place.
    fn write_after(file: &Path, change: impl FnOnce(&Path) + 'static) {
        let mut change = Some(change);
        SYNCED.set(Some(Box::new(move |path: &Path| {
            if let Some(change) = change.take() {
                change(path);
            }
        })));
        let written = write_setting(file, &["vim", "enabled"], true);
        SYNCED.set(None);
        written.unwrap();
    }

    /// A file saved, created, removed or given other permissions since
    /// write_setting read it isn't written over: the setting is made to
    /// what's there now.
    #[test]
    fn a_change_meanwhile_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.toml");

        fs::write(&file, OFF).unwrap();
        write_after(&file, |path| {
            fs::write(path, format!("# Saved.\n{OFF}")).unwrap();
        });
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("# Saved.\n{ON}")
        );

        fs::remove_file(&file).unwrap();
        write_after(&file, |path| fs::write(path, "# Created.\n").unwrap());
        let created = fs::read_to_string(&file).unwrap();
        assert_eq!(created, format!("# Created.\n\n{ON}"));

        fs::write(&file, OFF).unwrap();
        write_after(&file, |path| fs::remove_file(path).unwrap());
        let template = USER_SETTINGS_TEMPLATE.replace("[vim]\n", ON);
        assert_eq!(fs::read_to_string(&file).unwrap(), template);

        fs::write(&file, OFF).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        write_after(&file, |path| {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        });
        assert_eq!(fs::read_to_string(&file).unwrap(), ON);
        assert_eq!(mode(&file), 0o600);

        let left = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(left, 1, "no temporary file is left");
    }

    /// A symlink put in place of the file, or pointed at another, since
    /// write_setting resolved it is followed to the file it names now, and
    /// stays a link, even where that file holds the same text.
    #[test]
    fn a_link_changed_meanwhile_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.toml");
        let (a, b) = (dir.path().join("a.toml"), dir.path().join("b.toml"));

        fs::write(&file, OFF).unwrap();
        fs::write(&a, OFF).unwrap();
        write_after(&file, {
            let a = a.clone();
            move |path| {
                fs::remove_file(path).unwrap();
                symlink(&a, path).unwrap();
            }
        });
        assert_eq!(fs::read_link(&file).unwrap(), a);
        assert_eq!(fs::read_to_string(&a).unwrap(), ON);

        fs::write(&a, OFF).unwrap();
        fs::write(&b, OFF).unwrap();
        write_after(&file, {
            let (file, b) = (file.clone(), b.clone());
            move |_| {
                fs::remove_file(&file).unwrap();
                symlink(&b, &file).unwrap();
            }
        });
        assert_eq!(fs::read_link(&file).unwrap(), b);
        assert_eq!(fs::read_to_string(&a).unwrap(), OFF);
        assert_eq!(fs::read_to_string(&b).unwrap(), ON);

        let left = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(left, 3, "no temporary file is left");
    }
}
