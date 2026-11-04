//! Help writing an ignore file, through each editor's completion and hover
//! providers (`lsp.rs`), beside whatever a language server says about the file.
//! Docker's and Podman's files aren't helped, as they aren't checked
//! ([`crate::ignore_diagnostics`]).
//!
//! - **Completing a path.** On a line that isn't a comment, the text before the
//!   caret names a directory and the start of an entry, past a leading `!` and
//!   `/`: `/` asks, as `ctrl-space` does. The directory's entries are offered,
//!   read off the UI thread, directories with a trailing `/`, hidden ones too,
//!   `.git` not. It's relative to where the patterns are based, as the check
//!   bases them. Accepting one replaces the start of the entry, escaped so the
//!   line matches it. Nothing is offered where the text can't name a directory
//!   to git: a glob, `.`, `..`, an empty name, `.git` or a symlink on the way.
//!   Past [`MAX_ENTRIES`] of them the list is incomplete, so typing more asks
//!   again for what fits it.
//! - **Hovering a pattern.** The pattern's line says how many paths it matches
//!   and lists the first of them, from a [`check_ignore_file`] of the buffer's
//!   text off the UI thread. One walk serves every hover until the buffer's
//!   version, path or last seen file on disk changes, or the file tree does.
//!   A walk that was cut short doesn't claim a count it can't know. With no
//!   watcher (no root), only the first three end what a walk found.
//!
//! The providers are told a buffer and not which view asks, and a view's
//! language can be picked by hand. So a buffer is helped only if all of its open
//! editors show an ignore file: otherwise a hand-picked split would get paths
//! in place of its language's answers, which are never held back or changed.
//!
//! [`check_ignore_file`]: fe_project::check_ignore_file

use std::{
    cell::RefCell,
    collections::HashSet,
    fs,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{Bias, BufferId, BufferSnapshot, DiskStamp, Version};
use fe_editor::completion::{CompletionItem, CompletionList};
use fe_gpui::{EditorView, HoverContent};
use fe_project::IgnoreReport;
use futures::{FutureExt as _, future::Shared};
use gpui::{App, AppContext as _, AsyncApp, Entity, Task, WeakEntity};

use crate::{Workbench, ignore_diagnostics::is_ignore_file};

/// The most entries a completion list holds.
pub(crate) const MAX_ENTRIES: usize = 200;

/// The most matched paths a hover lists.
pub(crate) const MAX_HOVER_PATHS: usize = 10;

/// The character that asks for paths as it's typed.
pub(crate) const TRIGGER: &str = "/";

const BOM: char = '\u{feff}';

/// Which buffers are helped, and the last walk.
#[derive(Default)]
pub(crate) struct IgnoreAssist {
    /// Kept per frame, as a provider's trigger characters are asked for while
    /// the app is being updated, out of reach of the workbench.
    buffers: RefCell<HashSet<BufferId>>,
    walk: RefCell<Option<Walk>>,
}

/// A walk of the project, under way or done: its report is its cache.
struct Walk {
    key: WalkKey,
    cancel: Arc<AtomicBool>,
    report: Shared<Task<Arc<IgnoreReport>>>,
}

/// What a walk's report holds for.
#[derive(Clone, PartialEq)]
struct WalkKey {
    buffer: BufferId,
    version: Version,
    path: PathBuf,
    stamp: Option<DiskStamp>,
}

impl Drop for IgnoreAssist {
    fn drop(&mut self) {
        if let Some(walk) = self.walk.get_mut() {
            walk.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl IgnoreAssist {
    /// Whether `buffer` is helped.
    pub(crate) fn covers(&self, buffer: BufferId) -> bool {
        self.buffers.borrow().contains(&buffer)
    }

    /// Follows the open editors: run each frame.
    pub(crate) fn sync(&self, editors: &[Entity<EditorView>], cx: &App) {
        let mut buffers = self.buffers.borrow_mut();
        buffers.clear();
        let mut others = Vec::new();
        for editor in editors {
            let view = editor.read(cx);
            let id = view.model().buffer().id();
            if is_ignore_file(view) {
                buffers.insert(id);
            } else {
                others.push(id);
            }
        }
        for id in others {
            buffers.remove(&id);
        }
    }

    /// The file tree changed: the next hover walks again. A hover waiting on a
    /// walk under way still gets its answer, which is about as old.
    pub(crate) fn forget_matches(&self) {
        self.walk.borrow_mut().take();
    }

    /// What the pattern at byte `offset` of `buffer` at `version` matches. In
    /// the buffer as an editor shows it when first polled, as the hover is
    /// dropped once the version is no longer shown.
    pub(crate) async fn hover(
        self: Rc<Self>,
        workbench: WeakEntity<Workbench>,
        cx: AsyncApp,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> Option<HoverContent> {
        let request =
            cx.update(|cx| self.hover_request(&workbench, buffer, version, offset, cx))?;
        let report = request.report.await;
        if report.cancelled {
            return None;
        }
        let matched = report.matches.get(request.row)?;
        let paths = matched.paths.clone();
        let base = base_dir(&request.path);
        let shown = cx
            .background_spawn(async move { shown_paths(&base, &paths) })
            .await;
        Some(HoverContent {
            text: describe(matched.total, &shown, report.truncated),
            range: Some(request.range),
        })
    }

    /// What to look up for a hover, if it's on a pattern.
    fn hover_request(
        &self,
        workbench: &WeakEntity<Workbench>,
        buffer: BufferId,
        version: Version,
        offset: usize,
        cx: &App,
    ) -> Option<HoverRequest> {
        let workbench = workbench.upgrade()?;
        let workbench = workbench.read(cx);
        let shown = shown_buffer(workbench, buffer, version, cx)?;
        if shown.snapshot.version() != version {
            return None;
        }
        let text = shown.snapshot.text();
        let offset = text.clip_offset(offset, Bias::Left);
        let row = text.offset_to_point(offset).row;
        let range = pattern_range(shown.snapshot, row)?;
        if !range.contains(&offset) {
            return None;
        }
        let key = WalkKey {
            buffer,
            version,
            path: shown.path.to_owned(),
            stamp: shown.stamp,
        };
        let mut slot = self.walk.borrow_mut();
        let report = match slot.as_ref().filter(|walk| walk.key == key) {
            Some(walk) => walk.report.clone(),
            None => {
                let cancel = Arc::<AtomicBool>::default();
                let check = workbench.ignore_check();
                let (path, stop) = (shown.path.to_owned(), cancel.clone());
                let text = text.clone();
                let report = cx
                    .background_spawn(
                        async move { Arc::new(check(&path, &text.to_string(), &stop)) },
                    )
                    .shared();
                let walk = Walk {
                    key,
                    cancel,
                    report: report.clone(),
                };
                if let Some(old) = slot.replace(walk) {
                    old.cancel.store(true, Ordering::Relaxed);
                }
                report
            }
        };
        Some(HoverRequest {
            row: row as usize,
            range,
            path: shown.path.to_owned(),
            report,
        })
    }
}

/// A hover on a pattern.
struct HoverRequest {
    row: usize,
    /// The pattern's bytes.
    range: Range<usize>,
    /// The ignore file's.
    path: PathBuf,
    /// The walk to wait on.
    report: Shared<Task<Arc<IgnoreReport>>>,
}

/// An editor's view of a buffer asked about.
struct ShownBuffer<'a> {
    snapshot: &'a BufferSnapshot,
    path: &'a Path,
    stamp: Option<DiskStamp>,
}

/// `buffer` as one of `workbench`'s editors shows it now, at `version` or later.
fn shown_buffer<'a>(
    workbench: &Workbench,
    buffer: BufferId,
    version: Version,
    cx: &'a App,
) -> Option<ShownBuffer<'a>> {
    workbench.open_editors(cx).into_iter().find_map(|editor| {
        let view = editor.read(cx);
        let model = view.model();
        let snapshot = model.snapshot();
        let shows = snapshot.id() == buffer
            && snapshot.version() >= version
            && snapshot.oldest_version() <= version;
        shows.then_some(())?;
        Some(ShownBuffer {
            snapshot,
            path: model.path()?,
            stamp: model.buffer().disk_stamp(),
        })
    })
}

/// The pattern on `row`, as git reads the line: `None` for a comment or a blank
/// line.
fn pattern_range(snapshot: &BufferSnapshot, row: u32) -> Option<Range<usize>> {
    let text = snapshot.text();
    let (start, line) = row_text(snapshot, row);
    let line = line.strip_suffix('\r').unwrap_or(&line);
    let pattern = trim_trailing_spaces(line);
    if pattern.starts_with('#') || pattern.trim_end().is_empty() {
        return None;
    }
    debug_assert!(start + pattern.len() <= text.len());
    Some(start..start + pattern.len())
}

/// Where `row` starts, past a byte order mark, and its text from there.
fn row_text(snapshot: &BufferSnapshot, row: u32) -> (usize, String) {
    let text = snapshot.text();
    let start = text.row_start(row);
    let line = text.slice_to_string(start..start + text.row_len(row) as usize);
    match line.strip_prefix(BOM) {
        Some(rest) if row == 0 => (start + BOM.len_utf8(), rest.to_owned()),
        _ => (start, line),
    }
}

/// `line` without its trailing spaces, keeping a backslash-escaped one, as git
/// trims a line.
fn trim_trailing_spaces(line: &str) -> &str {
    let trimmed = line.trim_end_matches(' ');
    let backslashes = trimmed.bytes().rev().take_while(|&b| b == b'\\').count();
    match backslashes % 2 == 1 && trimmed.len() < line.len() {
        true => &line[..trimmed.len() + 1],
        false => trimmed,
    }
}

/// The directory `path`'s patterns are based at, as the check bases them: its
/// own, but for a repository's `info/exclude`, based at the worktree.
fn base_dir(path: &Path) -> PathBuf {
    let ancestor = if path.ends_with(".git/info/exclude") {
        3
    } else {
        1
    };
    match path.ancestors().nth(ancestor) {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_owned(),
        _ => PathBuf::from("."),
    }
}

/// `paths`, those to list, directories with a trailing `/`. Reads the disk.
fn shown_paths(base: &Path, paths: &[PathBuf]) -> Vec<String> {
    (paths.iter().take(MAX_HOVER_PATHS))
        .map(|path| {
            let is_dir = fs::symlink_metadata(base.join(path)).is_ok_and(|meta| meta.is_dir());
            let slash = if is_dir { "/" } else { "" };
            format!("{}{slash}", path.display())
        })
        .collect()
}

/// A hover's text: how many paths matched and those `shown`. `truncated` says
/// the walk didn't see the whole project, so `total` is a lower bound.
fn describe(total: usize, shown: &[String], truncated: bool) -> String {
    let count = match (total, truncated) {
        (0, false) => return "This pattern matches no files".to_owned(),
        (0, true) => return "This pattern matches no files in the part of the project read".into(),
        (1, false) => "This pattern matches 1 file".to_owned(),
        (_, false) => format!("This pattern matches {total} files"),
        (1, true) => "This pattern matches at least 1 file".to_owned(),
        (_, true) => format!("This pattern matches at least {total} files"),
    };
    // Longer than any run of backticks in the paths.
    let longest = (shown.iter().flat_map(|path| path.split(|c| c != '`')))
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let mut text = format!("{count}:\n\n{fence}\n{}\n{fence}", shown.join("\n"));
    if total > shown.len() {
        text.push_str(&format!("\n\nand {} more", total - shown.len()));
    }
    text
}

/// The path being typed on a line.
#[derive(Debug, PartialEq)]
struct TypedPath {
    /// The directory it names, by name, below where the patterns are based.
    dirs: Vec<String>,
    /// The start of the entry being typed, unescaped.
    entry: String,
    /// The bytes of the line it's typed in.
    range: Range<usize>,
}

/// The path typed in `line` (without a byte order mark) up to byte `caret`:
/// `None` if the line is a comment, or that doesn't name a directory to git.
fn typed_path(line: &str, caret: usize) -> Option<TypedPath> {
    if line.starts_with('#') {
        return None;
    }
    let before = line.get(..caret)?;
    let negated = before.strip_prefix('!').unwrap_or(before);
    let name = negated.strip_prefix('/').unwrap_or(negated);
    let head = before.len() - name.len();

    let mut dirs = Vec::new();
    let mut entry = String::new();
    let mut entry_start = 0;
    let mut chars = name.char_indices();
    while let Some((ix, c)) = chars.next() {
        match c {
            '\\' => entry.push(chars.next()?.1),
            '*' | '?' | '[' => return None,
            '/' => {
                if matches!(entry.as_str(), "" | "." | ".." | ".git") {
                    return None;
                }
                dirs.push(std::mem::take(&mut entry));
                entry_start = ix + 1;
            }
            c => entry.push(c),
        }
    }
    Some(TypedPath {
        dirs,
        entry,
        range: head + entry_start..caret,
    })
}

/// `name` as a pattern matches it literally. Starting the line, `at_line_start`,
/// a `#` or `!` would be taken as a comment or a negation.
fn escaped(name: &str, at_line_start: bool) -> String {
    let mut text = String::with_capacity(name.len());
    if at_line_start && name.starts_with(['#', '!']) {
        text.push('\\');
    }
    let kept = name.trim_end_matches(' ').len();
    for (ix, c) in name.char_indices() {
        if matches!(c, '\\' | '*' | '?' | '[') || ix >= kept {
            text.push('\\');
        }
        text.push(c);
    }
    text
}

/// Whether `typed`'s chars are in `name` in order, case aside, as the menu
/// matches what it's narrowed by.
fn in_order(typed: &str, name: &str) -> bool {
    let mut name = name.chars().flat_map(char::to_lowercase);
    (typed.chars().flat_map(char::to_lowercase)).all(|c| name.any(|n| n == c))
}

/// An entry of a directory: its name, and whether it's a directory.
type Entry = (String, bool);

/// The entries of `typed`'s directory below `base` that fit it, sorted, and
/// whether more did than are kept. Reads the disk. As in the check's walk,
/// symlinks aren't followed: git doesn't look through them.
fn listed_entries(base: &Path, typed: &TypedPath) -> (Vec<Entry>, bool) {
    let mut dir = base.to_owned();
    for name in &typed.dirs {
        dir.push(name);
        if !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
            return (Vec::new(), false);
        }
    }
    let Ok(read) = fs::read_dir(&dir) else {
        return (Vec::new(), false);
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|entry| {
            // A name that isn't text, or spans lines, can't go in the line.
            let name = entry.file_name().into_string().ok()?;
            let fits =
                name != ".git" && !name.contains(['\n', '\r']) && in_order(&typed.entry, &name);
            fits.then_some(())?;
            Some((name, entry.file_type().ok()?.is_dir()))
        })
        .collect();
    entries.sort();
    let more = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    (entries, more)
}

/// The item for `entry`, replacing `range`.
fn entry_item((name, is_dir): Entry, range: Range<usize>, at_line_start: bool) -> CompletionItem {
    let slash = if is_dir { "/" } else { "" };
    CompletionItem {
        label: format!("{name}{slash}"),
        kind: Some(if is_dir { "folder" } else { "file" }.to_owned()),
        edit: (range, format!("{}{slash}", escaped(&name, at_line_start))),
        ..CompletionItem::default()
    }
}

/// What to complete at byte `offset` of `buffer` at `version`, in that version's
/// offsets, as an editor shows it now: none if an edit since reached before
/// `offset`, as typing at the caret doesn't.
fn completion_request(
    workbench: &WeakEntity<Workbench>,
    buffer: BufferId,
    version: Version,
    offset: usize,
    cx: &App,
) -> Option<(PathBuf, TypedPath, usize)> {
    let workbench = workbench.upgrade()?;
    let shown = shown_buffer(workbench.read(cx), buffer, version, cx)?;
    let snapshot = shown.snapshot;
    let untouched = (snapshot.edits_since(version))
        .flat_map(|batch| batch.iter())
        .all(|edit| edit.old.start >= offset);
    if !untouched {
        return None;
    }
    let text = snapshot.text();
    let offset = text.clip_offset(offset, Bias::Left);
    let (start, line) = row_text(snapshot, text.offset_to_point(offset).row);
    let typed = typed_path(&line, offset.checked_sub(start)?)?;
    Some((base_dir(shown.path), typed, start))
}

/// The paths to offer at byte `offset` of `buffer` at `version`: see
/// [`completion_request`].
pub(crate) async fn path_completions(
    workbench: WeakEntity<Workbench>,
    cx: AsyncApp,
    buffer: BufferId,
    version: Version,
    offset: usize,
) -> Option<CompletionList> {
    let (base, typed, start) =
        cx.update(|cx| completion_request(&workbench, buffer, version, offset, cx))?;
    let (typed, (entries, more)) = cx
        .background_spawn(async move {
            let entries = listed_entries(&base, &typed);
            (typed, entries)
        })
        .await;
    let range = start + typed.range.start..start + typed.range.end;
    let at_line_start = typed.range.start == 0;
    Some(CompletionList {
        items: (entries.into_iter())
            .map(|entry| entry_item(entry, range.clone(), at_line_start))
            .collect(),
        is_incomplete: more,
    })
}

/// What a hover shows of a language server's answer and the pattern's matches.
pub(crate) fn merged_hover(
    server: Option<HoverContent>,
    matches: Option<HoverContent>,
) -> Option<HoverContent> {
    match (server, matches) {
        (Some(server), Some(matches)) => Some(HoverContent {
            text: format!("{}\n\n{}", server.text, matches.text),
            range: matches.range,
        }),
        (server, matches) => server.or(matches),
    }
}

#[cfg(test)]
#[path = "ignore_assist_tests.rs"]
mod ignore_assist_tests;
