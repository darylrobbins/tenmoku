//! Browsing a file's local history, as JetBrains' Local History > Show
//! History: `history.show_local` lists the entries
//! [`LocalHistory`](fe_buffer::LocalHistory) keeps of the active file, read on
//! the background executor, newest first, in a picker built as
//! `encoding_picker`'s is. Picking one opens it, read-only, against the open
//! buffer ([`Workbench::finish_saved_diff`]), as `diff.compare_saved` does.
//!
//! `history.restore_local`, in such a diff, turns the buffer's text into the
//! entry's as one undoable user edit. Its rows are diffed off the main thread,
//! and only what differs is replaced ([`restore_edits`]), so carets and folds
//! stay where the text does. The buffer is left dirty, not saved.
//!
//! Neither has a default keybinding, as JetBrains' have none.

use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use fe_buffer::{
    Buffer, Origin,
    local_history::{Entry, Reason},
};
use fe_gpui::{EditorView, pane_item::file_name};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _,
    IntoElement as _, ParentElement as _, SharedString, WeakEntity, Window, div,
};
use imara_diff::{Algorithm, Diff, InternedInput};
use kiln_l10n::{Text, text, tr};
use kiln_shell::ModalId;

use super::Workbench;
use crate::{
    picker::{Picker, PickerDelegate},
    workbench::toast::{self, Toast},
};

/// The modal layer's id for the picker.
const LOCAL_HISTORY_PICKER: ModalId = ModalId("local_history");

/// An entry newer than this many seconds shows how long ago it was.
const RELATIVE_UNDER: u64 = 24 * 60 * 60;

/// `time` in local time: `2026-11-01 14:05`.
pub(super) fn stamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    local_stamp(seconds).unwrap_or_else(|| String::from("1970-01-01 00:00"))
}

#[cfg(unix)]
fn local_stamp(seconds: u64) -> Option<String> {
    let seconds = libc::time_t::try_from(seconds).unwrap_or(libc::time_t::MAX);
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: `seconds` and `tm` are valid for the call.
    if unsafe { libc::localtime_r(&seconds, tm.as_mut_ptr()) }.is_null() {
        return None;
    }
    // SAFETY: `localtime_r` succeeded, so it filled `tm`.
    let tm = unsafe { tm.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
    ))
}

/// The C runtime's `localtime_s`, Windows' `localtime_r`.
#[cfg(windows)]
fn local_stamp(seconds: u64) -> Option<String> {
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    unsafe extern "C" {
        #[link_name = "_localtime64_s"]
        fn localtime_s(tm: *mut libc::tm, time: *const i64) -> libc::c_int;
    }
    // SAFETY: `seconds` and `tm` are valid for the call.
    if unsafe { localtime_s(tm.as_mut_ptr(), &seconds) } != 0 {
        return None;
    }
    // SAFETY: `localtime_s` succeeded, so it filled `tm`.
    let tm = unsafe { tm.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
    ))
}

/// When `time` was, seen at `now`, as the row message's `ago` and `count`
/// arguments: how long ago under a day (`now`, or a count of `minutes` or
/// `hours`), else `stamp`, its [`stamp`] shown, as for one from the
/// future, the clock having moved back.
fn when(time: SystemTime, now: SystemTime) -> (&'static str, u64) {
    let Some(seconds) = (now.duration_since(time).ok())
        .map(|ago| ago.as_secs())
        .filter(|seconds| *seconds < RELATIVE_UNDER)
    else {
        return ("stamp", 0);
    };
    match seconds {
        ..60 => ("now", 0),
        60..3600 => ("minutes", seconds / 60),
        _ => ("hours", seconds / 3600),
    }
}

/// The row an entry shows as, seen at `now`.
fn row_label(entry: &Entry, now: SystemTime) -> Text {
    let (ago, count) = when(entry.time, now);
    let reason = match entry.reason {
        Reason::Saved => "saved",
        Reason::External => "external",
    };
    text!(
        "workbench-navigation-local-history-row",
        ago = ago,
        count = count,
        stamp = stamp(entry.time),
        reason = reason,
        size = fe_gpui::size_text(entry.len)
    )
}

/// The left side's title in an entry's diff.
fn diff_label(entry: &Entry) -> String {
    let stamp = stamp(entry.time);
    tr!(
        "workbench-navigation-local-history-diff-title",
        stamp = stamp
    )
    .into()
}

/// The replacements turning `old` into `new`, ascending and disjoint, as a
/// buffer's edit takes them: each run of rows that differs, without the bytes
/// it starts and ends with in common.
pub(super) fn restore_edits(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let mut input = InternedInput::default();
    input.update_before(old.split_inclusive('\n'));
    input.update_after(new.split_inclusive('\n'));
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let (old_starts, new_starts) = (row_starts(old), row_starts(new));
    diff.hunks()
        .map(|hunk| {
            let old_range =
                old_starts[hunk.before.start as usize]..old_starts[hunk.before.end as usize];
            let new_range =
                new_starts[hunk.after.start as usize]..new_starts[hunk.after.end as usize];
            let (range, text) = trimmed(old, old_range, new, new_range);
            (range, text.to_owned())
        })
        .collect()
}

/// Where each row of `text` starts, split as [`restore_edits`] splits it, and
/// its end.
fn row_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.split_inclusive('\n').scan(0, |at, row| {
        *at += row.len();
        Some(*at)
    }));
    starts
}

/// `old_range` of `old` and the text of `new_range` of `new`, less the bytes
/// they start and end with in common, cut on char boundaries.
fn trimmed<'a>(
    old: &str,
    old_range: Range<usize>,
    new: &'a str,
    new_range: Range<usize>,
) -> (Range<usize>, &'a str) {
    let (before, after) = (&old[old_range.clone()], &new[new_range]);
    let mut prefix = (before.bytes().zip(after.bytes()))
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let (before, after) = (&before[prefix..], &after[prefix..]);
    let mut suffix = (before.bytes().rev().zip(after.bytes().rev()))
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix) {
        suffix -= 1;
    }
    let start = old_range.start + prefix;
    (
        start..start + before.len() - suffix,
        &after[..after.len() - suffix],
    )
}

/// One row: an entry and what it shows as, resolved as it's drawn and
/// matched.
struct Row {
    entry: Entry,
    label: Text,
}

pub(crate) struct LocalHistoryPicker {
    rows: Vec<Row>,
    /// The rows matching the query, as indices.
    matches: Vec<usize>,
    /// The file the entries are of.
    path: PathBuf,
    /// The editor it's opened from, focused again as it closes.
    editor: WeakEntity<EditorView>,
    workbench: WeakEntity<Workbench>,
}

impl LocalHistoryPicker {
    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

#[cfg(test)]
impl LocalHistoryPicker {
    /// The matches' labels.
    pub(super) fn labels(&self) -> Vec<String> {
        (self.matches.iter())
            .map(|&row| self.rows[row].label.resolve().to_string())
            .collect()
    }
}

impl PickerDelegate for LocalHistoryPicker {
    /// Rows whose label holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (0..self.rows.len())
            .filter(|&row| (self.rows[row].label.resolve().to_lowercase()).contains(&query))
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div()
            .child(self.rows[self.matches[ix]].label.resolve())
            .into_any_element()
    }

    /// The entry's label, as the row shows it.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        self.rows[self.matches[ix]].label.resolve()
    }

    /// Opens the entry's diff. If the editor has closed, a pick only
    /// dismisses the picker.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        self.focus_editor(window, cx);
        if self.editor.upgrade().is_some() {
            let entry = self.rows[self.matches[ix]].entry;
            let path = self.path.clone();
            self.workbench
                .update(cx, |workbench, cx| {
                    workbench.open_history_entry(&path, entry, window, cx)
                })
                .ok();
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }
}

impl Workbench {
    /// Lists the active file's local history in a picker, in place of any
    /// palette. A toast says if it has no file or no entries. Only the
    /// window's latest listing lands, and its picker shows only if nothing
    /// moved meanwhile ([`Listing::holds`]).
    pub(super) fn show_local_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx).cloned();
        let Some(editor) = editor else {
            return toast::show(cx, Toast::info(no_file()));
        };
        let Some(path) = self.op_path(&editor, cx) else {
            return toast::show(cx, Toast::info(no_file()));
        };
        let Some(history) = fe_gpui::pane_item::local_history(cx) else {
            return toast::show(cx, Toast::info(no_history(&path)));
        };
        let listing = Listing::now(self, editor, window, cx);
        #[cfg(test)]
        let hold = (cx.has_global::<ListingHolds>())
            .then(|| cx.global_mut::<ListingHolds>().next())
            .flatten();
        let at = path.clone();
        let entries = cx.background_spawn(async move { history.entries(&at) });
        cx.spawn_in(window, async move |workbench, cx| {
            let entries = entries.await;
            #[cfg(test)]
            if let Some(hold) = hold {
                hold.recv().await.ok();
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    // A newer listing replaces this one.
                    if !listing.latest(cx) {
                        return;
                    }
                    match entries {
                        Ok(entries) if entries.is_empty() => {
                            toast::show(cx, Toast::info(no_history(&path)))
                        }
                        Ok(entries) => {
                            // A picker over what the user has since moved
                            // to, or for a file they've left, isn't what
                            // they're looking at.
                            if listing.holds(workbench, window, cx) {
                                let editor = listing.editor.clone();
                                workbench.pick_history_entry(path, entries, &editor, window, cx);
                            }
                        }
                        Err(error) => {
                            let name = file_name(Some(&path));
                            let message = tr!(
                                "workbench-navigation-local-history-read-failed",
                                name = name
                            );
                            let toast = Toast::error(message.to_string()).detail(error.to_string());
                            toast::show(cx, toast);
                        }
                    }
                })
                .ok();
        })
        .detach();
    }

    fn pick_history_entry(
        &mut self,
        path: PathBuf,
        entries: Vec<Entry>,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = SystemTime::now();
        let rows: Vec<_> = (entries.into_iter())
            .map(|entry| Row {
                label: row_label(&entry, now),
                entry,
            })
            .collect();
        let delegate = LocalHistoryPicker {
            matches: (0..rows.len()).collect(),
            rows,
            path,
            editor: editor.downgrade(),
            workbench: cx.entity().downgrade(),
        };
        self.dismiss_palette(window, cx);
        let placeholder = text!("workbench-navigation-local-history-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(LOCAL_HISTORY_PICKER, picker, window, cx)
        });
    }

    /// Reads `entry` of `path` on the background executor and opens its diff,
    /// or shows the one already open. A toast says if it can't be read.
    fn open_history_entry(
        &mut self,
        path: &Path,
        entry: Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = diff_label(&entry);
        let Some(ask) = self.begin_history_diff(path, label, entry.id, window, cx) else {
            return;
        };
        let Some(history) = fe_gpui::pane_item::local_history(cx) else {
            return toast::show(cx, Toast::info(no_history(path)));
        };
        let (at, id) = (path.to_owned(), entry.id);
        let read = cx.background_spawn(async move {
            let bytes = history.read(&at, id)?;
            Ok::<_, std::io::Error>(Buffer::from_file_contents(&String::from_utf8_lossy(&bytes)))
        });
        let name = file_name(Some(path));
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| match read {
                    Ok(text) => workbench.finish_saved_diff(ask, text, window, cx),
                    Err(error) => {
                        let message = tr!(
                            "workbench-navigation-local-history-version-failed",
                            name = name
                        );
                        let toast = Toast::error(message.to_string()).detail(error.to_string());
                        toast::show(cx, toast);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Turns the buffer of the active entry's diff into the entry's text, as
    /// one undoable user edit, and focuses its editor. The buffer stays
    /// unsaved. If it changes while the rows are diffed, nothing is restored
    /// and a toast says so.
    pub(super) fn restore_local_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(diff) = self.active_history_diff(cx) else {
            let message = tr!("workbench-navigation-local-history-not-a-version");
            return toast::show(cx, Toast::info(message.to_string()));
        };
        let (left, right) = {
            let view = diff.read(cx);
            (view.left().clone(), view.right().clone())
        };
        let entry = left.read(cx).model().buffer().text().clone();
        let (current, version) = {
            let view = right.read(cx);
            let buffer = view.model().buffer();
            (buffer.text().clone(), buffer.version())
        };
        let edits =
            cx.background_spawn(
                async move { restore_edits(&current.to_string(), &entry.to_string()) },
            );
        cx.spawn_in(window, async move |workbench, cx| {
            let edits = edits.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let moved = right.read(cx).model().buffer().version() != version;
                    if moved {
                        let name = file_name(right.read(cx).model().path());
                        let message = tr!(
                            "workbench-navigation-local-history-restore-moved",
                            name = name
                        );
                        let detail = tr!("workbench-navigation-local-history-restore-moved-detail");
                        let toast = Toast::info(message.to_string()).detail(detail.to_string());
                        return toast::show(cx, toast);
                    }
                    right.update(cx, |editor, cx| {
                        let model = editor.model_mut();
                        model.sync_display();
                        let mut buffer = model.buffer_mut();
                        let version = buffer.version();
                        // Apart from any open one, a vim insert session's say.
                        buffer.with_transaction_aside(|buffer| buffer.edit(edits, Origin::User));
                        buffer.note_edits_since(version);
                        drop(buffer);
                        cx.notify();
                    });
                    if workbench.active_history_diff(cx).as_ref() == Some(&diff) {
                        window.focus(&right.focus_handle(cx), cx);
                    }
                })
                .ok();
        })
        .detach();
    }
}

/// What a command for the active file answers when it has none.
fn no_file() -> String {
    tr!("workbench-navigation-local-history-no-file").into()
}

/// What a file with no entries answers.
fn no_history(path: &Path) -> String {
    let name = file_name(Some(path));
    tr!("workbench-navigation-local-history-none", name = name).into()
}

/// Each window's latest local history listing, by its workbench.
#[derive(Default)]
struct Listings(HashMap<EntityId, u64>);

impl gpui::Global for Listings {}

/// Where a listing was asked, and its number among its window's.
struct Listing {
    number: u64,
    /// The active editor, whose file's entries are listed.
    editor: Entity<EditorView>,
    focused: Option<FocusHandle>,
    modal: Option<ModalId>,
}

impl Listing {
    /// A new listing asked of `workbench` now, of `editor`'s file.
    fn now(
        workbench: &Workbench,
        editor: Entity<EditorView>,
        window: &Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let id = cx.entity_id();
        let latest = cx.default_global::<Listings>().0.entry(id).or_default();
        *latest += 1;
        Self {
            number: *latest,
            editor,
            focused: window.focused(cx),
            modal: workbench.shell().read(cx).modals().read(cx).modal(),
        }
    }

    /// Whether it's its window's latest listing.
    fn latest(&self, cx: &Context<Workbench>) -> bool {
        let latest = cx
            .try_global::<Listings>()
            .and_then(|l| l.0.get(&cx.entity_id()));
        latest == Some(&self.number)
    }

    /// Whether nothing moved since it was asked: the same editor is
    /// active, focus is where it was, and no modal or palette shows but
    /// the one that did, if any (the command palette it was run from,
    /// closing): not quick open opened meanwhile, say.
    fn holds(&self, workbench: &Workbench, window: &Window, cx: &Context<Workbench>) -> bool {
        let modal = workbench.shell().read(cx).modals().read(cx).modal();
        workbench.active_editor(cx) == Some(&self.editor)
            && window.focused(cx) == self.focused
            && (modal.is_none() || modal == self.modal)
    }
}

/// Holds the next listings' landing, in order, each until its sender sends
/// or drops.
#[cfg(test)]
#[derive(Default)]
pub(super) struct ListingHolds(pub(super) std::collections::VecDeque<async_channel::Receiver<()>>);

#[cfg(test)]
impl ListingHolds {
    fn next(&mut self) -> Option<async_channel::Receiver<()>> {
        self.0.pop_front()
    }
}

#[cfg(test)]
impl gpui::Global for ListingHolds {}

#[cfg(test)]
impl Workbench {
    /// The local history picker in the modal layer, if it's the modal showing.
    pub(super) fn local_history_picker(
        &self,
        cx: &App,
    ) -> Option<Entity<Picker<LocalHistoryPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == LOCAL_HISTORY_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}
