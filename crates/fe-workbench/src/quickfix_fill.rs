//! Filling the quickfix and location lists ([`crate::quickfix`]): vim's
//! `:grep`, `:vimgrep` and `:make`, which fill the window's quickfix list,
//! and `:lgrep`, `:lvimgrep` and `:lmake`, which fill the location list of
//! the pane they're typed in ([`fe_vim::QuickfixAction`], which
//! `quickfix_commands.rs` hands here).
//!
//! - `:grep {pattern}`: the rest of the line is the pattern, a regex, over
//!   the project root, by the project search (`fe_project::search`), which
//!   keeps to the files the project tree shows (skipping what .gitignore
//!   ignores) and skips binary files. LazyVim's `grepprg` is `rg --vimgrep`,
//!   which this stands in for, smart case and all: the pattern matches case
//!   by case only if it has an uppercase letter (not one in an escape such
//!   as `\S` or `\p{Lu}`, or in a group's name). One item per matching line,
//!   at its first match. The list's title is `:grep {pattern}`. Neither
//!   rg's options nor paths are read from the line: `:grep -w foo src`
//!   searches the project for `-w foo src`.
//! - `:vimgrep /{pattern}/[g][j] [{glob}…]`: the argument as
//!   [`fe_vim::QuickfixAction::parse_vimgrep`] splits it. `g` lists every
//!   match on a line, not only the first; `j` doesn't jump to the first
//!   item. The globs are relative to the root, as vim's are to its working
//!   directory: `*.md` is the root's own, `**/*.rs` any under it, `src` a
//!   folder's files, and `%` the file of the editor it's typed in. With none,
//!   the whole project. An empty pattern (`//`) is the last search's, E35
//!   with none.
//!
//! **The pattern is the project search's regex (Rust's `regex` syntax), not
//! Vim's**: `\<foo\>`, `\v` and `\(a\|b\)` don't translate, and neither
//! does the last search's pattern for `//`. That's the known divergence.
//!
//! A search runs off the main thread with a cancel flag and the search
//! panel's cap ([`SearchOptions::default`]'s matches); a new one in the
//! window cancels the one running, and so does closing the window or
//! switching folders (its list is dropped). When
//! it ends, its list goes on the stack after the list in use, and unless
//! `j`, the first item is opened as `:cfirst` opens it; with `j` the status
//! line gets the list's summary. Either way the message says when the cap
//! cut it short. No match is `E480: No match: {pattern}` and the lists stay
//! as they were.
//!
//! - `:make`: runs the project's default build task, the one `tasks.run_build`
//!   runs ([`Workbench::run_task`]: a folder that isn't trusted gets the
//!   trust toast, and nothing runs). When its session exits, what its problem
//!   matcher found, the problems the Problems panel lists for its tab, is a
//!   new list titled `:make`, and the first error in it (else its first
//!   item) is opened. With no default build task, the task picker's `No
//!   build tasks` line says so. Arguments are ignored with a note: the task
//!   is the project's, not a shell line, so nothing can take them.
//!
//! An `:l` list goes to the pane typed in, though it's left before the run
//! ends; none if that pane has closed. Items are landed on in the active
//! pane, as `quickfix_commands.rs` lands.
//!
//! Item columns are in chars. A match on a line over 512 bytes reads the line
//! from disk, once, to count them.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read as _, Seek as _, SeekFrom},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_gpui::EditorView;
use fe_project::{
    FileMatches, LineMatch, PathFilter, SearchError, SearchOptions, SearchQuery, open_file_under,
};
use fe_vim::{ExError, QuickfixAction};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, WeakEntity, Window};
use kiln_l10n::{tr, verbatim};
use kiln_shell::ItemHandle as _;

use crate::{
    RootChanged, Workbench,
    quickfix::{QuickfixItem, QuickfixKind, QuickfixList},
    quickfix_commands::{say, say_error},
    task_commands::{default_build, nothing_to_run, read_tasks},
    task_defs::Group,
    task_problems,
    task_run::{RunHeard, RunProblem},
};

/// What `:grep` says with no folder open.
fn no_folder() -> String {
    tr!("workbench-lsp-quickfix-no-folder").into()
}

/// The running search of each window, by its workbench's id: kept apart
/// from the workbench, and dropped (so cancelled) with it.
#[derive(Default)]
struct Searches(HashMap<EntityId, Search>);

impl Global for Searches {}

/// A search filling a list. Dropping it cancels it.
struct Search {
    cancel: Arc<AtomicBool>,
    _task: gpui::Task<()>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A search to fill a list with.
struct Grep {
    /// The pattern, for E480.
    pattern: String,
    /// The include globs, relative to the root as a [`PathFilter`] reads
    /// them.
    globs: Vec<String>,
    /// `g`: every match on a line.
    every: bool,
    /// Not `j`: open the first item.
    jump: bool,
    title: String,
}

/// What a search found: its items in path, row and column order, and
/// whether the cap cut it short. None if it was cancelled.
type Found = Result<Option<(Vec<QuickfixItem>, bool)>, SearchError>;

impl Workbench {
    /// `:grep {args}`, or with `location` `:lgrep`: see the module docs.
    pub(crate) fn grep(
        &mut self,
        args: &str,
        location: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = if location { "lgrep" } else { "grep" };
        let pattern = match args.is_empty() {
            false => args.to_owned(),
            true => match fe_gpui::vim_registers(cx).and_then(|r| r.last_search()) {
                Some((pattern, _)) => pattern.to_owned(),
                None => return say_error(editor, ExError::NoPreviousSearch.to_string(), cx),
            },
        };
        let grep = Grep {
            title: format!(":{name} {pattern}"),
            pattern,
            globs: Vec::new(),
            every: false,
            jump: true,
        };
        self.run_grep(grep, location, editor, window, cx);
    }

    /// `:vimgrep {args}`, or with `location` `:lvimgrep`: see the module
    /// docs.
    pub(crate) fn vimgrep(
        &mut self,
        args: &str,
        location: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (pattern, flags, globs) = match QuickfixAction::parse_vimgrep(args) {
            Ok(parts) => parts,
            Err(error) => return say_error(editor, error.to_string(), cx),
        };
        let pattern = match pattern.is_empty() {
            false => pattern,
            true => match fe_gpui::vim_registers(cx).and_then(|r| r.last_search()) {
                Some((pattern, _)) => pattern.to_owned(),
                None => return say_error(editor, ExError::NoPreviousSearch.to_string(), cx),
            },
        };
        let Some(root) = self.root().map(Path::to_owned) else {
            return say_error(editor, no_folder(), cx);
        };
        let current = editor.and_then(|editor| editor.path(cx));
        let globs = globs
            .iter()
            .map(|glob| include_glob(glob, &root, current.as_deref()))
            .collect::<Result<Vec<_>, _>>();
        let globs = match globs {
            Ok(globs) => globs,
            Err(error) => return say_error(editor, error, cx),
        };
        let name = if location { "lvimgrep" } else { "vimgrep" };
        let grep = Grep {
            pattern,
            globs,
            every: flags.contains('g'),
            jump: !flags.contains('j'),
            title: format!(":{name} {args}"),
        };
        self.run_grep(grep, location, editor, window, cx);
    }

    /// Starts `grep` over the root, off the main thread, in place of the
    /// window's running search, its list for the quickfix stack or with
    /// `location` the active pane's location stack.
    fn run_grep(
        &mut self,
        grep: Grep,
        location: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root().map(Path::to_owned) else {
            return say_error(editor, no_folder(), cx);
        };
        let globs: Vec<&str> = grep.globs.iter().map(String::as_str).collect();
        let filter = match PathFilter::new(&root, &globs, &[]) {
            Ok(filter) => filter,
            Err(error) => {
                let text = tr!(
                    "workbench-lsp-quickfix-invalid-glob",
                    glob = error.glob,
                    error = error.message
                );
                return say_error(editor, text, cx);
            }
        };
        let query = SearchQuery {
            pattern: grep.pattern.clone(),
            regex: true,
            case_sensitive: smart_case(&grep.pattern),
            whole_word: false,
        };
        let pane = location.then(|| self.pane().entity_id());
        let editor = editor.map(Entity::downgrade);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let every = grep.every;
        let searching =
            cx.background_spawn(async move { search(&root, &query, &filter, every, flag) });
        let task = cx.spawn_in(window, async move |workbench, cx| {
            let found = searching.await;
            let done = workbench.update_in(cx, |workbench, window, cx| {
                workbench.grep_done(grep, pane, editor, found, window, cx);
            });
            done.ok();
        });
        let id = cx.entity_id();
        if !cx.default_global::<Searches>().0.contains_key(&id) {
            cx.on_release(move |_, cx| {
                cx.default_global::<Searches>().0.remove(&id);
            })
            .detach();
            // A new folder is a new project: the old one's results aren't
            // for this window any more.
            cx.subscribe_self(move |_, _: &RootChanged, cx| {
                cx.default_global::<Searches>().0.remove(&id);
            })
            .detach();
        }
        let search = Search {
            cancel,
            _task: task,
        };
        cx.default_global::<Searches>().0.insert(id, search);
    }

    /// Puts what `grep` found on the stack (the quickfix stack, or `pane`'s
    /// location stack) and opens its first item, or says why not, in
    /// `editor`.
    fn grep_done(
        &mut self,
        grep: Grep,
        pane: Option<EntityId>,
        editor: Option<WeakEntity<EditorView>>,
        found: Found,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = editor.and_then(|editor| editor.upgrade());
        let editor = editor.as_ref();
        let (items, truncated) = match found {
            Ok(Some(found)) => found,
            Ok(None) => return,
            Err(SearchError::InvalidPattern(reason)) => {
                let text = tr!(
                    "workbench-lsp-quickfix-e383",
                    pattern = &grep.pattern,
                    reason = last_line(&reason)
                );
                return say_error(editor, text, cx);
            }
        };
        if items.is_empty() {
            let text = tr!("workbench-lsp-quickfix-e480", pattern = &grep.pattern);
            return say_error(editor, text, cx);
        }
        let capped = |text: String| match truncated {
            true => tr!(
                "workbench-lsp-quickfix-capped",
                text = text,
                count = SearchOptions::default().max_matches
            )
            .into(),
            false => text,
        };
        let list = QuickfixList::new(grep.title, items);
        let landing = list.current_item().cloned();
        let text = list.landed_message();
        if !self.push_quickfix_list_to(pane, list, cx) {
            return;
        }
        match landing {
            Some(item) if grep.jump => self.land_on(item, capped(text), window, cx),
            _ => {
                let summary = self.quickfix_summary(pane, cx);
                say(editor, capped(summary), cx);
            }
        }
    }

    /// `:make {args}`, or with `location` `:lmake`: runs the default build
    /// task, once the root's tasks are read and if the root is still the one
    /// they're read from, and fills a list from its problems once it exits.
    pub(crate) fn make(
        &mut self,
        args: &str,
        location: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !args.is_empty() {
            let text = tr!("workbench-lsp-quickfix-make-ignoring", args = args);
            say(editor, text, cx);
        }
        let root = self.root().map(Path::to_owned);
        let reading = read_tasks(root.clone(), cx);
        let pane = location.then(|| self.pane().entity_id());
        let editor = editor.map(Entity::downgrade);
        let title = if location { ":lmake" } else { ":make" };
        let handle = window.window_handle();
        cx.spawn_in(window, async move |workbench, cx| {
            let (tasks, _) = reading.await;
            let ran = workbench.update_in(cx, |workbench, window, cx| {
                if workbench.root() != root.as_deref() {
                    return;
                }
                let Some(task) = default_build(&tasks).cloned() else {
                    let editor = editor.and_then(|editor| editor.upgrade());
                    return say_error(editor.as_ref(), nothing_to_run(Some(Group::Build)), cx);
                };
                let heard: RunHeard = Box::new(move |workbench, problems, cx| {
                    let landing = workbench.made(title, pane, problems, cx);
                    let Some((item, text)) = landing else {
                        return;
                    };
                    // The run is heard without a window to open the item in.
                    let workbench = cx.entity();
                    App::defer(cx, move |cx| {
                        let landed = handle.update(cx, |_, window, cx| {
                            workbench.update(cx, |workbench, cx| {
                                workbench.land_on(item, text, window, cx);
                            });
                        });
                        landed.ok();
                    });
                });
                workbench.run_task_then(task, Some(heard), window, cx);
            });
            ran.ok();
        })
        .detach();
    }

    /// Puts the list `problems` make, titled `title`, on the quickfix stack
    /// or `pane`'s location stack, its first error (else its first item)
    /// selected: that item and its landing message, if there is one to
    /// open.
    fn made(
        &mut self,
        title: &str,
        pane: Option<EntityId>,
        problems: Vec<RunProblem>,
        cx: &mut Context<Self>,
    ) -> Option<(QuickfixItem, String)> {
        let items: Vec<QuickfixItem> = problems.into_iter().map(made_item).collect();
        let first_error = (items.iter()).position(|item| item.kind == QuickfixKind::Error);
        let mut list = QuickfixList::new(title, items);
        list.current = first_error.unwrap_or(0);
        let landing = (list.current_item().cloned()).map(|item| (item, list.landed_message()));
        self.push_quickfix_list_to(pane, list, cx)
            .then_some(landing)
            .flatten()
    }
}

/// A build problem as a list's item.
fn made_item(problem: RunProblem) -> QuickfixItem {
    QuickfixItem {
        path: problem.path,
        row: problem.row,
        col: problem.column,
        text: problem.message,
        kind: match problem.severity {
            task_problems::Severity::Error => QuickfixKind::Error,
            task_problems::Severity::Warning => QuickfixKind::Warning,
        },
    }
}

/// Searches `root` for `query` through `filter`: an item per matching line
/// at its first match, or with `every` per match, in path, row and column
/// order.
fn search(
    root: &Path,
    query: &SearchQuery,
    filter: &PathFilter,
    every: bool,
    cancel: Arc<AtomicBool>,
) -> Found {
    let mut files: Vec<FileMatches> = Vec::new();
    let options = SearchOptions::default();
    let summary =
        fe_project::search_filtered(root, query, &options, filter, cancel.clone(), |batch| {
            files.extend(batch);
        })?;
    if summary.cancelled || cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut items = Vec::new();
    for file in &files {
        for line in &file.lines {
            let ranges = match every {
                true => &line.ranges[..],
                false => &line.ranges[..line.ranges.len().min(1)],
            };
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let starts: Vec<usize> = ranges.iter().map(|range| range.start).collect();
            let Some(found) = line_columns(root, &file.path, line, &starts, &cancel) else {
                return Ok(None);
            };
            for &col in &found.columns {
                items.push(QuickfixItem {
                    path: file.path.clone(),
                    row: u32::try_from(line.line_number.saturating_sub(1)).unwrap_or(u32::MAX),
                    col,
                    text: line.preview.clone(),
                    kind: QuickfixKind::Untyped,
                });
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some((items, summary.truncated)))
}

/// The columns of one matching line's matches, as [`line_columns`] finds them.
struct LineColumns {
    /// The column, in chars, of each start asked for.
    columns: Vec<u32>,
    /// How many times the line was read from disk: 0 or 1. For the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    reads: u32,
}

/// The columns, in chars, of the byte offsets `starts` (ascending) of `line`,
/// a line of the file at `path` under `root`: counted in the preview when it
/// starts the line and holds them all, else in the line read from disk once,
/// without following a symlink or blocking on a FIFO. A UTF-16 file's line is
/// decoded as the search saw it, and a UTF-8 byte order mark isn't a char.
/// None if `cancel` was set at any point before it returned.
fn line_columns(
    root: &Path,
    path: &Path,
    line: &LineMatch,
    starts: &[usize],
    cancel: &AtomicBool,
) -> Option<LineColumns> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let found = columns_of(root, path, line, starts, cancel)?;
    (!cancel.load(Ordering::Relaxed)).then_some(found)
}

fn columns_of(
    root: &Path,
    path: &Path,
    line: &LineMatch,
    starts: &[usize],
    cancel: &AtomicBool,
) -> Option<LineColumns> {
    let chars_before = |text: &str, end: usize| {
        let before = text.char_indices().take_while(|(ix, _)| *ix < end).count();
        u32::try_from(before).unwrap_or(u32::MAX)
    };
    let bom =
        line.line_offset == 0 && line.preview_start == 0 && line.preview.starts_with('\u{feff}');
    let in_preview = |start: usize| {
        let chars = chars_before(&line.preview, start);
        if bom && start >= '\u{feff}'.len_utf8() {
            chars - 1
        } else if bom {
            0
        } else {
            chars
        }
    };
    let preview_len = line.preview.len();
    if line.preview_start == 0 && starts.iter().all(|&start| start <= preview_len) {
        let columns = starts.iter().map(|&start| in_preview(start)).collect();
        return Some(LineColumns { columns, reads: 0 });
    }
    let last = starts.iter().copied().max().unwrap_or(0);
    match read_line(root, path, line, last, cancel) {
        Ok(Some(text)) => Some(LineColumns {
            columns: text.columns(starts, cancel)?,
            reads: 1,
        }),
        Ok(None) => None,
        // Gone since: the preview's chars past its start, a byte each before.
        Err(_) => {
            let skipped = u32::try_from(line.preview_start).unwrap_or(u32::MAX);
            let columns = (starts.iter())
                .map(|&start| {
                    let within = start.saturating_sub(line.preview_start);
                    skipped.saturating_add(chars_before(&line.preview, within))
                })
                .collect();
            Some(LineColumns { columns, reads: 0 })
        }
    }
}

/// A line's bytes from disk, as far as a read for its columns went.
struct LineText {
    bytes: Vec<u8>,
    /// UTF-16 in this byte order (big endian if true), as the search
    /// transcoded it; None for a UTF-8 (or other 8-bit) file.
    utf16: Option<bool>,
    /// The bytes open with a UTF-8 byte order mark.
    bom: bool,
}

impl LineText {
    /// The columns of `starts` (ascending), in one pass over the decoded
    /// chars, as the search decoded them. None if cancelled meanwhile.
    fn columns(&self, starts: &[usize], cancel: &AtomicBool) -> Option<Vec<u32>> {
        let mut lens: Box<dyn Iterator<Item = usize> + '_> = match self.utf16 {
            Some(big) => {
                let units = self
                    .bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(move |pair| match big {
                        true => u16::from_be_bytes(*pair),
                        false => u16::from_le_bytes(*pair),
                    });
                Box::new(char::decode_utf16(units).map(|ch| match ch {
                    Ok(ch) => ch.len_utf8(),
                    Err(_) => char::REPLACEMENT_CHARACTER.len_utf8(),
                }))
            }
            // A run of invalid bytes is one replacement char.
            None => Box::new(self.bytes.utf8_chunks().flat_map(|chunk| {
                let invalid = chunk.invalid().len();
                (chunk.valid().chars().map(char::len_utf8)).chain((invalid > 0).then_some(invalid))
            })),
        };
        let mut columns = Vec::with_capacity(starts.len());
        let (mut pos, mut count, mut steps) = (0, 0u32, 0u32);
        for &start in starts {
            while pos < start {
                let Some(len) = lens.next() else { break };
                pos += len;
                count = count.saturating_add(1);
                steps += 1;
                #[cfg(test)]
                STEP_HOOK.with_borrow_mut(|hook| hook.as_mut().map(|hook| hook(steps)));
                if steps % 64 == 0 && cancel.load(Ordering::Relaxed) {
                    return None;
                }
            }
            // The mark is one char of the line, not of the text.
            columns.push(match (self.bom, start) {
                (true, 0..=2) => 0,
                (true, _) => count.saturating_sub(1),
                _ => count,
            });
        }
        (!cancel.load(Ordering::Relaxed)).then_some(columns)
    }
}

#[cfg(test)]
type StepHook = Box<dyn FnMut(u32)>;

#[cfg(test)]
thread_local! {
    /// Called with the count of chars decoded so far, at each one.
    static STEP_HOOK: std::cell::RefCell<Option<StepHook>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Reads `line` from disk as far as byte `last` of it. None if cancelled.
fn read_line(
    root: &Path,
    path: &Path,
    line: &LineMatch,
    last: usize,
    cancel: &AtomicBool,
) -> io::Result<Option<LineText>> {
    let mut file = open_match(root, path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(verbatim!("not a regular file")));
    }
    let mut mark = [0; 2];
    let utf16 = match file.read_exact(&mut mark) {
        Ok(()) => matches!(mark, [0xFF, 0xFE] | [0xFE, 0xFF]),
        Err(_) => false,
    };
    // Each UTF-16 unit is 2 bytes and at least 1 of the line's UTF-8, and a
    // pair of them at most 4: with room for a char cut at `last`.
    let want = if utf16 {
        last.saturating_mul(2).saturating_add(4)
    } else {
        last
    };
    file.seek(SeekFrom::Start(line.line_offset))?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    while bytes.len() < want {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let room = chunk.len().min(want - bytes.len());
        match file.read(&mut chunk[..room])? {
            0 => break,
            n => bytes.extend_from_slice(&chunk[..n]),
        }
    }
    let bom = !utf16 && line.line_offset == 0 && bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let utf16 = utf16.then_some(mark == [0xFE, 0xFF]);
    Ok(Some(LineText { bytes, utf16, bom }))
}

/// Opens `path`, a file of the search of `root`, as the search did.
///
/// `path` is under `root` as given, but the file is opened under the root
/// canonicalized, as the search opens it.
fn open_match(root: &Path, path: &Path) -> io::Result<File> {
    let open_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    match path.strip_prefix(root) {
        Ok(rel) if !rel.as_os_str().is_empty() => open_file_under(&open_root, rel),
        // The root itself, a file: under its directory.
        _ => match (open_root.parent(), open_root.file_name()) {
            (Some(dir), Some(name)) => open_file_under(dir, Path::new(name)),
            _ => Err(io::Error::other(verbatim!("no file"))),
        },
    }
}

/// Whether `pattern` matches case by case, as ripgrep's `--smart-case`
/// has it: when it has an uppercase letter, not counting escapes (`\S`,
/// `\p{Lu}`, `\x4A`) or group names (`(?P<Name>…)`).
fn smart_case(pattern: &str) -> bool {
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let digits = match chars.next() {
                    Some('p' | 'P') => {
                        if chars.next_if_eq(&'{').is_some() {
                            while chars.next().is_some_and(|ch| ch != '}') {}
                        } else {
                            chars.next();
                        }
                        continue;
                    }
                    Some('x') => 2,
                    Some('u') => 4,
                    Some('U') => 8,
                    _ => continue,
                };
                // A hex escape is the literal it decodes to, which makes
                // the search case sensitive if it's uppercase, as in rg.
                let mut hex = String::new();
                if chars.next_if_eq(&'{').is_some() {
                    hex.extend(chars.by_ref().take_while(|&ch| ch != '}'));
                } else {
                    hex.extend(chars.by_ref().take(digits));
                }
                let decoded = u32::from_str_radix(hex.trim(), 16)
                    .ok()
                    .and_then(char::from_u32);
                if decoded.is_some_and(char::is_uppercase) {
                    return true;
                }
            }
            '(' if chars.peek() == Some(&'?') => {
                chars.next();
                chars.next_if_eq(&'P');
                if chars.next_if_eq(&'<').is_some() {
                    while chars.next().is_some_and(|ch| ch != '>') {}
                }
            }
            _ if ch.is_uppercase() => return true,
            _ => {}
        }
    }
    false
}

/// `glob`, typed after `:vimgrep`, as an include glob relative to `root`
/// that matches as vim's file argument does: anchored at the root (`*.md`
/// is the root's own), `%` being `current`'s path. A path outside `root`
/// can't be searched.
fn include_glob(glob: &str, root: &Path, current: Option<&Path>) -> Result<String, String> {
    if glob == "%" {
        let Some(current) = current else {
            return Err(tr!("workbench-lsp-quickfix-e499").into());
        };
        let rel = current
            .strip_prefix(root)
            .map_err(|_| outside(current.display()))?;
        let words: Vec<String> = words(rel).map(|word| escape_glob(&word)).collect();
        return Ok(format!("/{}", words.join("/")));
    }
    // Rooted, not only absolute: Windows' `\p\src` names a folder on the
    // current drive, as a root without one does.
    let rel = match Path::new(glob).has_root() {
        true => {
            let rel = Path::new(glob)
                .strip_prefix(root)
                .map_err(|_| outside(glob))?;
            words(rel).collect::<Vec<_>>().join("/")
        }
        false => glob.trim_start_matches("./").to_owned(),
    };
    Ok(format!("/{rel}"))
}

/// Why a `:vimgrep` file argument can't be searched: `path` is outside the
/// root.
fn outside(path: impl std::fmt::Display) -> String {
    tr!("workbench-lsp-quickfix-outside", path = path.to_string()).into()
}

/// The words of `rel`, a path under the root: a glob joins them with `/` on
/// every platform.
fn words(rel: &Path) -> impl Iterator<Item = String> + '_ {
    rel.components().filter_map(|component| match component {
        std::path::Component::Normal(word) => Some(word.to_string_lossy().into_owned()),
        std::path::Component::ParentDir => Some("..".to_owned()),
        _ => None,
    })
}

/// `path` with the chars a glob reads specially escaped.
fn escape_glob(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(ch, '*' | '?' | '[' | ']' | '\\' | '{' | '}' | ' ' | '!') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The last non-empty line of `text`, `error: ` dropped: a regex error's
/// reason, which comes after the pattern and a caret under it.
fn last_line(text: &str) -> &str {
    let line = text.lines().rev().find(|line| !line.trim().is_empty());
    let line = line.unwrap_or(text).trim();
    line.strip_prefix("error: ").unwrap_or(line)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::single_range_in_vec_init)]

    use super::{LineMatch, include_glob, line_columns, smart_case};
    #[cfg(unix)]
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt as _, fs::symlink},
    };
    use std::{fs, path::Path, sync::atomic::AtomicBool};

    #[test]
    fn smart_case_skips_escapes_and_group_names() {
        assert!(!smart_case("foo"));
        assert!(smart_case("Foo"));
        assert!(!smart_case(r"\S+\W\D\B"));
        assert!(!smart_case(r"\p{Lu}\pL\x6A\x{6a}\u00e9[[:upper:]]"));
        assert!(smart_case(r"\x4A"));
        assert!(smart_case(r"\x{4A}"));
        assert!(smart_case(r"\u00C9"));
        assert!(!smart_case("(?P<Name>a)(?<Other>b)"));
        assert!(smart_case("(?P<n>A)"));
        assert!(smart_case("É"));
    }

    #[test]
    fn globs_anchor_at_the_root() {
        let root = Path::new("/p");
        let glob = |glob: &str| include_glob(glob, root, Some(Path::new("/p/a [1].rs")));
        assert_eq!(glob("*.md").unwrap(), "/*.md");
        assert_eq!(glob("**/*.rs").unwrap(), "/**/*.rs");
        assert_eq!(glob("./src").unwrap(), "/src");
        assert_eq!(glob("/p/src/*.rs").unwrap(), "/src/*.rs");
        assert!(glob("/q/x").is_err());
        assert_eq!(glob("%").unwrap(), r"/a\ \[1\].rs");
        assert!(include_glob("%", root, None).is_err());
    }

    // The line reader: chars from the line as the search decoded it, from
    // one read of it, never through a symlink or a FIFO.
    fn line(
        offset: u64,
        ranges: Vec<std::ops::Range<usize>>,
        preview_start: usize,
        preview: &str,
    ) -> LineMatch {
        LineMatch {
            line_number: 1,
            line_offset: offset,
            ranges,
            preview: preview.to_owned(),
            preview_start,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_line_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "éééééé").unwrap();
        symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        let matched = line(0, vec![4..5], 3, "ab");
        let cancel = AtomicBool::new(false);
        let found = line_columns(
            dir.path(),
            &dir.path().join("link.txt"),
            &matched,
            &[4],
            &cancel,
        )
        .unwrap();
        assert_eq!(found.reads, 0, "the link was read");
        // The preview's chars past its start, a byte each before: 3 + 1.
        assert_eq!(found.columns, [4]);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_line_reread_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.txt");
        let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let matched = line(0, vec![4..5], 3, "ab");
        let cancel = AtomicBool::new(false);
        let found = line_columns(dir.path(), &fifo, &matched, &[4], &cancel).unwrap();
        assert_eq!(found.reads, 0);
        assert_eq!(found.columns, [4]);
    }

    #[test]
    fn utf16_lines_get_the_char_column() {
        let dir = tempfile::tempdir().unwrap();
        // "é" and "😀" in a long line: 2 and 4 bytes of the search's UTF-8.
        let text = format!("é😀{}x\n", "a".repeat(600));
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        fs::write(dir.path().join("u.txt"), &bytes).unwrap();
        let start = 'é'.len_utf8() + '😀'.len_utf8() + 600;
        let matched = line(2, vec![start..start + 1], 0, "é");
        let cancel = AtomicBool::new(false);
        let found = line_columns(
            dir.path(),
            &dir.path().join("u.txt"),
            &matched,
            &[start],
            &cancel,
        )
        .unwrap();
        assert_eq!(found.columns, [602]);
        // Big endian too.
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend(unit.to_be_bytes());
        }
        fs::write(dir.path().join("b.txt"), &bytes).unwrap();
        let found = line_columns(
            dir.path(),
            &dir.path().join("b.txt"),
            &matched,
            &[start],
            &cancel,
        )
        .unwrap();
        assert_eq!(found.columns, [602]);
    }

    #[test]
    fn many_matches_on_a_long_line_come_from_one_read() {
        let dir = tempfile::tempdir().unwrap();
        let text = "x".repeat(5000);
        fs::write(dir.path().join("a.txt"), &text).unwrap();
        let starts: Vec<usize> = (0..5000).collect();
        let matched = line(
            0,
            starts.iter().map(|&s| s..s + 1).collect(),
            0,
            &text[..512],
        );
        let cancel = AtomicBool::new(false);
        let found = line_columns(
            dir.path(),
            &dir.path().join("a.txt"),
            &matched,
            &starts,
            &cancel,
        )
        .unwrap();
        assert_eq!(found.reads, 1);
        let want: Vec<u32> = (0..5000).collect();
        assert_eq!(found.columns, want);
    }

    #[test]
    fn a_cancelled_fill_stops_reading() {
        let dir = tempfile::tempdir().unwrap();
        let text = "x".repeat(5000);
        fs::write(dir.path().join("a.txt"), &text).unwrap();
        let matched = line(0, vec![4999..5000], 0, &text[..512]);
        let cancel = AtomicBool::new(true);
        assert!(
            line_columns(
                dir.path(),
                &dir.path().join("a.txt"),
                &matched,
                &[4999],
                &cancel
            )
            .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_root_reached_through_a_symlink_is_resolved_to_open_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("a.txt"), "x".repeat(600)).unwrap();
        let root = dir.path().join("root");
        symlink(&real, &root).unwrap();
        let matched = line(0, vec![550..551], 1, "x");
        let cancel = AtomicBool::new(false);
        let found = line_columns(&root, &root.join("a.txt"), &matched, &[550], &cancel).unwrap();
        assert_eq!(found.reads, 1);
        assert_eq!(found.columns, [550]);
    }

    #[test]
    fn invalid_bytes_split_across_matches_count_as_one_char() {
        let dir = tempfile::tempdir().unwrap();
        // A cut-off emoji is one replacement char, however the matches fall.
        fs::write(dir.path().join("a.txt"), b"a\xF0\x9F\x98b").unwrap();
        let matched = line(0, vec![1..2, 2..3, 4..5], 1, "b");
        let cancel = AtomicBool::new(false);
        let found = line_columns(
            dir.path(),
            &dir.path().join("a.txt"),
            &matched,
            &[1, 2, 4],
            &cancel,
        )
        .unwrap();
        assert_eq!(found.reads, 1);
        assert_eq!(found.columns, [1, 2, 2]);
    }

    #[test]
    fn a_fill_cancelled_before_a_line_does_not_open_it() {
        let dir = tempfile::tempdir().unwrap();
        // Not there: an open would fall back to the preview, not stop.
        let matched = line(0, vec![4..5], 3, "ab");
        let cancel = AtomicBool::new(true);
        let gone = dir.path().join("gone.txt");
        assert!(line_columns(dir.path(), &gone, &matched, &[4], &cancel).is_none());
    }

    /// Sets `cancel` when the decoder has taken `after` chars.
    fn cancel_after(after: u32, cancel: &'static AtomicBool) {
        super::STEP_HOOK.with_borrow_mut(|hook| {
            *hook = Some(Box::new(move |steps| {
                if steps == after {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }));
        });
    }

    #[test]
    fn a_fill_cancelled_during_decoding_returns_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let text = "x".repeat(5000);
        fs::write(dir.path().join("a.txt"), &text).unwrap();
        let starts = [4999];
        let matched = line(0, vec![4999..5000], 0, &text[..512]);
        // Early, in the middle of a stretch between checks, and on the very
        // last char, which only the check at the end sees.
        for after in [10, 100, 4999] {
            static CANCEL: AtomicBool = AtomicBool::new(false);
            CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
            cancel_after(after, &CANCEL);
            let found = line_columns(
                dir.path(),
                &dir.path().join("a.txt"),
                &matched,
                &starts,
                &CANCEL,
            );
            super::STEP_HOOK.with_borrow_mut(|hook| *hook = None);
            assert!(found.is_none(), "cancelled after {after} chars");
        }
    }
}
