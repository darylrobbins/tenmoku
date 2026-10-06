//! One file's definitions, read and parsed on the calling thread.

use std::{
    fs,
    io::{self, Read},
    ops::Range,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::SystemTime,
};

use fe_buffer::Buffer;
use fe_syntax::{
    NoOutline, OutlineKind, Syntax, language_by_name, language_name_for_path, symbols,
    symbols_supported,
};

/// How much of a file's start is checked for a NUL byte.
const BINARY_SNIFF: usize = 8 * 1024;

/// The definitions of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSymbols {
    /// Relative to the project root.
    pub rel: PathBuf,
    /// The language's name, as [`fe_syntax::Language::name`] gives it.
    pub language: &'static str,
    /// The file's length on disk and its modification time, from the
    /// metadata of the handle that was read, so a watcher can tell an
    /// unchanged file apart.
    pub len: u64,
    pub modified: Option<SystemTime>,
    /// In the outline's pre-order: by start, a parent before its children.
    pub symbols: Vec<Symbol>,
    /// Whether the file had more than [`fe_syntax::MAX_OUTLINE_ITEMS`]
    /// definitions, of which only the first are listed.
    pub truncated: bool,
}

/// One definition.
///
/// Offsets are bytes into the text as the editor loads it, by
/// [`Buffer::from_reader`]: line endings normalised to `\n` and invalid
/// UTF-8 replaced with U+FFFD. So they index what an open editor shows,
/// not the bytes on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: OutlineKind,
    /// The bytes of the name.
    pub name_range: Range<usize>,
    /// The bytes of the whole item.
    pub range: Range<usize>,
    /// The 0-based row where the name starts.
    pub row: u32,
    /// The name of the item this one is nested in, if any. An `impl` gives
    /// its header, as in `impl Display for Foo`.
    pub container: Option<String>,
}

/// Why a file has no [`FileSymbols`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skipped {
    /// Its language, chosen by file name, has no definitions query, or it
    /// has no language.
    Unsupported,
    /// It's over its language's parse limit.
    TooLarge,
    /// Its first 8 KiB hold a NUL byte.
    Binary,
    /// It couldn't be opened or read, or isn't a regular file.
    Unreadable(io::ErrorKind),
    /// Parsing panicked or overran its time cap.
    ParseFailed,
}

/// The language of `rel` if it has definitions, chosen by file name alone.
/// Cheap: it reads nothing and compiles nothing.
pub(crate) fn symbols_language(rel: &Path) -> Option<&'static str> {
    language_name_for_path(rel).filter(|name| symbols_supported(name))
}

/// Reads `root/rel` as the editor's loader does and lists its definitions,
/// parsing it whole on this thread.
///
/// The file is opened without following a symlink or blocking on a FIFO,
/// and must be a regular file. It must also be the file `rel` names inside
/// the root with no symlink on the way: a directory swapped for a symlink
/// after the walk listed `rel` would otherwise lead outside the project, or
/// into an ignored directory. Such a file, like a `rel` that climbs out with
/// `..`, is [`Skipped::Unreadable`] with [`io::ErrorKind::NotFound`].
///
/// Its length is checked against the language's parse limit before anything
/// is read, and no more than the limit is read, so a file that grows
/// meanwhile is still bounded.
pub fn extract(root: &Path, rel: &Path) -> Result<FileSymbols, Skipped> {
    let root = fs::canonicalize(root).map_err(|err| Skipped::Unreadable(err.kind()))?;
    extract_in(&root, rel)
}

/// [`extract`] with `root` already canonical, as a build resolves it once.
pub(crate) fn extract_in(root: &Path, rel: &Path) -> Result<FileSymbols, Skipped> {
    extract_with(root, rel, |syntax| {
        syntax.finish(syntax.parse_job().run());
    })
}

/// [`extract_in`], parsing with `parse`, which leaves the tree unparsed to
/// give up (as [`Syntax::parse_until`] does when out of time): the file is
/// then [`Skipped::ParseFailed`].
pub(crate) fn extract_with(
    root: &Path,
    rel: &Path,
    parse: impl FnOnce(&mut Syntax),
) -> Result<FileSymbols, Skipped> {
    let language = symbols_language(rel)
        .and_then(language_by_name)
        .ok_or(Skipped::Unsupported)?;
    let unreadable = |err: io::Error| Skipped::Unreadable(err.kind());
    let path = root.join(rel);
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(unreadable)?;
    let metadata = file.metadata().map_err(unreadable)?;
    if metadata.is_dir() {
        return Err(Skipped::Unreadable(io::ErrorKind::IsADirectory));
    }
    if !metadata.is_file() {
        return Err(Skipped::Unreadable(io::ErrorKind::InvalidInput));
    }
    if !names_opened(&path, &metadata).map_err(unreadable)? {
        return Err(Skipped::Unreadable(io::ErrorKind::NotFound));
    }
    let limit = language.parse_limit();
    if metadata.len() > limit as u64 {
        return Err(Skipped::TooLarge);
    }
    let buffer = read(file, limit)?;
    let mut syntax = Syntax::new(language, buffer.current());
    parse(&mut syntax);
    let outline = symbols(&syntax).map_err(|err| match err {
        NoOutline::TooLarge => Skipped::TooLarge,
        NoOutline::Unsupported | NoOutline::NotParsed => Skipped::ParseFailed,
    })?;
    let items = &outline.items;
    let symbols = items
        .iter()
        .filter(|item| is_symbol(item.kind))
        .map(|item| Symbol {
            name: item.name.clone(),
            kind: item.kind,
            name_range: item.name_range.clone(),
            range: item.range.clone(),
            row: item.row,
            container: item
                .parent
                .map(|parent| items[parent as usize].name.clone()),
        })
        .collect();
    Ok(FileSymbols {
        rel: rel.to_path_buf(),
        language: language.name(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
        symbols,
        truncated: outline.truncated,
    })
}

/// Whether `path`, under a canonical root, reaches the file opened (whose
/// metadata is `opened`) with no symlink and no `..` on the way. Checked
/// after opening: the open may have followed a directory since swapped for
/// a symlink, and if it's been swapped back, the file `path` names now is
/// another one than was opened.
fn names_opened(path: &Path, opened: &fs::Metadata) -> io::Result<bool> {
    if fs::canonicalize(path)? != path {
        return Ok(false);
    }
    let named = fs::symlink_metadata(path)?;
    Ok(named.dev() == opened.dev() && named.ino() == opened.ino())
}

/// Reads at most `limit` bytes of `file` into a buffer, as the loader
/// does, after checking its start for a NUL byte.
fn read(mut file: fs::File, limit: usize) -> Result<Buffer, Skipped> {
    let unreadable = |err: io::Error| Skipped::Unreadable(err.kind());
    let mut head = Vec::with_capacity(BINARY_SNIFF);
    (&mut file)
        .take(BINARY_SNIFF as u64)
        .read_to_end(&mut head)
        .map_err(unreadable)?;
    if head.contains(&0) {
        return Err(Skipped::Binary);
    }
    // One byte past the limit tells a file that grew past it.
    let rest = (limit + 1).saturating_sub(head.len()) as u64;
    let mut reader = Counted {
        inner: io::Cursor::new(head).chain(file.take(rest)),
        read: 0,
    };
    let buffer = Buffer::from_reader(&mut reader).map_err(unreadable)?;
    if reader.read > limit {
        return Err(Skipped::TooLarge);
    }
    Ok(buffer)
}

/// Whether an outline item of this kind is a definition. An `impl` is a
/// header over its methods, and headings and tables aren't code; their
/// children still count.
pub(crate) fn is_symbol(kind: OutlineKind) -> bool {
    !matches!(
        kind,
        OutlineKind::Impl | OutlineKind::Heading(_) | OutlineKind::Table
    )
}

/// A reader that counts the bytes read through it.
struct Counted<R> {
    inner: R,
    read: usize,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n;
        Ok(n)
    }
}
