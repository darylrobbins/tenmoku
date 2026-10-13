//! Replacing the project search's matches: the files of the search panel's
//! results, each matched again with [`Replacer`] in its text as it is now,
//! never with the results' ranges, which may be stale.
//!
//! - **Open files.** A file shown in any editor of any window
//!   ([`Workbench::open_editors`]) gets its replacements as one transaction
//!   of the user's on its buffer, left unsaved, so one undo in an editor of
//!   it (cmd-z, or vim's `u`) takes the whole file's back. It's a
//!   transaction of its own: one the user has open (a vim insert) is set
//!   aside meanwhile, neither joined nor closed. A file open in two windows
//!   (two buffers) gets it in both, and counts once.
//! - **Files not open.** Read off the main thread as [`loader::read`]
//!   reads them (through [`Buffer::from_file`], detecting the encoding), so
//!   their encoding, BOM and line endings are kept; edited as one
//!   transaction on a headless [`EditorModel`]; and written off the main
//!   thread as a save writes ([`write_beneath`]: a new file beside it,
//!   synced, given its permissions, renamed over it), refusing a file
//!   changed since it was read. Known limit: nothing holds that model's
//!   history, so this change can't be undone from the UI.
//! - **Within the project.** The project folder is the one the search
//!   searched ([`Boundary`]: its resolved path, device and inode then), and
//!   every file is opened beneath it a component at a time, following no
//!   symlink and waiting on no FIFO (the folder itself too, from `/`, so
//!   one moved with a folder above it and a symlink put in its place is
//!   refused): to check it first, to read it, and to
//!   write it, through its folder's handle rather than its path, so the
//!   write lands in that folder or nowhere. Right before the rename, the
//!   folder is opened from the project again and must be the same one, and
//!   the file must still be the one read. A folder or file swapped since for
//!   a symlink, a FIFO or another folder is skipped, and its new file
//!   removed.
//! - **Opened meanwhile.** One file at a time, a file a pane is reading
//!   into an editor is waited for, however long it takes, and a file open
//!   by then is replaced in its editor instead of saved, whatever its read
//!   found. One opened while it was being saved is waited for too: an
//!   editor that loaded the text from before the save then gets the
//!   replacement as well, as a transaction on its text as it is. Unedited,
//!   it then holds what the save wrote, so it's marked saved as of that
//!   save (its stamp as written, never looked up again), and one undo makes
//!   it differ from the disk. Edited, it holds neither the file it loaded
//!   nor the one on disk, so it keeps the text and stamp it loaded: saving
//!   it asks before overwriting, as for any file changed on disk.
//! - **Skipped files.** One that can't be read, is binary (has a NUL, open
//!   or not), isn't valid in its encoding, can't hold its replacements in
//!   it, or whose save is refused is left as it was and named in the
//!   outcome; the others are still replaced.
//! - **Matches.** A byte-oriented regex can match inside a char; such a
//!   match is widened to whole chars, and matches that then overlap are
//!   made one replacement of their expansions in order, open file or not.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_buffer::{Buffer, BufferId, DiskStamp, Origin, ProposedChange, SaveSnapshot};
use fe_editor::EditorModel;
use fe_gpui::EditorView;
use fe_project::Replacer;
use gpui::{App, AppContext as _, AsyncApp, Entity};

#[cfg(doc)]
use crate::loader;
use crate::{Workbench, lsp};

/// How often to look again at a pane reading a file being replaced.
const READ_WAIT: Duration = Duration::from_millis(10);

/// A file to replace in: its path, and as the results name it.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) path: PathBuf,
    pub(crate) rel: String,
}

/// The project folder a search searched: the path it was given, that path
/// resolved, and the folder's device and inode, as they were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Boundary {
    root: PathBuf,
    resolved: PathBuf,
    id: (u64, u64),
}

impl Boundary {
    /// `root`'s now, or why it can't be had.
    #[cfg(unix)]
    pub(crate) fn of(root: &Path) -> io::Result<Boundary> {
        use std::os::unix::fs::MetadataExt as _;
        let resolved = std::fs::canonicalize(root)?;
        let metadata = std::fs::metadata(&resolved)?;
        Ok(Boundary {
            root: root.to_owned(),
            resolved,
            id: (metadata.dev(), metadata.ino()),
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn of(_: &Path) -> io::Result<Boundary> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// What a replacement did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// The matches replaced.
    pub(crate) replaced: usize,
    /// The files they were in.
    pub(crate) files: usize,
    /// The files left as they were, as the results name them, with why.
    pub(crate) skipped: Vec<(String, String)>,
}

impl Outcome {
    /// The status line's message, and whether it's an error: some file was
    /// skipped.
    pub(crate) fn message(&self) -> (String, bool) {
        let mut message = if self.replaced == 0 {
            "Replaced nothing".to_owned()
        } else {
            format!(
                "Replaced {} in {}",
                count(self.replaced, "occurrence"),
                count(self.files, "file")
            )
        };
        if !self.skipped.is_empty() {
            let skipped: Vec<_> = (self.skipped.iter())
                .map(|(rel, why)| format!("{rel} ({why})"))
                .collect();
            message += &format!("; skipped {}", skipped.join(", "));
        }
        (message, !self.skipped.is_empty())
    }
}

/// `n` and `noun`, plural unless `n` is 1.
pub(crate) fn count(n: usize, noun: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {noun}{s}")
}

/// Replaces `replacer`'s matches in `targets`, the search's files beneath
/// `boundary`: see the [module](self).
pub(crate) async fn replace(
    boundary: Boundary,
    targets: Vec<Target>,
    replacer: Arc<Replacer>,
    cx: &mut AsyncApp,
) -> Outcome {
    let mut outcome = Outcome::default();
    let boundary = Arc::new(boundary);
    let located = cx
        .background_spawn({
            let boundary = boundary.clone();
            async move {
                (targets.into_iter())
                    .map(|target| {
                        let located = locate(&boundary, &target.path);
                        (target, located)
                    })
                    .collect::<Vec<_>>()
            }
        })
        .await;
    let mut found = Vec::new();
    for (target, located) in located {
        match located {
            Ok((rel, resolved)) => found.push((target, rel, resolved)),
            Err(why) => outcome.skipped.push((target.rel, why)),
        }
    }

    // The open files now; the others are read.
    let closed = cx.update(|cx| {
        let mut closed = Vec::new();
        for (target, rel, resolved) in found {
            let editors = editors_of(&resolved, cx);
            if editors.is_empty() {
                closed.push((target, rel, resolved));
            } else {
                replace_open(&target, &editors, &replacer, &mut outcome, cx);
            }
        }
        closed
    });
    if closed.is_empty() {
        return outcome;
    }
    let read = cx.background_spawn({
        let (boundary, replacer) = (boundary.clone(), replacer.clone());
        async move {
            (closed.into_iter())
                .map(|(target, rel, resolved)| {
                    let read = read_closed(&boundary, &rel, &replacer);
                    (target, rel, resolved, read)
                })
                .collect::<Vec<_>>()
        }
    });

    // One at a time, as editors may open them meanwhile: a file a pane is
    // reading is waited for, and one open by then is replaced in its editor,
    // whatever its read found.
    for (target, rel, resolved, read) in read.await {
        wait_unread(&resolved, cx).await;
        let prepared = cx.update(|cx| {
            let editors = editors_of(&resolved, cx);
            if !editors.is_empty() {
                replace_open(&target, &editors, &replacer, &mut outcome, cx);
                return Ok(None);
            }
            let (buffer, edits) = read?;
            if edits.is_empty() {
                return Ok(None);
            }
            prepare_closed(&resolved, buffer, edits).map(Some)
        });
        let (snapshot, replaced, read_stamp) = match prepared {
            Ok(Some(prepared)) => prepared,
            Ok(None) => continue,
            Err(why) => {
                outcome.skipped.push((target.rel, why));
                continue;
            }
        };
        #[cfg(test)]
        tests::gate(&target.rel).await;
        let saved = cx.background_spawn({
            let boundary = boundary.clone();
            async move { write_beneath(&boundary, &rel, &snapshot, read_stamp) }
        });
        let saved_stamp = match saved.await {
            Ok(stamp) => stamp,
            Err(why) => {
                outcome.skipped.push((target.rel, why));
                continue;
            }
        };
        outcome.replaced += replaced;
        outcome.files += 1;

        // An editor that loaded the file as it was before the save gets
        // the replacement too.
        wait_unread(&resolved, cx).await;
        cx.update(|cx| {
            let stale: Vec<_> = (editors_of(&resolved, cx).into_iter())
                .filter(|editor| editor.read(cx).model().buffer().disk_stamp() == Some(read_stamp))
                .collect();
            if stale.is_empty() {
                return;
            }
            // Unedited, each holds the text the save replaced, so once
            // replaced it holds what's on disk: saved, as of the save.
            let clean: Vec<_> = (stale.iter())
                .map(|editor| !editor.read(cx).model().is_dirty())
                .collect();
            let mut live = Outcome::default();
            replace_open(&target, &stale, &replacer, &mut live, cx);
            if !live.skipped.is_empty() {
                for (rel, why) in live.skipped {
                    outcome
                        .skipped
                        .push((rel, format!("{why}: saved, but not in its editor")));
                }
                return;
            }
            outcome.replaced += live.replaced.saturating_sub(replaced);
            for (editor, clean) in stale.into_iter().zip(clean) {
                if clean {
                    editor.update(cx, |view, cx| {
                        let mut buffer = view.model_mut().buffer_mut();
                        let snapshot = buffer.save_snapshot();
                        buffer.did_save_snapshot(&snapshot, Some(saved_stamp));
                        drop(buffer);
                        cx.notify();
                    });
                }
            }
        });
    }
    outcome
}

/// `path`, one of the search's files, relative to `boundary`'s folder and
/// resolved, if it's still a regular file beneath it; else why it's
/// skipped.
fn locate(boundary: &Boundary, path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let rel = (path.strip_prefix(&boundary.root))
        .map_err(|_| "it isn't in the project".to_owned())?
        .to_owned();
    open_beneath(boundary, &rel)?;
    let resolved = boundary.resolved.join(&rel);
    Ok((rel, resolved))
}

/// The file at `rel` beneath `boundary`'s folder, if that's still the folder
/// the search searched, every folder on the way is one (not a symlink),
/// and the file is a regular file; opened a component at a time, following
/// no symlink and waiting on no FIFO. Else why it's skipped.
fn open_beneath(boundary: &Boundary, rel: &Path) -> Result<File, String> {
    let (folder, name) = split(rel)?;
    open_in(&open_folder_beneath(boundary, folder)?, name)
}

/// `rel`'s folder, relative too, and its name.
fn split(rel: &Path) -> Result<(&Path, &OsStr), String> {
    let name = rel.file_name().ok_or("it isn't in the project")?;
    Ok((rel.parent().unwrap_or(Path::new("")), name))
}

/// The folder at `rel` beneath `boundary`'s folder, if that's still the
/// folder the search searched and every folder on the way is one, opened a
/// component at a time following no symlink; else why the file in it is
/// skipped.
#[cfg(unix)]
fn open_folder_beneath(boundary: &Boundary, rel: &Path) -> Result<File, String> {
    use std::{
        os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
        path::Component,
    };

    const MOVED: &str = "the project folder has moved";
    let flags = SEARCH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // The folder itself, from the file system's root a component at a time
    // too: its resolved path has no symlink, so one on the way now means it
    // or a folder above it was swapped.
    let mut root = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(SEARCH | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| MOVED.to_owned())?;
    for name in boundary.resolved.components() {
        match name {
            Component::RootDir => {}
            Component::Normal(name) => {
                root = open_at(&root, name, flags, 0).map_err(|_| MOVED.to_owned())?;
            }
            _ => return Err(MOVED.into()),
        }
    }
    let metadata = root.metadata().map_err(|_| MOVED.to_owned())?;
    if (metadata.dev(), metadata.ino()) != boundary.id {
        return Err(MOVED.into());
    }
    let mut at = root;
    for name in rel.components() {
        let Component::Normal(name) = name else {
            return Err("it isn't in the project".into());
        };
        at = open_at(&at, name, flags, 0).map_err(|error| match error.raw_os_error() {
            Some(libc::ENOENT) => "it no longer exists".into(),
            Some(libc::ELOOP | libc::ENOTDIR) => "it's no longer in the project".into(),
            _ => format!("can't read it: {error}"),
        })?;
    }
    Ok(at)
}

#[cfg(not(unix))]
fn open_folder_beneath(_: &Boundary, _: &Path) -> Result<File, String> {
    Err("can't check it on this system".into())
}

/// How folders are opened on the way to a file: only to look up what's in
/// them, which needs search permission alone (`O_SEARCH` on macOS, as
/// fe-project's walk opens them, and `O_PATH` on Linux), so a project
/// beneath a folder that can be searched but not read is reached as the
/// search reached it.
#[cfg(target_os = "macos")]
const SEARCH: libc::c_int = libc::O_SEARCH;
#[cfg(any(target_os = "linux", target_os = "android"))]
const SEARCH: libc::c_int = libc::O_PATH;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "linux", target_os = "android"))
))]
const SEARCH: libc::c_int = libc::O_RDONLY;

/// The regular file `name` in `folder`, opened following no symlink and
/// waiting on no FIFO; else why it's skipped.
#[cfg(unix)]
fn open_in(folder: &File, name: &OsStr) -> Result<File, String> {
    let flags = libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let file = open_at(folder, name, flags, 0).map_err(|error| match error.raw_os_error() {
        Some(libc::ENOENT) => "it no longer exists".into(),
        Some(libc::ELOOP | libc::ENOTDIR) => "it's no longer a regular file".into(),
        _ => format!("can't read it: {error}"),
    })?;
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => Ok(file),
        Ok(_) => Err("it's no longer a regular file".into()),
        Err(error) => Err(format!("can't read it: {error}")),
    }
}

#[cfg(not(unix))]
fn open_in(_: &File, _: &OsStr) -> Result<File, String> {
    Err("can't check it on this system".into())
}

/// `openat(2)` of `name` in `folder`.
#[cfg(unix)]
fn open_at(
    folder: &File,
    name: &OsStr,
    flags: libc::c_int,
    mode: libc::c_uint,
) -> io::Result<File> {
    use std::{
        ffi::CString,
        os::unix::{
            ffi::OsStrExt as _,
            io::{AsRawFd as _, FromRawFd as _},
        },
    };
    let name =
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `folder` is an open file and `name` a NUL-terminated string.
    let fd = unsafe { libc::openat(folder.as_raw_fd(), name.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened, and nothing else owns it.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Writes `snapshot` over the file at `rel` beneath `boundary`'s folder, as
/// a save does (a new file beside it, written, synced and given its
/// permissions, then renamed over it), but through the folder opened beneath
/// the project, never by its path: so it writes there or nowhere. The file
/// must still be the one read, with `read_stamp`, and right before the
/// rename the folder must still be the one beneath the project at that path
/// and the file still the one read; else nothing is written and why is
/// returned. Returns the stamp of the file written.
#[cfg(unix)]
fn write_beneath(
    boundary: &Boundary,
    rel: &Path,
    snapshot: &SaveSnapshot,
    read_stamp: DiskStamp,
) -> Result<DiskStamp, String> {
    use std::{
        ffi::OsString,
        io::Write as _,
        os::unix::{
            ffi::OsStrExt as _,
            fs::{MetadataExt as _, PermissionsExt as _},
            io::AsRawFd as _,
        },
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEMPS: AtomicU64 = AtomicU64::new(0);
    let (folder_rel, name) = split(rel)?;
    let folder = open_folder_beneath(boundary, folder_rel)?;
    let as_read = |folder: &File| -> Result<std::fs::Metadata, String> {
        let metadata = open_in(folder, name)?
            .metadata()
            .map_err(|error| format!("can't read it: {error}"))?;
        match DiskStamp::from_metadata(&metadata) {
            Ok(stamp) if stamp == read_stamp => Ok(metadata),
            _ => Err("it changed on disk since it was read".into()),
        }
    };
    let mode = as_read(&folder)?.permissions().mode() & 0o7777;

    let mut temp_name = OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(
        ".replace-{}-{}",
        std::process::id(),
        TEMPS.fetch_add(1, Ordering::Relaxed)
    ));
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let temp = open_at(&folder, &temp_name, flags, 0o600)
        .map_err(|error| format!("not saved: {error}"))?;
    let remove_temp = |why: String| {
        if let Ok(name) = std::ffi::CString::new(temp_name.as_bytes()) {
            // SAFETY: `folder` is an open folder and `name` NUL-terminated.
            unsafe { libc::unlinkat(folder.as_raw_fd(), name.as_ptr(), 0) };
        }
        why
    };
    let written = (|| {
        let mut out = io::BufWriter::new(&temp);
        snapshot.write_to(&mut out)?;
        out.flush()?;
        drop(out);
        temp.set_permissions(std::fs::Permissions::from_mode(mode))?;
        temp.sync_all()?;
        DiskStamp::from_metadata(&temp.metadata()?)
    })()
    .map_err(|error| remove_temp(format!("not saved: {error}")))?;

    #[cfg(test)]
    tests::write_hook(tests::WriteStage::BeforeRename, rel);
    // Right before the rename: still the folder beneath the project, and
    // still the file read.
    let again = open_folder_beneath(boundary, folder_rel).map_err(remove_temp)?;
    let (folder_id, again_id) = (
        folder.metadata().map(|m| (m.dev(), m.ino())),
        again.metadata().map(|m| (m.dev(), m.ino())),
    );
    if folder_id.is_err() || folder_id.ok() != again_id.ok() {
        return Err(remove_temp("it's no longer in the project".into()));
    }
    as_read(&folder).map_err(remove_temp)?;
    let (from, to) = (
        std::ffi::CString::new(temp_name.as_bytes()),
        std::ffi::CString::new(name.as_bytes()),
    );
    let (Ok(from), Ok(to)) = (from, to) else {
        return Err(remove_temp("it isn't in the project".into()));
    };
    let fd = folder.as_raw_fd();
    // SAFETY: `fd` is an open folder and both names NUL-terminated.
    if unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        return Err(remove_temp(format!("not saved: {error}")));
    }
    // The rename on disk too, as far as the folder can be synced: its
    // handle is only for lookups, so it's opened to read for it.
    let read_flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    if let Ok(folder) = open_at(&folder, OsStr::new("."), read_flags, 0) {
        folder.sync_all().ok();
    }
    #[cfg(test)]
    tests::write_hook(tests::WriteStage::AfterRename, rel);
    Ok(written)
}

#[cfg(not(unix))]
fn write_beneath(
    _: &Boundary,
    _: &Path,
    _: &SaveSnapshot,
    _: DiskStamp,
) -> Result<DiskStamp, String> {
    Err("can't check it on this system".into())
}

/// The editors of every window's workbench showing the file at `path`,
/// resolved, as [`lsp::resolve`] resolves their paths.
fn editors_of(path: &Path, cx: &App) -> Vec<Entity<EditorView>> {
    (workbenches(cx).flat_map(|workbench| workbench.open_editors(cx)))
        .filter(|editor| {
            let model = editor.read(cx).model();
            model.path().is_some_and(|shown| {
                shown == path || lsp::resolve(shown).is_some_and(|shown| shown == path)
            })
        })
        .collect()
}

/// Every window's workbench.
fn workbenches(cx: &App) -> impl Iterator<Item = &Workbench> {
    (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .filter_map(|window| window.read(cx).ok())
}

/// Whether a pane of any window is reading the file at `path`, resolved,
/// into an editor, the host's launch read included.
fn being_read(path: &Path, cx: &App) -> bool {
    workbenches(cx).any(|workbench| {
        let panes = workbench.pane_group().panes();
        let reading = (panes.iter())
            .flat_map(|pane| {
                pane.read(cx)
                    .reading()
                    .map(Path::to_owned)
                    .collect::<Vec<_>>()
            })
            .chain(workbench.launch_reading().map(Path::to_owned));
        reading
            .map(|reading| lsp::resolve(&reading).unwrap_or(reading))
            .any(|reading| reading == path)
    })
}

/// Until no pane is reading the file at `path`, resolved, however long
/// that takes.
async fn wait_unread(path: &Path, cx: &mut AsyncApp) {
    while cx.update(|cx| being_read(path, cx)) {
        cx.background_executor().timer(READ_WAIT).await;
    }
}

/// Replaces in the buffers `editors` show, `target`'s, matched in their text
/// now: one transaction each, left unsaved. The file counts once, however
/// many buffers it has (one per window that opened it); none is edited if
/// any is binary.
fn replace_open(
    target: &Target,
    editors: &[Entity<EditorView>],
    replacer: &Replacer,
    outcome: &mut Outcome,
    cx: &mut App,
) {
    let mut buffers: Vec<(BufferId, &Entity<EditorView>)> = Vec::new();
    for editor in editors {
        let buffer = editor.read(cx).model().buffer().id();
        if buffers.iter().all(|(seen, _)| *seen != buffer) {
            buffers.push((buffer, editor));
        }
    }
    let binary = (buffers.iter())
        .any(|(_, editor)| (editor.read(cx).model().buffer().text().to_string()).contains('\0'));
    if binary {
        outcome
            .skipped
            .push((target.rel.clone(), "it's binary".into()));
        return;
    }
    let mut replaced = 0;
    for (_, editor) in buffers {
        let matched = editor.update(cx, |view, cx| {
            let model = view.model_mut();
            model.sync_display();
            let text = model.buffer().text().to_string();
            let matches = replacer.edits(&text);
            let matched = matches.len();
            if matched > 0 {
                let edits = coalesce(&text, matches);
                let mut buffer = model.buffer_mut();
                let mut change = ProposedChange::from_edits(buffer.current(), edits, Origin::User);
                change.accept_all(&mut buffer);
            }
            cx.notify();
            matched
        });
        replaced = replaced.max(matched);
    }
    if replaced > 0 {
        outcome.replaced += replaced;
        outcome.files += 1;
    }
    // The other editors of those buffers show it on the next frame.
    for editor in editors {
        editor.update(cx, |_, cx| cx.notify());
    }
}

/// `edits`, in order, each widened to whole chars of `text`, and those that
/// then overlap, or insert at one place, made one: as [`ProposedChange`]
/// takes them, and as a closed file's buffer gets them too.
fn coalesce(text: &str, edits: Edits) -> Edits {
    let mut merged: Edits = Vec::with_capacity(edits.len());
    for (range, expansion) in edits {
        let start = text.floor_char_boundary(range.start);
        let end = text.ceil_char_boundary(range.end.max(start));
        match merged.last_mut() {
            Some((last, last_text))
                if start < last.end
                    || (last.start == last.end && start == end && start == last.start) =>
            {
                last.end = last.end.max(end);
                last_text.push_str(&expansion);
            }
            _ => merged.push((start..end, expansion)),
        }
    }
    merged
}

/// Reads the file at `rel` beneath `boundary`'s folder as the editor loads
/// files and matches its text, or why it's skipped. Runs off the main
/// thread.
fn read_closed(
    boundary: &Boundary,
    rel: &Path,
    replacer: &Replacer,
) -> Result<(Buffer, Edits), String> {
    let file = open_beneath(boundary, rel)?;
    let buffer = Buffer::from_file(file).map_err(|error| format!("can't read it: {error}"))?;
    let text = buffer.text().to_string();
    if text.contains('\0') {
        return Err("it's binary".into());
    }
    // Unedited, it can be written back only if it read cleanly.
    if let Err(error) = buffer.save_snapshot().write_to(io::sink()) {
        let label = buffer.encoding().label();
        return Err(format!("it isn't valid {label}: {error}"));
    }
    let edits = replacer.edits(&text);
    Ok((buffer, edits))
}

/// `buffer`, read from `path` (resolved), with `edits` made as one
/// transaction on a model of it: what saving it writes, the number of
/// matches, and the stamp of the file as read.
fn prepare_closed(
    path: &Path,
    buffer: Buffer,
    edits: Edits,
) -> Result<(SaveSnapshot, usize, DiskStamp), String> {
    let read_stamp = buffer
        .disk_stamp()
        .ok_or_else(|| "can't tell whether it changed".to_owned())?;
    let replaced = edits.len();
    let label = buffer.encoding().label();
    let edits = coalesce(&buffer.text().to_string(), edits);
    let mut model = EditorModel::new(buffer, Some(path.to_owned()));
    model.buffer_mut().edit(edits, Origin::User);
    // What the save will write, so one that would fail is skipped here.
    let snapshot = model.buffer().save_snapshot();
    if let Err(error) = snapshot.write_to(io::sink()) {
        return Err(format!("it can't be written as {label}: {error}"));
    }
    Ok((snapshot, replaced, read_stamp))
}

/// Replacements: each match's range, and its expansion.
type Edits = Vec<(Range<usize>, String)>;

#[cfg(test)]
#[path = "search_replace_tests.rs"]
mod tests;
