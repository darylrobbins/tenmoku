//! The API chat's workbench tools (the agents record's § 6, card AG7):
//! `read_file`, `list_files`, `search` and `propose_edit`, and
//! [`ChatProposals`], the chat's one pending proposal per file.
//!
//! - **Paths** are root-relative. Each is resolved through
//!   [`chat_apply::locate`](crate::chat_panel::chat_apply::locate) (inside
//!   the root, every symlink followed and checked) and refused by
//!   [`chat_context::refusal`]'s deny list, both as written and where it
//!   really is ([`resolve`]).
//! - **[`list_files`]** lists quick open's files ([`FinderIndex::walk`]:
//!   ignored files left out) under `dir`, matching `glob`, the deny list
//!   applied: at most [`MAX_PATHS`], then `[truncated: N more]`.
//! - **[`search`]** runs [`fe_project::search_filtered`] over the root with
//!   the deny list as its [`PathFilter`]'s excludes: at most
//!   [`MAX_SEARCH_LINES`] lines of `path:line: preview`, or
//!   [`MAX_SEARCH_BYTES`], then `[truncated]`. It's cancellable.
//! - **[`read_file`]** reads the chat's view of a file ([`ChatProposals::view`]):
//!   its open buffer's text, unsaved edits included (an editor in any
//!   window, this one's first), else the file's, with the chat's pending
//!   proposal applied. Binary text and text over 1 MiB are refused, as
//!   @file refuses them, before any of it is copied. At most
//!   [`MAX_READ_LINES`] lines or [`MAX_READ_BYTES`] are returned
//!   ([`read_output`]), then `[truncated: N more lines; read from line M]`.
//! - **[`propose_edit`]** checks the edits as a set against the chat's view
//!   ([`apply_edits`]: each `old_text` found exactly once, none
//!   overlapping), then proposes the result on the file's buffer in this
//!   window, given a tab in the active pane if no editor here has it
//!   ([`ChatProposals::propose`]).
//!
//! # The file a call reaches
//!
//! A file is vetted once, off the main thread ([`vet`]): its path resolved
//! and checked against the deny list, then walked a component at a time and
//! opened ([`chat_context::open_vetted`]), which refuses anything but a
//! regular file or a folder (a FIFO, say) and a link out of the root. What
//! that walk reached is the file: its real path keys the chat's proposals,
//! an open buffer of it stands for it only if the buffer was read from the
//! file the walk opened (its device and inode), and a buffer made for it
//! otherwise is read through the handle the walk opened, at most 1 MiB, so
//! no file swapped in after the walk is ever read. A file over 1 MiB is
//! refused.
//!
//! # Proposals and the editors
//!
//! A proposal reaches the file's editors, where the user can accept its
//! hunks, only through its turn's changeset: once the loop hands that to
//! [`Workbench::set_changeset`], the AI Changes panel gives each editor its
//! proposal and arms every accept, so Revert Turn covers it. During the
//! turn the chat's proposals are its own, and nothing can decide them.
//!
//! # Supersession
//!
//! The chat holds at most one live proposal per file, keyed by its real
//! path, whichever turn made it. A valid edit set makes the file's new view;
//! the earlier proposal's still-pending hunks are rejected as superseded
//! ([`ChatProposals::is_superseded`]), and one new proposal is made from the
//! buffer to the new view, carrying the earlier pending work and the new
//! edits together. It joins the current turn's [`Changeset`]
//! ([`ChatProposals::end_turn`]). A pending new file is superseded the same
//! way: an earlier turn's entry is rejected in its changeset, if that's the
//! one the workbench shows, and the new text joins the current turn. An
//! edit set that doesn't validate changes nothing.
//!
//! Pending work moves to the latest turn's review: a turn's changeset
//! ([`ChatProposals::end_turn`], [`end_turn`]) takes every earlier turn's
//! proposal still undecided too, so the changeset the workbench shows owns
//! all of them, and decisions and rebinding go through it.
//!
//! A proposal is never forgotten because it can't be seen: only a later
//! one replaces it. Once handed in, its changeset owns its review: a file
//! whose editor was closed is opened again, and the AI Changes panel
//! follows the proposal to the new buffer ([`Changeset::rebind`]), or, if
//! it can't (a hunk was stale), the proposal no longer applies. One never
//! handed in, which nothing could decide, is made again on the reopened
//! buffer when it holds the text the proposal was made against and no hunk
//! was found stale. The turn's end does this for every such file before it
//! hands the changeset in, giving a tab in this window to one only another
//! window shows. A partly decided proposal goes on into the next review as
//! a new one of its pending changes, so each review keeps its own decisions
//! and Revert Turn checkpoint.
//!
//! When another review replaces one the chat handed in, the AI Changes
//! panel gives its changeset back ([`ChatProposals::returner`]), and the
//! chat keeps it observing as the panel observed it ([`watch`]): the
//! editors' decisions go into it (an accept under its checkpoint, the hunk
//! memo taken again), so what it says of a new file stays authoritative and
//! it follows a proposal to a reopened buffer ([`ChatProposals::rebind_kept`])
//! with every decision made since.
//!
//! The text here is the model's: the results, headers and truncation lines
//! are what the tool descriptions in `fe_chat::tools` promise, in English.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};

use fe_buffer::{
    Bias, BufferId, BufferSnapshot, DiskStamp, HunkId, HunkStatus, Origin, Outcome, ProposedChange,
    Text,
};
use fe_chat::{
    EditBody, ListFilesInput, NotRun, ProposeEditInput, ProposedEdit, ReadFileInput, SearchInput,
};
use fe_editor::{BufferSet, Changeset};
use fe_gpui::EditorView;
use fe_project::{FinderIndex, PathFilter, SearchOptions, SearchQuery};
use gpui::{App, AppContext as _, AsyncWindowContext, Entity, Task, Window};

use crate::{
    Pane, Workbench,
    chat_panel::{
        chat_apply::{is_read_only, locate, propose},
        chat_context::{self, MAX_FILE, Walked, open_vetted, refusal, unattachable},
    },
};

#[cfg(test)]
#[path = "agent_tools_tests.rs"]
mod tests;

/// The most paths `list_files` lists.
pub(crate) const MAX_PATHS: usize = 1000;
/// The most lines `read_file` returns.
pub(crate) const MAX_READ_LINES: usize = 2000;
/// The most bytes of text `read_file` returns.
pub(crate) const MAX_READ_BYTES: usize = 64 << 10;
/// The most matching lines `search` returns.
pub(crate) const MAX_SEARCH_LINES: usize = 200;
/// The most bytes `search` returns.
pub(crate) const MAX_SEARCH_BYTES: usize = 64 << 10;

/// The deny list of [`chat_context::refusal`] as search excludes: `.git`,
/// and names kept as secrets. Results are checked by `refusal` too, so a
/// file is never let through that it refuses.
const DENY_GLOBS: &[&str] = &[".git", ".env*", "id_*", "*.pem", "*.key"];

/// How far into a file a NUL makes it binary, as the chat checks.
const BINARY_PROBE: usize = 8 << 10;

/// What a tool call came to: its result's text, or why it failed, which is
/// answered `is_error`.
pub(crate) type ToolOutcome = Result<String, String>;

// The model's text.
const NO_FOLDER: &str = "No folder is open";
const NO_SUCH_FILE: &str = "No such file";
const NOT_A_FOLDER: &str = "Not a folder";
const IS_A_FOLDER: &str = "It's a folder, not a file";
const NO_EDITS: &str = "`edits` is empty";
const WHOLE_ALONE: &str =
    "An edit with an empty `old_text` replaces the whole file, so it must be the only edit";
const NO_FILE_TO_EDIT: &str =
    "The file doesn't exist; create it with one edit whose `old_text` is \"\"";
const NO_FOLDER_THERE: &str = "The file's folder doesn't exist";
const READ_ONLY: &str = "The file is read-only";
const BINARY: &str = "The file is binary";
const NOT_OPENED: &str = "The file couldn't be opened";
const CHANGED: &str = "The file changed while it was read; read it again";
const NO_TURN: &str = "No turn is running";
const NO_MATCHES: &str = "[no matches]";
const NO_FILES: &str = "[no files]";
const TRUNCATED: &str = "[truncated]";
const START_ZERO: &str = "`start_line` starts at 1";

/// A root-relative path, resolved: inside the root and through no link
/// out of it ([`locate`]), and passing the deny list as written and where
/// it really is. Its real path, which keys the chat's proposals. Reads the
/// disk.
pub(crate) fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let real = locate(root, rel, &[])?.path;
    let real_root = dunce::canonicalize(root)
        .map_err(|_| kiln_l10n::tr!("workbench-chat-why-folder-unreadable").to_string())?;
    let written = refusal(Some(root), &root.join(rel));
    match written.or_else(|| refusal(Some(&real_root), &real)) {
        Some(why) => Err(why),
        None => Ok(real),
    }
}

/// `path` relative to `root`, `/`-separated.
fn slashed(rel: &Path) -> String {
    let parts: Vec<String> = (rel.components())
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

// ---------------------------------------------------------------------------
// list_files

/// `list_files`: the files quick open lists under the root, ignored files
/// left out, under `dir` and matching `glob`, the deny list applied; sorted,
/// one per line, at most [`MAX_PATHS`]. Walks the disk: run it off the main
/// thread. `cancel` stops the walk.
pub(crate) fn list_files(
    root: &Path,
    input: &ListFilesInput,
    cancel: Arc<AtomicBool>,
) -> ToolOutcome {
    let glob = input.glob.as_deref().map(Glob::new).transpose()?;
    let under = match input.dir.as_deref().filter(|dir| !matches!(*dir, "" | ".")) {
        None => PathBuf::new(),
        Some(dir) => {
            let real = resolve(root, dir)?;
            if !real.is_dir() {
                return Err(NOT_A_FOLDER.to_owned());
            }
            let real_root = dunce::canonicalize(root).map_err(|error| error.to_string())?;
            real.strip_prefix(&real_root)
                .map_err(|_| kiln_l10n::tr!("workbench-chat-why-outside").to_string())?
                .to_owned()
        }
    };
    let mut paths: Vec<String> = Vec::new();
    let walked = FinderIndex::walk(root, cancel.clone(), |batch| {
        paths.extend(
            (batch.into_iter())
                .filter(|rel| rel.starts_with(&under))
                .filter(|rel| refusal(Some(root), &root.join(rel)).is_none())
                .map(|rel| slashed(&rel))
                .filter(|rel| glob.as_ref().is_none_or(|glob| glob.matches(rel))),
        );
    });
    if !walked || cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(NotRun::Stopped.text().to_owned());
    }
    paths.sort();
    Ok(list_output(&paths))
}

/// `paths`, one per line, at most [`MAX_PATHS`], then `[truncated: N more]`.
pub(crate) fn list_output(paths: &[String]) -> String {
    if paths.is_empty() {
        return NO_FILES.to_owned();
    }
    let shown = paths.len().min(MAX_PATHS);
    let mut out = paths[..shown].join("\n");
    if paths.len() > shown {
        out.push('\n');
        out.push_str(&format!("[truncated: {} more]", paths.len() - shown));
    }
    out
}

/// A glob as `list_files` takes it, gitignore-style as the search's are: one
/// with no `/` but at its end matches a name at any depth, else the path
/// from the root; `*` and `?` stay within a name, `**` crosses folders, and
/// `[...]` is a class. A file matches through a folder it's in too, and a
/// trailing `/` matches folders only.
pub(crate) struct Glob {
    tokens: Vec<Token>,
    /// Matches each name rather than the path.
    by_name: bool,
    folders_only: bool,
}

enum Token {
    Char(char),
    One,
    Star,
    /// `**`: anything, `/` too.
    Any,
    /// `**/`: nothing, or anything ending in `/`.
    AnyFolders,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Glob {
    pub(crate) fn new(glob: &str) -> Result<Glob, String> {
        let invalid = || format!("Invalid glob {glob:?}");
        let (folders_only, body) = match glob.strip_suffix('/') {
            Some(body) => (true, body),
            None => (false, glob),
        };
        let (anchored, body) = match body.strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, body),
        };
        if body.is_empty() {
            return Err(invalid());
        }
        let chars: Vec<char> = body.chars().collect();
        let mut tokens = Vec::new();
        let mut ix = 0;
        while ix < chars.len() {
            let ch = chars[ix];
            ix += 1;
            tokens.push(match ch {
                '\\' => {
                    let escaped = *chars.get(ix).ok_or_else(invalid)?;
                    ix += 1;
                    Token::Char(escaped)
                }
                '?' => Token::One,
                '*' if chars.get(ix) == Some(&'*') => {
                    ix += 1;
                    if chars.get(ix) == Some(&'/') {
                        ix += 1;
                        Token::AnyFolders
                    } else {
                        Token::Any
                    }
                }
                '*' => Token::Star,
                '[' => {
                    let negated = matches!(chars.get(ix), Some('!' | '^'));
                    if negated {
                        ix += 1;
                    }
                    let mut ranges = Vec::new();
                    let mut first = true;
                    loop {
                        let ch = *chars.get(ix).ok_or_else(invalid)?;
                        ix += 1;
                        if ch == ']' && !first {
                            break;
                        }
                        first = false;
                        let ch = if ch == '\\' {
                            ix += 1;
                            *chars.get(ix - 1).ok_or_else(invalid)?
                        } else {
                            ch
                        };
                        if chars.get(ix) == Some(&'-')
                            && chars.get(ix + 1).is_some_and(|c| *c != ']')
                        {
                            ranges.push((ch, chars[ix + 1]));
                            ix += 2;
                        } else {
                            ranges.push((ch, ch));
                        }
                    }
                    Token::Class { negated, ranges }
                }
                ch => Token::Char(ch),
            });
        }
        Ok(Glob {
            tokens,
            by_name: !anchored && !body.contains('/'),
            folders_only,
        })
    }

    /// Whether the file `rel`, `/`-separated from the root, matches, itself
    /// or through a folder it's in.
    pub(crate) fn matches(&self, rel: &str) -> bool {
        let names: Vec<&str> = rel.split('/').collect();
        // The file itself is last; a folders-only glob skips it.
        let upto = if self.folders_only {
            names.len() - 1
        } else {
            names.len()
        };
        if self.by_name {
            return names[..upto].iter().any(|name| self.matches_text(name));
        }
        (1..=upto).any(|count| self.matches_text(&names[..count].join("/")))
    }

    fn matches_text(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let mut failed = HashSet::new();
        self.match_at(0, &text, 0, &mut failed)
    }

    fn match_at(
        &self,
        token: usize,
        text: &[char],
        at: usize,
        failed: &mut HashSet<(usize, usize)>,
    ) -> bool {
        if failed.contains(&(token, at)) {
            return false;
        }
        let matched = match self.tokens.get(token) {
            None => at == text.len(),
            Some(Token::Char(ch)) => {
                text.get(at) == Some(ch) && self.match_at(token + 1, text, at + 1, failed)
            }
            Some(Token::One) => {
                text.get(at).is_some_and(|ch| *ch != '/')
                    && self.match_at(token + 1, text, at + 1, failed)
            }
            Some(Token::Class { negated, ranges }) => {
                text.get(at).is_some_and(|ch| {
                    *ch != '/'
                        && (ranges.iter().any(|(lo, hi)| (lo..=hi).contains(&ch))) != *negated
                }) && self.match_at(token + 1, text, at + 1, failed)
            }
            Some(Token::Star) => {
                let mut end = at;
                loop {
                    if self.match_at(token + 1, text, end, failed) {
                        break true;
                    }
                    if end == text.len() || text[end] == '/' {
                        break false;
                    }
                    end += 1;
                }
            }
            Some(Token::Any) => {
                (at..=text.len()).any(|end| self.match_at(token + 1, text, end, failed))
            }
            Some(Token::AnyFolders) => {
                self.match_at(token + 1, text, at, failed)
                    || (at..text.len())
                        .filter(|end| text[*end] == '/')
                        .any(|end| self.match_at(token + 1, text, end + 1, failed))
            }
        };
        if !matched {
            failed.insert((token, at));
        }
        matched
    }
}

// ---------------------------------------------------------------------------
// search

/// `search`: [`fe_project::search_filtered`] over the root, ignored files
/// skipped, the deny list excluded and `glob` included; each matching line
/// as `path:line: preview`, files in path order, at most
/// [`MAX_SEARCH_LINES`] lines or [`MAX_SEARCH_BYTES`], then `[truncated]`.
/// Reads the disk: run it off the main thread. `cancel` stops it.
pub(crate) fn search(root: &Path, input: &SearchInput, cancel: Arc<AtomicBool>) -> ToolOutcome {
    let query = SearchQuery {
        pattern: input.pattern.clone(),
        regex: input.regex,
        case_sensitive: input.case_sensitive,
        whole_word: false,
    };
    let include: Vec<&str> = input.glob.iter().map(String::as_str).collect();
    let filter = PathFilter::new(root, &include, DENY_GLOBS).map_err(|error| error.to_string())?;
    let options = SearchOptions {
        // Ranges, not lines: room past the line cap to tell it was hit.
        max_matches: MAX_SEARCH_LINES * 5,
        threads: 0,
    };
    let mut files = Vec::new();
    let summary = fe_project::search_filtered(root, &query, &options, &filter, cancel, |batch| {
        files.extend(batch)
    })
    .map_err(|error| error.to_string())?;
    if summary.cancelled {
        return Err(NotRun::Stopped.text().to_owned());
    }
    files.retain(|file| refusal(Some(root), &root.join(&file.rel)).is_none());
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let lines: Vec<String> = (files.iter())
        .flat_map(|file| {
            (file.lines.iter())
                .map(|line| format!("{}:{}: {}", file.rel, line.line_number, line.preview))
        })
        .collect();
    Ok(search_output(&lines, summary.truncated))
}

/// `lines`, at most [`MAX_SEARCH_LINES`] of them and [`MAX_SEARCH_BYTES`]
/// in all, then `[truncated]` if any were left out or `more` says there
/// were more than found.
pub(crate) fn search_output(lines: &[String], more: bool) -> String {
    let mut out = String::new();
    let mut shown = 0;
    for line in lines.iter().take(MAX_SEARCH_LINES) {
        if out.len() + line.len() + 1 > MAX_SEARCH_BYTES {
            break;
        }
        out.push_str(line);
        out.push('\n');
        shown += 1;
    }
    if shown < lines.len() || more {
        out.push_str(TRUNCATED);
    } else if lines.is_empty() {
        out.push_str(NO_MATCHES);
    } else {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------
// read_file

/// A view's text as `read_file` returns it: lines `start` to `end`,
/// 1-based and inclusive (the file's first and last when `None`), at most
/// [`MAX_READ_LINES`] or [`MAX_READ_BYTES`], then `[truncated: N more
/// lines; read from line M]`; headed by how the chat's proposals stand. A
/// first line over the cap is cut there, and the truncation line says so,
/// as no line can be read from part way.
pub(crate) fn read_output(
    text: &str,
    start: Option<u64>,
    end: Option<u64>,
    view: &View,
) -> ToolOutcome {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len();
    let start = usize::try_from(start.unwrap_or(1)).unwrap_or(usize::MAX);
    if start == 0 {
        return Err(START_ZERO.to_owned());
    }
    let end = end.map_or(total.max(start), |end| {
        usize::try_from(end).unwrap_or(usize::MAX)
    });
    if end < start {
        return Err(format!("`end_line` {end} is before `start_line` {start}"));
    }
    if start > total.max(1) {
        return Err(format!(
            "`start_line` {start} is past the file's {total} lines"
        ));
    }
    let end = end.min(total);
    let mut out = String::new();
    if view.pending > 0 {
        out.push_str(&format!(
            "[includes {} pending changes you proposed]\n",
            view.pending
        ));
    }
    if view.stale > 0 {
        out.push_str(&format!(
            "[{} of your earlier changes no longer apply]\n",
            view.stale
        ));
    }
    let (mut bytes, mut next) = (0, start);
    // A first line over the cap, cut: its number.
    let mut cut_line = None;
    while next <= end && next - start < MAX_READ_LINES {
        let line = lines[next - 1];
        if bytes + line.len() > MAX_READ_BYTES {
            if next == start {
                // As much of it as fits, on a char boundary.
                let mut cut = MAX_READ_BYTES;
                while !line.is_char_boundary(cut) {
                    cut -= 1;
                }
                out.push_str(&line[..cut]);
                cut_line = Some(next);
                next += 1;
            }
            break;
        }
        out.push_str(line);
        bytes += line.len();
        next += 1;
    }
    if cut_line.is_some() || next <= end {
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        let more = end + 1 - next;
        out.push_str(&match cut_line {
            None => format!("[truncated: {more} more lines; read from line {next}]"),
            Some(row) if more == 0 => {
                format!("[truncated: the rest of line {row}, which is over 64 KiB]")
            }
            Some(row) => format!(
                "[truncated: the rest of line {row}, which is over 64 KiB, and {more} more \
                 lines; read from line {next}]"
            ),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// propose_edit

/// `text` with `\r\n` as `\n`, as buffers keep it.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// `edits` applied to `view`, the chat's view of a file (`None` if there's
/// no file), checked as a set first: each `old_text` found exactly once,
/// and none overlapping; or one edit with an empty `old_text`, which is the
/// whole new text. Else why not, and nothing is applied.
pub(crate) fn apply_edits(view: Option<&str>, edits: &[ProposedEdit]) -> ToolOutcome {
    if edits.is_empty() {
        return Err(NO_EDITS.to_owned());
    }
    if edits.iter().any(|edit| edit.old_text.is_empty()) {
        if edits.len() > 1 {
            return Err(WHOLE_ALONE.to_owned());
        }
        return Ok(lf(&edits[0].new_text));
    }
    let Some(view) = view else {
        return Err(NO_FILE_TO_EDIT.to_owned());
    };
    let mut found: Vec<(Range<usize>, String, usize)> = Vec::with_capacity(edits.len());
    for (ix, edit) in edits.iter().enumerate() {
        let old = lf(&edit.old_text);
        let number = ix + 1;
        let Some(at) = view.find(old.as_str()) else {
            return Err(format!(
                "Edit {number}: `old_text` isn't in the file as you last read it"
            ));
        };
        // Overlapping matches count too.
        let step = old.chars().next().map_or(1, char::len_utf8);
        if view[at + step..].contains(old.as_str()) {
            return Err(format!(
                "Edit {number}: `old_text` is in the file more than once; include more text around it"
            ));
        }
        found.push((at..at + old.len(), lf(&edit.new_text), number));
    }
    found.sort_by_key(|(range, _, _)| (range.start, range.end));
    if let Some(pair) = found
        .windows(2)
        .find(|pair| pair[0].0.end > pair[1].0.start)
    {
        let (a, b) = (pair[0].2.min(pair[1].2), pair[0].2.max(pair[1].2));
        return Err(format!("Edits {a} and {b} overlap"));
    }
    let mut text = view.to_owned();
    for (range, new_text, _) in found.into_iter().rev() {
        text.replace_range(range, &new_text);
    }
    Ok(text)
}

/// A file as it is, under the chat's proposals.
#[derive(Clone, Copy)]
pub(crate) enum Base<'a> {
    /// The file's buffer, as it is now.
    Buffer(&'a BufferSnapshot),
    /// The file's text from disk, with no buffer open.
    Disk(&'a str),
    /// No file there.
    Missing,
}

/// The chat's view of a file: its text with the chat's pending proposal
/// applied (`None` if there's no file), how many of that proposal's
/// changes are pending, and how many no longer apply.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) text: Option<String>,
    pub(crate) pending: usize,
    pub(crate) stale: usize,
}

/// What a valid `propose_edit` did.
#[derive(Debug)]
pub(crate) struct Proposed {
    /// How many changes the file's proposal now holds for the user.
    pub(crate) changes: usize,
    /// The file's new proposal, on its buffer: none for a new file, or if
    /// the new view is the buffer's text.
    pub(crate) proposal: Option<Rc<RefCell<ProposedChange>>>,
    /// The proposal it replaced.
    pub(crate) replaced: Option<Rc<RefCell<ProposedChange>>>,
    /// Whether the changeset the workbench shows changed: an earlier turn's
    /// hunks or new file in it superseded.
    pub(crate) shown_changed: bool,
}

/// The chat's live proposal of a file.
struct Live {
    /// The turn that made it.
    turn: String,
    /// The path its changeset names it by: its editor's.
    path: PathBuf,
    kind: LiveKind,
    /// Whether a turn's changeset took it, which then owns its review: its
    /// decisions, and following it to a reopened buffer.
    handed_in: bool,
}

enum LiveKind {
    Existing {
        proposal: Rc<RefCell<ProposedChange>>,
        /// The buffer's text it was made against, and the text it makes:
        /// to make it again on a reopened buffer before it's handed in.
        base: Arc<str>,
        view: Arc<str>,
    },
    /// A new file's text, and whether its changeset's entry was pending
    /// when last seen.
    New { text: String, pending: bool },
}

impl Live {
    /// Whether some of it is still for the user to decide: a hunk pending or
    /// stale, or a new file pending.
    fn undecided(&self) -> bool {
        match &self.kind {
            LiveKind::Existing { proposal, .. } => {
                let proposal = proposal.borrow();
                proposal.status_count(HunkStatus::Pending)
                    + proposal.status_count(HunkStatus::Stale)
                    > 0
            }
            LiveKind::New { pending, .. } => *pending,
        }
    }
}

/// The turn running.
struct Turn {
    id: String,
    origin: Origin,
    /// The files it proposed to, by key, in the order first proposed.
    files: Vec<PathBuf>,
}

/// A [`BufferSet`] of none, for the parts of a changeset that need no
/// buffer: its new files.
struct NoBuffers;

impl BufferSet for NoBuffers {
    fn buffer_mut(&mut self, _: BufferId) -> Option<&mut fe_buffer::Buffer> {
        None
    }
}

/// Why `text` can't be a view, before any of it is copied: over
/// [`MAX_FILE`], or binary.
fn unviewable(text: &Text) -> Option<String> {
    let len = text.len();
    if len > MAX_FILE {
        return unattachable(&[], len);
    }
    let probe = text.clip_offset(BINARY_PROBE.min(len), Bias::Left);
    unattachable(text.slice_to_string(0..probe).as_bytes(), len)
}

/// `base`'s own text, as a view with no proposal.
fn plain_view(base: Base) -> Result<View, String> {
    let text = match base {
        Base::Buffer(snapshot) => {
            if let Some(why) = unviewable(snapshot.text()) {
                return Err(why);
            }
            Some(snapshot.text().to_string())
        }
        Base::Disk(text) => {
            if let Some(why) = unattachable(text.as_bytes(), text.len()) {
                return Err(why);
            }
            Some(text.to_owned())
        }
        Base::Missing => None,
    };
    Ok(View {
        text,
        ..View::default()
    })
}

/// The chat's proposals across its turns: see the module docs. Kept by the
/// tool loop, one per chat.
#[derive(Default)]
pub(crate) struct ChatProposals {
    /// The live proposal of each file, by its real path.
    files: HashMap<PathBuf, Live>,
    turn: Option<Turn>,
    superseded: HashSet<HunkId>,
    superseded_new: HashSet<(String, PathBuf)>,
    /// Changesets the AI Changes panel let go of ([`ChatProposals::returner`]),
    /// not yet looked at.
    inbox: Rc<RefCell<Vec<Changeset>>>,
    /// Each turn's changeset the panel gave back while a live proposal
    /// is still that turn's, by turn id: what its review last decided (a
    /// new file's state), and its hunk memo to follow a proposal to a
    /// reopened buffer ([`Changeset::rebind`]).
    kept: HashMap<String, Changeset>,
    /// The panes and tab editors [`watch`] follows, by entity.
    watched: HashMap<gpui::EntityId, gpui::Subscription>,
}

impl ChatProposals {
    pub(crate) fn new() -> ChatProposals {
        ChatProposals::default()
    }

    /// What takes a changeset this chat handed in back from the AI Changes
    /// panel once another review replaces it
    /// ([`Workbench::set_changeset_returning`]): it waits in the chat's
    /// inbox until the chat next looks.
    pub(crate) fn returner(&self) -> crate::ai_changes_panel::Returned {
        let inbox = self.inbox.clone();
        Box::new(move |changeset| inbox.borrow_mut().push(changeset))
    }

    /// Keeps each changeset given back whose turn a live proposal is
    /// still of, and lets go of the rest.
    fn absorb(&mut self) {
        let returned: Vec<Changeset> = self.inbox.borrow_mut().drain(..).collect();
        for changeset in returned {
            self.kept.insert(changeset.turn_id().to_owned(), changeset);
        }
        let files = &self.files;
        self.kept
            .retain(|turn, _| files.values().any(|live| live.turn == *turn));
    }

    /// Follows the proposal of `key` to `buffer`, its file reopened, through
    /// the changeset its review gave back, as the AI Changes panel would
    /// ([`Changeset::rebind`]), the buffer reached through `set`. Whether
    /// it now is on `buffer`.
    pub(crate) fn rebind_kept(
        &mut self,
        key: &Path,
        set: &mut impl BufferSet,
        buffer: BufferId,
    ) -> bool {
        self.absorb();
        let Some(live) = self.files.get(key) else {
            return false;
        };
        let LiveKind::Existing { proposal, .. } = &live.kind else {
            return false;
        };
        let Some(changeset) = self.kept.get_mut(&live.turn) else {
            return false;
        };
        // Its memo is current: the editors' decisions since the panel let it
        // go are observed ([`watch`]). A hunk found stale since, which the
        // memo may not have seen, refuses as `rebind` refuses a stale one.
        if proposal.borrow().status_count(HunkStatus::Stale) > 0 {
            return false;
        }
        if changeset.rebind(set, &live.path, buffer).is_err() {
            return false;
        }
        proposal.borrow().buffer() == buffer
    }

    /// Files `event`, a decision in the tab editor of `buffer` at `path`,
    /// with the changeset given back that holds its proposal, if one does,
    /// as the AI Changes panel files decisions with the review it shows: an
    /// accept under its checkpoint, so Revert Turn on it would cover it,
    /// then the hunks' memo taken again and the next accept armed. `set`
    /// reaches the buffer.
    fn decided(
        &mut self,
        buffer: BufferId,
        event: &fe_gpui::ProposalEvent,
        set: &mut impl BufferSet,
    ) {
        self.absorb();
        let Self { files, kept, .. } = self;
        let owner = files.values().find_map(|live| match &live.kind {
            LiveKind::Existing { proposal, .. } if proposal.borrow().buffer() == buffer => {
                Some((live.turn.clone(), live.path.clone()))
            }
            _ => None,
        });
        let Some((turn, path)) = owner else {
            return;
        };
        let Some(changeset) = kept.get_mut(&turn) else {
            return;
        };
        match *event {
            fe_gpui::ProposalEvent::Accepted { hunk, transaction } => {
                changeset.adopt_accept(set, &path, hunk, Some(transaction));
            }
            fe_gpui::ProposalEvent::AcceptedNoChange { hunk } => {
                changeset.adopt_accept(set, &path, hunk, None);
            }
            fe_gpui::ProposalEvent::Rejected { .. } => {}
        }
        changeset.observe(set);
        changeset.arm(set, &path);
    }

    /// Takes every changeset given back's memo again, and arms each file's
    /// next accept, over the buffers `set` reaches.
    fn observe_kept(&mut self, set: &mut impl BufferSet) {
        self.absorb();
        for changeset in self.kept.values_mut() {
            changeset.observe(set);
            let paths: Vec<PathBuf> = (changeset.files(set).into_iter())
                .filter(|entry| !entry.is_new)
                .map(|entry| entry.path)
                .collect();
            for path in paths {
                changeset.arm(set, &path);
            }
        }
    }

    /// Starts turn `id`, whose proposals are by `origin`. A turn not ended
    /// is dropped, its proposals live still.
    pub(crate) fn begin_turn(&mut self, id: impl Into<String>, origin: Origin) {
        self.turn = Some(Turn {
            id: id.into(),
            origin,
            files: Vec::new(),
        });
    }

    /// The id of the turn running.
    pub(crate) fn turn(&self) -> Option<&str> {
        self.turn.as_ref().map(|turn| turn.id.as_str())
    }

    /// Ends the turn running: its changeset, for [`Workbench::set_changeset`]
    /// (which shows its proposals in their editors), or `None` to leave the
    /// one shown. It holds each file the turn left a live proposal on, in
    /// the order first proposed, and then every earlier turn's proposal
    /// still undecided, by path: pending work moves to the latest turn's
    /// review, so the changeset shown owns all of it (a pending new file as
    /// `shown` last says, an existing file's proposal shared). Those become
    /// this turn's. A turn that proposed nothing returns one only if some
    /// pending work is in no changeset `shown` is: else `None`, as when no
    /// turn is running. See [`end_turn`] for the workbench's half.
    pub(crate) fn end_turn(
        &mut self,
        mut shown: Option<&mut Changeset>,
        snapshot_of: impl Fn(BufferId) -> Option<BufferSnapshot>,
    ) -> Option<Changeset> {
        self.absorb();
        let keys: Vec<PathBuf> = self.files.keys().cloned().collect();
        for key in &keys {
            self.refresh_new(key, shown.as_deref_mut());
        }
        let turn = self.turn.take()?;
        let shown_turn = shown.map(|shown| shown.turn_id().to_owned());
        let own: Vec<PathBuf> = (turn.files.iter())
            .filter(|key| {
                self.files
                    .get(*key)
                    .is_some_and(|live| live.turn == turn.id)
            })
            .cloned()
            .collect();
        let mut carried: Vec<PathBuf> = (self.files.iter())
            .filter(|(_, live)| live.turn != turn.id && live.undecided())
            .map(|(key, _)| key.clone())
            .collect();
        carried.sort();
        let orphaned = (carried.iter())
            .any(|key| shown_turn.as_deref() != Some(self.files[key].turn.as_str()));
        if own.is_empty() && !orphaned {
            return None;
        }
        // A proposal partly decided goes on as a new one of its pending
        // changes: its decisions, and Revert Turn's checkpoint of what it
        // accepted, stay with the changeset that has them.
        carried.retain(|key| self.carry_pending(key, &turn.origin, &snapshot_of));
        let mut changeset = Changeset::new(turn.id.clone(), turn.origin.clone());
        for key in own.iter().chain(&carried) {
            let live = self.files.get_mut(key).expect("listed above");
            live.turn = turn.id.clone();
            live.handed_in = true;
            match &live.kind {
                LiveKind::Existing { proposal, .. } => {
                    let buffer = proposal.borrow().buffer();
                    changeset.add_file(live.path.clone(), buffer, proposal.clone());
                }
                LiveKind::New { text, .. } => {
                    changeset.add_new_file(live.path.clone(), text.clone());
                }
            }
        }
        self.absorb();
        Some(changeset)
    }

    /// Readies the undecided proposal of `key` to go into a new turn's
    /// changeset: as it is if none of its hunks is decided, else made again
    /// on its buffer (`snapshot_of` gives it) from the buffer's text to its
    /// pending changes, which are then superseded in it. A stale hunk isn't
    /// carried. Whether there's something to carry.
    fn carry_pending(
        &mut self,
        key: &Path,
        origin: &Origin,
        snapshot_of: &impl Fn(BufferId) -> Option<BufferSnapshot>,
    ) -> bool {
        let Some(live) = self.files.get_mut(key) else {
            return false;
        };
        let LiveKind::Existing { proposal, .. } = &live.kind else {
            return true;
        };
        let (buffer, decided) = {
            let proposal = proposal.borrow();
            let decided = proposal.status_count(HunkStatus::Accepted)
                + proposal.status_count(HunkStatus::Rejected);
            (proposal.buffer(), decided > 0)
        };
        if !decided {
            return true;
        }
        let Some(snapshot) = snapshot_of(buffer).filter(|snapshot| snapshot.len() <= MAX_FILE)
        else {
            return false;
        };
        let pending = proposal.borrow().dry_run(&snapshot).text().to_string();
        let range = 0..snapshot.len();
        let Ok(fresh) = propose(
            &snapshot,
            range,
            EditBody::Replace(pending.clone()),
            origin.clone(),
        ) else {
            return false;
        };
        if fresh.hunk_count() == 0 {
            return false;
        }
        let hunks = proposal.borrow().hunks(&snapshot);
        for hunk in hunks
            .iter()
            .filter(|hunk| hunk.status == HunkStatus::Pending)
        {
            if proposal.borrow_mut().reject(hunk.id).is_ok() {
                self.superseded.insert(hunk.id);
            }
        }
        live.kind = LiveKind::Existing {
            proposal: Rc::new(RefCell::new(fresh)),
            base: snapshot.text().to_string().into(),
            view: pending.into(),
        };
        true
    }

    /// The keys of the files whose proposal is on a buffer and still has a
    /// hunk to decide.
    pub(crate) fn undecided_files(&self) -> Vec<PathBuf> {
        let mut keys: Vec<PathBuf> = (self.files.iter())
            .filter(|(_, live)| matches!(live.kind, LiveKind::Existing { .. }) && live.undecided())
            .map(|(key, _)| key.clone())
            .collect();
        keys.sort();
        keys
    }

    /// The live proposal on the file at `key`, if it's on a buffer.
    pub(crate) fn proposal(&self, key: &Path) -> Option<Rc<RefCell<ProposedChange>>> {
        match &self.files.get(key)?.kind {
            LiveKind::Existing { proposal, .. } => Some(proposal.clone()),
            LiveKind::New { .. } => None,
        }
    }

    /// The text of the new file at `key`, if the chat proposed one and it
    /// was pending when last seen.
    pub(crate) fn new_file(&self, key: &Path) -> Option<&str> {
        match &self.files.get(key)?.kind {
            LiveKind::New {
                text,
                pending: true,
            } => Some(text),
            _ => None,
        }
    }

    /// Whether the file at `key` is a new file the chat proposed and is still
    /// pending: see [`ChatProposals::view`] for `shown`.
    pub(crate) fn pending_new_file(&mut self, key: &Path, shown: Option<&mut Changeset>) -> bool {
        self.absorb();
        self.refresh_new(key, shown);
        self.new_file(key).is_some()
    }

    /// Whether hunk `id` was rejected because a later change replaced it:
    /// "replaced by a later change".
    pub(crate) fn is_superseded(&self, id: HunkId) -> bool {
        self.superseded.contains(&id)
    }

    /// Whether turn `turn`'s new file at `path` was replaced by a later
    /// change.
    pub(crate) fn is_new_file_superseded(&self, turn: &str, path: &Path) -> bool {
        self.superseded_new
            .contains(&(turn.to_owned(), path.to_owned()))
    }

    /// Reads again whether the new file at `key` is pending: the running
    /// turn's always is, and an earlier turn's is as its changeset says,
    /// `shown` if the workbench shows it, else the one the panel gave back
    /// when another review replaced it, as it then stood.
    fn refresh_new(&mut self, key: &Path, shown: Option<&mut Changeset>) {
        let running = self.turn.as_ref().map(|turn| turn.id.clone());
        let Self { files, kept, .. } = self;
        let Some(live) = files.get_mut(key) else {
            return;
        };
        let LiveKind::New { pending, .. } = &mut live.kind else {
            return;
        };
        if running.as_deref() == Some(live.turn.as_str()) {
            *pending = true;
            return;
        }
        let owner = match shown.filter(|shown| shown.turn_id() == live.turn) {
            Some(shown) => Some(shown),
            None => kept.get_mut(&live.turn),
        };
        if let Some(owner) = owner {
            *pending = (owner.files(&mut NoBuffers).into_iter())
                .any(|entry| entry.path == live.path && entry.is_new && entry.counts.pending > 0);
        }
    }

    /// Follows the proposal of `key` to `snapshot`'s buffer, a reopened one
    /// of the file, if it was never handed in: no changeset follows it, and
    /// nothing could decide it, as only a changeset shows it in an editor.
    /// It's made again there from the text it was made against, which the
    /// buffer must hold, every hunk pending; unless a hunk was found stale,
    /// as [`Changeset::rebind`] refuses one. A proposal handed in is its
    /// changeset's to follow.
    fn follow(&mut self, key: &Path, snapshot: &BufferSnapshot) {
        let Some(live) = self.files.get(key).filter(|live| !live.handed_in) else {
            return;
        };
        let LiveKind::Existing {
            proposal,
            base,
            view,
        } = &live.kind
        else {
            return;
        };
        let decided = {
            let proposal = proposal.borrow();
            (proposal.hunk_count()) != proposal.status_count(HunkStatus::Pending)
        };
        if proposal.borrow().buffer() == snapshot.id()
            || decided
            || snapshot.len() != base.len()
            || snapshot.text().to_string() != **base
        {
            return;
        }
        let origin = proposal.borrow().origin().clone();
        let range = 0..snapshot.len();
        if let Ok(fresh) = propose(snapshot, range, EditBody::Replace(view.to_string()), origin) {
            *proposal.borrow_mut() = fresh;
        }
    }

    /// The chat's view of the file at `key`, as it is in `base`: see
    /// [`View`]. `shown` is the changeset the workbench shows, if any. A
    /// proposal on another buffer, one it couldn't be followed to, counts
    /// as no longer applying. Refused if the view is over 1 MiB or binary.
    pub(crate) fn view(
        &mut self,
        key: &Path,
        base: Base,
        shown: Option<&mut Changeset>,
    ) -> Result<View, String> {
        self.absorb();
        self.refresh_new(key, shown);
        if let Base::Buffer(snapshot) = base {
            self.follow(key, snapshot);
        }
        match (self.files.get_mut(key).map(|live| &mut live.kind), base) {
            (Some(LiveKind::Existing { proposal, .. }), Base::Buffer(snapshot))
                if proposal.borrow().buffer() == snapshot.id() =>
            {
                // Bounded before the dry run, which copies conflicting text
                // out of the buffer.
                if snapshot.len() > MAX_FILE {
                    return Err(unattachable(&[], snapshot.len()).unwrap_or_default());
                }
                let dry_run = proposal.borrow().dry_run(snapshot);
                if let Some(why) = unviewable(dry_run.text()) {
                    return Err(why);
                }
                let count = |clean: bool| {
                    (dry_run.hunks().iter())
                        .filter(|hunk| match &hunk.outcome {
                            Outcome::Clean => clean,
                            Outcome::Conflict(_) => !clean,
                            Outcome::Resolved(_) => false,
                        })
                        .count()
                };
                Ok(View {
                    text: Some(dry_run.text().to_string()),
                    pending: count(true),
                    stale: count(false),
                })
            }
            (Some(LiveKind::Existing { proposal, .. }), _) => {
                let proposal = proposal.borrow();
                let lost = proposal.status_count(HunkStatus::Pending)
                    + proposal.status_count(HunkStatus::Stale);
                Ok(View {
                    stale: lost,
                    ..plain_view(base)?
                })
            }
            (
                Some(LiveKind::New {
                    text,
                    pending: true,
                }),
                _,
            ) => {
                if let Some(why) = unattachable(text.as_bytes(), text.len()) {
                    return Err(why);
                }
                Ok(View {
                    text: Some(text.clone()),
                    pending: 1,
                    stale: 0,
                })
            }
            _ => plain_view(base),
        }
    }

    /// `propose_edit` of `edits` to the file at `key`, named `path` in the
    /// changeset, as it is in `base`, in the turn running: see the module
    /// docs. `shown` is the changeset the workbench shows, if any. An
    /// existing file must be in a buffer. Else why not, and nothing changes.
    pub(crate) fn propose(
        &mut self,
        key: &Path,
        path: &Path,
        base: Base,
        edits: &[ProposedEdit],
        mut shown: Option<&mut Changeset>,
    ) -> Result<Proposed, String> {
        let Some(turn) = self.turn.as_ref() else {
            return Err(NO_TURN.to_owned());
        };
        let (turn_id, origin) = (turn.id.clone(), turn.origin.clone());
        let view = self.view(key, base, shown.as_deref_mut())?;
        let new_text = apply_edits(view.text.as_deref(), edits)?;
        if let Some(why) = unattachable(new_text.as_bytes(), new_text.len()) {
            return Err(why);
        }
        let pending_new = self.new_file(key).is_some();
        let snapshot = match base {
            Base::Buffer(snapshot) if !pending_new => Some(snapshot),
            Base::Disk(_) if !pending_new => return Err(NOT_OPENED.to_owned()),
            Base::Buffer(_) | Base::Disk(_) | Base::Missing => None,
        };
        let new = match snapshot {
            Some(snapshot) => {
                let range = 0..snapshot.len();
                let made = propose(snapshot, range, EditBody::Replace(new_text.clone()), origin)?;
                (made.hunk_count() > 0).then(|| LiveKind::Existing {
                    proposal: Rc::new(RefCell::new(made)),
                    base: snapshot.text().to_string().into(),
                    view: new_text.into(),
                })
            }
            None => Some(LiveKind::New {
                text: new_text,
                pending: true,
            }),
        };
        let mut proposed = Proposed {
            changes: 0,
            proposal: None,
            replaced: None,
            shown_changed: false,
        };
        let shows = |shown: &Option<&mut Changeset>, turn: &str| {
            (shown.as_deref()).is_some_and(|shown| shown.turn_id() == turn)
        };
        // The earlier proposal's pending hunks are superseded.
        if let Some(live) = self.files.remove(key) {
            let earlier_turn = live.turn != turn_id;
            match live.kind {
                LiveKind::Existing { proposal, .. } => {
                    // On another buffer, one it couldn't be followed to,
                    // nothing of it can be decided: it's only replaced.
                    if let Some(snapshot) = snapshot
                        && proposal.borrow().buffer() == snapshot.id()
                    {
                        let hunks = proposal.borrow().hunks(snapshot);
                        for hunk in hunks
                            .iter()
                            .filter(|hunk| hunk.status == HunkStatus::Pending)
                        {
                            if proposal.borrow_mut().reject(hunk.id).is_ok() {
                                self.superseded.insert(hunk.id);
                            }
                        }
                    }
                    proposed.shown_changed = earlier_turn && shows(&shown, &live.turn);
                    proposed.replaced = Some(proposal);
                }
                LiveKind::New { pending: false, .. } => {}
                LiveKind::New { .. } if !earlier_turn => {}
                LiveKind::New { .. } => {
                    if let Some(shown) = shown.filter(|shown| shown.turn_id() == live.turn)
                        && shown.reject_file(&mut NoBuffers, &live.path).is_ok()
                    {
                        proposed.shown_changed = true;
                    }
                    self.superseded_new.insert((live.turn, live.path));
                }
            }
        }
        if let Some(kind) = new {
            match &kind {
                LiveKind::Existing { proposal, .. } => {
                    proposed.changes = proposal.borrow().hunk_count();
                    proposed.proposal = Some(proposal.clone());
                }
                LiveKind::New { .. } => proposed.changes = 1,
            }
            self.files.insert(
                key.to_owned(),
                Live {
                    turn: turn_id,
                    path: path.to_owned(),
                    kind,
                    handed_in: false,
                },
            );
            let turn = self.turn.as_mut().expect("checked above");
            if !turn.files.iter().any(|file| file == key) {
                turn.files.push(key.to_owned());
            }
        }
        Ok(proposed)
    }
}

/// What a `propose_edit` result says of `proposed`.
pub(crate) fn proposed_text(rel: &str, proposed: &Proposed) -> String {
    format!(
        "Proposed {} changes to {rel}; the user will review them.",
        proposed.changes
    )
}

// ---------------------------------------------------------------------------
// The file a call reaches

/// The editors of files open in every window, `workbench`'s first (as
/// chat_context's @file finds them), each with its path.
fn live_editors(workbench: &Entity<Workbench>, cx: &App) -> Vec<(Entity<EditorView>, PathBuf)> {
    let this = workbench.read(cx);
    // The window being updated can't be read through its handle: hence
    // `this`, by itself.
    let others = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .filter_map(|window| window.read(cx).ok())
        .filter(|other| !std::ptr::eq(*other, this));
    (std::iter::once(this).chain(others))
        .flat_map(|workbench| workbench.file_editors(cx))
        .filter_map(|editor| {
            let path = editor.read(cx).model().path()?.to_owned();
            Some((editor, path))
        })
        .collect()
}

/// Whether `a` and `b`, stamps of a file or none, are of the same file: its
/// device and inode, where there are such.
fn same_file(a: Option<&DiskStamp>, b: Option<&DiskStamp>) -> bool {
    match (a, b) {
        (None, None) => true,
        #[cfg(unix)]
        (Some(a), Some(b)) => (a.dev, a.ino) == (b.dev, b.ino),
        #[cfg(not(unix))]
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// A file a call reaches, vetted: see the module docs.
struct Vetted {
    /// Where it really is: the proposals' key.
    real: PathBuf,
    /// The open buffer read from it, by its index among those asked about.
    open: Option<usize>,
    /// The stamp of the file the walk opened; `None` if there's none.
    stamp: Option<DiskStamp>,
    /// When no open buffer has it, a buffer of it read through the handle
    /// the walk opened, at most [`MAX_FILE`]: the only way a call reads a
    /// file, so what's read is what was vetted.
    loaded: Option<fe_buffer::Buffer>,
    read_only: bool,
    /// Whether the folder it's in is there.
    folder: bool,
}

/// Vets `rel` under `root` (see the module docs), matching it to one of
/// `open`, the open buffers' paths and stamps, and loading it if none is.
/// Reads the disk: run it off the main thread.
fn vet(root: &Path, rel: &str, open: &[chat_context::OpenBuffer]) -> Result<Vetted, String> {
    vet_with(root, rel, open, || {})
}

/// [`vet`], running `between` after the walk opened the file and before
/// it's read, as a test swaps the file then.
fn vet_with(
    root: &Path,
    rel: &str,
    open: &[chat_context::OpenBuffer],
    between: impl FnOnce(),
) -> Result<Vetted, String> {
    use std::io::Read as _;

    let resolved = resolve(root, rel)?;
    let real_root = dunce::canonicalize(root)
        .map_err(|_| kiln_l10n::tr!("workbench-chat-why-folder-unreadable").to_string())?;
    let (real, walked) = open_vetted(&real_root, &resolved)?;
    let index = open.iter().position(|(path, _)| {
        *path == real || chat_context::resolve(path).is_ok_and(|path| path == real)
    });
    let (stamp, file) = match walked {
        Walked::File(file) => {
            let meta = file.metadata().map_err(|error| error.to_string())?;
            if let Some(why) = unattachable(&[], usize::try_from(meta.len()).unwrap_or(usize::MAX))
            {
                return Err(why);
            }
            (DiskStamp::from_metadata(&meta).ok(), Some(file))
        }
        Walked::Missing => (None, None),
        Walked::Folder => return Err(IS_A_FOLDER.to_owned()),
        Walked::Unreadable(..) => {
            return Err(kiln_l10n::tr!("workbench-chat-why-unreadable").to_string());
        }
    };
    // An open buffer stands for the file only if it was read from it.
    if let Some(index) = index
        && !same_file(open[index].1.as_ref(), stamp.as_ref())
    {
        return Err(CHANGED.to_owned());
    }
    between();
    let loaded = match file.filter(|_| index.is_none()) {
        Some(file) => {
            let mut bytes = Vec::new();
            // One past the limit, to tell a file grown over it.
            ((&file).take(MAX_FILE as u64 + 1).read_to_end(&mut bytes))
                .map_err(|error| error.to_string())?;
            if let Some(why) = unattachable(&bytes, bytes.len()) {
                return Err(why);
            }
            Some(load(&bytes, &real, stamp).map_err(|error| error.to_string())?)
        }
        None => None,
    };
    Ok(Vetted {
        read_only: stamp.is_some() && is_read_only(&real),
        folder: real.parent().is_some_and(Path::is_dir),
        real,
        open: index,
        stamp,
        loaded,
    })
}

/// The buffer of `bytes`, all of the file the walk opened, at `real`, read
/// as the pane's loader reads a file (`loader::read`): in the encoding its
/// bytes say, or under a `.editorconfig` `charset` in that one's, unless
/// the bytes clearly disagree (another encoding's BOM, BOM-less UTF-16 that
/// isn't it, or bytes it can't decode), with the loader's warning. Stamped
/// as the file was before the read, so a save refuses a file changed
/// since.
fn load(bytes: &[u8], real: &Path, stamp: Option<DiskStamp>) -> std::io::Result<fe_buffer::Buffer> {
    use fe_buffer::{Encoding, FileEncoding};

    let detected = || {
        let mut buffer = fe_buffer::Buffer::from_reader_detecting(bytes)?;
        buffer.set_disk_stamp(stamp);
        Ok::<_, std::io::Error>(buffer)
    };
    let Some(charset) = fe_config::editorconfig_for(real).charset else {
        return detected();
    };
    let wanted = crate::loader::charset_encoding(charset);
    let found = FileEncoding::detect(bytes)?;
    let utf16 = |encoding: &'static Encoding| {
        Encoding::for_label(b"utf-16le") == Some(encoding)
            || Encoding::for_label(b"utf-16be") == Some(encoding)
    };
    let zero_bytes_utf16 = !found.bom && utf16(found.encoding);
    if (found.bom || zero_bytes_utf16) && found.encoding != wanted.encoding {
        return detected();
    }
    // In the charset's encoding, from the bytes read: nothing is read again.
    let mut buffer = fe_buffer::Buffer::from_reader_with(bytes, wanted)?;
    buffer.set_disk_stamp(stamp);
    let replaced = (buffer.text().chunks())
        .any(|chunk| chunk.contains(char::REPLACEMENT_CHARACTER))
        && buffer.save_snapshot().write_to(std::io::sink()).is_err();
    if replaced {
        log::warn!(
            "{} doesn't hold {} text, as its .editorconfig charset says; read as detected",
            real.display(),
            wanted.label(),
        );
        return detected();
    }
    buffer.set_encoding(wanted);
    Ok(buffer)
}

/// The changeset `workbench` shows, if any.
fn shown(workbench: &mut Workbench) -> Option<&mut Changeset> {
    workbench
        .ai_changes
        .as_mut()
        .map(|changes| &mut changes.changeset)
}

/// The file at `rel` under `workbench`'s root, vetted, and the editor of
/// the open buffer read from it, if one is, in any window. Resolving,
/// vetting and loading are off the main thread.
async fn reach(
    workbench: &Entity<Workbench>,
    rel: &str,
    cx: &mut AsyncWindowContext,
) -> Result<(Vetted, Option<Entity<EditorView>>), String> {
    let (root, editors, open, ids) = cx
        .update(|_, cx| {
            let root = workbench.read(cx).root().map(Path::to_owned);
            let editors = live_editors(workbench, cx);
            let (open, ids): (Vec<chat_context::OpenBuffer>, Vec<BufferId>) = (editors.iter())
                .map(|(editor, path)| {
                    let buffer = editor.read(cx).model().buffer();
                    ((path.clone(), buffer.disk_stamp()), buffer.id())
                })
                .unzip();
            (root, editors, open, ids)
        })
        .map_err(|_| NO_FOLDER.to_owned())?;
    let root = root.ok_or_else(|| NO_FOLDER.to_owned())?;
    let rel = rel.to_owned();
    let asked = open.clone();
    let vetted = cx
        .background_spawn(async move { vet(&root, &rel, &asked) })
        .await?;
    let editor = match vetted.open {
        None => None,
        Some(index) => {
            let (editor, path) = &editors[index];
            // Still the buffer matched, of the file vetted.
            let same = cx
                .update(|_, cx| {
                    let model = editor.read(cx).model();
                    let buffer = model.buffer();
                    buffer.id() == ids[index]
                        && buffer.disk_stamp() == open[index].1
                        && model.path() == Some(path.as_path())
                })
                .unwrap_or(false);
            if !same {
                return Err(CHANGED.to_owned());
            }
            Some(editor.clone())
        }
    };
    Ok((vetted, editor))
}

/// Whether `editor` is one of `workbench`'s own tabs, which its AI Changes
/// panel reaches (a diff view's working side isn't: a buffer only a diff
/// shows gets a tab of its own).
fn is_local(workbench: &Entity<Workbench>, editor: &Entity<EditorView>, cx: &App) -> bool {
    (workbench.read(cx).open_editors(cx).iter()).any(|local| local == editor)
}

/// A tab in `workbench`'s active pane for the vetted file: on `buffer`,
/// read through the vetted handle, or sharing `peer`'s buffer, an editor
/// of it in another window, so unsaved edits are kept. Its editor.
fn add_tab(
    workbench: &Entity<Workbench>,
    real: &Path,
    source: Result<fe_buffer::Buffer, &Entity<EditorView>>,
    cx: &mut App,
) -> Result<Entity<EditorView>, String> {
    let model = match source {
        Ok(buffer) => fe_editor::EditorModel::new(buffer, Some(real.to_owned())),
        Err(peer) => peer.read(cx).model().split(),
    };
    let path = model.path().map(Path::to_owned);
    let pane = workbench.read(cx).pane().clone();
    pane.update(cx, |pane, cx| pane.add(model, cx));
    let path = path.ok_or_else(|| NOT_OPENED.to_owned())?;
    editor_in(&pane, &path, cx).ok_or_else(|| NOT_OPENED.to_owned())
}

/// Whether the chat's proposal of `key` is on a buffer none of
/// `workbench`'s own tabs has, so its AI Changes panel can't reach it: its
/// editor was closed (here, whatever other windows or diff views show).
fn proposal_unseen(
    proposals: &Rc<RefCell<ChatProposals>>,
    key: &Path,
    workbench: &Entity<Workbench>,
    cx: &App,
) -> bool {
    let Some(proposal) = proposals.borrow().proposal(key) else {
        return false;
    };
    let buffer = proposal.borrow().buffer();
    !(workbench.read(cx).open_editors(cx).iter())
        .any(|editor| editor.read(cx).model().buffer().id() == buffer)
}

/// Keeps the reviews the AI Changes panel gave back observing, as the panel
/// observes the one it shows ([`ChatProposals::decided`]): every tab editor
/// of `workbench`'s is followed for its proposal decisions, and every pane
/// for new tabs to follow. Takes those reviews' memos again now, too.
pub(crate) fn watch(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    cx: &mut App,
) {
    let (panes, editors) = {
        let this = workbench.read(cx);
        (this.panes(cx), this.open_editors(cx))
    };
    let weak = Rc::downgrade(proposals);
    let owner = workbench.downgrade();
    for pane in panes {
        if proposals.borrow().watched.contains_key(&pane.entity_id()) {
            continue;
        }
        let (weak, owner) = (weak.clone(), owner.clone());
        let watching = cx.observe(&pane, move |_, cx| {
            if let (Some(proposals), Some(workbench)) = (weak.upgrade(), owner.upgrade()) {
                watch(&workbench, &proposals, cx);
            }
        });
        proposals
            .borrow_mut()
            .watched
            .insert(pane.entity_id(), watching);
    }
    for editor in &editors {
        if proposals.borrow().watched.contains_key(&editor.entity_id()) {
            continue;
        }
        let weak = weak.clone();
        let watching = cx.subscribe(editor, move |editor, event: &fe_gpui::ProposalEvent, cx| {
            let Some(proposals) = weak.upgrade() else {
                return;
            };
            let buffer = editor.read(cx).model().buffer().id();
            let mut made = crate::ai_changes_buffers::Made::default();
            crate::ai_changes_buffers::with_editors(
                std::slice::from_ref(&editor),
                &mut made,
                cx,
                |set| proposals.borrow_mut().decided(buffer, event, set),
            );
        });
        proposals
            .borrow_mut()
            .watched
            .insert(editor.entity_id(), watching);
    }
    let mut made = crate::ai_changes_buffers::Made::default();
    crate::ai_changes_buffers::with_editors(&editors, &mut made, cx, |set| {
        proposals.borrow_mut().observe_kept(set)
    });
}

/// After a file whose proposal lost its editor was given `editor` again,
/// follows the proposal to its buffer: the review shown rebinds it
/// ([`Workbench::sync_ai_changes`]), else the one its turn's review gave
/// back ([`ChatProposals::rebind_kept`]).
fn follow_reopened(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    key: &Path,
    editor: Option<&Entity<EditorView>>,
    cx: &mut App,
) {
    workbench.update(cx, |workbench, cx| workbench.sync_ai_changes(cx));
    let Some(editor) = editor else {
        return;
    };
    let buffer = editor.read(cx).model().buffer().id();
    let followed = (proposals.borrow().proposal(key))
        .is_none_or(|proposal| proposal.borrow().buffer() == buffer);
    if !followed {
        let mut made = crate::ai_changes_buffers::Made::default();
        let followed = crate::ai_changes_buffers::with_editors(
            std::slice::from_ref(editor),
            &mut made,
            cx,
            |set| proposals.borrow_mut().rebind_kept(key, set, buffer),
        );
        // Shown in its editor, as the panel shows its review's, to decide.
        if followed && let Some(proposal) = proposals.borrow().proposal(key) {
            editor.update(cx, |editor, cx| {
                if !editor
                    .proposals()
                    .iter()
                    .any(|shown| Rc::ptr_eq(shown, &proposal))
                {
                    let mut shown = editor.proposals().to_vec();
                    shown.push(proposal);
                    editor.set_proposals(shown, cx);
                }
            });
        }
        watch(workbench, proposals, cx);
    }
}

/// `editor`'s buffer, now.
fn snapshot_of(editor: &Entity<EditorView>, cx: &App) -> BufferSnapshot {
    editor.read(cx).model().buffer().snapshot()
}

// ---------------------------------------------------------------------------
// The executors over the workbench

/// `read_file`: the chat's view of the file at `input.path` under the
/// workbench's root, as [`read_output`] gives it. A file whose proposal's
/// editor was closed is opened again, from the buffer read through the
/// vetted handle, so the AI Changes panel follows the proposal to it.
pub(crate) fn read_file(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    input: ReadFileInput,
    window: &mut Window,
    cx: &mut App,
) -> Task<ToolOutcome> {
    let workbench = workbench.clone();
    let proposals = proposals.clone();
    window.spawn(cx, async move |cx| {
        let (mut vetted, editor) = reach(&workbench, &input.path, cx).await?;
        cx.update(|_, cx| {
            watch(&workbench, &proposals, cx);
            let reopen = proposal_unseen(&proposals, &vetted.real, &workbench, cx);
            let editor = match (editor, vetted.loaded.take()) {
                (Some(peer), _) if reopen && !is_local(&workbench, &peer, cx) => {
                    Some(add_tab(&workbench, &vetted.real, Err(&peer), cx)?)
                }
                (None, Some(buffer)) if reopen => {
                    Some(add_tab(&workbench, &vetted.real, Ok(buffer), cx)?)
                }
                (editor, loaded) => {
                    vetted.loaded = loaded;
                    editor
                }
            };
            if reopen {
                follow_reopened(&workbench, &proposals, &vetted.real, editor.as_ref(), cx);
            }
            let snapshot = editor.as_ref().map(|editor| snapshot_of(editor, cx));
            let disk = match &vetted.loaded {
                Some(buffer) if snapshot.is_none() => {
                    if let Some(why) = unviewable(buffer.text()) {
                        return Err(why);
                    }
                    Some(buffer.text().to_string())
                }
                _ => None,
            };
            let base = match (&snapshot, &disk) {
                (Some(snapshot), _) => Base::Buffer(snapshot),
                (None, Some(text)) => Base::Disk(text),
                (None, None) => Base::Missing,
            };
            let view = workbench.update(cx, |workbench, _| {
                (proposals.borrow_mut()).view(&vetted.real, base, shown(workbench))
            })?;
            let text = view
                .text
                .as_deref()
                .ok_or_else(|| NO_SUCH_FILE.to_owned())?;
            read_output(text, input.start_line, input.end_line, &view)
        })
        .unwrap_or_else(|_| Err(NO_FOLDER.to_owned()))
    })
}

/// `propose_edit`: `input.edits` proposed to the file at `input.path` under
/// the workbench's root, against the chat's view of it, through
/// [`ChatProposals::propose`]. The proposal is on a buffer of this
/// window's, which its AI Changes panel reaches: an existing file no editor
/// here has gets a tab in the active pane, sharing another window's buffer
/// of it if one has it, else on the buffer read through the vetted handle.
/// A pending new file's view is its proposed text, whatever editor
/// previews it. The proposal reaches the editors with its turn's changeset
/// (see the module docs).
pub(crate) fn propose_edit(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    input: ProposeEditInput,
    window: &mut Window,
    cx: &mut App,
) -> Task<ToolOutcome> {
    let workbench = workbench.clone();
    let proposals = proposals.clone();
    window.spawn(cx, async move |cx| {
        let (mut vetted, found) = reach(&workbench, &input.path, cx).await?;
        cx.update(|_, cx| {
            watch(&workbench, &proposals, cx);
            let pending_new = workbench.update(cx, |workbench, _| {
                (proposals.borrow_mut()).pending_new_file(&vetted.real, shown(workbench))
            });
            let unseen = proposal_unseen(&proposals, &vetted.real, &workbench, cx);
            let editor = if pending_new {
                // Its editor, if any, previews the proposed text.
                None
            } else if vetted.read_only {
                return Err(READ_ONLY.to_owned());
            } else {
                match (found, vetted.loaded.take()) {
                    (Some(editor), _) if is_local(&workbench, &editor, cx) => Some(editor),
                    (Some(peer), _) => Some(add_tab(&workbench, &vetted.real, Err(&peer), cx)?),
                    (None, Some(buffer)) => {
                        Some(add_tab(&workbench, &vetted.real, Ok(buffer), cx)?)
                    }
                    (None, None) if vetted.stamp.is_some() => {
                        return Err(NOT_OPENED.to_owned());
                    }
                    (None, None) if !vetted.folder => return Err(NO_FOLDER_THERE.to_owned()),
                    (None, None) => None,
                }
            };
            if unseen && !pending_new {
                follow_reopened(&workbench, &proposals, &vetted.real, editor.as_ref(), cx);
            }
            let snapshot = match &editor {
                Some(editor) if editor.read(cx).is_read_only() => {
                    return Err(READ_ONLY.to_owned());
                }
                Some(editor) => Some(snapshot_of(editor, cx)),
                None => None,
            };
            let base = snapshot.as_ref().map_or(Base::Missing, Base::Buffer);
            let path = match &editor {
                Some(editor) => (editor.read(cx).model().path().map(Path::to_owned))
                    .unwrap_or_else(|| vetted.real.clone()),
                None => vetted.real.clone(),
            };
            let proposed = workbench.update(cx, |workbench, cx| {
                let proposed = proposals.borrow_mut().propose(
                    &vetted.real,
                    &path,
                    base,
                    &input.edits,
                    shown(workbench),
                )?;
                if proposed.shown_changed {
                    workbench.sync_ai_changes(cx);
                }
                Ok::<_, String>(proposed)
            })?;
            Ok(proposed_text(&input.path, &proposed))
        })
        .unwrap_or_else(|_| Err(NOT_OPENED.to_owned()))
    })
}

/// The end of a turn, for the loop: ends `proposals`' turn
/// ([`ChatProposals::end_turn`]) and hands its changeset to the workbench
/// ([`Workbench::set_changeset`]). First each file whose undecided proposal
/// lost its editor is given a tab again, from the buffer read through the
/// vetted handle, so the proposal is followed to it (the shown changeset
/// rebinds it, or it's made again there) and the changeset that takes it
/// sees its buffer. Whether a changeset was set.
pub(crate) fn end_turn(
    workbench: &Entity<Workbench>,
    proposals: &Rc<RefCell<ChatProposals>>,
    window: &mut Window,
    cx: &mut App,
) -> Task<bool> {
    let workbench = workbench.clone();
    let proposals = proposals.clone();
    window.spawn(cx, async move |cx| {
        let (keys, root) = cx
            .update(|_, cx| {
                let keys: Vec<PathBuf> = (proposals.borrow().undecided_files().into_iter())
                    .filter(|key| proposal_unseen(&proposals, key, &workbench, cx))
                    .collect();
                (keys, workbench.read(cx).root().map(Path::to_owned))
            })
            .unwrap_or_default();
        let real_root = match root.filter(|_| !keys.is_empty()) {
            Some(root) => {
                cx.background_spawn(async move { dunce::canonicalize(root).ok() })
                    .await
            }
            None => None,
        };
        for key in keys {
            let Some(rel) = (real_root.as_ref())
                .and_then(|root| key.strip_prefix(root).ok())
                .map(slashed)
            else {
                continue;
            };
            let Ok((mut vetted, found)) = reach(&workbench, &rel, cx).await else {
                continue;
            };
            cx.update(|_, cx| {
                let editor = match (found, vetted.loaded.take()) {
                    (Some(editor), _) if is_local(&workbench, &editor, cx) => Some(editor),
                    (Some(peer), _) => add_tab(&workbench, &vetted.real, Err(&peer), cx).ok(),
                    (None, Some(buffer)) => add_tab(&workbench, &vetted.real, Ok(buffer), cx).ok(),
                    (None, None) => None,
                };
                follow_reopened(&workbench, &proposals, &vetted.real, editor.as_ref(), cx);
                if let Some(editor) = editor {
                    let snapshot = snapshot_of(&editor, cx);
                    workbench.update(cx, |workbench, _| {
                        let view = (proposals.borrow_mut()).view(
                            &vetted.real,
                            Base::Buffer(&snapshot),
                            shown(workbench),
                        );
                        drop(view);
                    });
                }
            })
            .ok();
        }
        cx.update(|window, cx| {
            let snapshots: Vec<BufferSnapshot> = (workbench.read(cx).open_editors(cx).iter())
                .map(|editor| snapshot_of(editor, cx))
                .collect();
            let set = workbench.update(cx, |workbench, cx| {
                let changeset = proposals.borrow_mut().end_turn(shown(workbench), |id| {
                    snapshots
                        .iter()
                        .find(|snapshot| snapshot.id() == id)
                        .cloned()
                });
                let set = changeset.is_some();
                if let Some(changeset) = changeset {
                    let returned = proposals.borrow().returner();
                    workbench.set_changeset_returning(changeset, Some(returned), window, cx);
                }
                set
            });
            watch(&workbench, &proposals, cx);
            set
        })
        .unwrap_or(false)
    })
}

/// The editor of `pane`'s tab on `path`.
fn editor_in(pane: &Entity<Pane>, path: &Path, cx: &App) -> Option<Entity<EditorView>> {
    let pane = pane.read(cx);
    (pane.active_editor().into_iter())
        .chain(pane.tabs().iter().filter_map(|tab| tab.editor()))
        .find(|editor| editor.read(cx).model().path() == Some(path))
        .cloned()
}
