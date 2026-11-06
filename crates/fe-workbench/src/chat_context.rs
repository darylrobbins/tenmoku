//! What the chat attaches from the workspace (the AI chat record's § 13),
//! read through its `buffer.read` grant (§ 9): with no such grant, nothing.
//!
//! - [`gather`] makes the automatic chips: the active editor's selection,
//!   and in a trusted root only (§ 10) the active file's text, its
//!   diagnostics and the open editors' paths.
//! - [`ChatPanel::pick_file`] opens quick open to pick a file, which
//!   [`ChatPanel::attach_file`] attaches as an @file: its open buffer's
//!   text, else the file read off the main thread. [`refusal`], [`open_vetted`]
//!   and [`unattachable`] say why a file is never attached, which a toast
//!   says.
//!
//! The same deny list, applied to the file's real path too ([`open_vetted`]),
//! keeps the active file of a secret, or outside the root, out of the
//! automatic chips.

use std::{
    fs::File,
    io::{self, Read as _},
    path::{Component, Path, PathBuf},
};

use fe_buffer::{BufferId, DiskStamp};
use fe_chat::{ContextItem, ContextKind, Rows};
use fe_editor::{EditorModel, diagnostics::Severity};
use fe_gpui::EditorView;
use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Window};
use kiln::command::{Capability, Grants};

use super::ChatPanel;
use crate::{
    Picker, QuickOpen, Workbench,
    quick_open::Pick,
    workbench::{
        Palette,
        toast::{self, Toast},
    },
};

/// The most bytes a file attached may have.
pub(crate) const MAX_FILE: usize = 1 << 20;

/// How far into a file a NUL makes it binary.
const BINARY_PROBE: usize = 8 << 10;

/// The picker's placeholder.
const ATTACH_FILE: &str = "Attach a file…";

/// The automatic context: see the module docs. Empty without `buffer.read`.
/// None comes from the active file unless it's at `vetted`, which [`open_vetted`]
/// passed for its text as it is now, or has no file.
pub(crate) fn gather(
    workbench: &Workbench,
    grants: &Grants,
    vetted: Option<&Path>,
    cx: &App,
) -> Vec<ContextItem> {
    if !grants.allows(&Capability::BufferRead) {
        return Vec::new();
    }
    let (root, trusted) = (workbench.root(), workbench.root_trusted());
    let mut items = Vec::new();
    let active = workbench.active_editor(cx).map(|editor| editor.read(cx));
    let active =
        active.filter(|editor| (editor.model().path()).is_none_or(|path| Some(path) == vetted));
    if let Some(editor) = active {
        let model = editor.model();
        let path = shown_path(root, model.path());
        items.extend(selection(&path, model));
        if trusted {
            items.extend(active_file(&path, model));
            items.extend(diagnostics(&path, model));
        }
    }
    if trusted {
        items.extend(open_editors(workbench, root, cx));
    }
    items
}

/// `path` as the chat names it: from the root, whole if outside it, and
/// `untitled` for a buffer with no file.
fn shown_path(root: Option<&Path>, path: Option<&Path>) -> String {
    let Some(path) = path else {
        return "untitled".to_owned();
    };
    let relative = root.and_then(|root| path.strip_prefix(root).ok());
    relative.unwrap_or(path).display().to_string()
}

/// The primary selection, if it isn't empty nor over [`MAX_FILE`], with
/// its 1-based rows: a selection ending at a row's start doesn't take that
/// row.
fn selection(path: &str, model: &EditorModel) -> Option<ContextItem> {
    let range = model.primary_selection().range;
    if range.is_empty() || range.len() > MAX_FILE {
        return None;
    }
    let text = model.text();
    let (start, end) = (
        text.offset_to_point(range.start),
        text.offset_to_point(range.end),
    );
    let last = if end.column == 0 && end.row > start.row {
        end.row - 1
    } else {
        end.row
    };
    let rows = Rows {
        first: start.row + 1,
        last: last + 1,
    };
    Some(ContextItem {
        kind: ContextKind::Selection {
            path: path.to_owned(),
            rows,
        },
        label: format!("{path}:{rows}"),
        text: text.slice_to_string(range),
    })
}

/// The active file's text, unsaved edits included, unless it's too large
/// or binary.
fn active_file(path: &str, model: &EditorModel) -> Option<ContextItem> {
    let text = attachable_text(model).ok()?;
    Some(ContextItem {
        kind: ContextKind::ActiveFile {
            path: path.to_owned(),
            rows: None,
        },
        label: path.to_owned(),
        text,
    })
}

/// The active file's problems, one a line: severity, 1-based row,
/// message and source. None when it has none.
fn diagnostics(path: &str, model: &EditorModel) -> Option<ContextItem> {
    let entries = model.diagnostics();
    if entries.is_empty() {
        return None;
    }
    let text = model.text();
    let lines: Vec<String> = (entries.iter())
        .map(|entry| {
            let severity = match entry.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Information => "info",
                Severity::Hint => "hint",
            };
            let row = text.offset_to_point(entry.range.start).row + 1;
            let mut line = format!("{path}:{row}: {severity}: {}", entry.message);
            if let Some(source) = &entry.source {
                line.push_str(&format!(" ({source})"));
            }
            line
        })
        .collect();
    let count = lines.len();
    Some(ContextItem {
        kind: ContextKind::Diagnostics,
        label: format!("Problems ({count})"),
        text: lines.join("\n"),
    })
}

/// The open editors' paths, once each, in tab order. None with none.
fn open_editors(workbench: &Workbench, root: Option<&Path>, cx: &App) -> Option<ContextItem> {
    let mut paths: Vec<String> = Vec::new();
    for editor in workbench.open_editors(cx) {
        let Some(path) = editor.read(cx).model().path() else {
            continue;
        };
        let path = shown_path(root, Some(path));
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return None;
    }
    let count = paths.len();
    Some(ContextItem {
        kind: ContextKind::OpenEditors,
        label: format!("Open editors ({count})"),
        text: paths.join("\n"),
    })
}

/// `model`'s buffer's text, unsaved edits included, unless it's over
/// [`MAX_FILE`], which it doesn't copy, or binary.
fn attachable_text(model: &EditorModel) -> Result<String, &'static str> {
    let buffer = model.buffer();
    let len = buffer.text().len();
    if let Some(reason) = unattachable(&[], len) {
        return Err(reason);
    }
    let text = buffer.text().to_string();
    match unattachable(text.as_bytes(), len) {
        Some(reason) => Err(reason),
        None => Ok(text),
    }
}

/// Why the file at `path` is never attached, by its path alone: outside
/// `root` (or with no root), under `.git/`, or named as a secret is:
/// `.env*`, `*.pem`, `*.key` or `id_*`.
pub(crate) fn refusal(root: Option<&Path>, path: &Path) -> Option<&'static str> {
    let relative = root.and_then(|root| path.strip_prefix(root).ok());
    let Some(relative) = relative.filter(|relative| {
        (relative.components()).all(|component| matches!(component, Component::Normal(_)))
    }) else {
        return Some("it's outside the folder");
    };
    if (relative.components()).any(|component| component.as_os_str() == ".git") {
        return Some("it's in .git");
    }
    let name = relative.file_name()?.to_string_lossy();
    let secret = name.starts_with(".env")
        || name.starts_with("id_")
        || name.ends_with(".pem")
        || name.ends_with(".key");
    secret.then_some("it may hold secrets")
}

/// Why a file of `len` bytes, whose first bytes are `start`, is never
/// attached: over [`MAX_FILE`], or binary, a NUL in its first 8 KiB.
pub(crate) fn unattachable(start: &[u8], len: usize) -> Option<&'static str> {
    if len > MAX_FILE {
        return Some("it's over 1 MiB");
    }
    let probe = &start[..start.len().min(BINARY_PROBE)];
    probe.contains(&0).then_some("it's binary")
}

/// What a [`walk`] ends at.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum Walked {
    /// A regular file, opened: what's read from it, or checked of it, is
    /// what the walk reached, whatever's at its path since.
    File(File),
    /// A regular file that can't be opened to read: its device and inode,
    /// as looked up in the folder the walk holds.
    Unreadable(u64, u64),
    Folder,
    /// Nothing there.
    Missing,
}

/// The file at `path`, opened, and where it really is, if both it and that
/// pass [`refusal`] under `root`, which is walked too: so a link can't reach
/// past the deny list. A folder refuses.
pub(crate) fn open_vetted(root: &Path, path: &Path) -> Result<(PathBuf, Walked), String> {
    if let Some(reason) = refusal(Some(root), path) {
        return Err(reason.to_owned());
    }
    let real_root = match walk(root) {
        Ok((real, Walked::Folder)) => real,
        Ok(_) => return Err("the folder isn't there".to_owned()),
        Err(error) => return Err(error.to_string()),
    };
    let (real, walked) = walk(path).map_err(|error| error.to_string())?;
    if let Some(reason) = refusal(Some(&real_root), &real) {
        return Err(reason.to_owned());
    }
    if matches!(walked, Walked::Folder) {
        return Err("it's a folder".to_owned());
    }
    Ok((real, walked))
}

/// Whether a buffer stamped `stamp`, when it was read or saved, holds text
/// from the file at `path`, which must pass [`open_vetted`]: the file the
/// walk opened is the one stamped, or there's none and none was read. Any
/// doubt refuses. `between` runs after the walk, as a test swaps the file
/// then.
pub(crate) fn vet_text(
    root: &Path,
    path: &Path,
    stamp: Option<&DiskStamp>,
    between: impl FnOnce(),
) -> Result<(), String> {
    let (_, walked) = open_vetted(root, path)?;
    between();
    text_of(&walked, stamp)
}

/// Whether `walked` is what a buffer stamped `stamp` read: the same file,
/// by the handle the walk opened, or none.
fn text_of(walked: &Walked, stamp: Option<&DiskStamp>) -> Result<(), String> {
    let ok = match (walked, stamp) {
        (Walked::File(file), Some(stamp)) => (file.metadata().ok())
            .and_then(|meta| DiskStamp::from_metadata(&meta).ok())
            .is_some_and(|now| same_stamped_file(&now, stamp)),
        #[cfg(unix)]
        (Walked::Unreadable(dev, ino), Some(stamp)) => (*dev, *ino) == (stamp.dev, stamp.ino),
        (Walked::Missing, None) => true,
        _ => false,
    };
    ok.then_some(()).ok_or_else(|| NOT_ITS_TEXT.to_owned())
}

/// How many links [`walk`] follows before giving up, as Linux does.
#[cfg(unix)]
const MAX_LINKS: usize = 40;

/// The absolute `path` walked from `/` a component at a time, each opened
/// from the folder before it and checked to be what was looked up, and no
/// link followed but by reading it and walking its target in its place
/// (a dangling one's too): where it really is, and what's there. Past a
/// component that doesn't exist the rest is taken as written. Fails, as
/// what's there can't be known, if a link can't be read, links loop, `..`
/// follows a folder that doesn't exist, something changes under the walk,
/// or it reaches something other than a folder or a regular file.
#[cfg(unix)]
pub(crate) fn walk(path: &Path) -> io::Result<(PathBuf, Walked)> {
    use std::os::unix::fs::MetadataExt as _;

    let changed = || io::Error::other("it changed while it was read");
    // The folders walked, `/` first, beside the path they make.
    let mut folders = vec![open_root()?];
    let mut resolved = PathBuf::from("/");
    // The components still to walk, the next last; a link's target's take
    // its place.
    let owned = |path: &Path| -> Vec<PathBuf> {
        (path.components().rev())
            .map(|component| PathBuf::from(component.as_os_str()))
            .collect()
    };
    let mut rest = owned(path);
    let (mut links, mut missing) = (0, false);
    let mut end = Walked::Folder;
    while let Some(next) = rest.pop() {
        let Some(component) = next.components().next() else {
            continue;
        };
        let name = match component {
            Component::Prefix(_) | Component::RootDir => {
                folders.truncate(1);
                (resolved, missing, end) = (PathBuf::from("/"), false, Walked::Folder);
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir
                if missing || matches!(end, Walked::File(_) | Walked::Unreadable(..)) =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "a link leads through a folder that doesn't exist",
                ));
            }
            Component::ParentDir => {
                if folders.len() > 1 {
                    folders.pop();
                    resolved.pop();
                }
                continue;
            }
            Component::Normal(name) => name.to_owned(),
        };
        if missing {
            resolved.push(&name);
            continue;
        }
        if matches!(end, Walked::File(_) | Walked::Unreadable(..)) {
            return Err(io::Error::from_raw_os_error(libc::ENOTDIR));
        }
        let folder = folders.last().expect("`/` at least");
        let looked = match stat_at(folder, &name) {
            Ok(looked) => looked,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (missing, end) = (true, Walked::Missing);
                resolved.push(&name);
                continue;
            }
            Err(error) => return Err(error),
        };
        let kind = looked.st_mode & libc::S_IFMT;
        if kind == libc::S_IFLNK {
            links += 1;
            if links > MAX_LINKS {
                return Err(io::Error::other("too many links"));
            }
            // A relative target is from the link's folder, where the walk
            // still is.
            rest.extend(owned(&read_link_at(folder, &name)?));
            continue;
        }
        let flags = match kind {
            libc::S_IFDIR => SEARCH | libc::O_DIRECTORY,
            libc::S_IFREG => libc::O_RDONLY | libc::O_NONBLOCK,
            _ => return Err(io::Error::other("it isn't a regular file")),
        };
        let opened = match open_at(folder, &name, flags | libc::O_NOFOLLOW | libc::O_CLOEXEC) {
            Ok(opened) => opened,
            // As looked up in the folder held, which pins it.
            Err(error)
                if kind == libc::S_IFREG
                    && matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) =>
            {
                resolved.push(&name);
                end = Walked::Unreadable(looked.st_dev as u64, looked.st_ino as u64);
                continue;
            }
            Err(_) => return Err(changed()),
        };
        // What was opened is what was looked up, of the same kind.
        let meta = opened.metadata().map_err(|_| changed())?;
        let same = (meta.dev(), meta.ino()) == (looked.st_dev as u64, looked.st_ino as u64)
            && if kind == libc::S_IFDIR {
                meta.is_dir()
            } else {
                meta.is_file()
            };
        if !same {
            return Err(changed());
        }
        resolved.push(&name);
        if kind == libc::S_IFDIR {
            folders.push(opened);
            end = Walked::Folder;
        } else {
            end = Walked::File(opened);
        }
    }
    Ok((resolved, end))
}

/// The absolute `path`'s real path, its links (symlinks and junctions)
/// resolved by the system, and what's there: the file opened, so what's read
/// is what was resolved. Past a component that doesn't exist the rest is
/// taken as written. Fails if it reaches something other than a folder or a
/// regular file.
#[cfg(windows)]
pub(crate) fn walk(path: &Path) -> io::Result<(PathBuf, Walked)> {
    match dunce::canonicalize(path) {
        Ok(real) => {
            let meta = std::fs::metadata(&real)?;
            if meta.is_dir() {
                Ok((real, Walked::Folder))
            } else if meta.is_file() {
                let file = File::open(&real)?;
                Ok((real, Walked::File(file)))
            } else {
                Err(io::Error::other("it isn't a regular file"))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // The deepest folder that exists, resolved, and the rest as
            // written.
            let mut rest = Vec::new();
            let mut at = path;
            loop {
                let Some(parent) = at.parent() else {
                    return Err(error);
                };
                rest.push(at.file_name().unwrap_or_default().to_owned());
                if let Ok(real) = dunce::canonicalize(parent) {
                    if !std::fs::metadata(&real)?.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::NotADirectory,
                            "a file where a folder should be",
                        ));
                    }
                    let mut resolved = real;
                    resolved.extend(rest.iter().rev());
                    return Ok((resolved, Walked::Missing));
                }
                at = parent;
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn walk(_: &Path) -> io::Result<(PathBuf, Walked)> {
    Err(io::Error::other("can't check it on this system"))
}

/// Where the absolute `path` really is, by [`walk`].
pub(crate) fn resolve(path: &Path) -> io::Result<PathBuf> {
    walk(path).map(|(real, _)| real)
}

/// How folders are opened on the way: only to look up what's in them,
/// which needs search permission alone, as `search_replace.rs` opens them.
#[cfg(target_os = "macos")]
const SEARCH: libc::c_int = libc::O_SEARCH;
#[cfg(any(target_os = "linux", target_os = "android"))]
const SEARCH: libc::c_int = libc::O_PATH;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "linux", target_os = "android"))
))]
const SEARCH: libc::c_int = libc::O_RDONLY;

#[cfg(unix)]
fn open_root() -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(SEARCH | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open("/")
}

#[cfg(unix)]
fn c_name(name: &std::ffi::OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt as _;
    std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// `openat(2)` of `name` in `folder`.
#[cfg(unix)]
fn open_at(folder: &File, name: &std::ffi::OsStr, flags: libc::c_int) -> io::Result<File> {
    use std::os::unix::io::{AsRawFd as _, FromRawFd as _};
    let name = c_name(name)?;
    // SAFETY: `folder` is an open file and `name` a NUL-terminated string.
    let fd = unsafe { libc::openat(folder.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened, and nothing else owns it.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// `fstatat(2)` of `name` in `folder`, not following a link.
#[cfg(unix)]
fn stat_at(folder: &File, name: &std::ffi::OsStr) -> io::Result<libc::stat> {
    use std::os::unix::io::AsRawFd as _;
    let name = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: as `open_at`; `stat` is written in full on success.
    let result = unsafe {
        libc::fstatat(
            folder.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstatat` succeeded.
    Ok(unsafe { stat.assume_init() })
}

/// `readlinkat(2)` of `name` in `folder`.
#[cfg(unix)]
fn read_link_at(folder: &File, name: &std::ffi::OsStr) -> io::Result<PathBuf> {
    use std::os::unix::{ffi::OsStringExt as _, io::AsRawFd as _};
    let name = c_name(name)?;
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: as `open_at`; `buffer` has room for `len` bytes.
    let len = unsafe {
        libc::readlinkat(
            folder.as_raw_fd(),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if len < 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(len as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
}

/// Where an @file's text comes from: an open buffer, by its index among
/// those asked about, else the disk.
pub(crate) enum Source {
    Open(usize),
    /// The file's text, or `None` if there's no file.
    Disk(Option<String>),
}

/// Vets the file at `path` and finds its text's source: the first of
/// `open`, the paths of open buffers and their stamps, that's the same
/// file, else the file read from disk, through the handle the vetting
/// opened. A buffer of it whose text isn't from that file refuses.
fn find_source(root: &Path, path: &Path, open: &[OpenBuffer]) -> Result<Source, String> {
    find_source_with(root, path, open, || {})
}

/// [`find_source`], running `between` after the file is vetted and opened,
/// as a test swaps it then.
pub(crate) fn find_source_with(
    root: &Path,
    path: &Path,
    open: &[OpenBuffer],
    between: impl FnOnce(),
) -> Result<Source, String> {
    let (real, walked) = open_vetted(root, path)?;
    between();
    let same =
        |(open, _): &OpenBuffer| open == path || resolve(open).is_ok_and(|open| open == real);
    if let Some(index) = open.iter().position(same) {
        text_of(&walked, open[index].1.as_ref())?;
        return Ok(Source::Open(index));
    }
    let file = match walked {
        Walked::File(file) => file,
        Walked::Unreadable(..) => return Err("it can't be read".to_owned()),
        Walked::Folder | Walked::Missing => return Ok(Source::Disk(None)),
    };
    let mut bytes = Vec::new();
    // One past the limit, to tell a file over it.
    (file.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes)).map_err(|error| error.to_string())?;
    if let Some(reason) = unattachable(&bytes, bytes.len()) {
        return Err(reason.to_owned());
    }
    Ok(Source::Disk(Some(
        String::from_utf8_lossy(&bytes).into_owned(),
    )))
}

/// An open buffer's path, and its stamp from when it was read or saved.
pub(crate) type OpenBuffer = (PathBuf, Option<DiskStamp>);

/// Why a file whose text came from elsewhere is refused.
const NOT_ITS_TEXT: &str = "its text isn't from this file";

/// Why a file that changed while it was vetted and read is refused.
const CHANGED: &str = "it changed while it was read";

/// Whether `a` and `b` are of the same file: its device and inode, where
/// there are such; else its size and time.
fn same_stamped_file(a: &DiskStamp, b: &DiskStamp) -> bool {
    #[cfg(unix)]
    return (a.dev, a.ino) == (b.dev, b.ino);
    #[cfg(not(unix))]
    return a == b;
}

/// The open editors of files in every window, `this` workbench's first:
/// their tabs' and their diff views' working sides.
fn live_file_editors(this: &Workbench, cx: &App) -> Vec<Entity<EditorView>> {
    // This window may be being updated, which leaves it unreadable through
    // its handle: hence `this`, by itself.
    let others = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .filter_map(|window| window.read(cx).ok())
        .filter(|other| !std::ptr::eq(*other, this));
    (std::iter::once(this).chain(others))
        .flat_map(|workbench| workbench.file_editors(cx))
        .collect()
}

/// Toasts that `path` wasn't attached, and why.
fn refused(path: &str, reason: &str, cx: &mut App) {
    toast::show(cx, Toast::warning(format!("Can't attach {path}: {reason}")));
}

impl ChatPanel {
    /// Opens quick open to pick a file to attach, as `@` or Attach asks.
    pub(super) fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let panel = cx.weak_entity();
        let pick: Pick = Box::new(move |path, window, cx| {
            (panel.update(cx, |panel, cx| panel.attach_file(path, window, cx))).ok();
        });
        workbench.update(cx, |workbench, cx| {
            let root = workbench.root().map(Path::to_owned);
            let pane = workbench.pane().downgrade();
            let picker = cx.new(|cx| {
                Picker::new(
                    QuickOpen::new(root, pane),
                    ATTACH_FILE,
                    kiln::ui::ui_theme(cx),
                    cx,
                )
            });
            workbench.dismiss_palette(window, cx);
            let previous = window.focused(cx);
            picker.update(cx, |picker, cx| {
                picker.delegate.set_pick(pick);
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            });
            workbench.show_palette(Palette::QuickOpen, picker, window, cx);
        });
    }

    /// Attaches the file at `path` as an @file, in its place among the
    /// others as it's read, and focuses the composer. Its text is an open
    /// buffer's, unsaved edits included (a tab's or a diff's working
    /// side's in any window, found by its real path), else the file's read
    /// from disk; the vetting, the matching and the reading are off the
    /// main thread. Refused with a toast saying
    /// why: see [`refusal`], [`open_vetted`] and [`unattachable`]. A read that ends
    /// after its chat was replaced lands nowhere.
    pub(crate) fn attach_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.focus_handle(cx);
        window.focus(&focus, cx);
        let Some(workbench) = self.workbench() else {
            return;
        };
        if !self.grants.allows(&Capability::BufferRead) {
            return;
        }
        let workbench = workbench.read(cx);
        let root = workbench.root().map(Path::to_owned);
        let shown = shown_path(root.as_deref(), Some(&path));
        if let Some(reason) = refusal(root.as_deref(), &path) {
            return refused(&shown, reason, cx);
        }
        let Some(root) = root else {
            return;
        };
        // Held, not followed, so the buffer outlives an editor closed
        // meanwhile.
        let (editors, open): (Vec<(Entity<EditorView>, BufferId)>, Vec<OpenBuffer>) =
            (live_file_editors(workbench, cx))
                .into_iter()
                .filter_map(|editor| {
                    let model = editor.read(cx).model();
                    let path = model.path()?.to_owned();
                    let buffer = model.buffer();
                    let (id, stamp) = (buffer.id(), buffer.disk_stamp());
                    drop(buffer);
                    Some(((editor, id), (path, stamp)))
                })
                .unzip();
        let kind = ContextKind::File {
            path: shown.clone(),
            rows: None,
        };
        let view = self.view.downgrade();
        let id = (self.view).update(cx, |view, cx| view.reserve(kind.clone(), shown.clone(), cx));
        let at = path.clone();
        let started = root.clone();
        let found = cx.background_spawn(async move {
            let found = find_source(&root, &at, &open);
            (found, open)
        });
        cx.spawn(async move |panel, cx| {
            let (found, open) = found.await;
            (panel.update(cx, |panel, cx| {
                // Still read under the same root, with `buffer.read`: else
                // it's dropped. (A root change takes the reservation off.)
                let root = panel
                    .workbench()
                    .and_then(|workbench| workbench.read(cx).root().map(Path::to_owned));
                if root.as_ref() != Some(&started) || !panel.grants.allows(&Capability::BufferRead)
                {
                    (view.update(cx, |view, cx| view.fill(id, None, cx))).ok();
                    return;
                }
                let text = match found {
                    Err(reason) => Err(reason),
                    Ok(Source::Open(index)) => {
                        let (editor, id) = &editors[index];
                        let model = editor.read(cx).model();
                        // Still the buffer matched, of the file matched, read
                        // or saved when it was: else it's refused.
                        let (path, stamp) = &open[index];
                        let buffer = model.buffer();
                        let same = buffer.id() == *id
                            && buffer.disk_stamp() == *stamp
                            && model.path() == Some(path.as_path());
                        drop(buffer);
                        if same {
                            attachable_text(model).map_err(str::to_owned)
                        } else {
                            Err(CHANGED.to_owned())
                        }
                    }
                    Ok(Source::Disk(text)) => text.ok_or_else(|| "no such file".to_owned()),
                };
                let item = match text {
                    Ok(text) => Some(ContextItem {
                        kind,
                        label: shown.clone(),
                        text,
                    }),
                    Err(reason) => {
                        refused(&shown, &reason, cx);
                        None
                    }
                };
                (view.update(cx, |view, cx| view.fill(id, item, cx))).ok();
            }))
            .ok();
        })
        .detach();
    }
}
