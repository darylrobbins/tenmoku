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
//! from disk to count them: in a UTF-16 file that count is off.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_gpui::EditorView;
use fe_project::{FileMatches, LineMatch, PathFilter, SearchError, SearchOptions, SearchQuery};
use fe_vim::{ExError, QuickfixAction};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, WeakEntity, Window};
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
pub(crate) const NO_FOLDER: &str = "Open a folder to search it";

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
            return say_error(editor, NO_FOLDER, cx);
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
            return say_error(editor, NO_FOLDER, cx);
        };
        let globs: Vec<&str> = grep.globs.iter().map(String::as_str).collect();
        let filter = match PathFilter::new(&root, &globs, &[]) {
            Ok(filter) => filter,
            Err(error) => {
                let text = format!("Invalid glob {}: {}", error.glob, error.message);
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
                let text = format!(
                    "E383: Invalid search string: {} ({})",
                    grep.pattern,
                    last_line(&reason)
                );
                return say_error(editor, text, cx);
            }
        };
        if items.is_empty() {
            return say_error(editor, format!("E480: No match: {}", grep.pattern), cx);
        }
        let capped = |text: String| match truncated {
            true => format!(
                "{text} (the first {} matches)",
                SearchOptions::default().max_matches
            ),
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
            let text = format!("Ignoring {args}: :make runs the default build task");
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
            for range in ranges {
                items.push(QuickfixItem {
                    path: file.path.clone(),
                    row: u32::try_from(line.line_number.saturating_sub(1)).unwrap_or(u32::MAX),
                    col: char_column(&file.path, line, range.start),
                    text: line.preview.clone(),
                    kind: QuickfixKind::Untyped,
                });
            }
        }
    }
    Ok(Some((items, summary.truncated)))
}

/// The column, in chars, of byte `start` of `line`, a line of the file at
/// `path`: counted in the preview when it starts the line, else in the
/// line read from disk.
fn char_column(path: &Path, line: &LineMatch, start: usize) -> u32 {
    let chars_before = |text: &str, end: usize| {
        let before = text.char_indices().take_while(|(ix, _)| *ix < end).count();
        u32::try_from(before).unwrap_or(u32::MAX)
    };
    if line.preview_start == 0 {
        return chars_before(&line.preview, start);
    }
    let mut bytes = vec![0; start];
    let read = File::open(path).and_then(|mut file| {
        file.seek(SeekFrom::Start(line.line_offset))?;
        file.read_exact(&mut bytes)
    });
    match read {
        Ok(()) => chars_before(&String::from_utf8_lossy(&bytes), start),
        // Gone since: the preview's chars past its start, a byte each before.
        Err(_) => {
            let skipped = u32::try_from(line.preview_start).unwrap_or(u32::MAX);
            let within = start.saturating_sub(line.preview_start);
            skipped.saturating_add(chars_before(&line.preview, within))
        }
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
            return Err("E499: Empty file name for '%' or '#'".to_owned());
        };
        let rel = current
            .strip_prefix(root)
            .map_err(|_| format!("{} is outside the folder", current.display()))?;
        let words: Vec<String> = words(rel).map(|word| escape_glob(&word)).collect();
        return Ok(format!("/{}", words.join("/")));
    }
    // Rooted, not only absolute: Windows' `\p\src` names a folder on the
    // current drive, as a root without one does.
    let rel = match Path::new(glob).has_root() {
        true => {
            let rel = Path::new(glob)
                .strip_prefix(root)
                .map_err(|_| format!("{glob} is outside the folder"))?;
            words(rel).collect::<Vec<_>>().join("/")
        }
        false => glob.trim_start_matches("./").to_owned(),
    };
    Ok(format!("/{rel}"))
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
    use super::{include_glob, smart_case};
    use std::path::Path;

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
}
